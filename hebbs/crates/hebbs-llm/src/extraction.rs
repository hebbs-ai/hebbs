//! LLM-based proposition, entity, and relation extraction from content.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{LlmError, Result};
use crate::provider::{LlmProvider, LlmRequest, ResponseFormat};

/// An atomic fact extracted from content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedProposition {
    pub content: String,
    pub confidence: f32,
}

/// An entity mentioned in content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    pub name: String,
    pub entity_type: String,
}

/// A relationship between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelation {
    pub source: String,
    pub target: String,
    pub relation_type: String,
    pub confidence: f32,
}

/// Combined output from a single extraction call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionOutput {
    pub propositions: Vec<ExtractedProposition>,
    pub entities: Vec<ExtractedEntity>,
    pub relations: Vec<ExtractedRelation>,
}

impl Default for ExtractionOutput {
    fn default() -> Self {
        Self {
            propositions: Vec::new(),
            entities: Vec::new(),
            relations: Vec::new(),
        }
    }
}

/// Extract propositions, entities, and relations from content using an LLM.
///
/// The `context` parameter provides document-level context (file path, heading)
/// to improve extraction quality (Anthropic contextual retrieval technique).
///
/// Complexity: O(1) LLM call.
pub fn extract_from_content(
    provider: &dyn LlmProvider,
    content: &str,
    context: &str,
) -> Result<ExtractionOutput> {
    if content.trim().is_empty() {
        return Ok(ExtractionOutput::default());
    }

    let mut metadata = HashMap::new();
    metadata.insert("stage".into(), "extraction".into());

    let request = LlmRequest {
        system_message: EXTRACTION_SYSTEM_PROMPT.to_string(),
        user_message: format!(
            "Context: {}\n\nContent:\n{}\n\nExtract propositions, entities, and relations as JSON.",
            context, content
        ),
        max_tokens: 4000,
        temperature: 0.0,
        response_format: ResponseFormat::Json,
        metadata,
    };

    let response = provider.complete(request)?;
    parse_extraction_response(&response.content)
}

/// Summarize content for document-level memory.
///
/// Complexity: O(1) LLM call.
pub fn summarize_content(
    provider: &dyn LlmProvider,
    content: &str,
) -> Result<String> {
    if content.trim().is_empty() {
        return Ok(String::new());
    }

    let request = LlmRequest {
        system_message: "Summarize the following content in 2-3 sentences, capturing the key facts and themes. Be concise and factual.".to_string(),
        user_message: content.to_string(),
        max_tokens: 500,
        temperature: 0.0,
        response_format: ResponseFormat::Text,
        metadata: HashMap::new(),
    };

    let response = provider.complete(request)?;
    Ok(response.content)
}

const EXTRACTION_SYSTEM_PROMPT: &str = r#"Extract atomic propositions, named entities, and relationships from the given content.

Rules for propositions:
- Each proposition is a single, self-contained factual statement
- Include enough context to be understood without the source document
- Prefer specific facts over vague summaries
- Maximum 20 propositions per chunk

Rules for entities:
- Extract named entities (people, organizations, products, concepts)
- Classify each entity by type: person, organization, product, concept, location, event

Rules for relations:
- Identify relationships between extracted entities
- Use descriptive relation types: "works_at", "founded", "competes_with", "uses", etc.

Output JSON:
{
  "propositions": [{"content": "...", "confidence": 0.0-1.0}],
  "entities": [{"name": "...", "entity_type": "..."}],
  "relations": [{"source": "...", "target": "...", "relation_type": "...", "confidence": 0.0-1.0}]
}"#;

fn parse_extraction_response(content: &str) -> Result<ExtractionOutput> {
    serde_json::from_str(content).map_err(|e| LlmError::ResponseParse {
        message: format!("invalid extraction JSON: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extraction_output() {
        let json = r#"{
            "propositions": [{"content": "Perplexity was founded in 2022", "confidence": 0.95}],
            "entities": [{"name": "Perplexity", "entity_type": "organization"}],
            "relations": []
        }"#;
        let output = parse_extraction_response(json).unwrap();
        assert_eq!(output.propositions.len(), 1);
        assert_eq!(output.entities.len(), 1);
    }

    #[test]
    fn extract_empty_content() {
        let mock = crate::mock::MockLlmProvider::new();
        let output = extract_from_content(&mock, "", "test.md").unwrap();
        assert!(output.propositions.is_empty());
    }
}
