use serde::Serialize;
// Note: hebbs-llm doesn't depend on tracing; use eprintln for diagnostics.

use crate::error::{LlmError, Result};
use crate::http::{http_get, http_post_json, make_batch_agent, make_http_agent};
use crate::provider::{LlmProvider, LlmProviderConfig, LlmRequest, LlmResponse, ResponseFormat};

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

    fn complete_batch(&self, requests: Vec<LlmRequest>) -> Result<Vec<LlmResponse>> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        // For small batches, sequential is faster (no upload/poll overhead)
        if requests.len() <= 5 {
            return requests.into_iter().map(|r| self.complete(r)).collect();
        }

        eprintln!("OpenAI batch: submitting {} requests", requests.len());

        let batch_agent = make_batch_agent();
        let auth_val = format!("Bearer {}", self.api_key);

        // 1. Build JSONL content
        let mut jsonl = String::new();
        for (i, req) in requests.iter().enumerate() {
            let resp_fmt = if req.response_format == ResponseFormat::Json {
                Some(serde_json::json!({"type": "json_object"}))
            } else {
                None
            };
            let mut body = serde_json::json!({
                "model": self.model,
                "max_tokens": req.max_tokens,
                "temperature": req.temperature,
                "messages": [
                    {"role": "system", "content": req.system_message},
                    {"role": "user", "content": req.user_message}
                ]
            });
            if let Some(rf) = resp_fmt {
                body["response_format"] = rf;
            }
            let line = serde_json::json!({
                "custom_id": format!("req-{i}"),
                "method": "POST",
                "url": "/v1/chat/completions",
                "body": body
            });
            jsonl.push_str(&serde_json::to_string(&line).unwrap_or_default());
            jsonl.push('\n');
        }

        // 2. Upload JSONL file
        let upload_url = format!("{}/v1/files", self.base_url);
        let boundary = format!(
            "hebbs-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let mut multipart_body = Vec::new();
        // purpose field
        multipart_body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nbatch\r\n"
            )
            .as_bytes(),
        );
        // file field
        multipart_body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"batch.jsonl\"\r\nContent-Type: application/jsonl\r\n\r\n").as_bytes());
        multipart_body.extend_from_slice(jsonl.as_bytes());
        multipart_body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let content_type = format!("multipart/form-data; boundary={boundary}");
        let upload_resp = batch_agent
            .post(&upload_url)
            .header("Authorization", &auth_val)
            .header("Content-Type", &content_type)
            .send(multipart_body.as_slice())
            .map_err(|e| LlmError::Provider {
                message: format!("batch file upload failed: {e}"),
            })?;

        let upload_text =
            upload_resp
                .into_body()
                .read_to_string()
                .map_err(|e| LlmError::Provider {
                    message: format!("read upload response: {e}"),
                })?;
        let upload_json: serde_json::Value =
            serde_json::from_str(&upload_text).map_err(|e| LlmError::ResponseParse {
                message: format!("parse upload response: {e}"),
            })?;
        let file_id = upload_json["id"]
            .as_str()
            .ok_or_else(|| LlmError::ResponseParse {
                message: "no file id in upload response".into(),
            })?
            .to_string();

        eprintln!("OpenAI batch: uploaded file {}", file_id);

        // 3. Create batch
        #[derive(Serialize)]
        struct BatchCreate {
            input_file_id: String,
            endpoint: String,
            completion_window: String,
        }
        let create_url = format!("{}/v1/batches", self.base_url);
        let create_body = BatchCreate {
            input_file_id: file_id,
            endpoint: "/v1/chat/completions".into(),
            completion_window: "24h".into(),
        };
        let create_text = http_post_json(
            &batch_agent,
            &create_url,
            &[("Authorization", auth_val.as_str())],
            &create_body,
            2,
            2000,
        )?;
        let create_json: serde_json::Value =
            serde_json::from_str(&create_text).map_err(|e| LlmError::ResponseParse {
                message: format!("parse batch create: {e}"),
            })?;
        let batch_id = create_json["id"]
            .as_str()
            .ok_or_else(|| LlmError::ResponseParse {
                message: "no batch id".into(),
            })?
            .to_string();

        eprintln!("OpenAI batch: created {}, polling...", batch_id);

        // 4. Poll for completion
        let status_url = format!("{}/v1/batches/{}", self.base_url, batch_id);
        let poll_headers = [("Authorization", auth_val.as_str())];
        let output_file_id = loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let status_text = http_get(&batch_agent, &status_url, &poll_headers)?;
            let status_json: serde_json::Value =
                serde_json::from_str(&status_text).map_err(|e| LlmError::ResponseParse {
                    message: format!("parse batch status: {e}"),
                })?;

            let status = status_json["status"].as_str().unwrap_or("unknown");
            eprintln!("OpenAI batch {} status: {}", batch_id, status);

            match status {
                "completed" => {
                    let fid = status_json["output_file_id"]
                        .as_str()
                        .ok_or_else(|| LlmError::ResponseParse {
                            message: "no output_file_id".into(),
                        })?
                        .to_string();
                    break fid;
                }
                "failed" | "expired" | "cancelled" => {
                    let errors = status_json["errors"].to_string();
                    return Err(LlmError::Provider {
                        message: format!("batch {batch_id} {status}: {errors}"),
                    });
                }
                _ => continue, // in_progress, validating, etc.
            }
        };

        eprintln!("OpenAI batch {} completed, downloading results", batch_id);

        // 5. Download results
        let download_url = format!("{}/v1/files/{}/content", self.base_url, output_file_id);
        let results_text = http_get(&batch_agent, &download_url, &poll_headers)?;

        // 6. Parse JSONL results, order by custom_id
        let mut result_map: std::collections::HashMap<usize, LlmResponse> =
            std::collections::HashMap::new();
        for line in results_text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value =
                serde_json::from_str(line).map_err(|e| LlmError::ResponseParse {
                    message: format!("parse batch result line: {e}"),
                })?;
            let custom_id = v["custom_id"].as_str().unwrap_or("");
            let idx: usize = custom_id
                .strip_prefix("req-")
                .and_then(|s| s.parse().ok())
                .unwrap_or(usize::MAX);

            let content = v["response"]["body"]["choices"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|c| c["message"]["content"].as_str())
                .unwrap_or("")
                .to_string();

            if v["error"].is_null() || v["error"].as_object().map_or(true, |o| o.is_empty()) {
                result_map.insert(idx, LlmResponse { content });
            } else {
                let err_msg = v["error"].to_string();
                eprintln!("OpenAI batch item {} error: {}", custom_id, err_msg);
                result_map.insert(
                    idx,
                    LlmResponse {
                        content: String::new(),
                    },
                );
            }
        }

        // 7. Assemble in order
        let mut responses = Vec::with_capacity(requests.len());
        for i in 0..requests.len() {
            let resp = result_map.remove(&i).unwrap_or(LlmResponse {
                content: String::new(),
            });
            responses.push(resp);
        }

        eprintln!("OpenAI batch: {} results returned", responses.len());
        Ok(responses)
    }

    fn supports_batch(&self) -> bool {
        true
    }
}
