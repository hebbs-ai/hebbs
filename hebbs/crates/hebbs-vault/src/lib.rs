//! # hebbs-vault
//!
//! File-first markdown sync for HEBBS. Implements the content plane / cognition plane
//! architecture where markdown files are the source of truth and `.hebbs/` is a
//! rebuildable index.
//!
//! The foundational loop: **files -> index -> queries -> new files (insights) -> index**.

pub mod config;
pub mod contradiction_writer;
pub mod daemon;
pub mod error;
pub mod extract;
pub mod ingest;
pub mod insight_writer;
pub mod manifest;
pub mod panel;
pub mod parser;
pub mod query;
pub mod query_log;
pub mod watcher;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::info;

use hebbs_core::engine::Engine;
use hebbs_embed::Embedder;

use crate::config::VaultConfig;
use crate::error::{Result, VaultError};
use crate::ingest::{phase1_ingest, phase2_ingest, Phase1Stats, Phase2Stats};
use crate::manifest::Manifest;
use crate::watcher::collect_md_files;

// Re-exports for convenience
pub use config::VaultConfig as Config;
pub use error::VaultError as Error;
pub use manifest::SectionState;
pub use parser::{ParsedFile, ParsedSection, WikiLink};

/// Initialize a new vault: create `.hebbs/` directory with default config and empty manifest.
///
/// Analogous to `git init`. If `llm_config` is `Some`, it is written into the config
/// and validated before init completes. If validation fails, `.hebbs/` is removed.
pub fn init(vault_root: &Path, force: bool) -> Result<()> {
    init_with_llm(vault_root, force, None)
}

/// Initialize a vault with explicit LLM configuration.
///
/// When `llm_config` is provided, the LLM key is validated by making a test call.
/// If validation fails, the `.hebbs/` directory is cleaned up and an error is returned.
pub fn init_with_llm(
    vault_root: &Path,
    force: bool,
    llm_config: Option<config::LlmConfig>,
) -> Result<()> {
    if !vault_root.exists() || !vault_root.is_dir() {
        return Err(VaultError::InvalidPath {
            reason: format!(
                "{} does not exist or is not a directory",
                vault_root.display()
            ),
        });
    }

    let hebbs_dir = vault_root.join(".hebbs");

    if hebbs_dir.exists() && !force {
        return Err(VaultError::AlreadyInitialized {
            path: vault_root.to_path_buf(),
        });
    }

    // Create directory structure
    std::fs::create_dir_all(hebbs_dir.join("index"))?;

    // Build config with LLM section if provided
    let mut vault_config = VaultConfig::default();
    if let Some(llm) = llm_config {
        vault_config.llm = llm;
    }
    vault_config.save(&hebbs_dir)?;

    // Validate LLM configuration if present
    if vault_config.llm.is_configured() {
        match vault_config.llm.create_provider() {
            Ok(provider) => {
                if let Err(e) = hebbs_llm::validate_provider(provider.as_ref()) {
                    let err_str = format!("{e}");
                    if err_str.contains("429") || err_str.contains("rate") {
                        // Rate limited - not a config error, just throttled
                        tracing::warn!(
                            "LLM rate limited during validation (will work when limit resets)"
                        );
                    } else {
                        // Actual validation failure: clean up and return error
                        let _ = std::fs::remove_dir_all(&hebbs_dir);
                        let hint = if err_str.contains("401") || err_str.contains("auth") {
                            "Check your API key."
                        } else if vault_config.llm.provider == "ollama" {
                            "Is Ollama running? (`ollama serve`)"
                        } else {
                            "Check your provider config."
                        };
                        return Err(VaultError::Config {
                            reason: format!("LLM validation failed. {hint} Error: {e}"),
                        });
                    }
                } else {
                    info!("LLM provider validated successfully");
                }
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&hebbs_dir);
                return Err(VaultError::Config {
                    reason: format!("failed to create LLM provider: {e}"),
                });
            }
        }
    }

    // Write empty manifest
    let manifest = Manifest::new();
    manifest.save(&hebbs_dir)?;

    // Write a vault epoch ID. The daemon uses this to detect vault
    // re-initialization and reopen the engine with fresh state.
    let epoch_id = ulid::Ulid::new().to_string();
    std::fs::write(hebbs_dir.join("epoch"), &epoch_id)?;

    // Add .hebbs/ to .gitignore if this is a git repo
    add_to_gitignore(vault_root)?;

    info!("initialized vault at {}", vault_root.display());
    Ok(())
}

