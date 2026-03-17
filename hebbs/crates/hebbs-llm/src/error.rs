/// Errors from the LLM provider layer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LlmError {
    #[error("LLM provider error: {message}")]
    Provider { message: String },

    #[error("LLM response parse error: {message}")]
    ResponseParse { message: String },

    #[error("LLM configuration error: {message}")]
    Config { message: String },
}

pub type Result<T> = std::result::Result<T, LlmError>;
