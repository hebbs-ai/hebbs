use std::path::{Path, PathBuf};

use chrono::Utc;
use tracing::{info, warn};

use crate::config::VaultConfig;
use crate::error::Result;
use crate::manifest::Manifest;

/// An insight to write to the vault as a markdown file.
#[derive(Debug, Clone)]
pub struct InsightOutput {
    /// The insight text content.
    pub content: String,
    /// Source memory IDs that contributed to this insight.
    pub source_memory_ids: Vec<Vec<u8>>,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f32,
}

/// Writes insight files to the vault's insight directory.
///
/// Each insight becomes a standalone `.md` file with `hebbs-*` frontmatter.
/// The watcher picks up these files and indexes them like any other note,
/// closing the loop: files -> index -> reflect -> new files -> index.
pub struct InsightWriter<'a> {
    vault_root: &'a Path,
    manifest: &'a Manifest,
    config: &'a VaultConfig,
}

impl<'a> InsightWriter<'a> {
    pub fn new(vault_root: &'a Path, manifest: &'a Manifest, config: &'a VaultConfig) -> Self {
        Self {
            vault_root,
            manifest,
            config,
        }
    }

    /// Write insight files to the vault. Returns paths of created files.
    pub fn write_insights(&self, insights: &[InsightOutput]) -> Result<Vec<PathBuf>> {
        if insights.is_empty() {
            return Ok(Vec::new());
        }

        let insight_dir = self.vault_root.join(&self.config.output.insight_dir);
        std::fs::create_dir_all(&insight_dir)?;

        let mut created_paths = Vec::new();

        for insight in insights {
            match self.write_single_insight(insight, &insight_dir) {
                Ok(path) => {
                    info!("wrote insight: {}", path.display());
                    created_paths.push(path);
                }
                Err(e) => {
                    warn!("failed to write insight: {}", e);
                }
            }
        }

        Ok(created_paths)
    }

    fn write_single_insight(&self, insight: &InsightOutput, insight_dir: &Path) -> Result<PathBuf> {
        // Resolve source memory IDs to human-readable file paths
        let sources = self.resolve_sources(&insight.source_memory_ids);

        // Build frontmatter
        let now = Utc::now().to_rfc3339();
        let mut frontmatter = String::new();
        frontmatter.push_str("---\n");
        frontmatter.push_str("hebbs-kind: insight\n");

        if !sources.is_empty() {
            frontmatter.push_str("hebbs-sources:\n");
            for source in &sources {
                frontmatter.push_str(&format!("  - {}\n", source));
            }
        }

        frontmatter.push_str(&format!("hebbs-confidence: {:.2}\n", insight.confidence));
        frontmatter.push_str(&format!("hebbs-created: {}\n", now));
        frontmatter.push_str("---\n\n");

        // Generate filename
        let ulid_prefix = &ulid::Ulid::new().to_string()[..8];
        let content_slug = slug::slugify(insight.content.chars().take(50).collect::<String>());
        let filename = format!("{}-{}.md", ulid_prefix, content_slug);

        let file_path = insight_dir.join(&filename);

        // Build file content
        let file_content = format!("{}{}\n", frontmatter, insight.content);

        // Write atomically
        let tmp_path = insight_dir.join(format!(".{}.tmp", filename));
        std::fs::write(&tmp_path, &file_content)?;
        std::fs::rename(&tmp_path, &file_path)?;

        Ok(file_path)
    }

    /// Resolve memory IDs to human-readable source paths like "notes/meeting.md#heading".
    fn resolve_sources(&self, memory_ids: &[Vec<u8>]) -> Vec<String> {
        let memory_index = self.manifest.build_memory_index();
        let mut sources = Vec::new();

        for id_bytes in memory_ids {
            // Convert bytes to ULID string for lookup
            if id_bytes.len() == 16 {
                let mut arr = [0u8; 16];
                arr.copy_from_slice(id_bytes);
                let ulid = ulid::Ulid::from(u128::from_be_bytes(arr));
                let ulid_str = ulid.to_string();

                if let Some((file_path, _, _)) = memory_index.get(&ulid_str) {
                    // Find heading path for this section
                    if let Some(file_entry) = self.manifest.files.get(file_path) {
                        if let Some(section) =
                            file_entry.sections.iter().find(|s| s.memory_id == ulid_str)
                        {
                            if section.heading_path.is_empty() {
                                sources.push(file_path.clone());
                            } else {
                                let heading = slug::slugify(section.heading_path.join("-"));
                                sources.push(format!("{}#{}", file_path, heading));
                            }
                            continue;
                        }
                    }
                    sources.push(file_path.clone());
                } else {
                    // Memory not in manifest, use raw ID
                    sources.push(ulid_str);
                }
            }
        }

        sources
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{FileEntry, Manifest, SectionEntry, SectionState};
    use chrono::Utc;

    #[test]
    fn test_write_insight_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = InsightWriter::new(dir.path(), &manifest, &config);

        let insights = vec![InsightOutput {
            content: "Vendor assessment revealed a pattern of missed deadlines.".to_string(),
            source_memory_ids: Vec::new(),
            confidence: 0.82,
        }];

        let paths = writer.write_insights(&insights).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].exists());

        let content = std::fs::read_to_string(&paths[0]).unwrap();
        assert!(content.contains("hebbs-kind: insight"));
        assert!(content.contains("hebbs-confidence: 0.82"));
        assert!(content.contains("Vendor assessment"));
    }

    #[test]
    fn test_insight_frontmatter_format() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();

        let writer = InsightWriter::new(dir.path(), &manifest, &config);

        let insights = vec![InsightOutput {
            content: "Test insight.".to_string(),
            source_memory_ids: Vec::new(),
            confidence: 0.95,
        }];

        let paths = writer.write_insights(&insights).unwrap();
        let content = std::fs::read_to_string(&paths[0]).unwrap();

        // Verify frontmatter structure
        assert!(content.starts_with("---\n"));
        assert!(content.contains("hebbs-kind: insight\n"));
        assert!(content.contains("hebbs-confidence: 0.95\n"));
        assert!(content.contains("hebbs-created:"));
        // Body after frontmatter
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        assert_eq!(parts.len(), 3);
        let body = parts[2].trim();
        assert_eq!(body, "Test insight.");
    }

    #[test]
    fn test_insight_dir_created_automatically() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = VaultConfig::default();
        config.output.insight_dir = "custom_insights/".to_string();
        let manifest = Manifest::new();

        let writer = InsightWriter::new(dir.path(), &manifest, &config);

        let insights = vec![InsightOutput {
            content: "Test.".to_string(),
            source_memory_ids: Vec::new(),
            confidence: 0.5,
        }];

        writer.write_insights(&insights).unwrap();
        assert!(dir.path().join("custom_insights").exists());
    }

    #[test]
    fn test_empty_insights_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        let manifest = Manifest::new();
        let writer = InsightWriter::new(dir.path(), &manifest, &config);

        let paths = writer.write_insights(&[]).unwrap();
        assert!(paths.is_empty());
    }

    #[test]
    fn test_source_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();

        let memory_ulid = ulid::Ulid::new();
        let memory_id_str = memory_ulid.to_string();
        let memory_id_bytes = memory_ulid.0.to_be_bytes().to_vec();

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
                proposition_hashes: Vec::new(),
            },
        );

        let writer = InsightWriter::new(dir.path(), &manifest, &config);
        let sources = writer.resolve_sources(&[memory_id_bytes]);

        assert_eq!(sources.len(), 1);
        assert!(sources[0].starts_with("notes/meeting.md#"));
    }
}
