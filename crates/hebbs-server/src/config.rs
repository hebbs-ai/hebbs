use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct HebbsConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub embedding: EmbeddingConfig,
    pub decay: DecaySection,
    pub reflect: ReflectSection,
    pub logging: LoggingConfig,
    pub metrics: MetricsConfig,
    pub auth: AuthConfig,
    pub tenancy: TenancyConfig,
    pub rate_limit: hebbs_core::rate_limit::RateLimitConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub grpc_port: u16,
    pub http_port: u16,
    pub bind_address: String,
    pub max_connections: usize,
    pub request_timeout_ms: u64,
    pub max_blocking_threads: usize,
    pub shutdown_timeout_secs: u64,
    pub max_request_size_bytes: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            grpc_port: 6380,
            http_port: 6381,
            bind_address: "0.0.0.0".to_string(),
            max_connections: 1000,
            request_timeout_ms: 30_000,
            max_blocking_threads: 256,
            shutdown_timeout_secs: 15,
            max_request_size_bytes: 1_048_576,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub data_dir: String,
    pub block_cache_mb: usize,
    pub write_buffer_mb: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: "./hebbs-data".to_string(),
            block_cache_mb: 256,
            write_buffer_mb: 64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    pub model_path: Option<String>,
    pub dimensions: usize,
    pub max_batch_size: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model_path: None,
            dimensions: 384,
            max_batch_size: 256,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DecaySection {
    pub enabled: bool,
    pub half_life_days: u64,
    pub sweep_interval_secs: u64,
    pub batch_size: usize,
    pub auto_forget_threshold: f32,
}

