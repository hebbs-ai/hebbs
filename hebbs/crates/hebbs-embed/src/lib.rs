pub mod config;
pub mod error;
pub mod mock;
pub mod model;
pub mod normalize;
pub mod onnx;
pub mod traits;

pub use config::{EmbedderConfig, ModelConfig, PoolingStrategy};
pub use error::{EmbedError, Result};
pub use mock::MockEmbedder;
pub use model::ensure_model_files;
pub use onnx::OnnxEmbedder;
pub use traits::Embedder;
