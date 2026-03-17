use std::path::{Path, PathBuf};

use chrono::Utc;
use tracing::{info, warn};

use hebbs_core::contradict::ClassifierMethod;

use crate::config::VaultConfig;
use crate::error::Result;
use crate::manifest::Manifest;

/// A contradiction to write to the vault as a markdown file.
#[derive(Debug, Clone)]
pub struct ContradictionOutput {
    /// Content of memory A (the newly ingested memory).
    pub content_a: String,
    /// Content of memory B (the contradicting existing memory).
    pub content_b: String,
    /// Memory ID of A.
    pub memory_id_a: [u8; 16],
    /// Memory ID of B.
    pub memory_id_b: [u8; 16],
    /// Classification confidence [0.0, 1.0].
    pub confidence: f32,
    /// Which classifier produced this result.
    pub method: ClassifierMethod,
}

/// Writes contradiction files to the vault's contradiction directory.
///
/// Each contradiction becomes a standalone `.md` file with `hebbs-*` frontmatter
/// containing both memory contents. The watcher ignores these files (contradiction_dir
/// is in the ignore set) so they don't get re-ingested.
pub struct ContradictionWriter<'a> {
    vault_root: &'a Path,
    manifest: &'a Manifest,
    config: &'a VaultConfig,
}

impl<'a> ContradictionWriter<'a> {
    pub fn new(vault_root: &'a Path, manifest: &'a Manifest, config: &'a VaultConfig) -> Self {
        Self {
            vault_root,
            manifest,
            config,
        }
    }

    /// Write contradiction files to the vault. Returns paths of created files.
    pub fn write_contradictions(
        &self,
        contradictions: &[ContradictionOutput],
    ) -> Result<Vec<PathBuf>> {
        if contradictions.is_empty() {
            return Ok(Vec::new());
        }

        let contradiction_dir = self.vault_root.join(&self.config.output.contradiction_dir);
        std::fs::create_dir_all(&contradiction_dir)?;

        let mut created_paths = Vec::new();

        for contradiction in contradictions {
            match self.write_single(contradiction, &contradiction_dir) {
                Ok(path) => {
                    info!("wrote contradiction: {}", path.display());
                    created_paths.push(path);
                }
                Err(e) => {
                    warn!("failed to write contradiction: {}", e);
                }
            }
        }

        Ok(created_paths)
    }

    fn write_single(
        &self,
        contradiction: &ContradictionOutput,
        contradiction_dir: &Path,
    ) -> Result<PathBuf> {
        let sources_a = self.resolve_source(&contradiction.memory_id_a);
        let sources_b = self.resolve_source(&contradiction.memory_id_b);

        let now = Utc::now().to_rfc3339();
        let classification = match contradiction.method {
            ClassifierMethod::Heuristic => "heuristic",
            ClassifierMethod::Llm => "llm",
        };

        let mut frontmatter = String::new();
        frontmatter.push_str("---\n");
        frontmatter.push_str("hebbs-kind: contradiction\n");
        frontmatter.push_str("hebbs-sources:\n");
        frontmatter.push_str(&format!("  - {}\n", sources_a));
        frontmatter.push_str(&format!("  - {}\n", sources_b));
        frontmatter.push_str(&format!(
            "hebbs-confidence: {:.2}\n",
            contradiction.confidence
        ));
        frontmatter.push_str(&format!("hebbs-classification: {}\n", classification));
        frontmatter.push_str(&format!("hebbs-created: {}\n", now));
        frontmatter.push_str("---\n\n");

        // Body: both memory contents separated by horizontal rule
        let body = format!(
            "{}\n\n---\n\n{}\n",
            contradiction.content_a.trim(),
            contradiction.content_b.trim()
        );

        let ulid_full = ulid::Ulid::new().to_string();
        let filename = format!("contradiction-{}.md", ulid_full);
        let file_path = contradiction_dir.join(&filename);

        let file_content = format!("{}{}", frontmatter, body);

        // Write atomically
        let tmp_path = contradiction_dir.join(format!(".{}.tmp", filename));
        std::fs::write(&tmp_path, &file_content)?;
        std::fs::rename(&tmp_path, &file_path)?;

        Ok(file_path)
    }