/// Full re-index: phase 1 for all files, then phase 2 in batches.
///
/// Idempotent: skips files with matching checksums.
/// Crash-safe: manifest written incrementally.
pub async fn index(
    vault_root: &Path,
    engine: &Engine,
    embedder: &Arc<dyn Embedder>,
    progress: Option<&dyn Fn(IndexProgress)>,
) -> Result<IndexResult> {
    let hebbs_dir = vault_root.join(".hebbs");
    if !hebbs_dir.exists() {
        return Err(VaultError::NotInitialized {
            path: vault_root.to_path_buf(),
        });
    }

    let config = VaultConfig::load(&hebbs_dir)?;
    let mut manifest = Manifest::load(&hebbs_dir)?;

    // Collect all .md files
    let all_files = collect_md_files(vault_root, &config)?;
    let total_files = all_files.len();

    if let Some(cb) = progress {
        cb(IndexProgress::Phase1Started { total_files });
    }

    // Phase 1: parse all files
    let p1_stats = phase1_ingest(&all_files, vault_root, &mut manifest, &config)?;
    manifest.save(&hebbs_dir)?;

    if let Some(cb) = progress {
        cb(IndexProgress::Phase1Complete {
            files_processed: p1_stats.files_processed,
            files_skipped: p1_stats.files_skipped,
            sections_new: p1_stats.sections_new,
            sections_modified: p1_stats.sections_modified,
        });
    }

    // Phase 2: embed and index
    if let Some(cb) = progress {
        let (_, stale, orphaned) = manifest.section_counts();
        cb(IndexProgress::Phase2Started {
            sections_to_process: stale + orphaned,
        });
    }

    let p2_stats = phase2_ingest(vault_root, &mut manifest, engine, embedder, &config).await?;
    manifest.save(&hebbs_dir)?;

    if let Some(cb) = progress {
        cb(IndexProgress::Phase2Complete {
            sections_embedded: p2_stats.sections_embedded,
            sections_remembered: p2_stats.sections_remembered,
            sections_revised: p2_stats.sections_revised,
            sections_forgotten: p2_stats.sections_forgotten,
        });
    }

    Ok(IndexResult {
        phase1: p1_stats,
        phase2: p2_stats,
        total_files,
    })
}

/// Full re-index without progress callback.
///
/// This variant avoids the non-`Send` `&dyn Fn` callback, making it
/// safe to call from within `tokio::spawn` (e.g., the daemon).
pub async fn index_no_progress(
    vault_root: &Path,
    engine: &Engine,
    embedder: &Arc<dyn Embedder>,
) -> Result<IndexResult> {
    let hebbs_dir = vault_root.join(".hebbs");
    if !hebbs_dir.exists() {
        return Err(VaultError::NotInitialized {
            path: vault_root.to_path_buf(),
        });
    }

    let config = VaultConfig::load(&hebbs_dir)?;
    let mut manifest = Manifest::load(&hebbs_dir)?;

    let all_files = collect_md_files(vault_root, &config)?;
    let total_files = all_files.len();

    let p1_stats = phase1_ingest(&all_files, vault_root, &mut manifest, &config)?;
    manifest.save(&hebbs_dir)?;

    let p2_stats = phase2_ingest(vault_root, &mut manifest, engine, embedder, &config).await?;
    manifest.save(&hebbs_dir)?;

    Ok(IndexResult {
        phase1: p1_stats,
        phase2: p2_stats,
        total_files,
    })
}

/// Delete `.hebbs/` and re-create from scratch. Preserves user config.
pub async fn rebuild(
    vault_root: &Path,
    engine: &Engine,
    embedder: &Arc<dyn Embedder>,
    progress: Option<&dyn Fn(IndexProgress)>,
) -> Result<IndexResult> {
    let hebbs_dir = vault_root.join(".hebbs");

    // Preserve config before deletion
    let config = if hebbs_dir.exists() {
        VaultConfig::load(&hebbs_dir)?
    } else {
        VaultConfig::default()
    };

    // Delete .hebbs/ entirely
    if hebbs_dir.exists() {
        std::fs::remove_dir_all(&hebbs_dir)?;
    }

    // Re-init
    init(vault_root, false)?;

    // Restore user config
    config.save(&hebbs_dir)?;

    // Full re-index
    index(vault_root, engine, embedder, progress).await
}

