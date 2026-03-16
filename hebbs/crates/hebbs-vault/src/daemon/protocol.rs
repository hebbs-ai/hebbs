//! Length-prefixed JSON protocol for daemon IPC over Unix domain socket.
//!
//! Wire format: `[4-byte big-endian length][JSON payload]`
//!
//! Maximum message size: 16 MiB (prevents unbounded allocation from
//! malformed length prefixes).

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Hard ceiling on a single message to prevent OOM from corrupt length prefixes.
const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024; // 16 MiB

// ── Request types ────────────────────────────────────────────────────

/// Top-level request envelope sent by the CLI client.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonRequest {
    pub command: Command,
    /// Absolute path to the vault root (contains `.hebbs/`).
    /// `None` for commands that do not target a vault (ping, shutdown).
    pub vault_path: Option<PathBuf>,
    /// Additional vault paths for multi-vault queries (`--all` flag).
    /// When set, the daemon queries both `vault_path` and these vaults,
    /// merges results by score, and returns top-k.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_paths: Option<Vec<PathBuf>>,
    /// Caller identity for query audit log (e.g. "cli", "hebbs-panel", "mcp:cursor").
    /// Defaults to "cli" if not specified.
    #[serde(default = "default_caller")]
    pub caller: String,
}

fn default_caller() -> String {
    "cli".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Ping,
    Shutdown,
    Remember {
        content: String,
        #[serde(default)]
        importance: Option<f32>,
        #[serde(default)]
        context: Option<String>,
        #[serde(default)]
        entity_id: Option<String>,
        #[serde(default)]
        edges: Vec<EdgeSpec>,
    },
    Get {
        id: String,
    },
    Recall {
        #[serde(default)]
        cue: Option<String>,
        #[serde(default)]
        strategy: Option<String>,
        #[serde(default = "default_top_k")]
        top_k: u32,
        #[serde(default)]
        entity_id: Option<String>,
        #[serde(default)]
        max_depth: Option<u32>,
        #[serde(default)]
        seed: Option<String>,
        #[serde(default)]
        weights: Option<String>,
        #[serde(default)]
        ef_search: Option<u32>,
        #[serde(default)]
        edge_types: Option<Vec<String>>,
        #[serde(default)]
        time_range: Option<String>,
        #[serde(default)]
        analogical_alpha: Option<f32>,
        #[serde(default)]
        context: Option<String>,
    },
    Forget {
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        entity_id: Option<String>,
        #[serde(default)]
        staleness_us: Option<u64>,
        #[serde(default)]
        access_floor: Option<u64>,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        decay_floor: Option<f32>,
    },
    Prime {
        entity_id: String,
        #[serde(default)]
        context: Option<String>,
        #[serde(default)]
        max_memories: Option<u32>,
        #[serde(default)]
        recency_us: Option<u64>,
        #[serde(default)]
        similarity_cue: Option<String>,
    },
    Inspect {
        id: String,
    },
    Export {
        #[serde(default)]
        entity_id: Option<String>,
        #[serde(default = "default_export_limit")]
        limit: u32,
    },
    Status,
    Index,
    List {
        #[serde(default)]
        sections: bool,
    },
    ReflectPrepare {
        #[serde(default)]
        entity_id: Option<String>,
        #[serde(default)]
        since_us: Option<u64>,
    },
    ReflectCommit {
        session_id: String,
        insights: String,
    },
    ContradictionPrepare {},
    ContradictionCommit {
        results: String,
    },
    Insights {
        #[serde(default)]
        entity_id: Option<String>,
        #[serde(default)]
        min_confidence: Option<f32>,
        #[serde(default)]
        max_results: Option<u32>,
    },
    Queries {
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        offset: Option<u32>,
        #[serde(default)]
        caller_filter: Option<String>,
        #[serde(default)]
        operation_filter: Option<String>,
    },
}

fn default_top_k() -> u32 {
    10
}
fn default_export_limit() -> u32 {
    1000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeSpec {
    pub target_id: String,
    pub edge_type: String,
    #[serde(default)]
    pub confidence: Option<f32>,
}

// ── Response types ───────────────────────────────────────────────────

/// Top-level response envelope returned by the daemon.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonResponse {
    pub status: ResponseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Ok,
    Error,
}

impl DaemonResponse {
    pub fn ok(data: serde_json::Value) -> Self {
        Self {
            status: ResponseStatus::Ok,
            data: Some(data),
            error: None,
        }
    }