impl Default for DecaySection {
    fn default() -> Self {
        Self {
            enabled: true,
            half_life_days: 30,
            sweep_interval_secs: 3600,
            batch_size: 10_000,
            auto_forget_threshold: 0.01,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReflectSection {
    pub enabled: bool,
    pub trigger_check_interval_secs: u64,
    pub threshold_trigger_count: usize,
    pub schedule_trigger_interval_secs: u64,
    pub max_memories_per_reflect: usize,
    pub min_memories_for_reflect: usize,
    pub proposal_provider: String,
    pub proposal_model: String,
    pub validation_provider: String,
    pub validation_model: String,
}

impl Default for ReflectSection {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger_check_interval_secs: 60,
            threshold_trigger_count: 50,
            schedule_trigger_interval_secs: 86400,
            max_memories_per_reflect: 5000,
            min_memories_for_reflect: 5,
            proposal_provider: "openai".to_string(),
            proposal_model: "gpt-4o".to_string(),
            validation_provider: "openai".to_string(),
            validation_model: "gpt-4o".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "pretty".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub endpoint: String,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            endpoint: "/v1/metrics".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub enabled: bool,
    pub keys_file: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keys_file: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TlsConfig {
    pub enabled: bool,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub client_ca_path: Option<String>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cert_path: None,
            key_path: None,
            client_ca_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TenancyConfig {
    pub max_tenants: u32,
    pub max_memories_per_tenant: u64,
    pub hnsw_eviction_secs: u64,
    pub max_loaded_hnsw: u32,
    pub max_snapshots_per_memory: u32,
}

impl Default for TenancyConfig {
    fn default() -> Self {
        Self {
            max_tenants: 10_000,
            max_memories_per_tenant: 10_000_000,
            hnsw_eviction_secs: 3_600,
            max_loaded_hnsw: 100,
            max_snapshots_per_memory: 100,
        }
    }
}

impl HebbsConfig {
    /// Load configuration from file, then apply environment variable overrides.
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        let mut config = if let Some(p) = path {
            let contents = std::fs::read_to_string(p)
                .map_err(|e| format!("failed to read config file {}: {}", p.display(), e))?;
            toml::from_str(&contents)
                .map_err(|e| format!("failed to parse config file {}: {}", p.display(), e))?
        } else if let Some(found) = Self::discover_config_file() {
            let contents = std::fs::read_to_string(&found)
                .map_err(|e| format!("failed to read config file {}: {}", found.display(), e))?;
            toml::from_str(&contents)
                .map_err(|e| format!("failed to parse config file {}: {}", found.display(), e))?
        } else {
            Self::default()
        };

        config.apply_env_overrides();
        Ok(config)
    }

    fn discover_config_file() -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = vec![PathBuf::from("./hebbs.toml")];
        if let Some(config_dir) = dirs_next() {
            candidates.push(config_dir.join("hebbs/hebbs.toml"));
        }
        candidates.push(PathBuf::from("/etc/hebbs/hebbs.toml"));
        candidates.into_iter().find(|c| c.exists())
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("HEBBS_SERVER_GRPC_PORT") {
            if let Ok(p) = v.parse() {
                self.server.grpc_port = p;
            }
        }
        if let Ok(v) = std::env::var("HEBBS_SERVER_HTTP_PORT") {
            if let Ok(p) = v.parse() {
                self.server.http_port = p;
            }
        }
        if let Ok(v) = std::env::var("HEBBS_SERVER_BIND_ADDRESS") {
            self.server.bind_address = v;
        }
        if let Ok(v) = std::env::var("HEBBS_STORAGE_DATA_DIR") {
            self.storage.data_dir = v;
        }
        if let Ok(v) = std::env::var("HEBBS_LOGGING_LEVEL") {
            self.logging.level = v;
        }
        if let Ok(v) = std::env::var("HEBBS_LOGGING_FORMAT") {
            self.logging.format = v;
        }
        if let Ok(v) = std::env::var("HEBBS_DECAY_ENABLED") {
            if let Ok(b) = v.parse() {
                self.decay.enabled = b;
            }
        }
        if let Ok(v) = std::env::var("HEBBS_REFLECT_ENABLED") {
            if let Ok(b) = v.parse() {
                self.reflect.enabled = b;
            }
        }
        if let Ok(v) = std::env::var("HEBBS_AUTH_ENABLED") {
            if let Ok(b) = v.parse() {
                self.auth.enabled = b;
            }
        }
        if let Ok(v) = std::env::var("HEBBS_TENANCY_MAX_TENANTS") {
            if let Ok(n) = v.parse() {
                self.tenancy.max_tenants = n;
            }
        }
        if let Ok(v) = std::env::var("HEBBS_TENANCY_MAX_MEMORIES_PER_TENANT") {
            if let Ok(n) = v.parse() {
                self.tenancy.max_memories_per_tenant = n;
            }
        }
        if let Ok(v) = std::env::var("HEBBS_RATE_LIMIT_ENABLED") {
            if let Ok(b) = v.parse() {
                self.rate_limit.enabled = b;
            }
        }
        if let Ok(v) = std::env::var("HEBBS_RATE_LIMIT_WRITE_RATE") {
            if let Ok(n) = v.parse() {
                self.rate_limit.write_rate = n;
            }
        }
        if let Ok(v) = std::env::var("HEBBS_RATE_LIMIT_READ_RATE") {
            if let Ok(n) = v.parse() {
                self.rate_limit.read_rate = n;
            }
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.server.grpc_port == 0 {
            return Err("server.grpc_port must be non-zero".to_string());
        }
        if self.server.http_port == 0 {
            return Err("server.http_port must be non-zero".to_string());
        }
        if self.server.grpc_port == self.server.http_port {
            return Err("server.grpc_port and server.http_port must be different".to_string());
        }
        if self.server.shutdown_timeout_secs == 0 {
            return Err("server.shutdown_timeout_secs must be > 0".to_string());
        }
        if self.server.max_request_size_bytes == 0 {
            return Err("server.max_request_size_bytes must be > 0".to_string());
        }
        Ok(())
    }
}

fn dirs_next() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = HebbsConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn parses_minimal_toml() {
        let toml_str = r#"
[server]
grpc_port = 7000
http_port = 7001
"#;
        let config: HebbsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.grpc_port, 7000);
        assert_eq!(config.server.http_port, 7001);
        assert_eq!(config.storage.data_dir, "./hebbs-data");
    }

    #[test]
    fn validates_same_port_error() {
        let mut config = HebbsConfig::default();
        config.server.grpc_port = 8080;
        config.server.http_port = 8080;
        assert!(config.validate().is_err());
    }

    #[test]
    fn validates_zero_port_error() {
        let mut config = HebbsConfig::default();
        config.server.grpc_port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn full_toml_round_trip() {
        let config = HebbsConfig::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: HebbsConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config.server.grpc_port, deserialized.server.grpc_port);
        assert_eq!(config.storage.data_dir, deserialized.storage.data_dir);
    }
}
