use serde::Serialize;

use crate::error::{LlmError, Result};
use crate::http::{http_post_json, make_http_agent};
use crate::provider::{
    LlmProvider, LlmProviderConfig, LlmRequest, LlmResponse, ResponseFormat,
};

/// Ollama local provider.
pub struct OllamaProvider {
    agent: ureq::Agent,
    model: String,
    base_url: String,
    max_retries: usize,
    retry_backoff_ms: u64,
}

impl OllamaProvider {
    pub fn new(config: &LlmProviderConfig) -> Self {
        Self {
            agent: make_http_agent(config.timeout_secs),
            model: config.model.clone(),
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".into()),
            max_retries: config.max_retries,
            retry_backoff_ms: config.retry_backoff_ms,
        }
    }
}

impl LlmProvider for OllamaProvider {
    fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        let url = format!("{}/api/chat", self.base_url);

        #[derive(Serialize)]
        struct Msg {
            role: String,
            content: String,
        }
        #[derive(Serialize)]
        struct Body {
            model: String,
            messages: Vec<Msg>,
            stream: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            format: Option<&'static str>,
            options: Options,
        }
        #[derive(Serialize)]
        struct Options {
            temperature: f32,
            num_predict: usize,
        }

        let format = if request.response_format == ResponseFormat::Json {
            Some("json")
        } else {
            None
        };

        let body = Body {
            model: self.model.clone(),
            messages: vec![
                Msg {
                    role: "system".into(),
                    content: request.system_message,
                },
                Msg {
                    role: "user".into(),
                    content: request.user_message,
                },
            ],
            stream: false,
            format,
            options: Options {
                temperature: request.temperature,
                num_predict: request.max_tokens,
            },
        };

        let text = http_post_json(
            &self.agent,
            &url,
            &[],
            &body,
            self.max_retries,
            self.retry_backoff_ms,
        )?;

        let parsed: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| LlmError::ResponseParse {
                message: format!("invalid JSON from Ollama: {e}"),
            })?;

        // Some models (e.g. qwen3) use a thinking/reasoning mode where the main
        // response lands in "content" only after internal reasoning completes.
        // If "content" is empty, fall back to the "thinking" field.
        let content = parsed["message"]["content"]
            .as_str()
            .unwrap_or("");
        let content = if content.is_empty() {
            parsed["message"]["thinking"]
                .as_str()
                .unwrap_or("")
                .to_string()
        } else {
            content.to_string()
        };

        Ok(LlmResponse { content })
    }
}
