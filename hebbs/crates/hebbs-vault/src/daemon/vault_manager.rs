//! Manages open vault handles for the daemon.
//!
//! Each vault is opened on demand and closed after an idle period.
//! The ONNX embedder is loaded once and shared across all vaults.
//! File watchers are started when vaults open and stopped when they close
//! (Milestone 3: watch merged into serve).
//!
//! Complexity:
//! - `get_or_open`: O(1) HashMap lookup + O(1) amortized open on miss
//! - `evict_idle`: O(n) scan where n = number of open vaults (bounded by total vaults)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{Event, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{info, warn};

use hebbs_core::decay::DecayConfig as CoreDecayConfig;
use hebbs_core::engine::Engine;
use hebbs_core::reflect::ReflectConfig;
use hebbs_embed::Embedder;

use crate::config::VaultConfig;

pub type VaultHandle = (Arc<Engine>, Arc<dyn Embedder>, DecayParams);

/// Default idle timeout before a vault handle is closed: 5 minutes.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Maximum number of concurrently open vaults to bound memory usage.
const MAX_OPEN_VAULTS: usize = 64;

/// A file-system event tagged with the vault it belongs to.
#[derive(Debug)]
pub struct VaultFsEvent {
    pub vault_path: PathBuf,
    pub event: Event,
}

/// Decay parameters extracted from vault config for use in serialization.
/// Avoids threading the full VaultConfig through response handlers.
#[derive(Debug, Clone, Copy)]
pub struct DecayParams {
    pub half_life_us: u64,
    pub reinforcement_cap: u64,
}

impl Default for DecayParams {
    fn default() -> Self {
        Self {
            half_life_us: hebbs_core::decay::DEFAULT_HALF_LIFE_US,
            reinforcement_cap: hebbs_core::decay::DEFAULT_REINFORCEMENT_CAP,
        }
    }
}

/// A single open vault with its engine handle and last-access timestamp.
struct OpenVault {
    engine: Arc<Engine>,
    embedder: Arc<dyn Embedder>,
    decay_params: DecayParams,
    last_accessed: Instant,
    /// Vault epoch from `.hebbs/epoch`. Used to detect vault re-initialization.
    epoch: String,
    /// File watcher handle. Dropped when vault closes (stops watching).
    _watcher: Option<notify::RecommendedWatcher>,
}

/// Manages the lifecycle of vault engine handles.
///
/// The vault manager owns a shared embedder (ONNX model loaded once) and
/// opens per-vault RocksDB instances on demand. Idle vaults are evicted
/// after `idle_timeout`. File watchers are started per vault.
pub struct VaultManager {
    open_vaults: HashMap<PathBuf, OpenVault>,
    embedder: Arc<dyn Embedder>,
    idle_timeout: Duration,
    /// Channel sender for file-system events from all watchers.
    watch_tx: mpsc::Sender<VaultFsEvent>,
}

impl VaultManager {
    /// Create a new vault manager with a shared embedder.
    ///
    /// The embedder is loaded once and reused across all vaults.
    /// Returns the manager and a receiver for file-system events.
    pub fn new(
        embedder: Arc<dyn Embedder>,
        idle_timeout: Option<Duration>,
    ) -> (Self, mpsc::Receiver<VaultFsEvent>) {
        let (tx, rx) = mpsc::channel(2000);
        let mgr = Self {
            open_vaults: HashMap::new(),
            embedder,
            idle_timeout: idle_timeout.unwrap_or(DEFAULT_IDLE_TIMEOUT),
            watch_tx: tx,
        };
        (mgr, rx)
    }

