pub mod anthropic;
pub mod contradiction;
pub mod error;
pub mod extraction;
pub mod gemini;
mod http;
pub mod mock;
pub mod ollama;
pub mod openai;
pub mod provider;

pub use error::{LlmError, Result};
pub use provider::{
    create_provider, validate_provider, LlmProvider, LlmProviderConfig, LlmRequest, LlmResponse,
    ProviderType, ResponseFormat,
};

// Provider implementations
pub use anthropic::AnthropicProvider;
pub use gemini::GeminiProvider;
pub use mock::MockLlmProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
