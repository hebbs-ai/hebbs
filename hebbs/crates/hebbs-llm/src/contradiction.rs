//! LLM-based contradiction classification prompts and response parsing.

use std::collections::HashMap;

use crate::error::{LlmError, Result};
use crate::provider::{LlmProvider, LlmRequest, ResponseFormat};

/// Verdict from LLM contradiction classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContradictionVerdict {
    /// The two memories assert opposing or incompatible facts.
    Contradiction,
    /// Memory B updates or supersedes memory A (evolution of thinking).
    Revision,
    /// The memories are compatible or unrelated.
    Dismiss,
}

/// Result of LLM contradiction classification.
#[derive(Debug, Clone)]
pub struct ContradictionClassification {
    pub verdict: ContradictionVerdict,
    pub confidence: f32,
    pub reasoning: String,
}

/// Classify the relationship between two memory contents using an LLM.
///
/// Complexity: O(1) LLM call per pair.
pub fn llm_classify_contradiction(
    provider: &dyn LlmProvider,
    content_a: &str,
    content_b: &str,
) -> Result<ContradictionClassification> {
    // Truncate to avoid excessive token usage
    let max_chars = 2000;
    let a = truncate_content(content_a, max_chars);
    let b = truncate_content(content_b, max_chars);

    let mut metadata = HashMap::new();
    metadata.insert("stage".into(), "contradiction".into());

    let request = LlmRequest {
        system_message: "You are a contradiction detector for a memory system. Given two memory statements, determine their relationship. Output valid JSON only.".to_string(),
        user_message: format!(
            "Memory A: \"{}\"\nMemory B: \"{}\" (newer)\n\nClassify as one of:\n- contradiction: opposing or incompatible facts\n- revision: Memory B updates or supersedes Memory A\n- dismiss: compatible or unrelated\n\nConsider temporal context. \"I used to think X\" or \"updated:\" suggests revision, not contradiction.\n\nOutput JSON: {{\"verdict\": \"contradiction|revision|dismiss\", \"confidence\": 0.0-1.0, \"reasoning\": \"...\"}}",
            a, b
        ),
        max_tokens: 200,
        temperature: 0.0,
        response_format: ResponseFormat::Json,
        metadata,
    };

    let response = provider.complete(request)?;
    parse_contradiction_response(&response.content)
}

fn parse_contradiction_response(content: &str) -> Result<ContradictionClassification> {
    let parsed: serde_json::Value =
        serde_json::from_str(content).map_err(|e| LlmError::ResponseParse {
            message: format!("invalid JSON from contradiction classifier: {e}"),
        })?;

    let verdict_str = parsed["verdict"]
        .as_str()
        .unwrap_or("dismiss")
        .to_lowercase();

    let verdict = match verdict_str.as_str() {
        "contradiction" => ContradictionVerdict::Contradiction,
        "revision" => ContradictionVerdict::Revision,
        _ => ContradictionVerdict::Dismiss,
    };

    let confidence = parsed["confidence"]
        .as_f64()
        .map(|f| f as f32)
        .unwrap_or(0.5);

    let reasoning = parsed["reasoning"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(ContradictionClassification {
        verdict,
        confidence,
        reasoning,
    })
}

fn truncate_content(content: &str, max_chars: usize) -> &str {
    if content.len() <= max_chars {
        content
    } else {
        // Find a char boundary
        let end = content
            .char_indices()
            .take_while(|(i, _)| *i < max_chars)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        &content[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_contradiction_verdict() {
        let json = r#"{"verdict":"contradiction","confidence":0.85,"reasoning":"opposing facts"}"#;
        let result = parse_contradiction_response(json).unwrap();
        assert_eq!(result.verdict, ContradictionVerdict::Contradiction);
        assert!((result.confidence - 0.85).abs() < 0.01);
    }

    #[test]
    fn parse_revision_verdict() {
        let json = r#"{"verdict":"revision","confidence":0.72,"reasoning":"update"}"#;
        let result = parse_contradiction_response(json).unwrap();
        assert_eq!(result.verdict, ContradictionVerdict::Revision);
    }

    #[test]
    fn parse_dismiss_verdict() {
        let json = r#"{"verdict":"dismiss","confidence":0.9,"reasoning":"unrelated"}"#;
        let result = parse_contradiction_response(json).unwrap();
        assert_eq!(result.verdict, ContradictionVerdict::Dismiss);
    }

    #[test]
    fn mock_provider_contradiction() {
        let mock = crate::mock::MockLlmProvider::new();
        let result =
            llm_classify_contradiction(&mock, "Budget is $5K", "Budget is $2K").unwrap();
        assert_eq!(result.verdict, ContradictionVerdict::Contradiction);
    }
}