    /// Get an existing vault handle or open a new one.
    ///
    /// Returns `(engine, embedder)` for the requested vault.
    /// Updates the last-accessed timestamp on hit.
    /// Starts a file watcher on first open.
    pub fn get_or_open(&mut self, vault_path: &Path) -> Result<VaultHandle, String> {
        let canonical = vault_path
            .canonicalize()
            .unwrap_or_else(|_| vault_path.to_path_buf());

        // Fast path: already open (verify epoch hasn't changed)
        if let Some(entry) = self.open_vaults.get(&canonical) {
            let current_epoch = read_epoch(&canonical);
            if current_epoch == entry.epoch {
                // Same vault instance, reuse
                let entry = self.open_vaults.get_mut(&canonical).unwrap();
                entry.last_accessed = Instant::now();
                return Ok((
                    entry.engine.clone(),
                    entry.embedder.clone(),
                    entry.decay_params,
                ));
            }
            // Vault was re-initialized (epoch changed). Evict stale handle.
            info!("vault epoch changed for {}, reopening", canonical.display());
            self.open_vaults.remove(&canonical);
        }

        // Check capacity before opening
        if self.open_vaults.len() >= MAX_OPEN_VAULTS {
            self.evict_lru();
        }

        // Open the vault
        let hebbs_dir = canonical.join(".hebbs");
        if !hebbs_dir.exists() {
            return Err(format!(
                "vault not initialized at {}: run `hebbs init` first",
                canonical.display()
            ));
        }

        let db_path = hebbs_dir.join("index").join("db");
        std::fs::create_dir_all(&db_path)
            .map_err(|e| format!("failed to create db directory: {}", e))?;

        let storage = Arc::new(
            hebbs_storage::RocksDbBackend::open(&db_path)
                .map_err(|e| format!("failed to open RocksDB: {}", e))?,
        );

        let engine = Arc::new(
            Engine::new(storage, self.embedder.clone())
                .map_err(|e| format!("failed to create engine: {}", e))?,
        );

        // Start decay worker if vault config enables it
        let decay_params = match VaultConfig::load(&hebbs_dir) {
            Ok(vault_config) => {
                let half_life_us =
                    (vault_config.decay.half_life_days as f64 * 24.0 * 3600.0 * 1_000_000.0) as u64;
                let sweep_interval_us = vault_config.decay.sweep_interval_secs.max(1) * 1_000_000;
                let core_decay = CoreDecayConfig {
                    half_life_us,
                    auto_forget_threshold: vault_config.decay.auto_forget_threshold,
                    reinforcement_cap: vault_config.decay.reinforcement_cap,
                    sweep_interval_us,
                    ..CoreDecayConfig::default()
                };
                engine.start_decay(core_decay);
                info!("decay worker started for {}", canonical.display());

                // Start reflect worker if LLM is configured
                if vault_config.llm.is_configured() {
                    let provider_config = vault_config.llm.to_provider_config();
                    let reflect_config = ReflectConfig {
                        proposal_provider_config: provider_config.clone(),
                        validation_provider_config: provider_config,
                        ..ReflectConfig::default()
                    };
                    engine.start_reflect(reflect_config);
                    info!("reflect worker started for {}", canonical.display());
                }

                DecayParams {
                    half_life_us,
                    reinforcement_cap: vault_config.decay.reinforcement_cap,
                }
            }
            Err(e) => {
                warn!(
                    "failed to load vault config for decay: {}, using defaults",
                    e
                );
                engine.start_decay(CoreDecayConfig::default());
                DecayParams::default()
            }
        };

        // Start file watcher for this vault
        let watcher = self.start_watcher(&canonical);

        info!("opened vault: {}", canonical.display());

        let epoch = read_epoch(&canonical);

        let entry = OpenVault {
            engine: engine.clone(),
            embedder: self.embedder.clone(),
            decay_params,
            last_accessed: Instant::now(),
            epoch,
            _watcher: watcher,
        };
        self.open_vaults.insert(canonical, entry);

        Ok((engine, self.embedder.clone(), decay_params))
    }

    /// Start a file watcher for a vault directory.
    fn start_watcher(&self, vault_path: &Path) -> Option<notify::RecommendedWatcher> {
        let tx = self.watch_tx.clone();
        let vault_path_owned = vault_path.to_path_buf();

        let mut watcher = match notify::recommended_watcher(
            move |res: std::result::Result<Event, notify::Error>| match res {
                Ok(event) => {
                    let _ = tx.blocking_send(VaultFsEvent {
                        vault_path: vault_path_owned.clone(),
                        event,
                    });
                }
                Err(e) => {
                    warn!("watcher error for {}: {}", vault_path_owned.display(), e);
                }
            },
        ) {
            Ok(w) => w,
            Err(e) => {
                warn!(
                    "failed to create watcher for {}: {}",
                    vault_path.display(),
                    e
                );
                return None;
            }
        };

        if let Err(e) = watcher.watch(vault_path, RecursiveMode::Recursive) {
            warn!("failed to start watching {}: {}", vault_path.display(), e);
            return None;
        }

        info!("watching {} for file changes", vault_path.display());
        Some(watcher)
    }

