//! REST transport for remote HEBBS servers (enterprise mode).
//!
//! Handles all commands that work over HTTP to the platform API.
//! Used when the CLI detects a remote endpoint (port 8080/443).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::cli::{Commands, KeyCommands, WorkspaceCommands};
use crate::config::{CliConfig, OutputFormat};
use crate::error::CliError;

/// REST client wrapping reqwest.
#[cfg(feature = "rest")]
pub struct RestClient {
    client: reqwest::Client,
    endpoint: String,
    api_key: Option<String>,
    timeout_ms: u64,
}

#[cfg(feature = "rest")]
impl RestClient {
    pub fn new(endpoint: &str, api_key: Option<String>, timeout_ms: u64) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            api_key,
            timeout_ms,
        }
    }

    async fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .request(method, format!("{}{}", self.endpoint, path))
            .timeout(std::time::Duration::from_millis(self.timeout_ms));

        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        req
    }

    async fn get(&self, path: &str) -> Result<Value, CliError> {
        let resp = self
            .request(reqwest::Method::GET, path)
            .await
            .send()
            .await
            .map_err(|e| CliError::ConnectionFailed {
                endpoint: self.endpoint.clone(),
                source: e.to_string(),
            })?;

        self.handle_response(resp).await
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value, CliError> {
        let resp = self
            .request(reqwest::Method::POST, path)
            .await
            .json(&body)
            .send()
            .await
            .map_err(|e| CliError::ConnectionFailed {
                endpoint: self.endpoint.clone(),
                source: e.to_string(),
            })?;

        self.handle_response(resp).await
    }

    async fn delete(&self, path: &str) -> Result<Value, CliError> {
        let resp = self
            .request(reqwest::Method::DELETE, path)
            .await
            .send()
            .await
            .map_err(|e| CliError::ConnectionFailed {
                endpoint: self.endpoint.clone(),
                source: e.to_string(),
            })?;

        self.handle_response(resp).await
    }

    async fn upload(&self, path: &str, files: Vec<(String, Vec<u8>)>) -> Result<Value, CliError> {
        let mut form = reqwest::multipart::Form::new();
        for (name, content) in files {
            let part = reqwest::multipart::Part::bytes(content).file_name(name);
            form = form.part("files", part);
        }

        let mut req = self
            .client
            .post(format!("{}{}", self.endpoint, path))
            .timeout(std::time::Duration::from_millis(self.timeout_ms))
            .multipart(form);

        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let resp = req.send().await.map_err(|e| CliError::ConnectionFailed {
            endpoint: self.endpoint.clone(),
            source: e.to_string(),
        })?;

        self.handle_response(resp).await
    }

    async fn handle_response(&self, resp: reqwest::Response) -> Result<Value, CliError> {
        let status = resp.status();
        let body: Value = resp.json().await.map_err(|e| CliError::Internal {
            message: format!("failed to parse response: {}", e),
        })?;

        if status.is_success() {
            return Ok(body);
        }

        let error_msg = body
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown error")
            .to_string();

        match status.as_u16() {
            401 => Err(CliError::ConnectionFailed {
                endpoint: self.endpoint.clone(),
                source: format!("Authentication failed: {}", error_msg),
            }),
            403 => Err(CliError::Internal {
                message: format!("Access denied: {}", error_msg),
            }),
            404 => Err(CliError::NotFound { message: error_msg }),
            _ => Err(CliError::ServerError { message: error_msg }),
        }
    }
}

/// Execute a command in REST mode against a remote server.
#[cfg(feature = "rest")]
pub async fn execute_rest(
    cmd: Commands,
    config: &CliConfig,
    api_key: Option<String>,
    output_format: OutputFormat,
) -> i32 {
    let mut w = std::io::stdout();
    let client = RestClient::new(&config.endpoint, api_key, config.timeout_ms);

    let result = match cmd {
        Commands::Login { endpoint, api_key } => {
            exec_login(&endpoint, api_key.as_deref(), &mut w).await
        }
        Commands::Remember {
            content,
            importance,
            entity_id,
            ..
        } => {
            exec_remember(
                &client,
                content,
                importance,
                entity_id,
                output_format,
                &mut w,
            )
            .await
        }
        Commands::Recall {
            cue,
            strategy,
            top_k,
            entity_id,
            ..
        } => {
            let strat = strategy.map(|s| match s {
                crate::cli::StrategyArg::Similarity => "similarity",
                crate::cli::StrategyArg::Temporal => "temporal",
                crate::cli::StrategyArg::Causal => "causal",
                crate::cli::StrategyArg::Analogical => "analogical",
            });
            exec_recall(
                &client,
                cue,
                top_k,
                entity_id,
                strat.map(String::from),
                output_format,
                &mut w,
            )
            .await
        }
        Commands::Prime {
            entity_id,
            max_memories,
            ..
        } => exec_prime(&client, entity_id, max_memories, output_format, &mut w).await,
        Commands::Forget { ids, entity_id, .. } => {
            exec_forget(&client, ids, entity_id, &mut w).await
        }
        Commands::Status => exec_status(&client, &mut w).await,
        Commands::Push { path } => exec_push(&client, &path, &mut w).await,
        Commands::Workspaces(sub) => exec_workspaces(&client, sub, &mut w).await,
        Commands::Keys(sub) => exec_keys(&client, sub, &mut w).await,
        Commands::Dashboard => exec_dashboard(&config.endpoint, &mut w),
        Commands::Version => {
            writeln!(w, "hebbs {}", env!("CARGO_PKG_VERSION")).ok();
            Ok(())
        }
        Commands::Insights { entity_id, .. } => exec_insights(&client, entity_id, &mut w).await,
        _ => {
            eprintln!("This command requires local mode (gRPC).");
            eprintln!("Install the full binary: brew install hebbs-ai/tap/hebbs");
            return 1;
        }
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e);
            e.exit_code()
        }
    }
}

