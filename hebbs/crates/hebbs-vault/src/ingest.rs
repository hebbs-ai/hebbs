use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use tracing::{debug, warn};

use hebbs_core::engine::Engine;
use hebbs_embed::Embedder;

use crate::config::VaultConfig;
use crate::contradiction_writer::{ContradictionOutput, ContradictionWriter};
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
    pub contradiction_files_written: usize,
    pub errors: usize,
}

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

/// Phase 2: Embed content-stale sections and push to engine.
///
/// 1. Collect all ContentStale/Orphaned sections
/// 2. Read content from files at byte offsets
/// 3. Batch embed
/// 4. Call engine remember/revise/delete
/// 5. Resolve wiki-links to RELATED_TO edges
///
/// Time complexity: O(N * D) for embedding where N = sections, D = dimensions.
pub async fn phase2_ingest(
    vault_root: &Path,
    manifest: &mut Manifest,
    engine: &Engine,
    embedder: &Arc<dyn Embedder>,
    config: &VaultConfig,
) -> Result<Phase2Stats> {
    let mut stats = Phase2Stats::default();

    // Collect work items
    let mut new_items: Vec<(String, String, String, SectionWorkItem)> = Vec::new(); // (rel_path, memory_id, content, work)
    let mut modified_items: Vec<(String, String, String, SectionWorkItem)> = Vec::new();
    let mut delete_ids: Vec<(String, String)> = Vec::new(); // (rel_path, memory_id)

    for (rel_path, file_entry) in &manifest.files {
        for section in &file_entry.sections {
            match section.state {
                SectionState::ContentStale => {
                    // Read content from file
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

                    // Determine if this is new (no memory in engine) or modified
                    let memory_id_bytes = parse_ulid_to_bytes(&section.memory_id);
                    let is_new = memory_id_bytes
                        .as_ref()
                        .map(|id| engine.get(id).is_err())
                        .unwrap_or(true);

                    let work = SectionWorkItem {
                        heading_path: section.heading_path.clone(),
                    };

                    if is_new {
                        new_items.push((
                            rel_path.clone(),
                            section.memory_id.clone(),
                            content,
                            work,
                        ));
                    } else {
                        modified_items.push((
                            rel_path.clone(),
                            section.memory_id.clone(),
                            content,
                            work,
                        ));
                    }
                }
                SectionState::Orphaned => {
                    delete_ids.push((rel_path.clone(), section.memory_id.clone()));
                }
                SectionState::Synced => {}
            }
        }
    }

    // Batch embed all new + modified sections
    let all_texts: Vec<String> = new_items
        .iter()
        .chain(modified_items.iter())
        .map(|(_, _, content, _)| content.clone())
        .collect();

    let mut all_embeddings: Vec<Vec<f32>> = Vec::new();
    if !all_texts.is_empty() {
        let batch_size = config.embedding.batch_size;
        for chunk in all_texts.chunks(batch_size) {
            let text_refs: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
            match embedder.embed_batch(&text_refs) {
                Ok(embeddings) => {
                    all_embeddings.extend(embeddings);
                    stats.embed_batches += 1;
                }
                Err(e) => {
                    warn!("embedding batch failed: {}", e);
                    stats.errors += 1;
                    // Fill with empty vecs to keep index alignment
                    all_embeddings.extend(std::iter::repeat_with(Vec::new).take(chunk.len()));
                }
            }
        }
        stats.sections_embedded = all_embeddings.iter().filter(|e| !e.is_empty()).count();
    }

    // Track successfully processed sections by their ORIGINAL memory_id
    // (before engine.remember() assigns a new one).
    let mut processed_ids: HashSet<(String, String)> = HashSet::new();

    // Collect contradiction outputs for file writing (done after manifest updates)
    let mut contradiction_outputs: Vec<ContradictionOutput> = Vec::new();

    // Process new items (remember)
    let mut embed_idx = 0;
    for (rel_path, memory_id, content, work) in &new_items {
        let embedding = all_embeddings.get(embed_idx).cloned().unwrap_or_default();
        embed_idx += 1;

        if embedding.is_empty() {
            stats.errors += 1;
            continue;
        }

        // Build context from heading path and file path
        let mut context = HashMap::new();
        context.insert(
            "file_path".to_string(),
            serde_json::Value::String(rel_path.clone()),
        );
        if !work.heading_path.is_empty() {
            context.insert(
                "heading_path".to_string(),
                serde_json::json!(work.heading_path),
            );
        }

        let input = hebbs_core::engine::RememberInput {
            content: content.clone(),
            importance: Some(0.5),
            context: Some(context),
            entity_id: None,
            edges: Vec::new(),
        };

        match engine.remember(input) {
            Ok(memory) => {
                // Update manifest's memory_id to match the engine's assigned ID
                let engine_id_bytes = &memory.memory_id;
                let assigned_id = if engine_id_bytes.len() == 16 {
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(engine_id_bytes);

                    // Run contradiction detection on the new memory
                    if config.contradiction.enabled {
                        let core_config = config.contradiction.to_core_config();
                        match engine.check_contradictions(&arr, &core_config, None) {
                            Ok(contradictions) => {
                                if !contradictions.is_empty() {
                                    debug!(
                                        "found {} contradiction(s) for memory {}",
                                        contradictions.len(),
                                        hex::encode(arr),
                                    );
                                    stats.contradictions_found += contradictions.len();

                                    // Collect for file output
                                    for c in &contradictions {
                                        if let Ok(other_mem) = engine.get(&c.memory_id_b) {
                                            contradiction_outputs.push(ContradictionOutput {
                                                content_a: content.clone(),
                                                content_b: other_mem.content.clone(),
                                                memory_id_a: c.memory_id_a,
                                                memory_id_b: c.memory_id_b,
                                                confidence: c.confidence,
                                                method: c.method,
                                            });
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                debug!(
                                    "contradiction check failed for {}: {}",
                                    hex::encode(arr),
                                    e
                                );
                            }
                        }
                    }

                    let engine_ulid = ulid::Ulid::from_bytes(arr).to_string();
                    // Find and update the section in manifest
                    if let Some(file_entry) = manifest.files.get_mut(rel_path.as_str()) {
                        for sec in &mut file_entry.sections {
                            if sec.memory_id == *memory_id {
                                sec.memory_id = engine_ulid.clone();
                                break;
                            }
                        }
                    }
                    engine_ulid
                } else {
                    memory_id.clone()
                };
                // Track the NEW (engine-assigned) memory_id since that's what
                // the manifest section now holds after the update above.
                processed_ids.insert((rel_path.clone(), assigned_id));
                stats.sections_remembered += 1;
            }
            Err(e) => {
                warn!("remember failed for {}: {}", memory_id, e);
                stats.errors += 1;
            }
        }
    }

    // Process modified items (revise)
    for (rel_path, memory_id, content, work) in &modified_items {
        let embedding = all_embeddings.get(embed_idx).cloned().unwrap_or_default();
        embed_idx += 1;

        if embedding.is_empty() {
            stats.errors += 1;
            continue;
        }

        let memory_id_bytes = match parse_ulid_to_bytes(memory_id) {
            Some(id) => id,
            None => {
                stats.errors += 1;
                continue;
            }
        };

        let mut context = HashMap::new();
        context.insert(
            "file_path".to_string(),
            serde_json::Value::String(rel_path.clone()),
        );
        if !work.heading_path.is_empty() {
            context.insert(
                "heading_path".to_string(),
                serde_json::json!(work.heading_path),
            );
        }

        let input = hebbs_core::revise::ReviseInput {
            memory_id: memory_id_bytes.to_vec(),
            content: Some(content.clone()),
            importance: None,
            context: Some(context),
            context_mode: hebbs_core::revise::ContextMode::Merge,
            entity_id: None,
            edges: Vec::new(),
        };

        match engine.revise(input) {
            Ok(_) => {
                processed_ids.insert((rel_path.clone(), memory_id.clone()));
                stats.sections_revised += 1;
            }
            Err(e) => {
                warn!("revise failed for {}: {}", memory_id, e);
                stats.errors += 1;
            }
        }
    }

    // Process deletions (forget)
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

    // Update manifest: mark processed sections as Synced, remove fully-orphaned files
    for (rel_path, file_entry) in manifest.files.iter_mut() {
        let now = Utc::now();
        let mut any_embedded = false;

        for section in &mut file_entry.sections {
            match section.state {
                SectionState::ContentStale => {
                    // Check if we successfully processed it
                    let was_processed =
                        processed_ids.contains(&(rel_path.clone(), section.memory_id.clone()));
                    if was_processed {
                        section.state = SectionState::Synced;
                        any_embedded = true;
                    }
                }
                _ => {}
            }
        }

        if any_embedded {
            file_entry.last_embedded = Some(now);
        }

        // Remove orphaned sections that have been successfully forgotten
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

    // Write contradiction files (after all manifest mutations are done)
    if !contradiction_outputs.is_empty() {
        let writer = ContradictionWriter::new(vault_root, manifest, config);
        match writer.write_contradictions(&contradiction_outputs) {
            Ok(paths) => {
                stats.contradiction_files_written = paths.len();
            }
            Err(e) => {
                warn!("failed to write contradiction files: {}", e);
            }
        }
    }

    Ok(stats)
}

struct SectionWorkItem {
    heading_path: Vec<String>,
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
        phase1_ingest(&[file_path.clone()], dir.path(), &mut manifest, &config).unwrap();

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

        phase1_ingest(&[file_path.clone()], dir.path(), &mut manifest, &config).unwrap();
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

        phase1_ingest(&[file_path.clone()], dir.path(), &mut manifest, &config).unwrap();

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

        phase1_ingest(&[file_path.clone()], dir.path(), &mut manifest, &config).unwrap();

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