    /// Get a vault handle without opening (returns None if not already open).
    pub fn get(&mut self, vault_path: &Path) -> Option<(Arc<Engine>, Arc<dyn Embedder>)> {
        let canonical = vault_path
            .canonicalize()
            .unwrap_or_else(|_| vault_path.to_path_buf());

        self.open_vaults.get_mut(&canonical).map(|entry| {
            entry.last_accessed = Instant::now();
            (entry.engine.clone(), entry.embedder.clone())
        })
    }

    /// Close vaults that have been idle longer than `idle_timeout`.
    ///
    /// Returns the number of vaults evicted. Watcher handles are dropped
    /// automatically, stopping file watching for evicted vaults.
    pub fn evict_idle(&mut self) -> usize {
        let now = Instant::now();
        let timeout = self.idle_timeout;

        let stale_keys: Vec<PathBuf> = self
            .open_vaults
            .iter()
            .filter(|(_, v)| now.duration_since(v.last_accessed) > timeout)
            .map(|(k, _)| k.clone())
            .collect();

        let count = stale_keys.len();
        for key in stale_keys {
            info!("evicting idle vault (watcher stopped): {}", key.display());
            self.open_vaults.remove(&key);
        }
        count
    }

    /// Evict the least recently used vault to make room.
    fn evict_lru(&mut self) {
        if let Some(oldest_key) = self
            .open_vaults
            .iter()
            .min_by_key(|(_, v)| v.last_accessed)
            .map(|(k, _)| k.clone())
        {
            info!("evicting LRU vault: {}", oldest_key.display());
            self.open_vaults.remove(&oldest_key);
        }
    }

    /// Close a specific vault handle.
    pub fn close(&mut self, vault_path: &Path) {
        let canonical = vault_path
            .canonicalize()
            .unwrap_or_else(|_| vault_path.to_path_buf());
        if self.open_vaults.remove(&canonical).is_some() {
            info!("closed vault (watcher stopped): {}", canonical.display());
        }
    }

    /// Close all open vaults.
    pub fn close_all(&mut self) {
        let count = self.open_vaults.len();
        self.open_vaults.clear();
        info!("closed all {} vaults (watchers stopped)", count);
    }

    /// Number of currently open vaults.
    pub fn open_count(&self) -> usize {
        self.open_vaults.len()
    }

    /// Check health of all open vaults.
    /// Returns paths of vaults whose `.hebbs/` directory has disappeared.
    pub fn health_check(&mut self) -> Vec<PathBuf> {
        let mut unhealthy = Vec::new();
        for path in self.open_vaults.keys() {
            let hebbs_dir = path.join(".hebbs");
            if !hebbs_dir.exists() {
                warn!("vault data directory missing: {}", hebbs_dir.display());
                unhealthy.push(path.clone());
            }
        }

        // Remove unhealthy vaults
        for path in &unhealthy {
            self.open_vaults.remove(path);
        }

        unhealthy
    }

    /// Check if a vault is currently open.
    pub fn is_open(&self, vault_path: &Path) -> bool {
        let canonical = vault_path
            .canonicalize()
            .unwrap_or_else(|_| vault_path.to_path_buf());
        self.open_vaults.contains_key(&canonical)
    }

    /// Get the shared embedder reference.
    pub fn embedder(&self) -> Arc<dyn Embedder> {
        self.embedder.clone()
    }
}

/// Read the vault epoch from `.hebbs/epoch`. Returns empty string if missing.
fn read_epoch(vault_path: &Path) -> String {
    let epoch_path = vault_path.join(".hebbs").join("epoch");
    std::fs::read_to_string(epoch_path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // VaultManager tests require a real embedder and storage,
    // so they are integration tests in tests/daemon_integration.rs.
    // Unit tests here cover the simpler invariants.

    #[test]
    fn test_default_idle_timeout() {
        assert_eq!(DEFAULT_IDLE_TIMEOUT, Duration::from_secs(300));
    }

    #[test]
    fn test_max_open_vaults() {
        assert_eq!(MAX_OPEN_VAULTS, 64);
    }
}
