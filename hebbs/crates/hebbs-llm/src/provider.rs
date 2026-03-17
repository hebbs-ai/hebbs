use std::collections::HashMap;

use crate::error::Result;

/// Request sent to an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub system_message: String,
    pub user_message: String,
    pub max_tokens: usize,
    pub temperature: f32,
    pub response_format: ResponseFormat,
    /// Opaque metadata for routing (e.g. `"stage" -> "proposal"`).
    /// Real providers ignore this; MockLlmProvider uses it.
    pub metadata: HashMap<String, String>,
}

/// Expected response format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseFormat {
    Text,
    Json,
}

/// Response from an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
}

/// Trait for LLM completion providers.
///
/// Implementations must be `Send + Sync` for use from background threads.
/// All calls are blocking (no async runtime required).
pub trait LlmProvider: Send + Sync {
    fn complete(&self, request: LlmRequest) -> Result<LlmResponse>;
}

/// Configuration for an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmProviderConfig {
    pub provider_type: ProviderType,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: String,
    pub timeout_secs: u64,
    pub max_retries: usize,
    pub retry_backoff_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderType {
    Mock,
    Anthropic,
    OpenAi,
    Gemini,
    Ollama,
}

impl ProviderType {
    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "anthropic" | "claude" => Self::Anthropic,
            "openai" | "gpt" => Self::OpenAi,
            "gemini" | "google" => Self::Gemini,
            "ollama" | "local" => Self::Ollama,
            _ => Self::Mock,
        }
    }
}

impl Default for LlmProviderConfig {
    fn default() -> Self {
        Self {
            provider_type: ProviderType::Mock,
            api_key: None,
            base_url: None,
            model: "mock".into(),
            timeout_secs: 60,
            max_retries: 3,
            retry_backoff_ms: 1000,
        }
    }
}

/// Create a provider from configuration.
pub fn create_provider(config: &LlmProviderConfig) -> Result<Box<dyn LlmProvider>> {
    match config.provider_type {
        ProviderType::Mock => Ok(Box::new(crate::mock::MockLlmProvider::new())),
        ProviderType::Anthropic => Ok(Box::new(crate::anthropic::AnthropicProvider::new(config)?)),
        ProviderType::OpenAi => Ok(Box::new(crate::openai::OpenAiProvider::new(config)?)),
        ProviderType::Gemini => Ok(Box::new(crate::gemini::GeminiProvider::new(config)?)),
        ProviderType::Ollama => Ok(Box::new(crate::ollama::OllamaProvider::new(config))),
    }
}

/// Validate that an LLM provider is reachable by sending a trivial test prompt.
///
/// Uses a higher token limit (256) to accommodate thinking/reasoning models
/// (e.g. qwen3) that consume tokens on internal reasoning before producing output.
///
/// Returns `Ok(())` if the provider responds, `Err` with details otherwise.
pub fn validate_provider(provider: &dyn LlmProvider) -> Result<()> {
    let request = LlmRequest {
        system_message: "Respond with exactly: OK".to_string(),
        user_message: "Respond with exactly: OK".to_string(),
        max_tokens: 256,
        temperature: 0.0,
        response_format: ResponseFormat::Text,
        metadata: HashMap::new(),
    };
    provider.complete(request)?;
    Ok(())
}
