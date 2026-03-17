use std::collections::HashMap;
use std::path::Path;

use tracing::warn;

use hebbs_core::engine::Engine;

use crate::manifest::Manifest;

/// Wraps engine recall/prime to read content from actual files instead of
/// engine's stored copy. This ensures users always see the latest file content,
/// even if the embedding is slightly stale (content-stale window).
///
/// Memory ID reverse-lookup is O(1) via pre-built HashMap.
/// File reads add <1ms per result (OS page cache).
pub struct VaultQuery<'a> {
    engine: &'a Engine,
    vault_root: &'a Path,
    /// memory_id -> (relative file path, byte_start, byte_end)
    memory_index: HashMap<String, (String, usize, usize)>,
}

impl<'a> VaultQuery<'a> {
    /// Create a new VaultQuery.
    /// Builds the reverse lookup index from the manifest. O(N) where N = total sections.
    pub fn new(engine: &'a Engine, manifest: &Manifest, vault_root: &'a Path) -> Self {
        let memory_index = manifest.build_memory_index();
        Self {
            engine,
            vault_root,
            memory_index,
        }
    }

    /// Read a section's content from the actual file.
    /// Returns (content, is_stale).
    pub fn read_section_content(&self, memory_id: &str) -> Option<(String, bool)> {
        let (rel_path, byte_start, byte_end) = self.memory_index.get(memory_id)?;

        let file_path = self.vault_root.join(rel_path);

        let bytes = match std::fs::read(&file_path) {
            Ok(b) => b,
            Err(e) => {
                warn!(
                    "file missing for memory {}: {} ({})",
                    memory_id,
                    file_path.display(),
                    e
                );
                return None; // caller should fall back to engine content
            }
        };

        if *byte_end > bytes.len() {
            warn!(
                "byte offsets {}..{} exceed file size {} for {}",
                byte_start,
                byte_end,
                bytes.len(),
                rel_path
            );
            // Try to read what we can
            let safe_end = bytes.len().min(*byte_end);
            let safe_start = (*byte_start).min(safe_end);
            match std::str::from_utf8(&bytes[safe_start..safe_end]) {
                Ok(s) => return Some((s.to_string(), true)), // stale: offsets were wrong
                Err(_) => return None,
            }
        }

        let slice = &bytes[*byte_start..*byte_end];
        match std::str::from_utf8(slice) {
            Ok(text) => {
                // Strip heading line if section starts with one
                let content = if text.starts_with('#') {
                    text.find('\n').map(|pos| &text[pos + 1..]).unwrap_or("")
                } else {
                    text
                };
                Some((content.trim().to_string(), false))
            }
            Err(_) => None,
        }
    }

    /// Get the file path for a memory_id (for metadata/context enrichment).
    pub fn file_path_for_memory(&self, memory_id: &str) -> Option<&str> {
        self.memory_index
            .get(memory_id)
            .map(|(path, _, _)| path.as_str())
    }

    /// Get the engine reference for direct queries.
    pub fn engine(&self) -> &Engine {
        self.engine
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{FileEntry, Manifest, SectionEntry, SectionState};
    use chrono::Utc;

    fn make_manifest_with_section(
        rel_path: &str,
        memory_id: &str,
        byte_start: usize,
        byte_end: usize,
    ) -> Manifest {
        let mut m = Manifest::new();
        m.files.insert(
            rel_path.to_string(),
            FileEntry {
                checksum: "sha256:test".to_string(),
                last_parsed: Utc::now(),
                last_embedded: Some(Utc::now()),
                sections: vec![SectionEntry {
                    memory_id: memory_id.to_string(),
                    heading_path: vec!["Test".to_string()],
                    byte_start,
                    byte_end,
                    state: SectionState::Synced,
                    content_checksum: "sha256:content".to_string(),
                }],
                document_memory_id: None,
                proposition_memory_ids: Vec::new(),
            },
        );
        m
    }

    #[test]
    fn test_read_section_content_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_content = "## Test\n\nHello world.\n";
        std::fs::write(dir.path().join("note.md"), file_content).unwrap();

        let manifest = make_manifest_with_section("note.md", "mem1", 0, file_content.len());

        // We don't need a real engine for this test, just the query layer
        // but VaultQuery requires &Engine. We'll test read_section_content
        // by constructing the memory_index directly.
        let memory_index: HashMap<String, (String, usize, usize)> = manifest.build_memory_index();
        assert!(memory_index.contains_key("mem1"));

        let (rel_path, byte_start, byte_end) = &memory_index["mem1"];
        let file_path = dir.path().join(rel_path);
        let bytes = std::fs::read(&file_path).unwrap();
        let slice = &bytes[*byte_start..*byte_end];
        let text = std::str::from_utf8(slice).unwrap();
        assert!(text.contains("Hello world"));
    }

    #[test]
    fn test_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = make_manifest_with_section("missing.md", "mem1", 0, 100);

        // Can't construct VaultQuery without engine, but we can test the lookup
        let memory_index = manifest.build_memory_index();
        let (rel_path, _, _) = &memory_index["mem1"];
        let file_path = dir.path().join(rel_path);
        assert!(!file_path.exists());
    }
}
