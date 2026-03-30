//! Daemon client: connect to the running daemon over Unix socket.
//!
//! Used by the CLI to send commands through the daemon instead of
//! cold-starting the engine on every invocation.

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use tokio::net::UnixStream;
use tracing::{debug, info};

use super::protocol::*;

/// Maximum time to wait for the daemon to start (backoff polling).
/// Model files are pre-downloaded during `hebbs init`, so the daemon
/// only needs to load the ONNX session (~2-5s).
const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(30);

/// Backoff steps for polling the socket after daemon launch.
/// After exhausting these steps, continues polling at the last interval
/// until DAEMON_START_TIMEOUT is reached.
const BACKOFF_STEPS: &[Duration] = &[
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(800),
    Duration::from_millis(1200),
];

/// A connected daemon client.
pub struct DaemonClient {
    stream: UnixStream,
}

impl DaemonClient {
    /// Connect to a running daemon at the given socket path.
    pub async fn connect(socket_path: &Path) -> Result<Self, String> {
        let stream = UnixStream::connect(socket_path)
            .await
            .map_err(|e| format!("failed to connect to daemon socket: {}", e))?;
        Ok(Self { stream })
    }

    /// Send a request without waiting for the response.
    ///
    /// Used by `hebbs init` to kick off indexing in the daemon without
    /// blocking the CLI. The daemon processes the command; the client
    /// drops the connection immediately after sending.
    pub async fn send_fire_and_forget(&mut self, request: &DaemonRequest) -> Result<(), String> {
        let (_reader, mut writer) = self.stream.split();

        write_message(&mut writer, request)
            .await
            .map_err(|e| format!("failed to send request: {}", e))?;

        Ok(())
    }

    /// Send a request and receive the final response with a default 30s timeout.
    ///
    /// Any `Progress` responses received before the final response are printed
    /// to stderr so the user sees live status updates.
    pub async fn send(&mut self, request: &DaemonRequest) -> Result<DaemonResponse, String> {
        self.send_with_timeout(request, std::time::Duration::from_secs(30))
            .await
    }

    /// Send a request and receive the final response with a custom timeout.
    pub async fn send_with_timeout(
        &mut self,
        request: &DaemonRequest,
        timeout: std::time::Duration,
    ) -> Result<DaemonResponse, String> {
        let (mut reader, mut writer) = self.stream.split();

        write_message(&mut writer, request)
            .await
            .map_err(|e| format!("failed to send request: {}", e))?;

        let read_loop = async {
            loop {
                let response: DaemonResponse = read_message(&mut reader)
                    .await
                    .map_err(|e| format!("failed to read response: {}", e))?
                    .ok_or_else(|| "daemon closed connection unexpectedly".to_string())?;

                if response.status == ResponseStatus::Progress {
                    if let Some(msg) = response
                        .data
                        .as_ref()
                        .and_then(|d| d.get("message"))
                        .and_then(|v| v.as_str())
                    {
                        eprintln!("  {}", msg);
                    }
                    continue;
                }

                return Ok(response);
            }
        };

        tokio::time::timeout(timeout, read_loop)
            .await
            .map_err(|_| format!(
                "Request timed out after {}s. The daemon may be unresponsive. \
                 Check status with `hebbs status` or restart with `hebbs daemon stop && hebbs serve`.",
                timeout.as_secs()
            ))?
    }
}

/// Resolve the default daemon socket path.
pub fn default_socket_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".hebbs").join("daemon.sock"))
}

/// Resolve the default daemon PID path.
pub fn default_pid_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".hebbs").join("daemon.pid"))
}

/// Options for starting/connecting to the daemon.
pub struct DaemonStartOpts {
    /// Panel HTTP port override.
    pub panel_port: Option<u16>,
    /// Initial vault path for the panel to open on startup.
    pub initial_vault: Option<PathBuf>,
}

/// Ensure the daemon is running, starting it if necessary.
/// Returns a connected client.
pub async fn ensure_daemon_with_opts(panel_port: Option<u16>) -> Result<DaemonClient, String> {
    ensure_daemon_full(DaemonStartOpts {
        panel_port,
        initial_vault: None,
    })
    .await
}

/// Ensure the daemon is running with full options (panel port + initial vault).
pub async fn ensure_daemon_full(opts: DaemonStartOpts) -> Result<DaemonClient, String> {
    let socket_path = default_socket_path().ok_or("cannot determine home directory")?;

    // Try to connect directly
    if let Ok(mut client) = DaemonClient::connect(&socket_path).await {
        // Health ping
        let ping = DaemonRequest {
            command: Command::Ping,
            vault_path: None,
            vault_paths: None,
            caller: "cli".to_string(),
        };
        match client.send(&ping).await {
            Ok(resp) if resp.status == ResponseStatus::Ok => {
                debug!("daemon already running, connected");
                // If an initial vault was requested and the daemon is already running,
                // switch the panel to that vault via HTTP POST so the user sees the right one.
                if let Some(ref vault) = opts.initial_vault {
                    if let Some(port) = opts.panel_port {
                        switch_panel_vault(port, vault).await;
                    }
                }
                // Need a fresh connection since we consumed the stream for ping
                return DaemonClient::connect(&socket_path).await;
            }
            _ => {
                debug!("daemon ping failed, will restart");
            }
        }
    }

    // Daemon not running, start it
    start_daemon(&opts)?;

    // Poll for socket with backoff, then continue at last interval until timeout
    let deadline = tokio::time::Instant::now() + DAEMON_START_TIMEOUT;
    let mut step_idx = 0;

    loop {
        let delay = BACKOFF_STEPS[step_idx.min(BACKOFF_STEPS.len() - 1)];
        tokio::time::sleep(delay).await;

        if let Ok(client) = DaemonClient::connect(&socket_path).await {
            info!("daemon started, connected");
            return Ok(client);
        }

        if tokio::time::Instant::now() >= deadline {
            break;
        }

        if step_idx < BACKOFF_STEPS.len() - 1 {
            step_idx += 1;
        }
    }

    Err(format!(
        "daemon failed to start within {}ms",
        DAEMON_START_TIMEOUT.as_millis()
    ))
}

