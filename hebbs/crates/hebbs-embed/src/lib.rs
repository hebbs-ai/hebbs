pub mod api;
pub mod config;
pub mod error;
pub mod mock;
#[cfg(feature = "local-embed")]
pub mod model;
pub mod normalize;
#[cfg(feature = "local-embed")]
pub mod onnx;
pub mod traits;

pub use api::{ApiEmbedder, ApiEmbedderConfig};
pub use config::{EmbedderConfig, ModelConfig, PoolingStrategy};
pub use error::{EmbedError, Result};
pub use mock::MockEmbedder;
#[cfg(feature = "local-embed")]
pub use model::ensure_model_files;
#[cfg(feature = "local-embed")]
pub use onnx::OnnxEmbedder;
pub use traits::Embedder;
