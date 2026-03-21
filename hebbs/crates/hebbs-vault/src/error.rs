use std::path::PathBuf;

/// Errors produced by the vault layer.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error in {path}: {reason}")]
    Parse { path: PathBuf, reason: String },

    #[error("manifest error: {reason}")]
    Manifest { reason: String },

    #[error("config error: {reason}")]
    Config { reason: String },

    #[error("engine error: {0}")]
    Engine(#[from] hebbs_core::error::HebbsError),

    #[error("embed error: {0}")]
    Embed(#[from] hebbs_embed::EmbedError),

    #[error("vault not initialized at {path}: run `hebbs init` first")]
    NotInitialized { path: PathBuf },

    #[error("vault already initialized at {path}: use --force to reinitialize")]
    AlreadyInitialized { path: PathBuf },

    #[error("invalid vault path: {reason}")]
    InvalidPath { reason: String },

    #[error("watcher error: {reason}")]
    Watcher { reason: String },

    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("toml parse error: {0}")]
    TomlDeserialize(#[from] toml::de::Error),

    #[error("toml serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

pub type Result<T> = std::result::Result<T, VaultError>;

/// Convert a raw Rust error string into a human-readable sentence.
///
/// Strips common Rust type prefixes and pattern-matches well-known error
/// messages to friendly alternatives that a non-technical user can act on.
pub fn humanize_error(err: &str) -> String {
    // Strip common Rust error type prefixes
    let cleaned = err
        .replace("ureq::Error::Transport(", "")
        .replace("std::io::Error: ", "")
        .replace("serde_json::Error: ", "")
        .replace("anyhow::Error: ", "");
    let cleaned = cleaned.trim_end_matches(')');

    // Pattern-match common errors to human-readable messages
    if cleaned.contains("Connection refused") && cleaned.contains("11434") {
        return "Could not reach Ollama at localhost:11434. Is it running? Start it with: ollama serve".to_string();
    }
    if cleaned.contains("Connection refused") {
        return format!("Could not connect to the server. {}", cleaned);
    }
    if cleaned.contains("No such file or directory") {
        return format!("File not found. {}", cleaned);
    }
    if cleaned.contains("Permission denied") {
        return format!("Permission denied. Check file permissions. {}", cleaned);
    }
    if cleaned.contains("ROCKSDB") || cleaned.contains("rocksdb") {
        return format!(
            "Database error. Try running `hebbs index` to rebuild. {}",
            cleaned
        );
    }

    cleaned.to_string()
}

/// Convert an error string + status code into an actionable user message.
///
/// Uses the status code for structured dispatch, falls back to `humanize_error`
/// for unknown codes. Every message follows: what happened, why, what to do.
pub fn humanize_error_with_code(err: &str, code: &str) -> String {
    match code {
        "VAULT_NOT_INITIALIZED" => {
            "Vault not initialized. Run `hebbs init <path>` to set up your vault.".to_string()
        }
        "ERR_LLM_REQUIRED" => {
            "LLM provider not configured. Run `hebbs init` with --provider and --model, or `hebbs config set llm.provider <provider>`.".to_string()
        }
        "ERR_LLM_AUTH" => {
            // Extract HTTP status if present
            if err.contains("401") {
                "LLM authentication failed (HTTP 401). Check your API key is valid and has not expired.".to_string()
            } else {
                format!("LLM authentication failed. Check your API key. Detail: {}", humanize_error(err))
            }
        }
        "ERR_LLM_RATE_LIMITED" => {
            if err.contains("429") {
                "LLM rate limited (HTTP 429). Wait a moment and retry, or upgrade your API plan.".to_string()
            } else {
                format!("LLM rate limited. Wait a moment and retry. Detail: {}", humanize_error(err))
            }
        }
        "ERR_LLM_TIMEOUT" => {
            "LLM request timed out. The provider may be overloaded. Retry or try a different model.".to_string()
        }
        "ERR_MANIFEST_CORRUPT" => {
            "Manifest is corrupt or unreadable. Run `hebbs rebuild` to recover from source files.".to_string()
        }
        "ERR_ENGINE_UNAVAILABLE" => {
            if err.contains("Resource temporarily unavailable") || err.contains("LOCK") {
                "Database locked by another process. Run `hebbs stop` first, then retry.".to_string()
            } else {
                format!("Database unavailable. Run `hebbs stop` and retry. Detail: {}", humanize_error(err))
            }
        }
        "INDEXING_IN_PROGRESS" => {
            "Indexing already in progress. Run `hebbs status` to check progress.".to_string()
        }
        _ => {
            // Fall back to pattern-matching for unknown codes
            format!("Error: {}", humanize_error(err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ureq_transport_prefix() {
        let input = "ureq::Error::Transport(Connection refused (os error 61))";
        let result = humanize_error(input);
        assert!(result.starts_with("Could not connect to the server."));
        assert!(!result.contains("ureq::Error"));
    }

    #[test]
    fn ollama_connection_refused() {
        let input = "ureq::Error::Transport(Connection refused on 127.0.0.1:11434)";
        let result = humanize_error(input);
        assert_eq!(
            result,
            "Could not reach Ollama at localhost:11434. Is it running? Start it with: ollama serve"
        );
    }

    #[test]
    fn generic_connection_refused() {
        let input = "Connection refused (os error 61)";
        let result = humanize_error(input);
        assert!(result.starts_with("Could not connect to the server."));
    }

    #[test]
    fn strips_std_io_error_prefix() {
        let input = "std::io::Error: No such file or directory (os error 2)";
        let result = humanize_error(input);
        assert!(result.starts_with("File not found."));
        assert!(!result.contains("std::io::Error"));
    }

    #[test]
    fn permission_denied() {
        let input = "std::io::Error: Permission denied (os error 13)";
        let result = humanize_error(input);
        assert!(result.starts_with("Permission denied. Check file permissions."));
    }

    #[test]
    fn rocksdb_error() {
        let input = "rocksdb error: corruption in column family";
        let result = humanize_error(input);
        assert!(result.starts_with("Database error. Try running `hebbs index` to rebuild."));
    }

    #[test]
    fn rocksdb_uppercase() {
        let input = "ROCKSDB: lock timeout";
        let result = humanize_error(input);
        assert!(result.starts_with("Database error."));
    }

    #[test]
    fn strips_serde_json_prefix() {
        let input = "serde_json::Error: expected value at line 1 column 1";
        let result = humanize_error(input);
        assert!(!result.contains("serde_json::Error"));
        assert!(result.contains("expected value"));
    }

    #[test]
    fn strips_anyhow_prefix() {
        let input = "anyhow::Error: something went wrong";
        let result = humanize_error(input);
        assert!(!result.contains("anyhow::Error"));
        assert!(result.contains("something went wrong"));
    }

    #[test]
    fn passthrough_unknown_error() {
        let input = "something unexpected happened";
        let result = humanize_error(input);
        assert_eq!(result, "something unexpected happened");
    }

    // --- humanize_error_with_code tests ---

    #[test]
    fn code_vault_not_initialized() {
        let result = humanize_error_with_code("not initialized at /foo", "VAULT_NOT_INITIALIZED");
        assert!(result.contains("hebbs init"));
    }

    #[test]
    fn code_llm_auth_401() {
        let result = humanize_error_with_code("HTTP 401 Unauthorized", "ERR_LLM_AUTH");
        assert!(result.contains("401"));
        assert!(result.contains("API key"));
    }

    #[test]
    fn code_llm_rate_limited() {
        let result = humanize_error_with_code("HTTP 429 Too Many Requests", "ERR_LLM_RATE_LIMITED");
        assert!(result.contains("rate limited"));
        assert!(result.contains("retry"));
    }

    #[test]
    fn code_llm_timeout() {
        let result = humanize_error_with_code("request timed out", "ERR_LLM_TIMEOUT");
        assert!(result.contains("timed out"));
    }

    #[test]
    fn code_manifest_corrupt() {
        let result = humanize_error_with_code("manifest parse error", "ERR_MANIFEST_CORRUPT");
        assert!(result.contains("hebbs rebuild"));
    }

    #[test]
    fn code_engine_locked() {
        let result = humanize_error_with_code(
            "Resource temporarily unavailable LOCK",
            "ERR_ENGINE_UNAVAILABLE",
        );
        assert!(result.contains("hebbs stop"));
    }

    #[test]
    fn code_indexing_in_progress() {
        let result =
            humanize_error_with_code("indexing already in progress", "INDEXING_IN_PROGRESS");
        assert!(result.contains("hebbs status"));
    }

    #[test]
    fn code_unknown_falls_back() {
        let result = humanize_error_with_code("something weird", "ERR_UNKNOWN");
        assert!(result.contains("something weird"));
    }
}
