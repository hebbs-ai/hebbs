use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use tracing::{debug, info, warn};

use hebbs_core::engine::Engine;
use hebbs_embed::Embedder;

use crate::config::VaultConfig;
use crate::error::{Result, VaultError};
use crate::manifest::{sha256_checksum, FileEntry, Manifest, SectionEntry, SectionState};
use crate::parser::parse_markdown_file;

/// Result of a phase 1 ingest run.
#[derive(Debug, Default)]
pub struct Phase1Stats {
    pub files_processed: usize,
    pub files_skipped: usize,
    pub sections_new: usize,
    pub sections_modified: usize,
    pub sections_unchanged: usize,
    pub sections_orphaned: usize,
}

/// Result of a phase 2 ingest run.
#[derive(Debug, Default)]
pub struct Phase2Stats {
    pub sections_embedded: usize,
    pub sections_remembered: usize,
    pub sections_revised: usize,
    pub sections_forgotten: usize,
    pub embed_batches: usize,
    pub edges_created: usize,
    pub contradictions_found: usize,
    pub errors: usize,
}

pub type ProgressCallback = Box<dyn Fn(usize, usize, &str) + Send>;

/// Phase 1: Parse changed files and update manifest. Cheap, runs on every file change.
///
/// For each file:
/// 1. Compute checksum; skip if unchanged
/// 2. Parse into sections
/// 3. Diff against manifest (match by heading_path)
/// 4. Update manifest incrementally
///
/// Time complexity: O(F * S) where F = files, S = avg sections per file.
pub fn phase1_ingest(
    paths: &[PathBuf],
    vault_root: &Path,
    manifest: &mut Manifest,
    config: &VaultConfig,
) -> Result<Phase1Stats> {
    let mut stats = Phase1Stats::default();
    let split_level = config.split_level();

    for path in paths {
        let rel_path = path
            .strip_prefix(vault_root)
            .map_err(|_| VaultError::InvalidPath {
                reason: format!(
                    "{} is not inside vault root {}",
                    path.display(),
                    vault_root.display()
                ),
            })?
            .to_string_lossy()
            .to_string();

        // Normalize path separators
        let rel_path = rel_path.replace('\\', "/");

        // Read file and compute checksum
        let file_bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                warn!("skipping {}: {}", rel_path, e);
                continue;
            }
        };
        let file_checksum = sha256_checksum(&file_bytes);

        // Check if file is unchanged
        if let Some(existing) = manifest.files.get(&rel_path) {
            if existing.checksum == file_checksum {
                stats.files_skipped += 1;
                debug!("skipping unchanged file: {}", rel_path);
                continue;
            }
        }

        // Parse the file
        let parsed = match parse_markdown_file(path, split_level) {
            Ok(p) => p,
            Err(e) => {
                warn!("failed to parse {}: {}", rel_path, e);
                continue;
            }
        };

        // Get existing sections for diffing
        let existing_sections: HashMap<Vec<String>, SectionEntry> = manifest
            .files
            .get(&rel_path)
            .map(|e| {
                e.sections
                    .iter()
                    .filter(|s| s.state != SectionState::Orphaned)
                    .map(|s| (s.heading_path.clone(), s.clone()))
                    .collect()
            })
            .unwrap_or_default();

        // Build new sections list
        let mut new_sections = Vec::new();
        let mut matched_paths = std::collections::HashSet::new();

        for parsed_section in &parsed.sections {
            let content_checksum = sha256_checksum(parsed_section.content.as_bytes());

            if let Some(existing) = existing_sections.get(&parsed_section.heading_path) {
                matched_paths.insert(parsed_section.heading_path.clone());

                if existing.content_checksum == content_checksum {
                    // Unchanged content, but byte offsets may have shifted
                    let mut entry = existing.clone();
                    entry.byte_start = parsed_section.byte_start;
                    entry.byte_end = parsed_section.byte_end;
                    new_sections.push(entry);
                    stats.sections_unchanged += 1;
                } else {
                    // Modified content
                    new_sections.push(SectionEntry {
                        memory_id: existing.memory_id.clone(),
                        heading_path: parsed_section.heading_path.clone(),
                        byte_start: parsed_section.byte_start,
                        byte_end: parsed_section.byte_end,
                        state: SectionState::ContentStale,
                        content_checksum,
                    });
                    stats.sections_modified += 1;
                }
            } else {
                // New section
                let memory_id = ulid::Ulid::new().to_string();
                new_sections.push(SectionEntry {
                    memory_id,
                    heading_path: parsed_section.heading_path.clone(),
                    byte_start: parsed_section.byte_start,
                    byte_end: parsed_section.byte_end,
                    state: SectionState::ContentStale,
                    content_checksum,
                });
                stats.sections_new += 1;
            }
        }

        // Mark removed headings as orphaned
        for (heading_path, existing) in &existing_sections {
            if !matched_paths.contains(heading_path) {
                new_sections.push(SectionEntry {
                    state: SectionState::Orphaned,
                    ..existing.clone()
                });
                stats.sections_orphaned += 1;
            }
        }

        // Update manifest entry
        manifest.files.insert(
            rel_path.clone(),
            FileEntry {
                checksum: file_checksum,
                last_parsed: Utc::now(),
                last_embedded: manifest.files.get(&rel_path).and_then(|e| e.last_embedded),
                sections: new_sections,
                document_memory_id: None,
                proposition_memory_ids: Vec::new(),
                proposition_hashes: Vec::new(),
            },
        );

        stats.files_processed += 1;
    }

    Ok(stats)
}

