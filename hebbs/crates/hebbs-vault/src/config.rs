use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Vault configuration stored in `.hebbs/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VaultConfig {
    #[serde(default)]
    pub chunking: ChunkingConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub watch: WatchConfig,
    #[serde(default)]
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkingConfig {
    /// Heading level to split on (e.g., "##" for level 2).
    #[serde(default = "default_split_on")]
    pub split_on: String,
    /// Sections shorter than this (chars) merge with parent.
    #[serde(default = "default_min_section_length")]
    pub min_section_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingConfig {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_dimensions")]
    pub dimensions: usize,
    /// Max sections per embed batch call.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WatchConfig {
    /// Glob patterns to ignore (relative to vault root).
    #[serde(default = "default_ignore_patterns")]
    pub ignore_patterns: Vec<String>,
    /// Phase 1 debounce in milliseconds.
    #[serde(default = "default_phase1_debounce_ms")]
    pub phase1_debounce_ms: u64,
    /// Phase 2 debounce in milliseconds.
    #[serde(default = "default_phase2_debounce_ms")]
    pub phase2_debounce_ms: u64,
    /// Burst threshold: if more than this many events arrive in a phase 1
    /// window, extend phase 2 debounce.
    #[serde(default = "default_burst_threshold")]
    pub burst_threshold: usize,
    /// Extended phase 2 debounce during burst (ms).
    #[serde(default = "default_burst_debounce_ms")]
    pub burst_debounce_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutputConfig {
    /// Directory for insight output files (relative to vault root).
    #[serde(default = "default_insight_dir")]
    pub insight_dir: String,
    /// Exclude insight directory from reflect input to prevent loops.
    #[serde(default = "default_true")]
    pub exclude_insight_dir_from_reflect: bool,
}

// Defaults

fn default_split_on() -> String {
    "##".to_string()
}
fn default_min_section_length() -> usize {
    50
}
fn default_model() -> String {
    "bge-small-en-v1.5".to_string()
}
fn default_dimensions() -> usize {
    384
}
fn default_batch_size() -> usize {
    50
}
fn default_ignore_patterns() -> Vec<String> {
    vec![
        ".hebbs/".to_string(),
        ".git/".to_string(),
        ".obsidian/".to_string(),
        "node_modules/".to_string(),
    ]
}
fn default_phase1_debounce_ms() -> u64 {
    500
}
fn default_phase2_debounce_ms() -> u64 {
    3000
}
fn default_burst_threshold() -> usize {
    20
}
fn default_burst_debounce_ms() -> u64 {
    10_000
}
fn default_insight_dir() -> String {
    "insights/".to_string()
}
fn default_true() -> bool {
    true
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            chunking: ChunkingConfig::default(),
            embedding: EmbeddingConfig::default(),
            watch: WatchConfig::default(),
            output: OutputConfig::default(),
        }
    }
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            split_on: default_split_on(),
            min_section_length: default_min_section_length(),
        }
    }
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            dimensions: default_dimensions(),
            batch_size: default_batch_size(),
        }
    }
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            ignore_patterns: default_ignore_patterns(),
            phase1_debounce_ms: default_phase1_debounce_ms(),
            phase2_debounce_ms: default_phase2_debounce_ms(),
            burst_threshold: default_burst_threshold(),
            burst_debounce_ms: default_burst_debounce_ms(),
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            insight_dir: default_insight_dir(),
            exclude_insight_dir_from_reflect: default_true(),
        }
    }
}

impl VaultConfig {
    /// Load config from `.hebbs/config.toml`.
    pub fn load(hebbs_dir: &Path) -> Result<Self> {
        let path = hebbs_dir.join("config.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save config to `.hebbs/config.toml`.
    pub fn save(&self, hebbs_dir: &Path) -> Result<()> {
        let path = hebbs_dir.join("config.toml");
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Parse the `split_on` config into a heading level (number of `#` chars).
    /// Returns 2 for "##", 3 for "###", etc.
    pub fn split_level(&self) -> usize {
        self.chunking
            .split_on
            .chars()
            .take_while(|c| *c == '#')
            .count()
            .max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = VaultConfig::default();
        assert_eq!(config.chunking.split_on, "##");
        assert_eq!(config.chunking.min_section_length, 50);
        assert_eq!(config.embedding.dimensions, 384);
        assert_eq!(config.watch.phase1_debounce_ms, 500);
        assert_eq!(config.watch.phase2_debounce_ms, 3000);
        assert_eq!(config.output.insight_dir, "insights/");
        assert!(config.output.exclude_insight_dir_from_reflect);
    }

    #[test]
    fn test_split_level() {
        let mut config = VaultConfig::default();
        assert_eq!(config.split_level(), 2);

        config.chunking.split_on = "###".to_string();
        assert_eq!(config.split_level(), 3);

        config.chunking.split_on = "#".to_string();
        assert_eq!(config.split_level(), 1);
    }

    #[test]
    fn test_config_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::default();
        config.save(dir.path()).unwrap();
        let loaded = VaultConfig::load(dir.path()).unwrap();
        assert_eq!(config, loaded);
    }

    #[test]
    fn test_config_load_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::load(dir.path()).unwrap();
        assert_eq!(config, VaultConfig::default());
    }
}
