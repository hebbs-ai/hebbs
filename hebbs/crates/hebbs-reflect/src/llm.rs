//! Thin re-export layer. All LLM provider types now live in `hebbs-llm`.

pub use hebbs_llm::{
    create_provider, AnthropicProvider, GeminiProvider, LlmProvider, LlmProviderConfig, LlmRequest,
    LlmResponse, MockLlmProvider, OllamaProvider, OpenAiProvider, ProviderType, ResponseFormat,
};

/// Bridge: convert `hebbs_llm::LlmError` to `ReflectError` for backward compat.
impl From<hebbs_llm::LlmError> for crate::error::ReflectError {
    fn from(e: hebbs_llm::LlmError) -> Self {
        match e {
            hebbs_llm::LlmError::Provider { message } => crate::error::ReflectError::Llm { message },
            hebbs_llm::LlmError::ResponseParse { message } => {
                crate::error::ReflectError::ResponseParse { message }
            }
            hebbs_llm::LlmError::Config { message } => crate::error::ReflectError::Config { message },
            _ => crate::error::ReflectError::Llm {
                message: e.to_string(),
            },
        }
    }
}
