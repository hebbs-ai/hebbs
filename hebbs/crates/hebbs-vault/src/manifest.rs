use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Result, VaultError};

/// The manifest tracks the mapping between vault files and indexed state.
/// Stored at `.hebbs/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    /// Schema version.
    pub version: u32,
    /// Relative file path -> file entry.
    pub files: HashMap<String, FileEntry>,
}

/// Tracking state for a single markdown file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    /// SHA-256 checksum of the file, prefixed with "sha256:".
    pub checksum: String,
    /// When the file was last parsed (phase 1).
    pub last_parsed: DateTime<Utc>,
    /// When the file was last embedded (phase 2). None if never embedded.
    pub last_embedded: Option<DateTime<Utc>>,
    /// Sections extracted from this file.
    pub sections: Vec<SectionEntry>,
}

/// Tracking state for a single section within a file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SectionEntry {
    /// ULID for this section's memory in the engine.
    pub memory_id: String,
    /// Heading path (e.g., ["Design", "API"]).
    pub heading_path: Vec<String>,
    /// Byte offset of section start in the file.
    pub byte_start: usize,
    /// Byte offset of section end in the file.
    pub byte_end: usize,
    /// Current sync state.
    pub state: SectionState,
    /// SHA-256 of the section content (for detecting content changes
    /// even when byte offsets shift).
    pub content_checksum: String,
}

/// Sync state of a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionState {
    /// Parsed, byte offsets current, but embedding is outdated.
    ContentStale,
    /// Embedding matches content. Fully synced.
    Synced,
    /// Section was removed from the file. Pending `forget()`.
    Orphaned,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: 1,
            files: HashMap::new(),
        }
    }
}

impl Manifest {
    /// Create a new empty manifest.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load manifest from `.hebbs/manifest.json`.
    pub fn load(hebbs_dir: &Path) -> Result<Self> {
        let path = hebbs_dir.join("manifest.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let manifest: Self = serde_json::from_str(&content).map_err(|e| VaultError::Manifest {
            reason: format!("failed to parse manifest: {e}"),
        })?;
        Ok(manifest)
    }

    /// Save manifest atomically: write to `.tmp`, then rename.
    /// This ensures a crash between write and rename leaves the old manifest intact.
    pub fn save(&self, hebbs_dir: &Path) -> Result<()> {
        let path = hebbs_dir.join("manifest.json");
        let tmp_path = hebbs_dir.join("manifest.json.tmp");
        let content = serde_json::to_string_pretty(self).map_err(|e| VaultError::Manifest {
            reason: format!("failed to serialize manifest: {e}"),
        })?;
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    /// Build a reverse lookup: memory_id -> (relative file path, byte_start, byte_end).
    /// O(N) where N = total sections across all files.
    pub fn build_memory_index(&self) -> HashMap<String, (String, usize, usize)> {
        let mut index = HashMap::new();
        for (file_path, entry) in &self.files {
            for section in &entry.sections {
                if section.state != SectionState::Orphaned {
                    index.insert(
                        section.memory_id.clone(),
                        (file_path.clone(), section.byte_start, section.byte_end),
                    );
                }
            }
        }
        index
    }

    /// Count sections by state.
    pub fn section_counts(&self) -> (usize, usize, usize) {
        let mut synced = 0;
        let mut stale = 0;
        let mut orphaned = 0;
        for entry in self.files.values() {
            for section in &entry.sections {
                match section.state {
                    SectionState::Synced => synced += 1,
                    SectionState::ContentStale => stale += 1,
                    SectionState::Orphaned => orphaned += 1,
                }
            }
        }
        (synced, stale, orphaned)
    }

    /// Total section count (all states).
    pub fn total_sections(&self) -> usize {
        self.files.values().map(|e| e.sections.len()).sum()
    }
}

