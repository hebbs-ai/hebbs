use serde::Serialize;

use crate::error::{LlmError, Result};
use crate::http::{http_post_json, make_http_agent};
use crate::provider::{
    LlmProvider, LlmProviderConfig, LlmRequest, LlmResponse, ResponseFormat,
};

/// OpenAI Chat Completions provider.
pub struct OpenAiProvider {
    agent: ureq::Agent,
    api_key: String,
    model: String,
    base_url: String,
    max_retries: usize,
    retry_backoff_ms: u64,
}

impl OpenAiProvider {
    pub fn new(config: &LlmProviderConfig) -> Result<Self> {
        let api_key = config.api_key.clone().ok_or_else(|| LlmError::Config {
            message: "OpenAI provider requires api_key".into(),
        })?;
        Ok(Self {
            agent: make_http_agent(config.timeout_secs),
            api_key,
            model: config.model.clone(),
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com".into()),
            max_retries: config.max_retries,
            retry_backoff_ms: config.retry_backoff_ms,
        })
    }
}

impl LlmProvider for OpenAiProvider {
    fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        let url = format!("{}/v1/chat/completions", self.base_url);

        #[derive(Serialize)]
        struct Msg {
            role: String,
            content: String,
        }
        #[derive(Serialize)]
        struct Body {
            model: String,
            max_tokens: usize,
            temperature: f32,
            messages: Vec<Msg>,
            #[serde(skip_serializing_if = "Option::is_none")]
            response_format: Option<RespFmt>,
        }
        #[derive(Serialize)]
        struct RespFmt {
            #[serde(rename = "type")]
            fmt_type: &'static str,
        }

        let resp_fmt = if request.response_format == ResponseFormat::Json {
            Some(RespFmt {
                fmt_type: "json_object",
            })
        } else {
            None
        };

        let body = Body {
            model: self.model.clone(),
            max_tokens: request.max_tokens,
            temperature: request.temperature,
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
            response_format: resp_fmt,
        };

        let auth_val = format!("Bearer {}", self.api_key);
        let headers = [("Authorization", auth_val.as_str())];
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
                message: format!("invalid JSON from OpenAI: {e}"),
            })?;

        let content = parsed["choices"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|c| c["message"]["content"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(LlmResponse { content })
    }
}