// ── Command implementations ──────────────────────────────────────────────

#[cfg(feature = "rest")]
async fn exec_login(
    endpoint: &str,
    api_key: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let endpoint = endpoint.trim_end_matches('/');

    // Test connection
    let url = format!("{}/v1/system/health", endpoint);
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| CliError::ConnectionFailed {
            endpoint: endpoint.to_string(),
            source: e.to_string(),
        })?;

    if !resp.status().is_success() {
        return Err(CliError::ConnectionFailed {
            endpoint: endpoint.to_string(),
            source: "Server returned error".to_string(),
        });
    }

    writeln!(w, "Connected to {}", endpoint).ok();

    // Save config
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hebbs");
    std::fs::create_dir_all(&config_dir).ok();

    let mut config_data: HashMap<String, String> = HashMap::new();
    config_data.insert("endpoint".to_string(), endpoint.to_string());
    if let Some(key) = api_key {
        config_data.insert("api_key".to_string(), key.to_string());
    }

    let config_str = serde_json::to_string_pretty(&config_data).unwrap_or_default();
    let config_path = config_dir.join("cli.toml");
    std::fs::write(&config_path, config_str).map_err(|e| CliError::Internal {
        message: format!("Failed to save config: {}", e),
    })?;

    if api_key.is_some() {
        writeln!(w, "API key saved. Ready.").ok();
    } else {
        writeln!(
            w,
            "Endpoint saved. Set API key with --api-key or HEBBS_API_KEY env var."
        )
        .ok();
    }

    Ok(())
}

#[cfg(feature = "rest")]
async fn exec_remember(
    client: &RestClient,
    content: Option<String>,
    importance: Option<f32>,
    entity_id: Option<String>,
    output_format: OutputFormat,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let content = match content {
        Some(c) => c,
        None => {
            // Read from stdin if piped
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| CliError::Internal {
                    message: format!("Failed to read stdin: {}", e),
                })?;
            buf.trim().to_string()
        }
    };

    if content.is_empty() {
        return Err(CliError::InvalidArgument {
            message: "Content is required".to_string(),
        });
    }

    let mut body = serde_json::json!({ "content": content });
    if let Some(imp) = importance {
        body["importance"] = serde_json::json!(imp);
    }
    if let Some(eid) = entity_id {
        body["entity_id"] = serde_json::json!(eid);
    }

    let resp = client.post("/v1/memories", body).await?;

    match output_format {
        OutputFormat::Json => writeln!(
            w,
            "{}",
            serde_json::to_string_pretty(&resp).unwrap_or_default()
        )
        .ok(),
        _ => {
            writeln!(w, "Remembered: {:?}", content).ok();
            if let Some(id) = resp.get("memory_id").and_then(|v| v.as_str()) {
                writeln!(w, "  id: {}", id).ok();
            }
            None
        }
    };

    Ok(())
}

