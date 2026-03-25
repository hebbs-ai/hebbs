use serde::Serialize;



use crate::error::{LlmError, Result};
use crate::http::{http_get, http_post_json, make_batch_agent, make_http_agent};
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

    fn complete_batch(&self, requests: Vec<LlmRequest>) -> Result<Vec<LlmResponse>> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        if requests.len() <= 5 {
            return requests.into_iter().map(|r| self.complete(r)).collect();
        }

        eprintln!("Anthropic batch: submitting {} requests", requests.len());

        let batch_agent = make_batch_agent();
        let batch_requests: Vec<serde_json::Value> = requests.iter().enumerate().map(|(i, req)| {
            serde_json::json!({
                "custom_id": format!("req-{i}"),
                "params": {
                    "model": self.model,
                    "max_tokens": req.max_tokens,
                    "temperature": req.temperature,
                    "system": req.system_message,
                    "messages": [{"role": "user", "content": req.user_message}]
                }
            })
        }).collect();

        // 1. Create batch
        let create_url = format!("{}/v1/messages/batches", self.base_url);
        let create_body = serde_json::json!({ "requests": batch_requests });
        let headers = [
            ("x-api-key", self.api_key.as_str()),
            ("anthropic-version", "2023-06-01"),
            ("anthropic-beta", "message-batches-2024-09-24"),
        ];
        let create_text = http_post_json(
            &batch_agent, &create_url, &headers, &create_body, 2, 2000,
        )?;
        let create_json: serde_json::Value = serde_json::from_str(&create_text)
            .map_err(|e| LlmError::ResponseParse { message: format!("parse batch create: {e}") })?;
        let batch_id = create_json["id"].as_str()
            .ok_or_else(|| LlmError::ResponseParse { message: "no batch id".into() })?
            .to_string();

        eprintln!("Anthropic batch: created {}, polling...", batch_id);

        // 2. Poll for completion
        let status_url = format!("{}/v1/messages/batches/{}", self.base_url, batch_id);
        let poll_headers = [
            ("x-api-key", self.api_key.as_str()),
            ("anthropic-version", "2023-06-01"),
            ("anthropic-beta", "message-batches-2024-09-24"),
        ];
        let results_url = loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let status_text = http_get(&batch_agent, &status_url, &poll_headers)?;
            let status_json: serde_json::Value = serde_json::from_str(&status_text)
                .map_err(|e| LlmError::ResponseParse { message: format!("parse batch status: {e}") })?;

            let status = status_json["processing_status"].as_str().unwrap_or("unknown");
            eprintln!("Anthropic batch {} status: {}", batch_id, status);

            match status {
                "ended" => {
                    let url = status_json["results_url"].as_str()
                        .ok_or_else(|| LlmError::ResponseParse { message: "no results_url".into() })?
                        .to_string();
                    break url;
                }
                "canceling" | "canceled" => {
                    return Err(LlmError::Provider {
                        message: format!("batch {batch_id} cancelled"),
                    });
                }
                _ => continue, // in_progress
            }
        };

        eprintln!("Anthropic batch {} completed, downloading results", batch_id);

        // 3. Download results (JSONL)
        let results_text = http_get(&batch_agent, &results_url, &poll_headers)?;

        // 4. Parse results
        let mut result_map: std::collections::HashMap<usize, LlmResponse> = std::collections::HashMap::new();
        for line in results_text.lines() {
            if line.trim().is_empty() { continue; }
            let v: serde_json::Value = serde_json::from_str(line)
                .map_err(|e| LlmError::ResponseParse { message: format!("parse batch result: {e}") })?;
            let custom_id = v["custom_id"].as_str().unwrap_or("");
            let idx: usize = custom_id.strip_prefix("req-")
                .and_then(|s| s.parse().ok())
                .unwrap_or(usize::MAX);

            let result_type = v["result"]["type"].as_str().unwrap_or("");
            let content = if result_type == "succeeded" {
                v["result"]["message"]["content"]
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|block| block["text"].as_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                eprintln!("Anthropic batch item {} failed: {}", custom_id, v["result"]);
                String::new()
            };

            result_map.insert(idx, LlmResponse { content });
        }

        // 5. Assemble in order
        let mut responses = Vec::with_capacity(requests.len());
        for i in 0..requests.len() {
            let resp = result_map.remove(&i).unwrap_or(LlmResponse { content: String::new() });
            responses.push(resp);
        }

        eprintln!("Anthropic batch: {} results returned", responses.len());
        Ok(responses)
    }

    fn supports_batch(&self) -> bool {
        true
    }
}