/// Ensure the daemon is running with default options.
pub async fn ensure_daemon() -> Result<DaemonClient, String> {
    ensure_daemon_with_opts(None).await
}

/// Start the daemon as a background process.
fn start_daemon(opts: &DaemonStartOpts) -> Result<(), String> {
    // Clean up stale PID file
    if let Some(pid_path) = default_pid_path() {
        if pid_path.exists() {
            let pid_str = std::fs::read_to_string(&pid_path).unwrap_or_default();
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                #[cfg(unix)]
                {
                    let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
                    if !alive {
                        debug!("removing stale PID file (PID {} not alive)", pid);
                        std::fs::remove_file(&pid_path).ok();
                        // Also clean stale socket file
                        if let Some(sock_path) = default_socket_path() {
                            if sock_path.exists() {
                                debug!("removing stale socket file");
                                std::fs::remove_file(&sock_path).ok();
                            }
                        }
                    } else {
                        // PID is alive -- check if it's actually a hebbs process
                        // to avoid false positives from PID reuse
                        #[cfg(target_os = "macos")]
                        {
                            let output = std::process::Command::new("ps")
                                .args(["-p", &pid.to_string(), "-o", "comm="])
                                .output();
                            if let Ok(out) = output {
                                let comm = String::from_utf8_lossy(&out.stdout);
                                if !comm.contains("hebbs") {
                                    debug!(
                                        "PID {} is alive but not hebbs ({}), cleaning stale files",
                                        pid,
                                        comm.trim()
                                    );
                                    std::fs::remove_file(&pid_path).ok();
                                    if let Some(sock_path) = default_socket_path() {
                                        std::fs::remove_file(&sock_path).ok();
                                    }
                                }
                            }
                        }
                        #[cfg(target_os = "linux")]
                        {
                            let cmdline = std::fs::read_to_string(format!("/proc/{}/cmdline", pid))
                                .unwrap_or_default();
                            if !cmdline.contains("hebbs") {
                                debug!("PID {} is alive but not hebbs, cleaning stale files", pid);
                                std::fs::remove_file(&pid_path).ok();
                                if let Some(sock_path) = default_socket_path() {
                                    std::fs::remove_file(&sock_path).ok();
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Get the path to the current executable
    let exe = std::env::current_exe()
        .map_err(|e| format!("failed to determine executable path: {}", e))?;

    info!("starting daemon: {} serve --foreground", exe.display());

    // Spawn the daemon as a detached background process
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;

        // Create log file for daemon output
        let log_path = dirs::home_dir()
            .map(|h| h.join(".hebbs").join("daemon.log"))
            .ok_or("cannot determine home directory")?;

        // Ensure ~/.hebbs/ exists
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("failed to open daemon log: {}", e))?;

        let log_err = log_file
            .try_clone()
            .map_err(|e| format!("failed to clone log handle: {}", e))?;

        let mut cmd = StdCommand::new(&exe);
        cmd.arg("serve").arg("--foreground");
        if let Some(port) = opts.panel_port {
            cmd.arg("--panel-port").arg(port.to_string());
        }
        if let Some(ref vault) = opts.initial_vault {
            cmd.arg("--initial-vault").arg(vault);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_err));

        // Create a new process group so the daemon outlives the CLI
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }

        cmd.spawn()
            .map_err(|e| format!("failed to spawn daemon: {}", e))?;
    }

    #[cfg(not(unix))]
    {
        let mut cmd = StdCommand::new(&exe);
        cmd.arg("serve").arg("--foreground");
        if let Some(port) = opts.panel_port {
            cmd.arg("--panel-port").arg(port.to_string());
        }
        if let Some(ref vault) = opts.initial_vault {
            cmd.arg("--initial-vault").arg(vault);
        }
        cmd.spawn()
            .map_err(|e| format!("failed to spawn daemon: {}", e))?;
    }

    Ok(())
}

/// Send a vault switch request to the panel HTTP server.
///
/// Fire-and-forget: errors are silently ignored since the panel may not be
/// enabled or the HTTP server may not be up yet when called from panel startup.
async fn switch_panel_vault(port: u16, vault: &std::path::Path) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let body = format!(r#"{{"path":"{}"}}"#, vault.display());
    let req = format!(
        "POST /api/panel/vaults/switch HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        port,
        body.len(),
        body
    );
    // Retry briefly: the panel HTTP server may still be binding.
    for attempt in 0..5u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        if let Ok(mut stream) = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await
        {
            if stream.write_all(req.as_bytes()).await.is_ok() {
                let mut buf = [0u8; 256];
                let _ = stream.read(&mut buf).await;
                return;
            }
        }
    }
}

/// Check if the daemon is running by attempting a connection and ping.
pub async fn is_daemon_running() -> bool {
    let socket_path = match default_socket_path() {
        Some(p) => p,
        None => return false,
    };

    if let Ok(mut client) = DaemonClient::connect(&socket_path).await {
        let ping = DaemonRequest {
            command: Command::Ping,
            vault_path: None,
            vault_paths: None,
            caller: "cli".to_string(),
        };
        if let Ok(resp) = client.send(&ping).await {
            return resp.status == ResponseStatus::Ok;
        }
    }

    false
}
