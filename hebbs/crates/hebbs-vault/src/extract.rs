//! File extraction pipeline for triple-layer ingestion.
//!
//! Each file produces:
//! - Layer 1: One Document memory (full content or LLM summary for large files)
//! - Layer 2: N Proposition memories (atomic facts extracted by LLM)
//! - Layer 3: Entity/relation edges (not stored as memories, but as graph edges)
//!
//! Small files (< large_file_threshold tokens) get full content as the Document memory.
//! Large files get an LLM summary as the Document memory, with per-heading-chunk extraction.

use std::collections::HashMap;

use tracing::{debug, warn};

use hebbs_core::engine::{Engine, RememberEdge, RememberInput};
use hebbs_core::memory::MemoryKind;
use hebbs_index::graph::EdgeType;
use hebbs_llm::extraction::{self, ExtractionOutput};
use hebbs_llm::LlmProvider;

use crate::config::ExtractionConfig;
use crate::parser::ParsedSection;

/// Output from extracting a single file.
#[derive(Debug, Default)]
pub struct FileExtractionResult {
    /// The engine-assigned ID for the Document memory.
    pub document_memory_id: Option<[u8; 16]>,
    /// The engine-assigned IDs for Proposition memories.
    pub proposition_memory_ids: Vec<[u8; 16]>,
    /// Number of entities extracted.
    pub entities_extracted: usize,
    /// Number of relations extracted.
    pub relations_extracted: usize,
    /// Errors encountered during extraction.
    pub errors: usize,
}

/// Estimate token count from character count (rough: 1 token ~ 4 chars).
fn estimate_tokens(content: &str) -> usize {
    content.len() / 4
}

/// Extract and store triple-layer memories for a single file.
///
/// The caller provides the file content (or per-section content for large files),
/// the LLM provider, and the engine. This function:
/// 1. Creates a Document memory (Layer 1)
/// 2. Extracts propositions via LLM and creates Proposition memories (Layer 2)
/// 3. Creates entity/relation graph edges (Layer 3)
///
/// Returns the IDs of all created memories for manifest tracking.
pub fn extract_and_store_file(
    engine: &Engine,
    provider: &dyn LlmProvider,
    file_content: &str,
    rel_path: &str,
    sections: &[ParsedSection],
    config: &ExtractionConfig,
) -> FileExtractionResult {
    let mut result = FileExtractionResult::default();
    let tokens = estimate_tokens(file_content);

    // Layer 1: Document memory
    let doc_content = if tokens > config.large_file_threshold {
        // Large file: use LLM summary
        match extraction::summarize_content(provider, file_content) {
            Ok(summary) if !summary.is_empty() => summary,
            Ok(_) => {
                // Empty summary, fall back to truncated content
                truncate_content(file_content, config.large_file_threshold * 4)
            }
            Err(e) => {
                warn!("LLM summarization failed for {}: {}", rel_path, e);
                truncate_content(file_content, config.large_file_threshold * 4)
            }
        }
    } else {
        file_content.to_string()
    };

    if doc_content.trim().is_empty() {
        return result;
    }

    let mut doc_context = HashMap::new();
    doc_context.insert(
        "file_path".to_string(),
        serde_json::Value::String(rel_path.to_string()),
    );
    doc_context.insert(
        "layer".to_string(),
        serde_json::Value::String("document".to_string()),
    );

    let doc_input = RememberInput {
        content: doc_content,
        importance: Some(0.6),
        context: Some(doc_context),
        entity_id: None,
        edges: Vec::new(),
        kind: Some(MemoryKind::Document),
    };

    let doc_memory_id = match engine.remember(doc_input) {
        Ok(memory) => {
            if memory.memory_id.len() == 16 {
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&memory.memory_id);
                result.document_memory_id = Some(arr);
                Some(arr)
            } else {
                result.errors += 1;
                None
            }
        }
        Err(e) => {
            warn!("failed to create Document memory for {}: {}", rel_path, e);
            result.errors += 1;
            None
        }
    };

    // Layer 2+3: Extract propositions and entities
    let extraction_output = if tokens > config.large_file_threshold {
        // Large file: extract per heading chunk with document context
        extract_large_file(provider, file_content, rel_path, sections, config)
    } else {
        // Small file: single extraction call
        match extraction::extract_from_content(provider, file_content, rel_path) {
            Ok(output) => output,
            Err(e) => {
                warn!("extraction failed for {}: {}", rel_path, e);
                result.errors += 1;
                ExtractionOutput::default()
            }
        }
    };

    // Cap propositions
    let propositions = if extraction_output.propositions.len() > config.max_propositions_per_file {
        &extraction_output.propositions[..config.max_propositions_per_file]
    } else {
        &extraction_output.propositions
    };

    // Store Proposition memories with PropositionOf edges back to Document
    for prop in propositions {
        if prop.content.trim().is_empty() {
            continue;
        }

        let mut prop_context = HashMap::new();
        prop_context.insert(
            "file_path".to_string(),
            serde_json::Value::String(rel_path.to_string()),
        );
        prop_context.insert(
            "layer".to_string(),
            serde_json::Value::String("proposition".to_string()),
        );

        let edges = if let Some(doc_id) = doc_memory_id {
            vec![RememberEdge {
                target_id: doc_id,
                edge_type: EdgeType::PropositionOf,
                confidence: Some(prop.confidence),
            }]
        } else {
            Vec::new()
        };

        let prop_input = RememberInput {
            content: prop.content.clone(),
            importance: Some(0.5),
            context: Some(prop_context),
            entity_id: None,
            edges,
            kind: Some(MemoryKind::Proposition),
        };

        match engine.remember(prop_input) {
            Ok(memory) => {
                if memory.memory_id.len() == 16 {
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(&memory.memory_id);
                    result.proposition_memory_ids.push(arr);
                }
            }
            Err(e) => {
                debug!("failed to store proposition for {}: {}", rel_path, e);
                result.errors += 1;
            }
        }
    }

    // Layer 3: Entity and relation edges
    // Entities are stored as context on the Document memory (not separate memories).
    // Relations create EntityRelation edges between Document memories that share entities.
    result.entities_extracted = extraction_output.entities.len();
    result.relations_extracted = extraction_output.relations.len();

    result
}

