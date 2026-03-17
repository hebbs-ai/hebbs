use serde::Serialize;

use crate::error::{LlmError, Result};
use crate::http::{http_post_json, make_http_agent};
use crate::provider::{
    LlmProvider, LlmProviderConfig, LlmRequest, LlmResponse, ResponseFormat,
};

/// Google Gemini provider (generateContent REST API).
pub struct GeminiProvider {
    agent: ureq::Agent,
    api_key: String,
    model: String,
    base_url: String,
    max_retries: usize,
    retry_backoff_ms: u64,
}

impl GeminiProvider {
    pub fn new(config: &LlmProviderConfig) -> Result<Self> {
        let api_key = config.api_key.clone().ok_or_else(|| LlmError::Config {
            message: "Gemini provider requires api_key (set GEMINI_API_KEY)".into(),
        })?;
        Ok(Self {
            agent: make_http_agent(config.timeout_secs),
            api_key,
            model: config.model.clone(),
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com".into()),
            max_retries: config.max_retries,
            retry_backoff_ms: config.retry_backoff_ms,
        })
    }
}

impl LlmProvider for GeminiProvider {
    fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url, self.model, self.api_key
        );

        #[derive(Serialize)]
        struct Part {
            text: String,
        }
        #[derive(Serialize)]
        struct Content {
            role: &'static str,
            parts: Vec<Part>,
        }
        #[derive(Serialize)]
        struct SystemInstruction {
            parts: Vec<Part>,
        }
        #[derive(Serialize)]
        struct GenConfig {
            temperature: f32,
            #[serde(rename = "maxOutputTokens")]
            max_output_tokens: usize,
            #[serde(skip_serializing_if = "Option::is_none", rename = "responseMimeType")]
            response_mime_type: Option<&'static str>,
        }
        #[derive(Serialize)]
        struct Body {
            contents: Vec<Content>,
            #[serde(rename = "systemInstruction")]
            system_instruction: SystemInstruction,
            #[serde(rename = "generationConfig")]
            generation_config: GenConfig,
        }

        let response_mime_type = if request.response_format == ResponseFormat::Json {
            Some("application/json")
        } else {
            None
        };

        let body = Body {
            contents: vec![Content {
                role: "user",
                parts: vec![Part {
                    text: request.user_message,
                }],
            }],
            system_instruction: SystemInstruction {
                parts: vec![Part {
                    text: request.system_message,
                }],
            },
            generation_config: GenConfig {
                temperature: request.temperature,
                max_output_tokens: request.max_tokens,
                response_mime_type,
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
                message: format!("invalid JSON from Gemini: {e}"),
            })?;

        let content = parsed["candidates"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|c| c["content"]["parts"].as_array())
            .and_then(|parts| parts.first())
            .and_then(|p| p["text"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(LlmResponse { content })
    }
}