#[cfg(feature = "rest")]
async fn exec_recall(
    client: &RestClient,
    cue: Option<String>,
    top_k: u32,
    entity_id: Option<String>,
    strategy: Option<String>,
    output_format: OutputFormat,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let cue = cue.unwrap_or_default();
    if cue.is_empty() {
        return Err(CliError::InvalidArgument {
            message: "Cue text is required".to_string(),
        });
    }

    let mut body = serde_json::json!({
        "cue": cue,
        "top_k": top_k,
    });
    if let Some(eid) = entity_id {
        body["entity_id"] = serde_json::json!(eid);
    }
    if let Some(strat) = strategy {
        body["strategy"] = serde_json::json!(strat);
    }

    let resp = client.post("/v1/recall", body).await?;

    match output_format {
        OutputFormat::Json => {
            writeln!(
                w,
                "{}",
                serde_json::to_string_pretty(&resp).unwrap_or_default()
            )
            .ok();
        }
        _ => {
            let results = resp.get("results").and_then(|v| v.as_array());
            match results {
                Some(arr) if !arr.is_empty() => {
                    for (i, r) in arr.iter().enumerate() {
                        let score = r.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let content = r.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        let ctx = r.get("context").and_then(|v| v.as_object());
                        writeln!(w, "  {}. [{:.3}] {}", i + 1, score, content).ok();
                        if let Some(ctx) = ctx {
                            if let Some(fp) = ctx.get("file_path").and_then(|v| v.as_str()) {
                                writeln!(w, "     source: {}", fp).ok();
                            }
                            if let Some(layer) = ctx.get("layer").and_then(|v| v.as_str()) {
                                writeln!(w, "     type: {}", layer).ok();
                            }
                        }
                    }
                }
                _ => {
                    writeln!(w, "No results.").ok();
                }
            }
        }
    }

    Ok(())
}

#[cfg(feature = "rest")]
async fn exec_prime(
    client: &RestClient,
    entity_id: String,
    max_memories: Option<u32>,
    output_format: OutputFormat,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let mut body = serde_json::json!({ "entity_id": entity_id });
    if let Some(max) = max_memories {
        body["max_memories"] = serde_json::json!(max);
    }

    let resp = client.post("/v1/prime", body).await?;

    match output_format {
        OutputFormat::Json => {
            writeln!(
                w,
                "{}",
                serde_json::to_string_pretty(&resp).unwrap_or_default()
            )
            .ok();
        }
        _ => {
            let results = resp.get("results").and_then(|v| v.as_array());
            let count = results.map(|a| a.len()).unwrap_or(0);
            writeln!(w, "{} memories for \"{}\":", count, entity_id).ok();
            if let Some(arr) = results {
                for r in arr {
                    let content = r.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    writeln!(w, "  - {}", content).ok();
                }
            }
        }
    }

    Ok(())
}

#[cfg(feature = "rest")]
async fn exec_forget(
    client: &RestClient,
    ids: Vec<String>,
    entity_id: Option<String>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    if ids.is_empty() && entity_id.is_none() {
        return Err(CliError::InvalidArgument {
            message: "Specify --ids or --entity-id".to_string(),
        });
    }

    let mut body = serde_json::json!({});
    if !ids.is_empty() {
        body["ids"] = serde_json::json!(ids);
    }
    if let Some(eid) = entity_id {
        body["entity_id"] = serde_json::json!(eid);
    }

    let resp = client.post("/v1/forget", body).await?;
    let count = resp
        .get("forgotten_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    writeln!(w, "Forgotten: {} memories", count).ok();

    Ok(())
}

#[cfg(feature = "rest")]
async fn exec_status(client: &RestClient, w: &mut dyn Write) -> Result<(), CliError> {
    let resp = client.get("/v1/system/health").await?;

    let status = resp
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let version = resp
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let engine = resp
        .get("engine")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    writeln!(w, "Engine:  {}", engine).ok();
    writeln!(w, "Version: {}", version).ok();
    writeln!(w, "Status:  {}", status).ok();

    Ok(())
}

#[cfg(feature = "rest")]
async fn exec_push(client: &RestClient, dir_path: &str, w: &mut dyn Write) -> Result<(), CliError> {
    let path = Path::new(dir_path);
    if !path.exists() {
        return Err(CliError::InvalidArgument {
            message: format!("Path does not exist: {}", dir_path),
        });
    }

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    fn collect(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                {
                    continue;
                }
                if p.is_dir() {
                    collect(&p, base, out);
                } else if matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("md" | "txt" | "pdf")
                ) {
                    if let Ok(content) = std::fs::read(&p) {
                        let relative = p.strip_prefix(base).unwrap_or(&p);
                        out.push((relative.to_string_lossy().to_string(), content));
                    }
                }
            }
        }
    }

    if path.is_file() {
        let content = std::fs::read(path).map_err(|e| CliError::Internal {
            message: format!("Failed to read file: {}", e),
        })?;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        files.push((name, content));
    } else {
        collect(path, path, &mut files);
    }

    if files.is_empty() {
        writeln!(w, "No files found (.md, .txt, .pdf)").ok();
        return Ok(());
    }

    writeln!(w, "Pushing {} file(s)...", files.len()).ok();
    let resp = client.upload("/v1/upload", files).await?;

    let uploaded = resp.get("uploaded").and_then(|v| v.as_u64()).unwrap_or(0);
    writeln!(w, "Uploaded: {} file(s). Indexing triggered.", uploaded).ok();

    Ok(())
}