    pub fn ok_empty() -> Self {
        Self {
            status: ResponseStatus::Ok,
            data: None,
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            status: ResponseStatus::Error,
            data: None,
            error: Some(msg.into()),
        }
    }
}

// ── Wire helpers ─────────────────────────────────────────────────────

/// Write a length-prefixed JSON message to an async writer.
///
/// O(1) allocations beyond the JSON serialization itself.
pub async fn write_message<W, T>(writer: &mut W, msg: &T) -> io::Result<()>
where
    W: AsyncWriteExt + Unpin,
    T: Serialize,
{
    let payload =
        serde_json::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let len = payload.len() as u32;
    if len > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "message too large: {} bytes (max {})",
                len, MAX_MESSAGE_SIZE
            ),
        ));
    }
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

/// Read a length-prefixed JSON message from an async reader.
///
/// Returns `None` on clean EOF (peer closed connection).
pub async fn read_message<R, T>(reader: &mut R) -> io::Result<Option<T>>
where
    R: AsyncReadExt + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    let len = u32::from_be_bytes(len_buf);
    if len > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "message length {} exceeds maximum {}",
                len, MAX_MESSAGE_SIZE
            ),
        ));
    }

    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload).await?;

    serde_json::from_slice(&payload)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Blocking (sync) versions for use outside tokio context.
pub mod sync {
    use super::*;
    use std::io::{Read, Write};

    pub fn write_message_sync<W, T>(writer: &mut W, msg: &T) -> io::Result<()>
    where
        W: Write,
        T: Serialize,
    {
        let payload =
            serde_json::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let len = payload.len() as u32;
        if len > MAX_MESSAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "message too large: {} bytes (max {})",
                    len, MAX_MESSAGE_SIZE
                ),
            ));
        }
        writer.write_all(&len.to_be_bytes())?;
        writer.write_all(&payload)?;
        writer.flush()?;
        Ok(())
    }

    pub fn read_message_sync<R, T>(reader: &mut R) -> io::Result<Option<T>>
    where
        R: Read,
        T: for<'de> Deserialize<'de>,
    {
        let mut len_buf = [0u8; 4];
        match reader.read_exact(&mut len_buf) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }

        let len = u32::from_be_bytes(len_buf);
        if len > MAX_MESSAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "message length {} exceeds maximum {}",
                    len, MAX_MESSAGE_SIZE
                ),
            ));
        }

        let mut payload = vec![0u8; len as usize];
        reader.read_exact(&mut payload)?;

        serde_json::from_slice(&payload)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_roundtrip_ping() {
        let req = DaemonRequest {
            command: Command::Ping,
            vault_path: None,
            vault_paths: None,
            caller: "test".to_string(),
        };

        let mut buf = Vec::new();
        write_message(&mut buf, &req).await.unwrap();

        let mut cursor = io::Cursor::new(buf);
        let decoded: DaemonRequest = read_message(&mut cursor).await.unwrap().unwrap();
        assert!(matches!(decoded.command, Command::Ping));
    }

    #[tokio::test]
    async fn test_roundtrip_response() {
        let resp = DaemonResponse::ok(serde_json::json!({"pong": true}));

        let mut buf = Vec::new();
        write_message(&mut buf, &resp).await.unwrap();

        let mut cursor = io::Cursor::new(buf);
        let decoded: DaemonResponse = read_message(&mut cursor).await.unwrap().unwrap();
        assert_eq!(decoded.status, ResponseStatus::Ok);
        assert_eq!(decoded.data.unwrap()["pong"], true);
    }

    #[tokio::test]
    async fn test_eof_returns_none() {
        let mut cursor = io::Cursor::new(Vec::<u8>::new());
        let result: Option<DaemonRequest> = read_message(&mut cursor).await.unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_sync_roundtrip() {
        let req = DaemonRequest {
            command: Command::Ping,
            vault_path: None,
            vault_paths: None,
            caller: "test".to_string(),
        };

        let mut buf = Vec::new();
        sync::write_message_sync(&mut buf, &req).unwrap();

        let mut cursor = io::Cursor::new(buf);
        let decoded: DaemonRequest = sync::read_message_sync(&mut cursor).unwrap().unwrap();
        assert!(matches!(decoded.command, Command::Ping));
    }
}