    /// Resolve a memory ID to a human-readable source path.
    fn resolve_source(&self, memory_id: &[u8; 16]) -> String {
        let memory_index = self.manifest.build_memory_index();
        let ulid = ulid::Ulid::from(u128::from_be_bytes(*memory_id));
        let ulid_str = ulid.to_string();

        if let Some((file_path, _, _)) = memory_index.get(&ulid_str) {
            if let Some(file_entry) = self.manifest.files.get(file_path) {
                if let Some(section) = file_entry.sections.iter().find(|s| s.memory_id == ulid_str)
                {
                    if section.heading_path.is_empty() {
                        return file_path.clone();
                    } else {
                        let heading = slug::slugify(section.heading_path.join("-"));
                        return format!("{}#{}", file_path, heading);
                    }
                }
            }
            return file_path.clone();
        }

        // Fallback: raw hex ID
        hex::encode(memory_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{FileEntry, Manifest, SectionEntry, SectionState};
    use chrono::Utc;

    fn make_contradiction(content_a: &str, content_b: &str) -> ContradictionOutput {
        ContradictionOutput {
            content_a: content_a.to_string(),
            content_b: content_b.to_string(),
            memory_id_a: [1u8; 16],
            memory_id_b: [2u8; 16],
            confidence: 0.85,
            method: ClassifierMethod::Heuristic,
        }
    }

    #[test]
    fn test_write_contradiction_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);

        let contradictions = vec![make_contradiction(
            "Vendor X is reliable and always delivers on time.",
            "Vendor X missed three consecutive deadlines.",
        )];

        let paths = writer.write_contradictions(&contradictions).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].exists());

        let content = std::fs::read_to_string(&paths[0]).unwrap();
        assert!(content.contains("hebbs-kind: contradiction"));
        assert!(content.contains("hebbs-confidence: 0.85"));
        assert!(content.contains("Vendor X is reliable"));
        assert!(content.contains("missed three consecutive"));
    }

    #[test]
    fn test_contradiction_frontmatter_format() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let contradictions = vec![make_contradiction("Statement A.", "Statement B.")];

        let paths = writer.write_contradictions(&contradictions).unwrap();
        let content = std::fs::read_to_string(&paths[0]).unwrap();

        assert!(content.starts_with("---\n"));
        assert!(content.contains("hebbs-kind: contradiction\n"));
        assert!(content.contains("hebbs-confidence: 0.85\n"));
        assert!(content.contains("hebbs-classification: heuristic\n"));
        assert!(content.contains("hebbs-created:"));
        assert!(content.contains("hebbs-sources:\n"));

        // Body: two statements separated by horizontal rule
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        assert_eq!(parts.len(), 3);
        let body = parts[2];
        assert!(body.contains("Statement A."));
        assert!(body.contains("Statement B."));
        // Body has its own separator
        assert!(body.matches("---").count() >= 1);
    }

    #[test]
    fn test_contradiction_dir_created_automatically() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = VaultConfig::default();
        config.output.contradiction_dir = "custom_contradictions/".to_string();
        let manifest = Manifest::new();

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let contradictions = vec![make_contradiction("A.", "B.")];

        writer.write_contradictions(&contradictions).unwrap();
        assert!(dir.path().join("custom_contradictions").exists());
    }

    #[test]
    fn test_empty_contradictions_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();
        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);

        let paths = writer.write_contradictions(&[]).unwrap();
        assert!(paths.is_empty());
    }

    #[test]
    fn test_source_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();

        let memory_ulid = ulid::Ulid::new();
        let memory_id_str = memory_ulid.to_string();
        let memory_id_bytes = memory_ulid.0.to_be_bytes();

        let mut manifest = Manifest::new();
        manifest.files.insert(
            "notes/meeting.md".to_string(),
            FileEntry {
                checksum: "sha256:abc".to_string(),
                last_parsed: Utc::now(),
                last_embedded: Some(Utc::now()),
                sections: vec![SectionEntry {
                    memory_id: memory_id_str.clone(),
                    heading_path: vec!["Vendor Evaluation".to_string()],
                    byte_start: 0,
                    byte_end: 100,
                    state: SectionState::Synced,
                    content_checksum: "sha256:def".to_string(),
                }],
                document_memory_id: None,
                proposition_memory_ids: Vec::new(),
            },
        );

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let source = writer.resolve_source(&memory_id_bytes);

        assert!(source.starts_with("notes/meeting.md#"));
    }

    #[test]
    fn test_llm_classification_label() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let contradictions = vec![ContradictionOutput {
            content_a: "A.".to_string(),
            content_b: "B.".to_string(),
            memory_id_a: [1u8; 16],
            memory_id_b: [2u8; 16],
            confidence: 0.92,
            method: ClassifierMethod::Llm,
        }];

        let paths = writer.write_contradictions(&contradictions).unwrap();
        let content = std::fs::read_to_string(&paths[0]).unwrap();
        assert!(content.contains("hebbs-classification: llm\n"));
    }

    #[test]
    fn test_multiple_contradictions() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let contradictions = vec![
            make_contradiction("A1.", "B1."),
            make_contradiction("A2.", "B2."),
            make_contradiction("A3.", "B3."),
        ];

        let paths = writer.write_contradictions(&contradictions).unwrap();
        assert_eq!(paths.len(), 3);
        for path in &paths {
            assert!(path.exists());
            assert!(path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("contradiction-"));
        }
    }

    #[test]
    fn test_source_resolution_unknown_memory_falls_back_to_hex() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new(); // empty manifest, no files

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let unknown_id = [0xABu8; 16];
        let source = writer.resolve_source(&unknown_id);

        // Should fall back to hex encoding
        assert_eq!(source, hex::encode([0xAB; 16]));
    }

    #[test]
    fn test_source_resolution_no_heading_path() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();

        let memory_ulid = ulid::Ulid::new();
        let memory_id_str = memory_ulid.to_string();
        let memory_id_bytes = memory_ulid.0.to_be_bytes();

        let mut manifest = Manifest::new();
        manifest.files.insert(
            "notes/plain.md".to_string(),
            FileEntry {
                checksum: "sha256:abc".to_string(),
                last_parsed: Utc::now(),
                last_embedded: Some(Utc::now()),
                sections: vec![SectionEntry {
                    memory_id: memory_id_str,
                    heading_path: vec![], // no heading
                    byte_start: 0,
                    byte_end: 50,
                    state: SectionState::Synced,
                    content_checksum: "sha256:def".to_string(),
                }],
                document_memory_id: None,
                proposition_memory_ids: Vec::new(),
            },
        );

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let source = writer.resolve_source(&memory_id_bytes);

        // Should return file path without fragment
        assert_eq!(source, "notes/plain.md");
    }

    #[test]
    fn test_content_with_special_characters() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let contradictions = vec![make_contradiction(
            "Temperature was 98.6\u{00b0}F -- \"normal\" range.",
            "Patient had a fever of 103\u{00b0}F; clearly abnormal!",
        )];

        let paths = writer.write_contradictions(&contradictions).unwrap();
        let content = std::fs::read_to_string(&paths[0]).unwrap();
        assert!(content.contains("98.6"));
        assert!(content.contains("103"));
        assert!(content.contains("hebbs-kind: contradiction"));
    }

    #[test]
    fn test_whitespace_trimming_in_body() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let contradictions = vec![make_contradiction(
            "  leading and trailing whitespace  ",
            "\n\n  also whitespace here \n\n",
        )];

        let paths = writer.write_contradictions(&contradictions).unwrap();
        let content = std::fs::read_to_string(&paths[0]).unwrap();

        // Body content should be trimmed
        assert!(content.contains("leading and trailing whitespace\n"));
        assert!(content.contains("also whitespace here\n"));
        // Should not have double blank lines from untrimmed content
        assert!(!content.contains("\n\n\n\n"));
    }

    #[test]
    fn test_confidence_formatting() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let contradictions = vec![ContradictionOutput {
            content_a: "A.".to_string(),
            content_b: "B.".to_string(),
            memory_id_a: [1u8; 16],
            memory_id_b: [2u8; 16],
            confidence: 0.7,
            method: ClassifierMethod::Heuristic,
        }];

        let paths = writer.write_contradictions(&contradictions).unwrap();
        let content = std::fs::read_to_string(&paths[0]).unwrap();
        // Should format to 2 decimal places
        assert!(content.contains("hebbs-confidence: 0.70\n"));
    }

    #[test]
    fn test_file_is_valid_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);
        let contradictions = vec![make_contradiction(
            "The project is on schedule.",
            "The project is three weeks behind.",
        )];

        let paths = writer.write_contradictions(&contradictions).unwrap();
        let content = std::fs::read_to_string(&paths[0]).unwrap();

        // Valid YAML frontmatter: starts with ---, has closing ---
        assert!(content.starts_with("---\n"));
        let frontmatter_end = content[4..].find("---\n").unwrap() + 4;
        let after_frontmatter = &content[frontmatter_end + 4..];

        // After frontmatter, body should contain both memories
        assert!(after_frontmatter.contains("on schedule"));
        assert!(after_frontmatter.contains("three weeks behind"));
    }

    #[test]
    fn test_multiple_writes_produce_separate_files() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = ContradictionWriter::new(dir.path(), &manifest, &config);

        let paths1 = writer
            .write_contradictions(&[make_contradiction("A.", "B.")])
            .unwrap();
        let paths2 = writer
            .write_contradictions(&[make_contradiction("C.", "D.")])
            .unwrap();

        // Both files should exist (even if ULID prefix collides,
        // the randomness portion of ULID ensures uniqueness)
        assert!(paths1[0].exists());
        assert!(paths2[0].exists());

        // Verify contents differ
        let c1 = std::fs::read_to_string(&paths1[0]).unwrap();
        let c2 = std::fs::read_to_string(&paths2[0]).unwrap();
        assert!(c1.contains("A."));
        assert!(c2.contains("C."));
    }
}