/// Mark all sections of a deleted file as orphaned.
pub fn phase1_delete(path: &Path, vault_root: &Path, manifest: &mut Manifest) -> Result<usize> {
    let rel_path = path
        .strip_prefix(vault_root)
        .map_err(|_| VaultError::InvalidPath {
            reason: format!(
                "{} is not inside vault root {}",
                path.display(),
                vault_root.display()
            ),
        })?
        .to_string_lossy()
        .replace('\\', "/");

    let orphaned_count = if let Some(entry) = manifest.files.get_mut(&rel_path) {
        let count = entry
            .sections
            .iter()
            .filter(|s| s.state != SectionState::Orphaned)
            .count();
        for section in &mut entry.sections {
            section.state = SectionState::Orphaned;
        }
        count
    } else {
        0
    };

    Ok(orphaned_count)
}

/// Phase 2: File-first LLM extraction pipeline.
///
/// For each file with stale sections:
/// 1. LLM extracts propositions (hash-based merge preserves stable facts)
/// 2. LLM generates document summary
/// 3. Orphaned sections are deleted
///
/// LLM provider is REQUIRED. Returns error if not configured.
///
/// Time complexity: O(F * LLM_call) for extraction, O(N * D) for embedding.
pub async fn phase2_ingest(
    vault_root: &Path,
    manifest: &mut Manifest,
    engine: &Engine,
    embedder: &Arc<dyn Embedder>,
    config: &VaultConfig,
) -> Result<Phase2Stats> {
    phase2_ingest_inner(vault_root, manifest, engine, embedder, config, true, None).await
}

/// Phase 2 variant that skips LLM contradiction detection.
/// Used during initial full index where there are no existing memories to contradict against.
pub async fn phase2_ingest_no_contradict(
    vault_root: &Path,
    manifest: &mut Manifest,
    engine: &Engine,
    embedder: &Arc<dyn Embedder>,
    config: &VaultConfig,
) -> Result<Phase2Stats> {
    phase2_ingest_inner(vault_root, manifest, engine, embedder, config, false, None).await
}

/// Phase 2 with progress callback for file-level reporting.
pub async fn phase2_ingest_with_progress(
    vault_root: &Path,
    manifest: &mut Manifest,
    engine: &Engine,
    embedder: &Arc<dyn Embedder>,
    config: &VaultConfig,
    run_contradictions: bool,
    progress: Option<ProgressCallback>,
) -> Result<Phase2Stats> {
    phase2_ingest_inner(
        vault_root,
        manifest,
        engine,
        embedder,
        config,
        run_contradictions,
        progress,
    )
    .await
}

