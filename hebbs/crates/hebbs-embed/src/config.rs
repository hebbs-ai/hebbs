use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Pooling strategy for converting token-level outputs to sentence embeddings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolingStrategy {
    /// Average all non-padding token embeddings.
    /// BGE-small-en-v1.5 uses this.
    #[default]
    Mean,
    /// Use the \[CLS\] token embedding (index 0).
    Cls,
    /// Model outputs pre-pooled sentence embeddings (2D output).
    /// EmbeddingGemma-300M uses this.
    None,
}

/// Model configuration metadata.
///
/// Stored alongside the ONNX model as `config.json` to configure
/// tokenization and post-processing without hardcoding model-specific logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_name: String,
    /// Output embedding dimensions (e.g. 384 for BGE-small-en-v1.5).
    pub dimensions: usize,
    /// Maximum input sequence length in tokens. Longer inputs are truncated.
    pub max_seq_length: usize,
    /// How to pool token-level outputs into a sentence embedding.
    pub pooling_strategy: PoolingStrategy,
    /// Whether the model accepts `token_type_ids` as input.
    /// BERT-family models use this; Gemma-family models do not.
    #[serde(default = "default_true_serde")]
    pub uses_token_type_ids: bool,
    /// Whether the ONNX model has an external data file (`model.onnx_data`).
    #[serde(default)]
    pub has_external_data: bool,
}

fn default_true_serde() -> bool {
    true
}

impl ModelConfig {
    /// Default configuration for BGE-small-en-v1.5.
    pub fn bge_small_en_v1_5() -> Self {
        Self {
            model_name: "bge-small-en-v1.5".to_string(),
            dimensions: 384,
            max_seq_length: 512,
            pooling_strategy: PoolingStrategy::Mean,
            uses_token_type_ids: true,
            has_external_data: false,
        }
    }

    /// Configuration for EmbeddingGemma-300M (768 dims, 2048 seq len).
    pub fn embeddinggemma_300m() -> Self {
        Self {
            model_name: "embeddinggemma-300m".to_string(),
            dimensions: 768,
            max_seq_length: 2048,
            pooling_strategy: PoolingStrategy::None,
            uses_token_type_ids: false,
            has_external_data: true,
        }
    }
}

/// Full configuration for the embedding engine.
#[derive(Debug, Clone)]
pub struct EmbedderConfig {
    /// Directory to store model files.
    pub model_dir: PathBuf,
    /// Model configuration metadata.
    pub model_config: ModelConfig,
    /// Base URL for model downloads.
    pub download_base_url: String,
    /// Whether to auto-download missing model files.
    pub auto_download: bool,
}

impl EmbedderConfig {
    /// Create a default configuration for BGE-small-en-v1.5.
    ///
    /// Model files are stored under `{data_dir}/models/bge-small-en-v1.5/`.
    pub fn default_bge_small(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            model_dir: data_dir.join("models").join("bge-small-en-v1.5"),
            model_config: ModelConfig::bge_small_en_v1_5(),
            download_base_url: "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main"
                .to_string(),
            auto_download: true,
        }
    }

    /// Create a default configuration for EmbeddingGemma-300M.
    ///
    /// Model files are stored under `{data_dir}/models/embeddinggemma-300m/`.
    pub fn default_embeddinggemma(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            model_dir: data_dir.join("models").join("embeddinggemma-300m"),
            model_config: ModelConfig::embeddinggemma_300m(),
            download_base_url:
                "https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX/resolve/main"
                    .to_string(),
            auto_download: true,
        }
    }

    /// Create an embedder config from a model name string.
    ///
    /// Recognized names: "bge-small-en-v1.5", "embeddinggemma-300m".
    /// Falls back to BGE-small for unrecognized names.
    pub fn from_model_name(model_name: &str, data_dir: impl Into<PathBuf>) -> Self {
        match model_name {
            "embeddinggemma-300m" | "embeddinggemma" | "gemma-embed" => {
                Self::default_embeddinggemma(data_dir)
            }
            _ => Self::default_bge_small(data_dir),
        }
    }

    /// Create an embedder config using the OS-level cache directory for model storage.
    ///
    /// Models are stored at:
    ///   macOS:  ~/Library/Caches/hebbs/models/<name>/
    ///   Linux:  ~/.cache/hebbs/models/<name>/
    ///
    /// This directory is separate from the daemon runtime (~/.hebbs/) so that
    /// removing or recreating vaults never triggers a re-download.
    pub fn from_model_name_cached(model_name: &str) -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join(".cache")
            })
            .join("hebbs");
        Self::from_model_name(model_name, cache_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bge_small_defaults() {
        let cfg = ModelConfig::bge_small_en_v1_5();
        assert_eq!(cfg.dimensions, 384);
        assert_eq!(cfg.max_seq_length, 512);
        assert_eq!(cfg.pooling_strategy, PoolingStrategy::Mean);
    }

    #[test]
    fn config_json_roundtrip() {
        let cfg = ModelConfig::bge_small_en_v1_5();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let restored: ModelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg.dimensions, restored.dimensions);
        assert_eq!(cfg.max_seq_length, restored.max_seq_length);
        assert_eq!(cfg.pooling_strategy, restored.pooling_strategy);
    }

    #[test]
    fn embedder_config_default_paths() {
        let cfg = EmbedderConfig::default_bge_small("/tmp/hebbs");
        assert!(cfg.model_dir.ends_with("models/bge-small-en-v1.5"));
        assert!(cfg.auto_download);
    }

    #[test]
    fn embeddinggemma_defaults() {
        let cfg = ModelConfig::embeddinggemma_300m();
        assert_eq!(cfg.dimensions, 768);
        assert_eq!(cfg.max_seq_length, 2048);
        assert_eq!(cfg.pooling_strategy, PoolingStrategy::None);
        assert!(!cfg.uses_token_type_ids);
        assert!(cfg.has_external_data);
    }

    #[test]
    fn embedder_config_embeddinggemma_paths() {
        let cfg = EmbedderConfig::default_embeddinggemma("/tmp/hebbs");
        assert!(cfg.model_dir.ends_with("models/embeddinggemma-300m"));
        assert!(cfg.auto_download);
    }

    #[test]
    fn from_model_name_routing() {
        let bge = EmbedderConfig::from_model_name("bge-small-en-v1.5", "/tmp");
        assert_eq!(bge.model_config.model_name, "bge-small-en-v1.5");

        let gemma = EmbedderConfig::from_model_name("embeddinggemma-300m", "/tmp");
        assert_eq!(gemma.model_config.model_name, "embeddinggemma-300m");

        let fallback = EmbedderConfig::from_model_name("unknown", "/tmp");
        assert_eq!(fallback.model_config.model_name, "bge-small-en-v1.5");
    }
}