/// Compute SHA-256 checksum of bytes, returning "sha256:<hex>" string.
pub fn sha256_checksum(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(data);
    format!("sha256:{}", hex::encode(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_default() {
        let m = Manifest::default();
        assert_eq!(m.version, 1);
        assert!(m.files.is_empty());
    }

    #[test]
    fn test_manifest_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = Manifest::new();
        m.files.insert(
            "notes/test.md".to_string(),
            FileEntry {
                checksum: sha256_checksum(b"hello"),
                last_parsed: Utc::now(),
                last_embedded: None,
                sections: vec![SectionEntry {
                    memory_id: "01JABCDEF".to_string(),
                    heading_path: vec!["Introduction".to_string()],
                    byte_start: 0,
                    byte_end: 100,
                    state: SectionState::ContentStale,
                    content_checksum: sha256_checksum(b"content"),
                }],
            },
        );

        m.save(dir.path()).unwrap();
        let loaded = Manifest::load(dir.path()).unwrap();
        assert_eq!(m, loaded);
    }

    #[test]
    fn test_manifest_load_missing() {
        let dir = tempfile::tempdir().unwrap();
        let m = Manifest::load(dir.path()).unwrap();
        assert_eq!(m, Manifest::default());
    }

    #[test]
    fn test_atomic_save_no_corruption() {
        let dir = tempfile::tempdir().unwrap();
        // Write initial manifest
        let m1 = Manifest::new();
        m1.save(dir.path()).unwrap();

        // Write second manifest
        let mut m2 = Manifest::new();
        m2.files.insert(
            "test.md".to_string(),
            FileEntry {
                checksum: "sha256:abc".to_string(),
                last_parsed: Utc::now(),
                last_embedded: None,
                sections: Vec::new(),
            },
        );
        m2.save(dir.path()).unwrap();

        // .tmp file should not exist after successful save
        assert!(!dir.path().join("manifest.json.tmp").exists());

        let loaded = Manifest::load(dir.path()).unwrap();
        assert_eq!(loaded.files.len(), 1);
    }

    #[test]
    fn test_build_memory_index() {
        let mut m = Manifest::new();
        m.files.insert(
            "a.md".to_string(),
            FileEntry {
                checksum: "sha256:aaa".to_string(),
                last_parsed: Utc::now(),
                last_embedded: None,
                sections: vec![
                    SectionEntry {
                        memory_id: "id1".to_string(),
                        heading_path: vec!["A".to_string()],
                        byte_start: 0,
                        byte_end: 50,
                        state: SectionState::Synced,
                        content_checksum: "sha256:xxx".to_string(),
                    },
                    SectionEntry {
                        memory_id: "id2".to_string(),
                        heading_path: vec!["B".to_string()],
                        byte_start: 50,
                        byte_end: 100,
                        state: SectionState::Orphaned,
                        content_checksum: "sha256:yyy".to_string(),
                    },
                ],
            },
        );

        let idx = m.build_memory_index();
        assert!(idx.contains_key("id1"));
        assert!(!idx.contains_key("id2")); // orphaned, excluded
    }

    #[test]
    fn test_section_counts() {
        let mut m = Manifest::new();
        m.files.insert(
            "a.md".to_string(),
            FileEntry {
                checksum: "sha256:a".to_string(),
                last_parsed: Utc::now(),
                last_embedded: None,
                sections: vec![
                    SectionEntry {
                        memory_id: "1".into(),
                        heading_path: vec![],
                        byte_start: 0,
                        byte_end: 10,
                        state: SectionState::Synced,
                        content_checksum: "sha256:x".into(),
                    },
                    SectionEntry {
                        memory_id: "2".into(),
                        heading_path: vec![],
                        byte_start: 10,
                        byte_end: 20,
                        state: SectionState::ContentStale,
                        content_checksum: "sha256:y".into(),
                    },
                    SectionEntry {
                        memory_id: "3".into(),
                        heading_path: vec![],
                        byte_start: 20,
                        byte_end: 30,
                        state: SectionState::Orphaned,
                        content_checksum: "sha256:z".into(),
                    },
                ],
            },
        );
        let (synced, stale, orphaned) = m.section_counts();
        assert_eq!(synced, 1);
        assert_eq!(stale, 1);
        assert_eq!(orphaned, 1);
    }

    #[test]
    fn test_sha256_checksum() {
        let checksum = sha256_checksum(b"hello");
        assert!(checksum.starts_with("sha256:"));
        assert_eq!(checksum.len(), 7 + 64); // "sha256:" + 64 hex chars
    }
}