async fn phase2_ingest_inner(
    vault_root: &Path,
    manifest: &mut Manifest,
    engine: &Engine,
    _embedder: &Arc<dyn Embedder>,
    config: &VaultConfig,
    run_contradictions: bool,
    progress: Option<ProgressCallback>,
) -> Result<Phase2Stats> {
    // LLM provider is required for extraction
    let llm_provider: Arc<dyn hebbs_llm::LlmProvider> = if config.llm.is_configured() {
        match config.llm.create_provider() {
            Ok(p) => Arc::from(p),
            Err(e) => {
                return Err(VaultError::Config {
                    reason: format!("LLM provider required but failed to create: {e}"),
                });
            }
        }
    } else {
        return Err(VaultError::Config {
            reason: "LLM provider not configured. Run `hebbs init` with --provider/--model or `hebbs config set llm.provider <provider>`.".to_string(),
        });
    };

    let mut stats = Phase2Stats::default();

    // Collect files with stale sections, grouped by file path
    let mut stale_files: HashSet<String> = HashSet::new();
    let mut delete_ids: Vec<(String, String)> = Vec::new();
    let mut empty_content_ids: Vec<(String, String)> = Vec::new();

    for (rel_path, file_entry) in &manifest.files {
        for section in &file_entry.sections {
            match section.state {
                SectionState::ContentStale => {
                    // Check if the section has any body content
                    let file_path = vault_root.join(rel_path);
                    let content = match read_section_content(
                        &file_path,
                        section.byte_start,
                        section.byte_end,
                    ) {
                        Ok(c) => c,
                        Err(e) => {
                            warn!("failed to read section from {}: {}", rel_path, e);
                            stats.errors += 1;
                            continue;
                        }
                    };

                    if content.is_empty() {
                        empty_content_ids.push((rel_path.clone(), section.memory_id.clone()));
                    } else {
                        stale_files.insert(rel_path.clone());
                    }
                }
                SectionState::Orphaned => {
                    delete_ids.push((rel_path.clone(), section.memory_id.clone()));
                }
                SectionState::Synced => {}
            }
        }
    }

    let total_files = stale_files.len();
    let total_delete = delete_ids.len();
    info!(
        "phase2: {} file(s) to extract, {} orphaned section(s) to delete",
        total_files, total_delete
    );

    // Process each file: LLM extraction is the primary path
    let stale_files_sorted: Vec<String> = {
        let mut v: Vec<String> = stale_files.into_iter().collect();
        v.sort();
        v
    };

    // Pre-read file content and parse sections for all stale files
    struct FilePrep {
        rel_path: String,
        file_content: String,
        parsed_sections: Vec<crate::parser::ParsedSection>,
        existing_prop_ids: Vec<String>,
        existing_prop_hashes: Vec<String>,
        existing_doc_id: Option<String>,
    }

    let split_level = config.split_level();
    let mut file_preps: Vec<FilePrep> = Vec::with_capacity(stale_files_sorted.len());

    for rel_path in &stale_files_sorted {
        let file_path = vault_root.join(rel_path);
        let file_content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => {
                warn!("skipping extraction for {}: {}", rel_path, e);
                stats.errors += 1;
                continue;
            }
        };
        let parsed_sections = match crate::parser::parse_markdown_file(&file_path, split_level) {
            Ok(mut parsed) => {
                crate::parser::merge_short_sections(
                    &mut parsed.sections,
                    config.chunking.min_section_length,
                );
                parsed.sections
            }
            Err(_) => Vec::new(),
        };
        let (existing_prop_ids, existing_prop_hashes, existing_doc_id) =
            if let Some(file_entry) = manifest.files.get(rel_path.as_str()) {
                (
                    file_entry.proposition_memory_ids.clone(),
                    file_entry.proposition_hashes.clone(),
                    file_entry.document_memory_id.clone(),
                )
            } else {
                (Vec::new(), Vec::new(), None)
            };
        file_preps.push(FilePrep {
            rel_path: rel_path.clone(),
            file_content,
            parsed_sections,
            existing_prop_ids,
            existing_prop_hashes,
            existing_doc_id,
        });
    }

    // Batch extraction: collect all LLM requests, then submit as one batch
    // Each file may produce 1+ requests (large files split into chunks)
    let mut all_requests: Vec<hebbs_llm::LlmRequest> = Vec::new();
    let mut file_request_ranges: Vec<(usize, usize)> = Vec::new(); // (start, count) per file

    for prep in &file_preps {
        let requests = crate::extract::build_extraction_requests(
            &prep.file_content,
            &prep.rel_path,
            &prep.parsed_sections,
            &config.extraction,
        );
        let start = all_requests.len();
        let count = requests.len();
        all_requests.extend(requests);
        file_request_ranges.push((start, count));
    }

    // Submit batch (providers that support batch API use it; others fall back to sequential)
    info!(
        "phase2: submitting {} extraction requests for {} files (parallel)",
        all_requests.len(),
        file_preps.len()
    );
    // Default: concurrent real-time calls for speed.
    // Batch API (--batch flag) handled separately at the caller level.
    let all_results = llm_provider.complete_parallel(
        all_requests,
        Some(config.api.max_concurrent_requests),
    );
    let all_responses: Vec<hebbs_llm::LlmResponse> = all_results
        .into_iter()
        .map(|r| {
            r.unwrap_or_else(|e| {
                warn!("extraction request failed: {}", e);
                hebbs_llm::LlmResponse {
                    content: String::new(),
                }
            })
        })
        .collect();

    // Process each file with its batch results
    for (file_idx, prep) in file_preps.iter().enumerate() {
        let rel_path = &prep.rel_path;

        // Report progress
        if let Some(ref cb) = progress {
            cb(file_idx + 1, total_files, rel_path);
        }
        info!(
            "phase2: processing [{}/{}] {}",
            file_idx + 1,
            total_files,
            rel_path
        );

        // Reconstruct ExtractionOutput from batch responses for this file
        let prefetched = if !all_responses.is_empty() {
            let (start, count) = file_request_ranges[file_idx];
            if start + count <= all_responses.len() {
                let outputs: Vec<hebbs_llm::extraction::ExtractionOutput> = all_responses
                    [start..start + count]
                    .iter()
                    .map(hebbs_llm::extraction::parse_extraction_result)
                    .collect();
                if outputs.len() == 1 {
                    Some(outputs.into_iter().next().unwrap())
                } else {
                    Some(crate::extract::merge_extraction_outputs(outputs))
                }
            } else {
                None // batch didn't return enough results for this file
            }
        } else {
            None // batch failed entirely, extract_and_store_file will call LLM directly
        };

        // Delete old document memory (will be replaced by new one)
        if let Some(ref doc_id_str) = prep.existing_doc_id {
            if let Ok(ulid) = doc_id_str.parse::<ulid::Ulid>() {
                let bytes = ulid.0.to_be_bytes();
                let _ = engine.delete(&bytes);
            }
        }

        // Extract and store (uses prefetched batch result if available, else calls LLM directly)
        let extraction_result =
            crate::extract::extract_and_store_file(crate::extract::ExtractFileParams {
                engine,
                provider: llm_provider.as_ref(),
                file_content: &prep.file_content,
                rel_path,
                sections: &prep.parsed_sections,
                config: &config.extraction,
                existing_proposition_ids: &prep.existing_prop_ids,
                existing_proposition_hashes: &prep.existing_prop_hashes,
                prefetched_extraction: prefetched,
            });

        // Update manifest with document and proposition IDs
        if let Some(file_entry) = manifest.files.get_mut(rel_path.as_str()) {
            if let Some(doc_id) = extraction_result.document_memory_id {
                let ulid_str = ulid::Ulid::from_bytes(doc_id).to_string();
                file_entry.document_memory_id = Some(ulid_str);
            }
            file_entry.proposition_memory_ids = extraction_result
                .proposition_memory_ids
                .iter()
                .map(|id| ulid::Ulid::from_bytes(*id).to_string())
                .collect();
            file_entry.proposition_hashes = extraction_result.proposition_hashes.clone();

            // Only mark sections as synced if extraction succeeded (has propositions
            // or no errors). If extraction failed, leave as ContentStale so the next
            // index retries LLM extraction for this file.
            let extraction_succeeded = extraction_result.errors == 0
                || !extraction_result.proposition_memory_ids.is_empty();
            if extraction_succeeded {
                let now = Utc::now();
                for section in &mut file_entry.sections {
                    if section.state == SectionState::ContentStale {
                        section.state = SectionState::Synced;
                    }
                }
                file_entry.last_embedded = Some(now);
            }
        }

        if !extraction_result.proposition_memory_ids.is_empty() {
            info!(
                "extracted {} propositions from {}",
                extraction_result.proposition_memory_ids.len(),
                rel_path
            );
        }

        stats.sections_remembered += extraction_result.proposition_memory_ids.len();
        if extraction_result.document_memory_id.is_some() {
            stats.sections_embedded += 1;
        }
        stats.errors += extraction_result.errors;
        stats.edges_created += extraction_result.edges_created;

        // Run contradiction detection on the document memory.
        // Document memories represent file-level content and are the right
        // granularity for pairwise contradiction detection. Propositions are
        // atomic facts checked only if no document memory exists.
        if run_contradictions && config.contradiction.enabled {
            let contra_config = hebbs_core::contradict::ContradictionConfig {
                candidates_k: config.contradiction.candidates_k,
                min_similarity: config.contradiction.min_similarity,
                min_confidence: config.contradiction.min_confidence,
                enabled: true,
            };
            let llm_ref: Option<&dyn hebbs_llm::LlmProvider> = Some(llm_provider.as_ref());

            if let Some(doc_id) = extraction_result.document_memory_id {
                match engine.check_contradictions(&doc_id, &contra_config, llm_ref) {
                    Ok(result) => {
                        stats.contradictions_found +=
                            result.resolved_contradictions.len() + result.pending.len();
                    }
                    Err(e) => {
                        warn!(
                            "contradiction check failed for document in {}: {}",
                            rel_path, e
                        );
                    }
                }
            }
        }
    }

    // Process deletions (forget orphaned sections)
    if !delete_ids.is_empty() {
        info!(
            "phase2: forgetting {} orphaned section(s)...",
            delete_ids.len()
        );
    }
    for (rel_path, memory_id) in &delete_ids {
        let memory_id_bytes = match parse_ulid_to_bytes(memory_id) {
            Some(id) => id,
            None => {
                stats.errors += 1;
                continue;
            }
        };

        match engine.delete(&memory_id_bytes) {
            Ok(()) => {
                stats.sections_forgotten += 1;
            }
            Err(e) => {
                warn!("delete failed for {} in {}: {}", memory_id, rel_path, e);
                stats.errors += 1;
            }
        }
    }

    // Mark empty-content sections as synced
    for (rel_path, memory_id) in &empty_content_ids {
        if let Some(file_entry) = manifest.files.get_mut(rel_path.as_str()) {
            for section in &mut file_entry.sections {
                if section.memory_id == *memory_id && section.state == SectionState::ContentStale {
                    section.state = SectionState::Synced;
                }
            }
        }
    }

    // Remove orphaned sections that have been successfully forgotten
    for (rel_path, file_entry) in manifest.files.iter_mut() {
        file_entry.sections.retain(|s| {
            if s.state == SectionState::Orphaned {
                let was_forgotten = delete_ids
                    .iter()
                    .any(|(rp, mid)| rp == rel_path && *mid == s.memory_id);
                !was_forgotten
            } else {
                true
            }
        });
    }

    // Remove file entries with no sections left
    manifest.files.retain(|_, entry| !entry.sections.is_empty());

    Ok(stats)
}

