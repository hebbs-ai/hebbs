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
        return format!("Database error. Try running `hebbs index` to rebuild. {}", cleaned);
    }

    cleaned.to_string()
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
        assert_eq!(result, "Could not reach Ollama at localhost:11434. Is it running? Start it with: ollama serve");
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
}
