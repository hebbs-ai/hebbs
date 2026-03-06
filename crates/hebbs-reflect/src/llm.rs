use crate::error::{ReflectError, Result};
use serde::Serialize;
use std::collections::HashMap;

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
    Ollama,
}

impl ProviderType {
    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "anthropic" | "claude" => Self::Anthropic,
            "openai" | "gpt" => Self::OpenAi,
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

// ---------------------------------------------------------------------------
// MockLlmProvider -- deterministic, no-network provider for testing
// ---------------------------------------------------------------------------

/// Deterministic LLM provider that returns structured JSON based on
/// the `metadata["stage"]` field. Used for all unit and integration tests.
pub struct MockLlmProvider;

impl MockLlmProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockLlmProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmProvider for MockLlmProvider {
    fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        let stage = request
            .metadata
            .get("stage")
            .map(|s| s.as_str())
            .unwrap_or("");
        let content = match stage {
            "proposal" => mock_proposal_response(&request),
            "validation" => mock_validation_response(&request),
            _ => mock_generic_response(&request),
        };
        Ok(LlmResponse { content })
    }
}

/// Extracts memory_ids from the metadata and produces one insight per cluster.
fn mock_proposal_response(request: &LlmRequest) -> String {
    let memory_ids: Vec<String> = request
        .metadata
        .get("memory_ids")
        .map(|s| {
            s.split(',')
                .filter(|id| !id.is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    let cluster_topic = request
        .metadata
        .get("cluster_topic")
        .cloned()
        .unwrap_or_else(|| "general pattern".into());

    #[derive(Serialize)]
    struct Resp {
        insights: Vec<Insight>,
    }
    #[derive(Serialize)]
    struct Insight {
        content: String,
        confidence: f32,
        source_memory_ids: Vec<String>,
        tags: Vec<String>,
    }

    let resp = Resp {
        insights: vec![Insight {
            content: format!("Consolidated insight about {cluster_topic}"),
            confidence: 0.85,
            source_memory_ids: memory_ids,
            tags: vec!["mock".into()],
        }],
    };
    serde_json::to_string(&resp).unwrap_or_else(|_| r#"{"insights":[]}"#.into())
}

/// Accepts all candidates with confidence 0.85.
fn mock_validation_response(request: &LlmRequest) -> String {
    let count: usize = request
        .metadata
        .get("candidate_count")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    #[derive(Serialize)]
    struct Resp {
        results: Vec<Entry>,
    }
    #[derive(Serialize)]
    struct Entry {
        candidate_index: usize,
        verdict: &'static str,
        confidence: f32,
    }

    let resp = Resp {
        results: (0..count)
            .map(|i| Entry {
                candidate_index: i,
                verdict: "accepted",
                confidence: 0.85,
            })
            .collect(),
    };
    serde_json::to_string(&resp).unwrap_or_else(|_| r#"{"results":[]}"#.into())
}

fn mock_generic_response(_request: &LlmRequest) -> String {
    r#"{"message":"mock response"}"#.into()
}

// ---------------------------------------------------------------------------
// HTTP-based providers (Anthropic, OpenAI, Ollama)
// ---------------------------------------------------------------------------

fn make_http_agent(timeout_secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(timeout_secs)))
        .build()
        .new_agent()
}

fn http_post_json(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
    body: &impl Serialize,
    max_retries: usize,
    retry_backoff_ms: u64,
) -> Result<String> {
    let mut last_err = String::new();
    for attempt in 0..=max_retries {
        if attempt > 0 {
            let backoff = retry_backoff_ms * (1u64 << (attempt - 1).min(6));
            std::thread::sleep(std::time::Duration::from_millis(backoff));
        }
        let mut req = agent.post(url);
        for &(k, v) in headers {
            req = req.header(k, v);
        }
        req = req.header("content-type", "application/json");

        match req.send_json(body) {
            Ok(resp) => {
                let text = resp
                    .into_body()
                    .read_to_string()
                    .map_err(|e| ReflectError::Llm {
                        message: format!("failed to read response body: {e}"),
                    })?;
                return Ok(text);
            }
            Err(e) => {
                last_err = format!("{e}");
                let retryable = last_err.contains("429")
                    || last_err.contains("500")
                    || last_err.contains("timeout")
                    || last_err.contains("connection");
                if !retryable {
                    return Err(ReflectError::Llm { message: last_err });
                }
            }
        }
    }
    Err(ReflectError::Llm {
        message: format!("exhausted retries: {last_err}"),
    })
}

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
        let api_key = config.api_key.clone().ok_or_else(|| ReflectError::Config {
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
            serde_json::from_str(&text).map_err(|e| ReflectError::ResponseParse {
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
        let api_key = config.api_key.clone().ok_or_else(|| ReflectError::Config {
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
            serde_json::from_str(&text).map_err(|e| ReflectError::ResponseParse {
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
            serde_json::from_str(&text).map_err(|e| ReflectError::ResponseParse {
                message: format!("invalid JSON from Ollama: {e}"),
            })?;

        let content = parsed["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(LlmResponse { content })
    }
}

/// Create a provider from configuration.
pub fn create_provider(config: &LlmProviderConfig) -> Result<Box<dyn LlmProvider>> {
    match config.provider_type {
        ProviderType::Mock => Ok(Box::new(MockLlmProvider::new())),
        ProviderType::Anthropic => Ok(Box::new(AnthropicProvider::new(config)?)),
        ProviderType::OpenAi => Ok(Box::new(OpenAiProvider::new(config)?)),
        ProviderType::Ollama => Ok(Box::new(OllamaProvider::new(config))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_proposal_returns_valid_json() {
        let mock = MockLlmProvider::new();
        let mut meta = HashMap::new();
        meta.insert("stage".into(), "proposal".into());
        meta.insert("memory_ids".into(), "aabb,ccdd".into());
        meta.insert("cluster_topic".into(), "pricing objections".into());

        let req = LlmRequest {
            system_message: "test".into(),
            user_message: "test".into(),
            max_tokens: 1000,
            temperature: 0.0,
            response_format: ResponseFormat::Json,
            metadata: meta,
        };
        let resp = mock.complete(req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp.content).unwrap();
        assert!(parsed["insights"].is_array());
        assert!(!parsed["insights"].as_array().unwrap().is_empty());
    }

    #[test]
    fn mock_validation_returns_valid_json() {
        let mock = MockLlmProvider::new();
        let mut meta = HashMap::new();
        meta.insert("stage".into(), "validation".into());
        meta.insert("candidate_count".into(), "3".into());

        let req = LlmRequest {
            system_message: "test".into(),
            user_message: "test".into(),
            max_tokens: 1000,
            temperature: 0.0,
            response_format: ResponseFormat::Json,
            metadata: meta,
        };
        let resp = mock.complete(req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&resp.content).unwrap();
        let results = parsed["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn create_mock_provider() {
        let config = LlmProviderConfig::default();
        let provider = create_provider(&config).unwrap();
        let req = LlmRequest {
            system_message: "s".into(),
            user_message: "u".into(),
            max_tokens: 10,
            temperature: 0.0,
            response_format: ResponseFormat::Text,
            metadata: HashMap::new(),
        };
        let resp = provider.complete(req).unwrap();
        assert!(!resp.content.is_empty());
    }
}