/// Read section content from a file at byte offsets.
fn read_section_content(path: &Path, byte_start: usize, byte_end: usize) -> Result<String> {
    let bytes = std::fs::read(path)?;
    if byte_end > bytes.len() {
        return Err(VaultError::Manifest {
            reason: format!(
                "byte offsets {}..{} exceed file size {} for {}",
                byte_start,
                byte_end,
                bytes.len(),
                path.display()
            ),
        });
    }
    let slice = &bytes[byte_start..byte_end];
    let text = std::str::from_utf8(slice).map_err(|e| VaultError::Parse {
        path: path.to_path_buf(),
        reason: format!("invalid UTF-8 in section: {e}"),
    })?;

    // Strip heading line if present
    let content = if text.starts_with('#') {
        text.find('\n').map(|pos| &text[pos + 1..]).unwrap_or("")
    } else {
        text
    };

    Ok(content.trim().to_string())
}

/// Parse a ULID string to 16-byte array.
fn parse_ulid_to_bytes(ulid_str: &str) -> Option<[u8; 16]> {
    ulid_str
        .parse::<ulid::Ulid>()
        .ok()
        .map(|u| u.0.to_be_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase1_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.md");
        std::fs::write(&file_path, "## Hello\n\nWorld.\n").unwrap();

        let mut manifest = Manifest::new();
        let config = VaultConfig::default();

        let stats = phase1_ingest(&[file_path], dir.path(), &mut manifest, &config).unwrap();

        assert_eq!(stats.files_processed, 1);
        assert_eq!(stats.sections_new, 1);
        assert!(manifest.files.contains_key("test.md"));

        let entry = &manifest.files["test.md"];
        assert_eq!(entry.sections.len(), 1);
        assert_eq!(entry.sections[0].state, SectionState::ContentStale);
        assert_eq!(entry.sections[0].heading_path, vec!["Hello"]);
    }

    #[test]
    fn test_phase1_unchanged_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.md");
        std::fs::write(&file_path, "## Hello\n\nWorld.\n").unwrap();

        let mut manifest = Manifest::new();
        let config = VaultConfig::default();

        // First ingest
        phase1_ingest(
            std::slice::from_ref(&file_path),
            dir.path(),
            &mut manifest,
            &config,
        )
        .unwrap();

        // Second ingest (file unchanged)
        let stats = phase1_ingest(&[file_path], dir.path(), &mut manifest, &config).unwrap();
        assert_eq!(stats.files_skipped, 1);
        assert_eq!(stats.files_processed, 0);
    }

    #[test]
    fn test_phase1_modified_section() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.md");
        std::fs::write(&file_path, "## Hello\n\nWorld.\n").unwrap();

        let mut manifest = Manifest::new();
        let config = VaultConfig::default();

        phase1_ingest(
            std::slice::from_ref(&file_path),
            dir.path(),
            &mut manifest,
            &config,
        )
        .unwrap();
        let original_id = manifest.files["test.md"].sections[0].memory_id.clone();

        // Modify content
        std::fs::write(&file_path, "## Hello\n\nUpdated world.\n").unwrap();

        let stats = phase1_ingest(&[file_path], dir.path(), &mut manifest, &config).unwrap();
        assert_eq!(stats.sections_modified, 1);

        // Same memory_id (revise, not re-create)
        assert_eq!(manifest.files["test.md"].sections[0].memory_id, original_id);
        assert_eq!(
            manifest.files["test.md"].sections[0].state,
            SectionState::ContentStale
        );
    }

    #[test]
    fn test_phase1_deleted_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.md");
        std::fs::write(&file_path, "## Hello\n\nWorld.\n").unwrap();

        let mut manifest = Manifest::new();
        let config = VaultConfig::default();

        phase1_ingest(
            std::slice::from_ref(&file_path),
            dir.path(),
            &mut manifest,
            &config,
        )
        .unwrap();

        // Delete
        let orphaned = phase1_delete(&file_path, dir.path(), &mut manifest).unwrap();
        assert_eq!(orphaned, 1);
        assert_eq!(
            manifest.files["test.md"].sections[0].state,
            SectionState::Orphaned
        );
    }

    #[test]
    fn test_phase1_heading_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.md");
        std::fs::write(&file_path, "## Old Name\n\nContent.\n").unwrap();

        let mut manifest = Manifest::new();
        let config = VaultConfig::default();

        phase1_ingest(
            std::slice::from_ref(&file_path),
            dir.path(),
            &mut manifest,
            &config,
        )
        .unwrap();

        // Rename heading
        std::fs::write(&file_path, "## New Name\n\nContent.\n").unwrap();
        let stats = phase1_ingest(&[file_path], dir.path(), &mut manifest, &config).unwrap();

        assert_eq!(stats.sections_new, 1);
        assert_eq!(stats.sections_orphaned, 1);
    }

    #[test]
    fn test_parse_ulid_to_bytes() {
        let ulid = ulid::Ulid::new();
        let s = ulid.to_string();
        let bytes = parse_ulid_to_bytes(&s).unwrap();
        assert_eq!(bytes, ulid.0.to_be_bytes());
    }
}
