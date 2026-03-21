use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Vault configuration stored in `.hebbs/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VaultConfig {
    #[serde(default)]
    pub chunking: ChunkingConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub watch: WatchConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub decay: DecayConfig,
    #[serde(default)]
    pub contradiction: ContradictionConfig,
    #[serde(default, alias = "reflect_llm")]
    pub llm: LlmConfig,
    #[serde(default)]
    pub extraction: ExtractionConfig,
    #[serde(default)]
    pub query_log: QueryLogConfig,
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
    /// Directory for contradiction output files (relative to vault root).
    #[serde(default = "default_contradiction_dir")]
    pub contradiction_dir: String,
    /// Exclude insight directory from reflect input to prevent loops.
    #[serde(default = "default_true")]
    pub exclude_insight_dir_from_reflect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoringConfig {
    /// Weight for strategy-specific relevance signal.
    #[serde(default = "default_w_relevance")]
    pub w_relevance: f32,
    /// Weight for temporal recency.
    #[serde(default = "default_w_recency")]
    pub w_recency: f32,
    /// Weight for stored importance.
    #[serde(default = "default_w_importance")]
    pub w_importance: f32,
    /// Weight for access-count reinforcement.
    #[serde(default = "default_w_reinforcement")]
    pub w_reinforcement: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecayConfig {
    /// Half-life in days: memory strength halves every N days without access.
    #[serde(default = "default_half_life_days")]
    pub half_life_days: f32,
    /// Memories below this decay score are candidates for auto-forget.
    #[serde(default = "default_auto_forget_threshold")]
    pub auto_forget_threshold: f32,
    /// Maximum access count that affects reinforcement scoring.
    #[serde(default = "default_reinforcement_cap")]
    pub reinforcement_cap: u64,
    /// Sweep interval in seconds. How often the decay engine recalculates scores.
    /// Default: 3600 (1 hour). Minimum: 1 second.
    #[serde(default = "default_sweep_interval_secs")]
    pub sweep_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContradictionConfig {
    /// Enable contradiction detection during ingest.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum neighbors to check per memory.
    #[serde(default = "default_candidates_k")]
    pub candidates_k: usize,
    /// Minimum similarity to consider a pair.
    #[serde(default = "default_min_similarity")]
    pub min_similarity: f32,
    /// Minimum confidence to create a CONTRADICTS edge.
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,
}

/// LLM provider configuration. Required for all HEBBS subsystems.
///
/// When configured, enables autonomous contradiction detection, reflection,
/// and proposition extraction. `hebbs init` requires LLM configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LlmConfig {
    /// Provider name: "anthropic", "openai", "gemini", "ollama".
    #[serde(default)]
    pub provider: String,
    /// Model identifier (e.g. "claude-haiku-4-5-20251001", "gpt-4o-mini").
    #[serde(default)]
    pub model: String,
    /// API key. For security, prefer `api_key_env` instead.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Environment variable name holding the API key (e.g. "ANTHROPIC_API_KEY").
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Base URL override for the provider.
    #[serde(default)]
    pub base_url: Option<String>,
}

impl LlmConfig {
    /// Resolve the API key from either the direct value or the environment variable.
    pub fn resolved_api_key(&self) -> Option<String> {
        if let Some(ref key) = self.api_key {
            if !key.is_empty() {
                return Some(key.clone());
            }
        }
        if let Some(ref env_var) = self.api_key_env {
            if let Ok(val) = std::env::var(env_var) {
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
        None
    }

    /// Returns true if both provider and model are configured.
    pub fn is_configured(&self) -> bool {
        !self.provider.is_empty() && !self.model.is_empty()
    }

    /// Build an `LlmProviderConfig` from this config.
    pub fn to_provider_config(&self) -> hebbs_llm::LlmProviderConfig {
        hebbs_llm::LlmProviderConfig {
            provider_type: hebbs_llm::ProviderType::from_name(&self.provider),
            api_key: self.resolved_api_key(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            timeout_secs: 30,
            max_retries: 1,
            retry_backoff_ms: 500,
        }
    }

    /// Create an LLM provider from this config.
    pub fn create_provider(
        &self,
    ) -> std::result::Result<Box<dyn hebbs_llm::LlmProvider>, hebbs_llm::LlmError> {
        let config = self.to_provider_config();
        hebbs_llm::create_provider(&config)
    }
}

/// Backward-compatible type alias.
pub type ReflectLlmConfig = LlmConfig;

/// Configuration for LLM-based content extraction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtractionConfig {
    /// Files larger than this (in tokens, estimated as chars/4) get
    /// heading-split extraction with document summary.
    #[serde(default = "default_large_file_threshold")]
    pub large_file_threshold: usize,
    /// Maximum propositions to extract per file.
    #[serde(default = "default_max_propositions_per_file")]
    pub max_propositions_per_file: usize,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            large_file_threshold: default_large_file_threshold(),
            max_propositions_per_file: default_max_propositions_per_file(),
        }
    }
}

fn default_large_file_threshold() -> usize {
    4096
}

fn default_max_propositions_per_file() -> usize {
    200
}

/// Query audit log configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryLogConfig {
    /// Enable query logging.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum number of log entries to retain.
    #[serde(default = "default_query_log_max_entries")]
    pub max_entries: u64,
    /// Maximum age of log entries in days.
    #[serde(default = "default_query_log_max_age_days")]
    pub max_age_days: u32,
    /// Store the query text in log entries. Set to false for privacy.
    #[serde(default = "default_true")]
    pub log_query_text: bool,
    /// Store which memory IDs were returned. Set to false for privacy.
    #[serde(default = "default_true")]
    pub log_result_ids: bool,
}

impl Default for QueryLogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: default_query_log_max_entries(),
            max_age_days: default_query_log_max_age_days(),
            log_query_text: true,
            log_result_ids: true,
        }
    }
}

fn default_query_log_max_entries() -> u64 {
    10_000
}

fn default_query_log_max_age_days() -> u32 {
    30
}

impl Default for ContradictionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            candidates_k: default_candidates_k(),
            min_similarity: default_min_similarity(),
            min_confidence: default_min_confidence(),
        }
    }
}

impl ContradictionConfig {
    /// Convert to the core engine's ContradictionConfig.
    pub fn to_core_config(&self) -> hebbs_core::contradict::ContradictionConfig {
        hebbs_core::contradict::ContradictionConfig {
            candidates_k: self.candidates_k,
            min_similarity: self.min_similarity,
            min_confidence: self.min_confidence,
            enabled: self.enabled,
        }
    }
}

fn default_candidates_k() -> usize {
    10
}
fn default_min_similarity() -> f32 {
    0.7
}
fn default_min_confidence() -> f32 {
    0.7
}

// Defaults

fn default_w_relevance() -> f32 {
    0.5
}
fn default_w_recency() -> f32 {
    0.2
}
fn default_w_importance() -> f32 {
    0.2
}
fn default_w_reinforcement() -> f32 {
    0.1
}
fn default_half_life_days() -> f32 {
    30.0
}
fn default_auto_forget_threshold() -> f32 {
    0.01
}
fn default_reinforcement_cap() -> u64 {
    100
}
fn default_sweep_interval_secs() -> u64 {
    3600
}

fn default_split_on() -> String {
    "##".to_string()
}
fn default_min_section_length() -> usize {
    50
}
fn default_model() -> String {
    "embeddinggemma-300m".to_string()
}
fn default_dimensions() -> usize {
    768
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
        "contradictions/".to_string(),
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
fn default_contradiction_dir() -> String {
    "contradictions/".to_string()
}
fn default_true() -> bool {
    true
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
            contradiction_dir: default_contradiction_dir(),
            exclude_insight_dir_from_reflect: default_true(),
        }
    }
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            w_relevance: default_w_relevance(),
            w_recency: default_w_recency(),
            w_importance: default_w_importance(),
            w_reinforcement: default_w_reinforcement(),
        }
    }
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            half_life_days: default_half_life_days(),
            auto_forget_threshold: default_auto_forget_threshold(),
            reinforcement_cap: default_reinforcement_cap(),
            sweep_interval_secs: default_sweep_interval_secs(),
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

    /// Validate the config and return a map of field-specific errors.
    /// Returns an empty map if valid.
    pub fn validate(&self) -> std::collections::HashMap<String, String> {
        let mut errors = std::collections::HashMap::new();

        // Chunking
        if self.chunking.split_on.is_empty() {
            errors.insert(
                "chunking.split_on".to_string(),
                "must not be empty".to_string(),
            );
        } else if !self.chunking.split_on.starts_with('#') {
            errors.insert(
                "chunking.split_on".to_string(),
                "must start with '#' (e.g. \"##\")".to_string(),
            );
        }

        // Embedding
        if self.embedding.batch_size < 1 {
            errors.insert(
                "embedding.batch_size".to_string(),
                "must be >= 1".to_string(),
            );
        }

        // Watch
        if self.watch.phase1_debounce_ms < 50 {
            errors.insert(
                "watch.phase1_debounce_ms".to_string(),
                "must be >= 50".to_string(),
            );
        }
        if self.watch.phase2_debounce_ms < 50 {
            errors.insert(
                "watch.phase2_debounce_ms".to_string(),
                "must be >= 50".to_string(),
            );
        }
        if self.watch.burst_threshold < 1 {
            errors.insert(
                "watch.burst_threshold".to_string(),
                "must be >= 1".to_string(),
            );
        }
        if self.watch.burst_debounce_ms < 50 {
            errors.insert(
                "watch.burst_debounce_ms".to_string(),
                "must be >= 50".to_string(),
            );
        }
        for (i, pattern) in self.watch.ignore_patterns.iter().enumerate() {
            if pattern.trim().is_empty() {
                errors.insert(
                    format!("watch.ignore_patterns[{}]", i),
                    "pattern must not be empty".to_string(),
                );
            }
            // Test glob pattern validity
            if globset::Glob::new(pattern).is_err() {
                errors.insert(
                    format!("watch.ignore_patterns[{}]", i),
                    format!("invalid glob pattern: {}", pattern),
                );
            }
        }

        // Scoring weights
        if self.scoring.w_relevance < 0.0 {
            errors.insert(
                "scoring.w_relevance".to_string(),
                "must be >= 0".to_string(),
            );
        }
        if self.scoring.w_recency < 0.0 {
            errors.insert("scoring.w_recency".to_string(), "must be >= 0".to_string());
        }
        if self.scoring.w_importance < 0.0 {
            errors.insert(
                "scoring.w_importance".to_string(),
                "must be >= 0".to_string(),
            );
        }
        if self.scoring.w_reinforcement < 0.0 {
            errors.insert(
                "scoring.w_reinforcement".to_string(),
                "must be >= 0".to_string(),
            );
        }

        // Decay
        if self.decay.half_life_days <= 0.0 {
            errors.insert(
                "decay.half_life_days".to_string(),
                "must be > 0".to_string(),
            );
        }
        if self.decay.auto_forget_threshold < 0.0 || self.decay.auto_forget_threshold > 1.0 {
            errors.insert(
                "decay.auto_forget_threshold".to_string(),
                "must be between 0 and 1".to_string(),
            );
        }
        if self.decay.reinforcement_cap < 1 {
            errors.insert(
                "decay.reinforcement_cap".to_string(),
                "must be >= 1".to_string(),
            );
        }

        errors
    }

    /// Returns ignore patterns with output directories and `.hebbsignore`
    /// entries merged in. Ensures contradiction_dir is always excluded from
    /// the watcher even if the user changed it from the default.
    pub fn effective_ignore_patterns(&self, vault_root: &Path) -> Vec<String> {
        let mut patterns = self.watch.ignore_patterns.clone();
        let cdir = &self.output.contradiction_dir;
        if !patterns.iter().any(|p| p == cdir) {
            patterns.push(cdir.clone());
        }
        // Merge patterns from .hebbsignore (gitignore-style, one pattern per line)
        let ignore_file = vault_root.join(".hebbsignore");
        if let Ok(contents) = std::fs::read_to_string(&ignore_file) {
            for line in contents.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if !patterns.iter().any(|p| p == trimmed) {
                    patterns.push(trimmed.to_string());
                }
            }
        }
        patterns
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
        assert_eq!(config.embedding.dimensions, 768);
        assert_eq!(config.watch.phase1_debounce_ms, 500);
        assert_eq!(config.watch.phase2_debounce_ms, 3000);
        assert_eq!(config.output.insight_dir, "insights/");
        assert_eq!(config.output.contradiction_dir, "contradictions/");
        assert!(config.output.exclude_insight_dir_from_reflect);
    }

    #[test]
    fn test_default_ignore_patterns_include_contradictions() {
        let config = VaultConfig::default();
        assert!(
            config
                .watch
                .ignore_patterns
                .contains(&"contradictions/".to_string()),
            "default ignore patterns should include contradictions/"
        );
    }

    #[test]
    fn test_effective_ignore_patterns_includes_contradiction_dir() {
        let config = VaultConfig::default();
        let tmp = std::env::temp_dir();
        let patterns = config.effective_ignore_patterns(&tmp);
        assert!(patterns.contains(&"contradictions/".to_string()));
        // No duplicates
        let count = patterns.iter().filter(|p| *p == "contradictions/").count();
        assert_eq!(count, 1, "should not duplicate contradiction_dir");
    }

    #[test]
    fn test_effective_ignore_patterns_custom_contradiction_dir() {
        let mut config = VaultConfig::default();
        config.output.contradiction_dir = "my_contradictions/".to_string();
        let tmp = std::env::temp_dir();
        let patterns = config.effective_ignore_patterns(&tmp);
        assert!(
            patterns.contains(&"my_contradictions/".to_string()),
            "effective patterns should include custom contradiction_dir"
        );
    }

    #[test]
    fn test_hebbsignore_file_merged() {
        let dir = tempfile::tempdir().unwrap();
        let vault_root = dir.path();
        std::fs::write(
            vault_root.join(".hebbsignore"),
            "# comment\ntemplates/\ndrafts/*.md\n\n",
        )
        .unwrap();
        let config = VaultConfig::default();
        let patterns = config.effective_ignore_patterns(vault_root);
        assert!(patterns.contains(&"templates/".to_string()));
        assert!(patterns.contains(&"drafts/*.md".to_string()));
        // Comments and blank lines should not appear
        assert!(!patterns.iter().any(|p| p.starts_with('#')));
        assert!(!patterns.iter().any(|p| p.is_empty()));
    }

    #[test]
    fn test_hebbsignore_no_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let vault_root = dir.path();
        // .hebbsignore contains a pattern already in defaults
        std::fs::write(vault_root.join(".hebbsignore"), ".git/\n").unwrap();
        let config = VaultConfig::default();
        let patterns = config.effective_ignore_patterns(vault_root);
        let count = patterns.iter().filter(|p| *p == ".git/").count();
        assert_eq!(count, 1, "should not duplicate patterns from .hebbsignore");
    }

    #[test]
    fn test_contradiction_dir_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = VaultConfig::default();
        config.output.contradiction_dir = "custom_dir/".to_string();
        config.save(dir.path()).unwrap();
        let loaded = VaultConfig::load(dir.path()).unwrap();
        assert_eq!(loaded.output.contradiction_dir, "custom_dir/");
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
