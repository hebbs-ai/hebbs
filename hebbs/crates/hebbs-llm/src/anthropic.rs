use serde::Serialize;

use crate::error::{LlmError, Result};
use crate::http::{http_post_json, make_http_agent};
use crate::provider::{LlmProvider, LlmProviderConfig, LlmRequest, LlmResponse};

/// Anthropic Claude provider (Messages API).
pub struct AnthropicProvider {
    agent: ureq::Agent,
    api_key: String,
    model: String,
    base_url: String,
    max_retries: usize,
    retry_backoff_ms: u64,
}

impl AnthropicProvider {
    pub fn new(config: &LlmProviderConfig) -> Result<Self> {
        let api_key = config.api_key.clone().ok_or_else(|| LlmError::Config {
            message: "Anthropic provider requires api_key".into(),
        })?;
        Ok(Self {
            agent: make_http_agent(config.timeout_secs),
            api_key,
            model: config.model.clone(),
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com".into()),
            max_retries: config.max_retries,
            retry_backoff_ms: config.retry_backoff_ms,
        })
    }
}

impl LlmProvider for AnthropicProvider {
    fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        let url = format!("{}/v1/messages", self.base_url);

        #[derive(Serialize)]
        struct Msg {
            role: &'static str,
            content: String,
        }
        #[derive(Serialize)]
        struct Body {
            model: String,
            max_tokens: usize,
            temperature: f32,
            system: String,
            messages: Vec<Msg>,
        }

        let body = Body {
            model: self.model.clone(),
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            system: request.system_message,
            messages: vec![Msg {
                role: "user",
                content: request.user_message,
            }],
        };

        let headers = [
            ("x-api-key", self.api_key.as_str()),
            ("anthropic-version", "2023-06-01"),
        ];
        let text = http_post_json(
            &self.agent,
            &url,
            &headers,
            &body,
            self.max_retries,
            self.retry_backoff_ms,
        )?;

        let parsed: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| LlmError::ResponseParse {
                message: format!("invalid JSON from Anthropic: {e}"),
            })?;

        let content = parsed["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|block| block["text"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(LlmResponse { content })
    }
}
