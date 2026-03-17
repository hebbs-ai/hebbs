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

    /// Send a request and receive the final response.
    ///
    /// Any `Progress` responses received before the final response are printed
    /// to stderr so the user sees live status updates.
    pub async fn send(&mut self, request: &DaemonRequest) -> Result<DaemonResponse, String> {
        let (mut reader, mut writer) = self.stream.split();

        write_message(&mut writer, request)
            .await
            .map_err(|e| format!("failed to send request: {}", e))?;

        loop {
            let response: DaemonResponse = read_message(&mut reader)
                .await
                .map_err(|e| format!("failed to read response: {}", e))?
                .ok_or_else(|| "daemon closed connection unexpectedly".to_string())?;

            if response.status == ResponseStatus::Progress {
                if let Some(msg) = response.data
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

/// Ensure the daemon is running, starting it if necessary.
/// Returns a connected client.
///
/// `panel_port` is passed to the daemon on auto-start so the panel binds to the
/// caller's requested port. Pass `None` for the default port.
pub async fn ensure_daemon_with_opts(panel_port: Option<u16>) -> Result<DaemonClient, String> {
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
                // Need a fresh connection since we consumed the stream for ping
                return DaemonClient::connect(&socket_path).await;
            }
            _ => {
                debug!("daemon ping failed, will restart");
            }
        }
    }

    // Daemon not running, start it
    start_daemon(panel_port)?;

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
///
/// If `panel_port` is `Some(port)`, passes `--panel-port <port>` to the daemon.
fn start_daemon(panel_port: Option<u16>) -> Result<(), String> {
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
        if let Some(port) = panel_port {
            cmd.arg("--panel-port").arg(port.to_string());
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
        if let Some(port) = panel_port {
            cmd.arg("--panel-port").arg(port.to_string());
        }
        cmd.spawn()
            .map_err(|e| format!("failed to spawn daemon: {}", e))?;
    }

    Ok(())
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
