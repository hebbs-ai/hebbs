//! File extraction pipeline for triple-layer ingestion.
//!
//! Each file produces:
//! - Layer 1: One Document memory (full content or LLM summary for large files)
//! - Layer 2: N Proposition memories (atomic facts extracted by LLM)
//! - Layer 3: Entity/relation edges (not stored as memories, but as graph edges)
//!
//! Small files (< large_file_threshold tokens) get full content as the Document memory.
//! Large files get an LLM summary as the Document memory, with per-heading-chunk extraction.

use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};
use tracing::{debug, warn};

use hebbs_core::engine::{Engine, RememberEdge, RememberInput};
use hebbs_core::memory::MemoryKind;
use hebbs_index::graph::EdgeType;
use hebbs_llm::extraction::{self, ExtractionOutput};
use hebbs_llm::LlmProvider;

use crate::config::ExtractionConfig;
use crate::parser::{strip_boilerplate, ParsedSection};

/// Output from extracting a single file.
#[derive(Debug, Default)]
pub struct FileExtractionResult {
    /// The engine-assigned ID for the Document memory.
    pub document_memory_id: Option<[u8; 16]>,
    /// The engine-assigned IDs for Proposition memories.
    pub proposition_memory_ids: Vec<[u8; 16]>,
    /// SHA-256 content hashes for each proposition (parallel to proposition_memory_ids).
    pub proposition_hashes: Vec<String>,
    /// Number of entities extracted.
    pub entities_extracted: usize,
    /// Number of relations extracted.
    pub relations_extracted: usize,
    /// Number of graph edges created from relations (Layer 3).
    pub edges_created: usize,
    /// Errors encountered during extraction.
    pub errors: usize,
}

/// Compute SHA-256 hash of proposition content, returned as hex string.
fn proposition_hash(content: &str) -> String {
    let hash = Sha256::digest(content.trim().as_bytes());
    hex::encode(hash)
}

/// Delete old extraction memories (document + propositions) for a file.
///
/// Called before re-extraction to clean up stale memories.
/// Returns the number of memories deleted.
pub fn delete_extraction_memories(
    engine: &Engine,
    document_memory_id: Option<&str>,
    proposition_memory_ids: &[String],
) -> usize {
    let mut deleted = 0;

    if let Some(doc_id_str) = document_memory_id {
        if let Ok(ulid) = doc_id_str.parse::<ulid::Ulid>() {
            let bytes = ulid.0.to_be_bytes();
            if engine.delete(&bytes).is_ok() {
                deleted += 1;
            }
        }
    }

    for prop_id_str in proposition_memory_ids {
        if let Ok(ulid) = prop_id_str.parse::<ulid::Ulid>() {
            let bytes = ulid.0.to_be_bytes();
            if engine.delete(&bytes).is_ok() {
                deleted += 1;
            }
        }
    }

    deleted
}

/// Estimate token count from character count (rough: 1 token ~ 4 chars).
fn estimate_tokens(content: &str) -> usize {
    content.len() / 4
}

/// Find the primary entity mentioned in a proposition's content.
///
/// Returns the entity name with the earliest occurrence in the text (case-insensitive).
/// This becomes the proposition's entity_id, enabling temporal recall by entity.
fn find_primary_entity(
    content: &str,
    entities: &[extraction::ExtractedEntity],
) -> Option<String> {
    let lower = content.to_lowercase();
    entities
        .iter()
        .filter_map(|e| {
            lower
                .find(&e.name.to_lowercase())
                .map(|pos| (pos, &e.name))
        })
        .min_by_key(|(pos, _)| *pos)
        .map(|(_, name)| name.to_lowercase())
}

/// Record which entities appear in a proposition's content.
///
/// Populates the entity-to-proposition-id mapping used later
/// to create relation edges between propositions.
fn record_entity_mentions(
    content: &str,
    entities: &[extraction::ExtractedEntity],
    memory_id: [u8; 16],
    entity_map: &mut HashMap<String, Vec<[u8; 16]>>,
) {
    let lower = content.to_lowercase();
    for entity in entities {
        if lower.contains(&entity.name.to_lowercase()) {
            entity_map
                .entry(entity.name.to_lowercase())
                .or_default()
                .push(memory_id);
        }
    }
}

/// Parameters for [`extract_and_store_file`].
pub struct ExtractFileParams<'a> {
    pub engine: &'a Engine,
    pub provider: &'a dyn LlmProvider,
    pub file_content: &'a str,
    pub rel_path: &'a str,
    pub sections: &'a [ParsedSection],
    pub config: &'a ExtractionConfig,
    pub existing_proposition_ids: &'a [String],
    pub existing_proposition_hashes: &'a [String],
    /// Pre-fetched extraction output from batch mode. If Some, skips LLM call.
    pub prefetched_extraction: Option<ExtractionOutput>,
}

