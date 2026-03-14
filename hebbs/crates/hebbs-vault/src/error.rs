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
