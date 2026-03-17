use serde::Serialize;

use crate::error::Result;
use crate::provider::{LlmProvider, LlmRequest, LlmResponse};

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
            "contradiction" => mock_contradiction_response(&request),
            "extraction" => mock_extraction_response(),
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

/// Mock contradiction classification: always returns "contradiction" with 0.85 confidence.
fn mock_contradiction_response(_request: &LlmRequest) -> String {
    r#"{"verdict":"contradiction","confidence":0.85,"reasoning":"mock contradiction detected"}"#
        .into()
}

/// Mock extraction response: returns a single proposition.
fn mock_extraction_response() -> String {
    r#"{"propositions":[{"content":"Mock extracted proposition","confidence":0.9}],"entities":[],"relations":[]}"#.into()
}

fn mock_generic_response(_request: &LlmRequest) -> String {
    r#"{"message":"mock response"}"#.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::provider::ResponseFormat;

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
        let config = crate::provider::LlmProviderConfig::default();
        let provider = crate::provider::create_provider(&config).unwrap();
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