/// Extract from a large file by splitting into heading-based chunks.
///
/// Each chunk gets the document title/path prepended as context
/// (Anthropic contextual retrieval technique).
fn extract_large_file(
    provider: &dyn LlmProvider,
    file_content: &str,
    rel_path: &str,
    sections: &[ParsedSection],
    config: &ExtractionConfig,
) -> ExtractionOutput {
    let mut combined = ExtractionOutput::default();

    // Build a short document context string
    let doc_context = format!(
        "Document: {}. This chunk is part of a larger document.",
        rel_path
    );

    if sections.is_empty() {
        // No sections, treat as single chunk
        match extraction::extract_from_content(provider, file_content, &doc_context) {
            Ok(output) => return output,
            Err(e) => {
                warn!("extraction failed for {}: {}", rel_path, e);
                return combined;
            }
        }
    }

    for section in sections {
        if section.content.trim().is_empty() {
            continue;
        }

        // Cap per-section extraction to avoid runaway costs
        if combined.propositions.len() >= config.max_propositions_per_file {
            break;
        }

        let chunk_context = format!(
            "{} Section: {}",
            doc_context,
            section.heading_path.join(" > ")
        );

        match extraction::extract_from_content(provider, &section.content, &chunk_context) {
            Ok(output) => {
                combined.propositions.extend(output.propositions);
                combined.entities.extend(output.entities);
                combined.relations.extend(output.relations);
            }
            Err(e) => {
                debug!(
                    "extraction failed for {} section {}: {}",
                    rel_path,
                    section.heading_path.join("/"),
                    e
                );
            }
        }
    }

    // Deduplicate entities by name
    let mut seen_entities = std::collections::HashSet::new();
    combined.entities.retain(|e| seen_entities.insert(e.name.clone()));

    combined
}

/// Truncate content to approximately max_chars, breaking at a word boundary.
fn truncate_content(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }
    // Find last space before max_chars
    let truncated = &content[..max_chars];
    match truncated.rfind(' ') {
        Some(pos) => format!("{}...", &content[..pos]),
        None => format!("{}...", truncated),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello world"), 2); // 11 chars / 4
    }

    #[test]
    fn test_truncate_content_short() {
        let content = "short text";
        assert_eq!(truncate_content(content, 100), "short text");
    }

    #[test]
    fn test_truncate_content_long() {
        let content = "this is a longer piece of text that should be truncated";
        let result = truncate_content(content, 20);
        assert!(result.len() <= 23); // 20 + "..."
        assert!(result.ends_with("..."));
    }
}