/// Extract and store triple-layer memories for a single file.
///
/// The caller provides the file content (or per-section content for large files),
/// the LLM provider, and the engine. This function:
/// 1. Creates a Document memory (Layer 1)
/// 2. Extracts propositions via LLM and creates Proposition memories (Layer 2)
/// 3. Creates entity/relation graph edges (Layer 3)
///
/// Hash-based merge: existing proposition hashes are compared against new ones.
/// Matching propositions are kept (preserving reinforcement history), new ones
/// are stored, and missing ones are deleted.
///
/// Returns the IDs of all created memories for manifest tracking.
pub fn extract_and_store_file(params: ExtractFileParams<'_>) -> FileExtractionResult {
    let ExtractFileParams {
        engine,
        provider,
        file_content,
        rel_path,
        sections,
        config,
        existing_proposition_ids,
        existing_proposition_hashes,
        prefetched_extraction,
    } = params;
    let mut result = FileExtractionResult::default();

    // Strip boilerplate before processing
    let cleaned_content = strip_boilerplate(file_content);
    if cleaned_content.trim().is_empty() {
        return result;
    }

    let tokens = estimate_tokens(&cleaned_content);

    // Layer 1: Document memory
    let doc_content = if tokens > config.large_file_threshold {
        match extraction::summarize_content(provider, &cleaned_content) {
            Ok(summary) if !summary.is_empty() => summary,
            Ok(_) => truncate_content(&cleaned_content, config.large_file_threshold * 4),
            Err(e) => {
                warn!("LLM summarization failed for {}: {}", rel_path, e);
                truncate_content(&cleaned_content, config.large_file_threshold * 4)
            }
        }
    } else {
        cleaned_content.clone()
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

    // Layer 2+3: Extract propositions and entities via LLM (or use prefetched batch result)
    let extraction_output = if let Some(prefetched) = prefetched_extraction {
        prefetched
    } else if tokens > config.large_file_threshold {
        extract_large_file(provider, &cleaned_content, rel_path, sections, config)
    } else {
        match extraction::extract_from_content(provider, &cleaned_content, rel_path) {
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

    // Deduplicate propositions within this file by content hash
    let mut seen_hashes: HashSet<String> = HashSet::new();
    let existing_hash_set: HashSet<&str> = existing_proposition_hashes
        .iter()
        .map(|h| h.as_str())
        .collect();

    // Build map: existing hash -> existing memory ID (for keeping stable memories)
    let existing_hash_to_id: HashMap<&str, &str> = existing_proposition_hashes
        .iter()
        .zip(existing_proposition_ids.iter())
        .map(|(h, id)| (h.as_str(), id.as_str()))
        .collect();

    // Track which existing hashes were matched (to know which to delete)
    let mut matched_existing_hashes: HashSet<String> = HashSet::new();

    // Layer 3 prep: build entity-to-proposition mapping for relation edges
    let mut entity_to_prop_ids: HashMap<String, Vec<[u8; 16]>> = HashMap::new();

    for prop in propositions {
        if prop.content.trim().is_empty() {
            continue;
        }

        let hash = proposition_hash(&prop.content);

        // Skip duplicates within same file
        if !seen_hashes.insert(hash.clone()) {
            continue;
        }

        // Hash-based merge: if this proposition already exists, keep the old memory
        if existing_hash_set.contains(hash.as_str()) {
            matched_existing_hashes.insert(hash.clone());
            // Keep existing memory ID and hash
            if let Some(existing_id) = existing_hash_to_id.get(hash.as_str()) {
                if let Ok(ulid) = existing_id.parse::<ulid::Ulid>() {
                    let bytes = ulid.0.to_be_bytes();
                    result.proposition_memory_ids.push(bytes);
                    result.proposition_hashes.push(hash);
                    // Track entity mentions for relation edges (Layer 3)
                    record_entity_mentions(
                        &prop.content,
                        &extraction_output.entities,
                        bytes,
                        &mut entity_to_prop_ids,
                    );
                }
            }
            continue;
        }

        // New proposition: store it
        let mut prop_context = HashMap::new();
        prop_context.insert(
            "file_path".to_string(),
            serde_json::Value::String(rel_path.to_string()),
        );
        prop_context.insert(
            "layer".to_string(),
            serde_json::Value::String("proposition".to_string()),
        );
        prop_context.insert(
            "content_hash".to_string(),
            serde_json::Value::String(hash.clone()),
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

        // Layer 3: determine primary entity for this proposition
        let primary_entity =
            find_primary_entity(&prop.content, &extraction_output.entities);

        let prop_input = RememberInput {
            content: prop.content.clone(),
            importance: Some(0.5),
            context: Some(prop_context),
            entity_id: primary_entity,
            edges,
            kind: Some(MemoryKind::Proposition),
        };

        match engine.remember(prop_input) {
            Ok(memory) => {
                if memory.memory_id.len() == 16 {
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(&memory.memory_id);
                    result.proposition_memory_ids.push(arr);
                    result.proposition_hashes.push(hash);
                    // Track entity mentions for relation edges (Layer 3)
                    record_entity_mentions(
                        &prop.content,
                        &extraction_output.entities,
                        arr,
                        &mut entity_to_prop_ids,
                    );
                }
            }
            Err(e) => {
                debug!("failed to store proposition for {}: {}", rel_path, e);
                result.errors += 1;
            }
        }
    }

    // Delete old propositions whose hashes no longer appear (MISSING)
    for (i, old_hash) in existing_proposition_hashes.iter().enumerate() {
        if !matched_existing_hashes.contains(old_hash.as_str()) {
            if let Some(old_id) = existing_proposition_ids.get(i) {
                if let Ok(ulid) = old_id.parse::<ulid::Ulid>() {
                    let bytes = ulid.0.to_be_bytes();
                    if let Err(e) = engine.delete(&bytes) {
                        debug!("failed to delete stale proposition {}: {}", old_id, e);
                    }
                }
            }
        }
    }

    // Layer 3: Create relation edges between proposition memories
    let mut edges_created: usize = 0;
    for relation in &extraction_output.relations {
        let source_key = relation.source.to_lowercase();
        let target_key = relation.target.to_lowercase();

        let source_ids = entity_to_prop_ids.get(&source_key);
        let target_ids = entity_to_prop_ids.get(&target_key);

        if let (Some(src_ids), Some(tgt_ids)) = (source_ids, target_ids) {
            // Connect first proposition mentioning source to first mentioning target.
            // Avoids N*M edge explosion while capturing the relation.
            if let (Some(&src_id), Some(&tgt_id)) = (src_ids.first(), tgt_ids.first()) {
                if src_id != tgt_id {
                    if let Err(e) = engine.add_edge(
                        &src_id,
                        &tgt_id,
                        EdgeType::EntityRelation,
                        relation.confidence,
                    ) {
                        debug!(
                            "failed to create relation edge {} -[{}]-> {} in {}: {}",
                            relation.source, relation.relation_type, relation.target,
                            rel_path, e
                        );
                    } else {
                        edges_created += 1;
                    }
                }
            }
        }
    }

    result.entities_extracted = extraction_output.entities.len();
    result.relations_extracted = extraction_output.relations.len();
    result.edges_created = edges_created;

    result
}

/// Build LLM extraction requests for a file without calling the provider.
/// Used by batch mode to collect all requests before submitting them together.
/// Returns a list of (context, content) pairs that need extraction.
pub fn build_extraction_requests(
    file_content: &str,
    rel_path: &str,
    sections: &[ParsedSection],
    config: &ExtractionConfig,
) -> Vec<hebbs_llm::LlmRequest> {
    let cleaned_content = strip_boilerplate(file_content);
    if cleaned_content.trim().is_empty() {
        return Vec::new();
    }

    let tokens = estimate_tokens(&cleaned_content);

    if tokens > config.large_file_threshold {
        // Large file: one request per section
        let doc_context = format!(
            "Document: {}. This chunk is part of a larger document.",
            rel_path
        );
        if sections.is_empty() {
            return vec![extraction::build_extraction_request(
                &cleaned_content,
                &doc_context,
            )];
        }
        let mut requests = Vec::new();
        for section in sections {
            let cleaned = strip_boilerplate(&section.content);
            if cleaned.trim().is_empty() {
                continue;
            }
            let chunk_context = format!(
                "{} Section: {}",
                doc_context,
                section.heading_path.join(" > ")
            );
            requests.push(extraction::build_extraction_request(&cleaned, &chunk_context));
        }
        requests
    } else {
        // Small file: one request for the whole file
        vec![extraction::build_extraction_request(
            &cleaned_content,
            rel_path,
        )]
    }
}

/// Merge multiple extraction outputs into one (for large file chunks).
pub fn merge_extraction_outputs(outputs: Vec<ExtractionOutput>) -> ExtractionOutput {
    let mut combined = ExtractionOutput::default();
    for output in outputs {
        combined.propositions.extend(output.propositions);
        combined.entities.extend(output.entities);
        combined.relations.extend(output.relations);
    }
    // Deduplicate entities by name
    let mut seen_entities = std::collections::HashSet::new();
    combined
        .entities
        .retain(|e| seen_entities.insert(e.name.clone()));
    combined
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
        let cleaned = strip_boilerplate(&section.content);
        if cleaned.trim().is_empty() {
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

        match extraction::extract_from_content(provider, &cleaned, &chunk_context) {
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
    combined
        .entities
        .retain(|e| seen_entities.insert(e.name.clone()));

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

    #[test]
    fn test_proposition_hash_deterministic() {
        let h1 = proposition_hash("Hello world");
        let h2 = proposition_hash("Hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_proposition_hash_trims_whitespace() {
        let h1 = proposition_hash("Hello world");
        let h2 = proposition_hash("  Hello world  ");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_proposition_hash_different_content() {
        let h1 = proposition_hash("Hello world");
        let h2 = proposition_hash("Goodbye world");
        assert_ne!(h1, h2);
    }
}