/// Get vault status from manifest.
pub fn status(vault_root: &Path) -> Result<VaultStatus> {
    let hebbs_dir = vault_root.join(".hebbs");
    if !hebbs_dir.exists() {
        return Err(VaultError::NotInitialized {
            path: vault_root.to_path_buf(),
        });
    }

    let vault_config = VaultConfig::load(&hebbs_dir)?;
    let manifest = Manifest::load(&hebbs_dir)?;
    let (synced, stale, orphaned) = manifest.section_counts();
    let total_sections = synced + stale + orphaned;

    let last_parsed = manifest.files.values().map(|e| e.last_parsed).max();

    let last_embedded = manifest
        .files
        .values()
        .filter_map(|e| e.last_embedded)
        .max();

    let llm_provider = if vault_config.llm.is_configured() {
        Some(vault_config.llm.provider.clone())
    } else {
        None
    };
    let llm_model = if vault_config.llm.is_configured() {
        Some(vault_config.llm.model.clone())
    } else {
        None
    };

    Ok(VaultStatus {
        vault_root: vault_root.to_path_buf(),
        total_files: manifest.files.len(),
        total_sections,
        synced,
        content_stale: stale,
        orphaned,
        last_parsed,
        last_embedded,
        llm_provider,
        llm_model,
    })
}

/// Add `.hebbs/` to `.gitignore` if applicable.
fn add_to_gitignore(vault_root: &Path) -> Result<()> {
    let git_dir = vault_root.join(".git");
    if !git_dir.exists() {
        return Ok(()); // Not a git repo
    }

    let gitignore_path = vault_root.join(".gitignore");
    let entry = ".hebbs/";

    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        if content.lines().any(|line| line.trim() == entry) {
            return Ok(()); // Already present
        }
        // Append
        let mut new_content = content;
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(entry);
        new_content.push('\n');
        std::fs::write(&gitignore_path, new_content)?;
    } else {
        std::fs::write(&gitignore_path, format!("{}\n", entry))?;
    }

    Ok(())
}

/// Progress callback for index/rebuild operations.
#[derive(Debug)]
pub enum IndexProgress {
    Phase1Started {
        total_files: usize,
    },
    Phase1Complete {
        files_processed: usize,
        files_skipped: usize,
        sections_new: usize,
        sections_modified: usize,
    },
    Phase2Started {
        sections_to_process: usize,
    },
    Phase2Complete {
        sections_embedded: usize,
        sections_remembered: usize,
        sections_revised: usize,
        sections_forgotten: usize,
    },
}

/// Result of a full index operation.
#[derive(Debug)]
pub struct IndexResult {
    pub phase1: Phase1Stats,
    pub phase2: Phase2Stats,
    pub total_files: usize,
}

/// Vault status information.
#[derive(Debug)]
pub struct VaultStatus {
    pub vault_root: PathBuf,
    pub total_files: usize,
    pub total_sections: usize,
    pub synced: usize,
    pub content_stale: usize,
    pub orphaned: usize,
    pub last_parsed: Option<chrono::DateTime<chrono::Utc>>,
    pub last_embedded: Option<chrono::DateTime<chrono::Utc>>,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_creates_structure() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path(), false).unwrap();

        assert!(dir.path().join(".hebbs").exists());
        assert!(dir.path().join(".hebbs/config.toml").exists());
        assert!(dir.path().join(".hebbs/manifest.json").exists());
        assert!(dir.path().join(".hebbs/index").exists());
    }

    #[test]
    fn test_init_fails_if_already_initialized() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path(), false).unwrap();

        let result = init(dir.path(), false);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VaultError::AlreadyInitialized { .. }
        ));
    }

    #[test]
    fn test_init_force_reinitializes() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path(), false).unwrap();
        init(dir.path(), true).unwrap(); // should succeed
    }

    #[test]
    fn test_init_invalid_path() {
        let result = init(Path::new("/nonexistent/path"), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_status_not_initialized() {
        let dir = tempfile::tempdir().unwrap();
        let result = status(dir.path());
        assert!(matches!(
            result.unwrap_err(),
            VaultError::NotInitialized { .. }
        ));
    }

    #[test]
    fn test_status_empty_vault() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path(), false).unwrap();

        let s = status(dir.path()).unwrap();
        assert_eq!(s.total_files, 0);
        assert_eq!(s.total_sections, 0);
        assert_eq!(s.synced, 0);
    }

    #[test]
    fn test_gitignore_created() {
        let dir = tempfile::tempdir().unwrap();
        // Create .git directory to simulate git repo
        std::fs::create_dir(dir.path().join(".git")).unwrap();

        init(dir.path(), false).unwrap();

        let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gitignore.contains(".hebbs/"));
    }

    #[test]
    fn test_gitignore_no_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".gitignore"), ".hebbs/\n").unwrap();

        init(dir.path(), false).unwrap();

        let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        let count = gitignore.lines().filter(|l| l.trim() == ".hebbs/").count();
        assert_eq!(count, 1);
    }
}
