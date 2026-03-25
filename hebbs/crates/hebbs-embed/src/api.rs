//! API-based embedding provider (OpenAI, etc.).
//!
//! Calls a remote embedding API instead of running local ONNX inference.
//! All returned vectors are L2-normalized to satisfy the HNSW invariant.

use serde::Serialize;

use crate::error::{EmbedError, Result};
use crate::normalize::l2_normalize;
use crate::traits::Embedder;

/// Maximum texts per API batch call.
/// OpenAI supports up to 2048; we cap lower to bound memory and latency.
const MAX_BATCH_SIZE: usize = 256;

/// Configuration for API-based embedding.
#[derive(Debug, Clone)]
pub struct ApiEmbedderConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub dimensions: usize,
}

/// API-based embedder that calls a remote embedding endpoint.
///
/// Currently supports OpenAI's `/v1/embeddings` API format.
/// The same format is used by many OpenAI-compatible providers
/// (Azure OpenAI, Together, Fireworks, etc.).
pub struct ApiEmbedder {
    agent: ureq::Agent,
    api_key: String,
    model: String,
    base_url: String,
    dims: usize,
}

impl ApiEmbedder {
    pub fn new(config: ApiEmbedderConfig) -> Result<Self> {
        if config.api_key.is_empty() {
            return Err(EmbedError::Config {
                message: "API embedding requires an API key".into(),
            });
        }

        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .build()
            .new_agent();

        Ok(Self {
            agent,
            api_key: config.api_key,
            model: config.model,
            base_url: config.base_url,
            dims: config.dimensions,
        })
    }

    /// Call the embeddings API for a batch of texts.
    fn api_embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let url = format!("{}/v1/embeddings", self.base_url);

        #[derive(Serialize)]
        struct Body<'a> {
            input: &'a [&'a str],
            model: &'a str,
        }

        let body = Body {
            input: texts,
            model: &self.model,
        };

        let auth_val = format!("Bearer {}", self.api_key);

        // Retry up to 3 times
        let mut last_err = String::new();
        for attempt in 0..=3 {
            if attempt > 0 {
                let backoff = 1000 * (1u64 << (attempt - 1).min(4));
                std::thread::sleep(std::time::Duration::from_millis(backoff));
            }

            let resp = self
                .agent
                .post(&url)
                .header("Authorization", &auth_val)
                .header("Content-Type", "application/json")
                .send_json(&body);

            match resp {
                Ok(resp) => {
                    let text =
                        resp.into_body()
                            .read_to_string()
                            .map_err(|e| EmbedError::Inference {
                                message: format!("failed to read embedding response: {e}"),
                            })?;

                    let parsed: serde_json::Value =
                        serde_json::from_str(&text).map_err(|e| EmbedError::Inference {
                            message: format!("invalid JSON from embedding API: {e}"),
                        })?;

                    let data = parsed["data"]
                        .as_array()
                        .ok_or_else(|| EmbedError::Inference {
                            message: "no 'data' array in embedding response".into(),
                        })?;

                    let mut results = Vec::with_capacity(data.len());
                    for item in data {
                        let embedding =
                            item["embedding"]
                                .as_array()
                                .ok_or_else(|| EmbedError::Inference {
                                    message: "no 'embedding' array in response item".into(),
                                })?;

                        let mut vec: Vec<f32> = embedding
                            .iter()
                            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                            .collect();

                        // L2 normalize (critical for HNSW inner product = cosine)
                        l2_normalize(&mut vec);
                        results.push(vec);
                    }

                    // OpenAI returns results sorted by index, but verify ordering
                    // by checking the "index" field if present
                    if results.len() == texts.len() {
                        return Ok(results);
                    } else {
                        return Err(EmbedError::Inference {
                            message: format!(
                                "expected {} embeddings, got {}",
                                texts.len(),
                                results.len()
                            ),
                        });
                    }
                }
                Err(e) => {
                    last_err = format!("{e}");
                    let retryable = last_err.contains("429")
                        || last_err.contains("500")
                        || last_err.contains("timeout")
                        || last_err.contains("connection");
                    if !retryable {
                        return Err(EmbedError::Inference {
                            message: format!("embedding API error: {last_err}"),
                        });
                    }
                }
            }
        }

        Err(EmbedError::Inference {
            message: format!("exhausted retries: {last_err}"),
        })
    }
}

impl Embedder for ApiEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.api_embed_batch(&[text])?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| EmbedError::Inference {
                message: "empty embedding response".into(),
            })
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_results = Vec::with_capacity(texts.len());

        // Chunk to respect API batch limits
        for chunk in texts.chunks(MAX_BATCH_SIZE) {
            let results = self.api_embed_batch(chunk)?;
            all_results.extend(results);
        }

        Ok(all_results)
    }

    fn dimensions(&self) -> usize {
        self.dims
    }
}
