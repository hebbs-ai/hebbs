use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Vault configuration stored in `.hebbs/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VaultConfig {
    #[serde(default)]
    pub chunking: ChunkingConfig,
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
    #[serde(
        default,
        alias = "reflect_llm",
        skip_serializing_if = "LlmConfig::is_empty"
    )]
    pub llm: LlmConfig,
    #[serde(default, skip_serializing_if = "EmbeddingConfig::is_default")]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub extraction: ExtractionConfig,
    #[serde(default)]
    pub query_log: QueryLogConfig,
    #[serde(default, skip_serializing_if = "ApiConfig::is_default")]
    pub api: ApiConfig,
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
    /// Embedding provider: omit or "local" for ONNX, "openai" for OpenAI API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Direct API key for the embedding provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Environment variable holding the API key for the embedding provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Base URL override for the embedding API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

impl EmbeddingConfig {
    /// Returns true if this is the default config (local gemma, no API provider).
    /// Used by serde to skip serializing defaults in local config, allowing global inheritance.
    pub fn is_default(&self) -> bool {
        self.provider.is_none()
            && self.api_key.is_none()
            && self.api_key_env.is_none()
            && self.base_url.is_none()
            && self.model == default_model()
            && self.dimensions == default_dimensions()
    }

    /// Resolve the actual API key: check api_key first, then api_key_env env lookup.
    pub fn resolved_api_key(&self) -> Option<String> {
        if let Some(ref key) = self.api_key {
            if !key.is_empty() {
                return Some(key.clone());
            }
        }
        if let Some(ref env_name) = self.api_key_env {
            if let Ok(val) = std::env::var(env_name) {
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
        None
    }

    /// Auto-configure embedding from LLM config when embedding is unconfigured.
    /// If LLM is openai, sets embedding to text-embedding-3-small with same key.
    pub fn inherit_from_llm(&mut self, llm: &LlmConfig) {
        if self.provider.is_some() {
            return; // already explicitly configured
        }
        if llm.provider == "openai" {
            self.provider = Some("openai".to_string());
            self.model = "text-embedding-3-small".to_string();
            self.dimensions = 1536;
            // Inherit key: prefer direct key, then env var name
            if self.api_key.is_none() {
                self.api_key = llm.api_key.clone();
            }
            if self.api_key_env.is_none() {
                self.api_key_env = llm.api_key_env.clone();
            }
        }
    }
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
    /// Returns true when provider and model are both empty (default state).
    /// Used by serde to skip serializing empty `[llm]` sections so they
    /// don't shadow global config.
    pub fn is_empty(&self) -> bool {
        self.provider.is_empty()
            && self.model.is_empty()
            && self.api_key.is_none()
            && self.api_key_env.is_none()
            && self.base_url.is_none()
    }

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

/// API rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiConfig {
    /// Maximum concurrent API requests (LLM + embedding).
    /// Lower to 1-2 for low-tier API accounts that hit rate limits.
    #[serde(default = "default_max_concurrent_requests")]
    pub max_concurrent_requests: usize,
}

fn default_max_concurrent_requests() -> usize {
    10
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: default_max_concurrent_requests(),
        }
    }
}

impl ApiConfig {
    pub fn is_default(&self) -> bool {
        self.max_concurrent_requests == 10
    }
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
            provider: None,
            api_key: None,
            api_key_env: None,
            base_url: None,
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
    /// Returns the global config directory: `~/.hebbs/`.
    pub fn global_config_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".hebbs"))
    }

    /// Load the global config from `~/.hebbs/config.toml`.
    /// Returns `Self::default()` if the file does not exist.
    pub fn load_global() -> Result<Self> {
        match Self::global_config_dir() {
            Some(dir) => Self::load_local_only(&dir),
            None => Ok(Self::default()),
        }
    }

    /// Save config to `~/.hebbs/config.toml`, creating the directory if needed.
    pub fn save_global(&self) -> Result<()> {
        let dir = Self::global_config_dir().ok_or_else(|| crate::error::VaultError::Config {
            reason: "could not determine home directory".to_string(),
        })?;
        std::fs::create_dir_all(&dir)?;
        self.save(&dir)
    }

    /// Load config from a single `.hebbs/config.toml` without merging global.
    /// Useful for `config show --global` or inspecting local-only values.
    pub fn load_local_only(hebbs_dir: &Path) -> Result<Self> {
        let path = hebbs_dir.join("config.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load config from `.hebbs/config.toml` with global inheritance.
    ///
    /// Loads `~/.hebbs/config.toml` first, then merges the local config on top.
    /// Local values override global values at the field level. Empty strings in
    /// local config do not shadow global values.
    ///
    /// If `hebbs_dir` IS the global config directory, only one read is performed.
    pub fn load(hebbs_dir: &Path) -> Result<Self> {
        let global_dir = Self::global_config_dir();

        // If hebbs_dir is the global dir itself, skip double-read
        let is_global_dir = global_dir.as_ref().is_some_and(|g| {
            let g_canon = g.canonicalize().unwrap_or_else(|_| g.clone());
            let h_canon = hebbs_dir
                .canonicalize()
                .unwrap_or_else(|_| hebbs_dir.to_path_buf());
            g_canon == h_canon
        });

        if is_global_dir {
            return Self::load_local_only(hebbs_dir);
        }

        let global_path = global_dir.map(|d| d.join("config.toml"));
        let local_path = hebbs_dir.join("config.toml");

        let global_toml = match global_path {
            Some(ref p) if p.exists() => {
                let content = std::fs::read_to_string(p)?;
                content.parse::<toml::Value>().ok()
            }
            _ => None,
        };

        let local_toml = if local_path.exists() {
            let content = std::fs::read_to_string(&local_path)?;
            content.parse::<toml::Value>().ok()
        } else {
            None
        };

        let merged = match (global_toml, local_toml) {
            (Some(g), Some(l)) => merge_toml(g, l),
            (Some(g), None) => g,
            (None, Some(l)) => l,
            (None, None) => return Ok(Self::default()),
        };

        let config: Self =
            merged
                .try_into()
                .map_err(|e: toml::de::Error| crate::error::VaultError::Config {
                    reason: format!("failed to parse merged config: {e}"),
                })?;
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

/// Recursively merge two TOML values. Local overrides global at the field level.
/// Empty strings in local do not shadow global values.
fn merge_toml(global: toml::Value, local: toml::Value) -> toml::Value {
    match (global, local) {
        (toml::Value::Table(mut g), toml::Value::Table(l)) => {
            for (key, local_val) in l {
                if let Some(global_val) = g.remove(&key) {
                    g.insert(key, merge_toml(global_val, local_val));
                } else {
                    g.insert(key, local_val);
                }
            }
            toml::Value::Table(g)
        }
        // Empty string in local: keep global value
        (global_val, toml::Value::String(ref s)) if s.is_empty() => global_val,
        // Local overrides global for all other types
        (_global_val, local_val) => local_val,
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
        let loaded = VaultConfig::load_local_only(dir.path()).unwrap();
        assert_eq!(config, loaded);
    }

    #[test]
    fn test_config_load_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig::load_local_only(dir.path()).unwrap();
        assert_eq!(config, VaultConfig::default());
    }

    #[test]
    fn test_merge_toml_field_level_override() {
        let global: toml::Value = toml::from_str(
            r#"
            [llm]
            provider = "openai"
            model = "gpt-4o-mini"
            api_key_env = "OPENAI_API_KEY"
            "#,
        )
        .unwrap();
        let local: toml::Value = toml::from_str(
            r#"
            [llm]
            model = "gpt-4o"
            "#,
        )
        .unwrap();
        let merged = merge_toml(global, local);
        let config: VaultConfig = merged.try_into().unwrap();
        assert_eq!(config.llm.provider, "openai");
        assert_eq!(config.llm.model, "gpt-4o");
        assert_eq!(config.llm.api_key_env.as_deref(), Some("OPENAI_API_KEY"));
    }

    #[test]
    fn test_global_llm_inherited_when_local_has_no_llm() {
        let global_dir = tempfile::tempdir().unwrap();
        let local_dir = tempfile::tempdir().unwrap();

        // Write global config with LLM
        let global_cfg = VaultConfig {
            llm: LlmConfig {
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                api_key: None,
                api_key_env: Some("OPENAI_API_KEY".to_string()),
                base_url: None,
            },
            ..VaultConfig::default()
        };
        global_cfg.save(global_dir.path()).unwrap();

        // Write local config with NO llm section (default empty)
        let local_cfg = VaultConfig::default();
        local_cfg.save(local_dir.path()).unwrap();

        // Manually merge to test the logic (since load() uses the real home dir)
        let global_content =
            std::fs::read_to_string(global_dir.path().join("config.toml")).unwrap();
        let local_content = std::fs::read_to_string(local_dir.path().join("config.toml")).unwrap();
        let global_toml: toml::Value = global_content.parse().unwrap();
        let local_toml: toml::Value = local_content.parse().unwrap();
        let merged = merge_toml(global_toml, local_toml);
        let config: VaultConfig = merged.try_into().unwrap();

        assert_eq!(config.llm.provider, "openai");
        assert_eq!(config.llm.model, "gpt-4o-mini");
    }

    #[test]
    fn test_local_llm_overrides_global() {
        let global: toml::Value = toml::from_str(
            r#"
            [llm]
            provider = "openai"
            model = "gpt-4o-mini"
            "#,
        )
        .unwrap();
        let local: toml::Value = toml::from_str(
            r#"
            [llm]
            provider = "anthropic"
            model = "claude-haiku-4-5-20251001"
            "#,
        )
        .unwrap();
        let merged = merge_toml(global, local);
        let config: VaultConfig = merged.try_into().unwrap();
        assert_eq!(config.llm.provider, "anthropic");
        assert_eq!(config.llm.model, "claude-haiku-4-5-20251001");
    }

    #[test]
    fn test_no_global_config_backward_compatible() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig {
            llm: LlmConfig {
                provider: "ollama".to_string(),
                model: "gemma3:1b".to_string(),
                api_key: None,
                api_key_env: None,
                base_url: None,
            },
            ..VaultConfig::default()
        };
        config.save(dir.path()).unwrap();

        // load_local_only should work without any global config
        let loaded = VaultConfig::load_local_only(dir.path()).unwrap();
        assert_eq!(loaded.llm.provider, "ollama");
        assert_eq!(loaded.llm.model, "gemma3:1b");
    }

    #[test]
    fn test_empty_string_does_not_shadow_global() {
        let global: toml::Value = toml::from_str(
            r#"
            [llm]
            provider = "openai"
            model = "gpt-4o-mini"
            "#,
        )
        .unwrap();
        let local: toml::Value = toml::from_str(
            r#"
            [llm]
            provider = ""
            "#,
        )
        .unwrap();
        let merged = merge_toml(global, local);
        let config: VaultConfig = merged.try_into().unwrap();
        // Empty string should NOT shadow the global value
        assert_eq!(config.llm.provider, "openai");
        assert_eq!(config.llm.model, "gpt-4o-mini");
    }

    #[test]
    fn test_llm_is_empty() {
        let empty = LlmConfig::default();
        assert!(empty.is_empty());

        let configured = LlmConfig {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            api_key: None,
            api_key_env: None,
            base_url: None,
        };
        assert!(!configured.is_empty());
    }

    #[test]
    fn test_skip_serializing_empty_llm() {
        let config = VaultConfig::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        // Empty LLM section should NOT appear in serialized output
        assert!(
            !serialized.contains("[llm]"),
            "empty [llm] section should be skipped in serialization"
        );
    }

    #[test]
    fn test_mixed_config_merge() {
        // Global has provider, local has base_url. Merge produces both.
        let global: toml::Value = toml::from_str(
            r#"
            [llm]
            provider = "openai"
            model = "gpt-4o-mini"
            api_key_env = "OPENAI_API_KEY"
            "#,
        )
        .unwrap();
        let local: toml::Value = toml::from_str(
            r#"
            [llm]
            base_url = "https://custom.api.example.com"
            "#,
        )
        .unwrap();
        let merged = merge_toml(global, local);
        let config: VaultConfig = merged.try_into().unwrap();
        assert_eq!(config.llm.provider, "openai");
        assert_eq!(config.llm.model, "gpt-4o-mini");
        assert_eq!(
            config.llm.base_url.as_deref(),
            Some("https://custom.api.example.com")
        );
    }

    #[test]
    fn test_global_save_and_load() {
        // Test save/load roundtrip through a temp dir acting as global
        let dir = tempfile::tempdir().unwrap();
        let config = VaultConfig {
            llm: LlmConfig {
                provider: "anthropic".to_string(),
                model: "claude-haiku-4-5-20251001".to_string(),
                api_key: None,
                api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
                base_url: None,
            },
            ..VaultConfig::default()
        };
        config.save(dir.path()).unwrap();
        let loaded = VaultConfig::load_local_only(dir.path()).unwrap();
        assert_eq!(loaded.llm.provider, "anthropic");
        assert_eq!(loaded.llm.model, "claude-haiku-4-5-20251001");
        assert_eq!(loaded.llm.api_key_env.as_deref(), Some("ANTHROPIC_API_KEY"));
    }
}