#[cfg(feature = "rest")]
async fn exec_workspaces(
    client: &RestClient,
    sub: WorkspaceCommands,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    match sub {
        WorkspaceCommands::List => {
            let resp = client.get("/v1/workspaces").await?;
            let workspaces = resp.get("workspaces").and_then(|v| v.as_array());
            match workspaces {
                Some(arr) => {
                    for ws in arr {
                        let slug = ws.get("slug").and_then(|v| v.as_str()).unwrap_or("?");
                        let stats = ws.get("stats").and_then(|v| v.as_object());
                        let mems = stats
                            .and_then(|s| s.get("memories"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let files = stats
                            .and_then(|s| s.get("files"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        writeln!(w, "  {}  memories:{}  files:{}", slug, mems, files).ok();
                    }
                }
                None => {
                    writeln!(w, "No workspaces.").ok();
                }
            }
        }
        WorkspaceCommands::Create { name } => {
            let resp = client
                .post("/v1/workspaces", serde_json::json!({ "name": name }))
                .await?;
            let slug = resp
                .get("workspace")
                .and_then(|w| w.get("slug"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let key = resp.get("api_key").and_then(|v| v.as_str()).unwrap_or("?");
            writeln!(w, "Created: {}", slug).ok();
            writeln!(w, "API key: {}", key).ok();
        }
        WorkspaceCommands::Switch { slug } => {
            // Save to config
            let config_dir = dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("hebbs");
            std::fs::create_dir_all(&config_dir).ok();
            let ws_path = config_dir.join("workspace");
            std::fs::write(&ws_path, &slug).ok();
            writeln!(w, "Switched to workspace: {}", slug).ok();
        }
    }

    Ok(())
}

#[cfg(feature = "rest")]
async fn exec_keys(
    client: &RestClient,
    sub: KeyCommands,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    match sub {
        KeyCommands::List => {
            let resp = client.get("/v1/keys").await?;
            let keys = resp.get("keys").and_then(|v| v.as_array());
            match keys {
                Some(arr) => {
                    for k in arr {
                        let revoked = k.get("revokedAt").and_then(|v| v.as_str());
                        if revoked.is_some() {
                            continue;
                        }
                        let prefix = k.get("prefix").and_then(|v| v.as_str()).unwrap_or("?");
                        let label = k.get("label").and_then(|v| v.as_str()).unwrap_or("?");
                        let role = k.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                        writeln!(w, "  {}  {}  ({})", prefix, label, role).ok();
                    }
                }
                None => {
                    writeln!(w, "No keys.").ok();
                }
            }
        }
        KeyCommands::Create { label } => {
            let resp = client
                .post("/v1/keys", serde_json::json!({ "label": label }))
                .await?;
            let key = resp.get("api_key").and_then(|v| v.as_str()).unwrap_or("?");
            writeln!(w, "API key: {}", key).ok();
        }
        KeyCommands::Revoke { id } => {
            client.delete(&format!("/v1/keys/{}", id)).await?;
            writeln!(w, "Key revoked.").ok();
        }
    }

    Ok(())
}

#[cfg(feature = "rest")]
async fn exec_insights(
    client: &RestClient,
    entity_id: Option<String>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let path = match entity_id {
        Some(ref eid) => format!("/v1/insights?entity_id={}", eid),
        None => "/v1/insights".to_string(),
    };
    let resp = client.get(&path).await?;
    let count = resp.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
    writeln!(w, "{} insights", count).ok();

    if let Some(arr) = resp.get("insights").and_then(|v| v.as_array()) {
        for ins in arr {
            let content = ins.get("content").and_then(|v| v.as_str()).unwrap_or("?");
            writeln!(w, "  - {}", content).ok();
        }
    }

    Ok(())
}

#[cfg(feature = "rest")]
fn exec_dashboard(endpoint: &str, w: &mut dyn Write) -> Result<(), CliError> {
    writeln!(w, "Open: {}", endpoint).ok();
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(endpoint)
            .spawn()
            .ok();
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(endpoint)
            .spawn()
            .ok();
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("start")
            .arg(endpoint)
            .spawn()
            .ok();
    }
    Ok(())
}

/// Determine if the endpoint is a remote REST server or local gRPC daemon.
pub fn is_rest_endpoint(endpoint: &str) -> bool {
    // Common REST ports: 8080 (enterprise platform), 443 (HTTPS)
    // gRPC port: 6380 (local daemon)
    if let Some(port_str) = endpoint.rsplit(':').next() {
        if let Ok(port) = port_str.trim_matches('/').parse::<u16>() {
            return port != 6380;
        }
    }
    // Default: if it looks like an external host, use REST
    !endpoint.contains("localhost:6380") && !endpoint.contains("127.0.0.1:6380")
}
