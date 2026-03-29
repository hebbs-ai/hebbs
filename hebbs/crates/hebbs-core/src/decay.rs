use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError};

use hebbs_storage::{BatchOperation, ColumnFamilyName, StorageBackend};

use crate::keys;
use crate::memory::Memory;

/// Default decay half-life: 30 days in microseconds.
pub const DEFAULT_HALF_LIFE_US: u64 = 30 * 24 * 3600 * 1_000_000;

/// Default sweep interval: 1 hour in microseconds.
pub const DEFAULT_SWEEP_INTERVAL_US: u64 = 3600 * 1_000_000;

/// Default batch size for decay sweeps.
pub const DEFAULT_DECAY_BATCH_SIZE: usize = 10_000;

/// Default maximum batches per sweep (caps total work).
pub const DEFAULT_MAX_BATCHES_PER_SWEEP: usize = 100;

/// Default auto-forget threshold.
pub const DEFAULT_AUTO_FORGET_THRESHOLD: f32 = 0.01;

/// Default epsilon for score-change detection.
pub const DEFAULT_DECAY_EPSILON: f32 = 0.001;

/// Default reinforcement cap for the decay formula.
pub const DEFAULT_REINFORCEMENT_CAP: u64 = 100;

/// Meta CF key for the decay sweep cursor.
const DECAY_CURSOR_KEY: &str = "decay_sweep_cursor";

/// Meta CF key prefix for auto-forget candidates.
const AUTO_FORGET_PREFIX: &str = "auto_forget_candidates:";

/// Configuration for the decay engine.
#[derive(Debug, Clone)]
pub struct DecayConfig {
    /// Exponential decay half-life in microseconds.
    /// At `age == half_life`, the time factor equals 0.5.
    pub half_life_us: u64,

    /// Time between sweep starts in microseconds.
    /// Minimum: 1_000_000 (1 second).
    pub sweep_interval_us: u64,

    /// Memories processed per batch within a sweep.
    /// Bounded at [100, 1_000_000].
    pub batch_size: usize,

    /// Maximum batches per sweep. Caps total work to `batch_size × max_batches`.
    /// Bounded at [1, 10_000].
    pub max_batches_per_sweep: usize,

    /// Decay score below which a memory becomes an auto-forget candidate.
    pub auto_forget_threshold: f32,

    /// Minimum score change to trigger a write. Avoids unnecessary write amplification.
    pub epsilon: f32,

    /// Cap on access_count reinforcement to prevent runaway amplification.
    pub reinforcement_cap: u64,

    /// Master switch. When false, the worker thread idles.
    pub enabled: bool,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            half_life_us: DEFAULT_HALF_LIFE_US,
            sweep_interval_us: DEFAULT_SWEEP_INTERVAL_US,
            batch_size: DEFAULT_DECAY_BATCH_SIZE,
            max_batches_per_sweep: DEFAULT_MAX_BATCHES_PER_SWEEP,
            auto_forget_threshold: DEFAULT_AUTO_FORGET_THRESHOLD,
            epsilon: DEFAULT_DECAY_EPSILON,
            reinforcement_cap: DEFAULT_REINFORCEMENT_CAP,
            enabled: true,
        }
    }
}

impl DecayConfig {
    /// Validate and clamp configuration parameters to their documented bounds.
    pub fn validated(mut self) -> Self {
        self.half_life_us = self.half_life_us.max(1);
        self.sweep_interval_us = self.sweep_interval_us.max(1_000_000);
        self.batch_size = self.batch_size.clamp(100, 1_000_000);
        self.max_batches_per_sweep = self.max_batches_per_sweep.clamp(1, 10_000);
        self.auto_forget_threshold = self.auto_forget_threshold.clamp(0.0, 1.0);
        self.epsilon = self.epsilon.max(f32::EPSILON);
        self.reinforcement_cap = self.reinforcement_cap.max(1);
        self
    }
}

/// Compute the decay score for a memory.
///
/// ## Corrected formula (differs from PhasePlan.md)
///
/// ```text
/// decay_score = importance × 2^(−age / half_life) × (1 + log₂(1 + access_count) / log₂(1 + reinforcement_cap))
/// ```
///
/// The PhasePlan.md formula `importance × 2^(-age/half_life) × log₂(1 + access_count)`
/// produces 0 when `access_count = 0`, which would make freshly-remembered memories
/// immediate auto-forget candidates. The corrected formula uses a reinforcement
/// multiplier in the range [1.0, ~2.0], where:
///
/// - 1.0 = no reinforcement (access_count = 0)
/// - ~2.0 = maximum reinforcement (access_count >= reinforcement_cap)
///
/// A never-accessed memory decays purely by `importance × 2^(-age/half_life)`.
///
/// ## Age computation
///
/// `age = now_us - last_accessed_at` (NOT `created_at`).
/// Reinforcement resets the decay clock.
///
/// ## Complexity: O(1)
///
/// Two floating-point divisions, two log2 calls, two multiplies.
#[inline]
pub fn compute_decay_score(
    importance: f32,
    last_accessed_at: u64,
    access_count: u64,
    now_us: u64,
    half_life_us: u64,
    reinforcement_cap: u64,
) -> f32 {
    if importance <= 0.0 || half_life_us == 0 {
        return 0.0;
    }

    let age_us = now_us.saturating_sub(last_accessed_at);
    let time_factor = 2.0_f64.powf(-(age_us as f64) / (half_life_us as f64));

    let capped_access = access_count.min(reinforcement_cap);
    let log_cap = (1.0 + reinforcement_cap as f64).log2();
    let reinforcement = if log_cap > 0.0 {
        1.0 + (1.0 + capped_access as f64).log2() / log_cap
    } else {
        1.0
    };

    (importance as f64 * time_factor * reinforcement) as f32
}

/// Control signals sent to the decay worker thread.
#[derive(Debug)]
pub enum DecaySignal {
    Resume,
    Pause,
    Shutdown,
    Reconfigure(DecayConfig),
}

/// Handle for controlling the decay engine from the Engine.
///
/// The worker runs on a dedicated OS thread (not tokio) because the
/// decay sweep is CPU-bound computation interspersed with sequential I/O.
/// This avoids starving the tokio executor when Phase 8 introduces
/// the async gRPC server.
pub struct DecayHandle {
    tx: Sender<DecaySignal>,
    thread: Option<thread::JoinHandle<()>>,
}

impl DecayHandle {
    /// Send a resume signal to start or continue sweeping.
    pub fn resume(&self) {
        let _ = self.tx.send(DecaySignal::Resume);
    }

    /// Send a pause signal. Worker finishes current batch then idles.
    pub fn pause(&self) {
        let _ = self.tx.send(DecaySignal::Pause);
    }

    /// Send a shutdown signal and wait for the thread to exit.
    pub fn shutdown(&mut self) {
        let _ = self.tx.send(DecaySignal::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }

    /// Reconfigure the decay engine. Applied before the next batch.
    pub fn reconfigure(&self, config: DecayConfig) {
        let _ = self.tx.send(DecaySignal::Reconfigure(config));
    }
}

impl Drop for DecayHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(DecaySignal::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Spawn the decay worker thread.
///
/// Returns a `DecayHandle` for controlling the worker from the engine.
/// The worker starts in a paused state and must be resumed explicitly.
pub fn spawn_decay_worker(
    storage: std::sync::Weak<dyn StorageBackend>,
    config: DecayConfig,
) -> DecayHandle {
    let (tx, rx) = crossbeam_channel::unbounded();

    let thread = thread::Builder::new()
        .name("hebbs-decay".into())
        .spawn(move || {
            decay_worker_loop(storage, config, rx);
        })
        .expect("failed to spawn decay worker thread");

    DecayHandle {
        tx,
        thread: Some(thread),
    }
}

fn decay_worker_loop(
    weak_storage: std::sync::Weak<dyn StorageBackend>,
    mut config: DecayConfig,
    rx: Receiver<DecaySignal>,
) {
    let mut paused = true;

    loop {
        if paused {
            match rx.recv() {
                Ok(DecaySignal::Resume) => {
                    paused = false;
                    continue;
                }
                Ok(DecaySignal::Shutdown) => return,
                Ok(DecaySignal::Reconfigure(new_config)) => {
                    config = new_config.validated();
                    continue;
                }
                Ok(DecaySignal::Pause) => continue,
                Err(_) => return,
            }
        }

        if !config.enabled {
            paused = true;
            continue;
        }

        // Upgrade Weak to temporary Arc for this sweep cycle.
        // If the Engine has been dropped (vault evicted), exit the worker.
        let storage = match weak_storage.upgrade() {
            Some(s) => s,
            None => return,
        };

        // Run one sweep
        run_decay_sweep(&storage, &config);

        // Drop the temporary Arc so it doesn't extend Storage lifetime
        // between sweep cycles.
        drop(storage);

        // Wait for the sweep interval, checking for signals
        let interval = Duration::from_micros(config.sweep_interval_us);
        let deadline = std::time::Instant::now() + interval;

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            match rx.recv_timeout(remaining) {
                Ok(DecaySignal::Pause) => {
                    paused = true;
                    break;
                }
                Ok(DecaySignal::Shutdown) => return,
                Ok(DecaySignal::Resume) => continue,
                Ok(DecaySignal::Reconfigure(new_config)) => {
                    config = new_config.validated();
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => break,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
            }
        }

        if paused {
            continue;
        }

        // Check for signals that arrived during the sweep
        match rx.try_recv() {
            Ok(DecaySignal::Pause) => {
                paused = true;
            }
            Ok(DecaySignal::Shutdown) => return,
            Ok(DecaySignal::Resume) => {}
            Ok(DecaySignal::Reconfigure(new_config)) => {
                config = new_config.validated();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return,
        }
    }
}

/// Execute one full decay sweep: iterate through memories in batches,
/// update decay scores, and identify auto-forget candidates.
fn run_decay_sweep(storage: &Arc<dyn StorageBackend>, config: &DecayConfig) {
    let now_us = crate::engine::now_microseconds();

    // Load cursor from meta CF
    let cursor_key = keys::encode_meta_key(DECAY_CURSOR_KEY);
    let cursor = match storage.get(ColumnFamilyName::Meta, &cursor_key) {
        Ok(Some(bytes)) => bytes,
        _ => Vec::new(),
    };

    let mut current_cursor = cursor;
    let mut _total_processed = 0usize;
    let mut _total_updated = 0usize;
    let mut _total_candidates = 0usize;

    for _batch_num in 0..config.max_batches_per_sweep {
        let entries = if current_cursor.is_empty() {
            match storage.prefix_iterator(ColumnFamilyName::Default, &[]) {
                Ok(e) => e,
                Err(_) => break,
            }
        } else {
            // Range from cursor to end. RocksDB range_iterator uses [start, end).
            // We use start=cursor and scan forward.
            match storage.prefix_iterator(ColumnFamilyName::Default, &[]) {
                Ok(all) => all
                    .into_iter()
                    .filter(|(k, _)| k.as_slice() > current_cursor.as_slice())
                    .collect(),
                Err(_) => break,
            }
        };

        if entries.is_empty() {
            // Wrap around: reset cursor to start
            current_cursor = Vec::new();
            let _ = storage.put(ColumnFamilyName::Meta, &cursor_key, &current_cursor);
            break;
        }

        let batch_entries: Vec<_> = entries.into_iter().take(config.batch_size).collect();
        if batch_entries.is_empty() {
            break;
        }

        let mut update_ops = Vec::new();
        let mut candidate_ops = Vec::new();

        for (key, value) in &batch_entries {
            let memory = match Memory::from_bytes(value) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let new_score = compute_decay_score(
                memory.importance,
                memory.last_accessed_at,
                memory.access_count,
                now_us,
                config.half_life_us,
                config.reinforcement_cap,
            );

            if (new_score - memory.decay_score).abs() > config.epsilon {
                let mut updated = memory.clone();
                updated.decay_score = new_score;
                update_ops.push(BatchOperation::Put {
                    cf: ColumnFamilyName::Default,
                    key: key.clone(),
                    value: updated.to_bytes(),
                });
                _total_updated += 1;
            }

            if new_score < config.auto_forget_threshold {
                let candidate_key = format!("{}{}", AUTO_FORGET_PREFIX, hex::encode(key));
                candidate_ops.push(BatchOperation::Put {
                    cf: ColumnFamilyName::Meta,
                    key: candidate_key.into_bytes(),
                    value: new_score.to_le_bytes().to_vec(),
                });
                _total_candidates += 1;
            }

            _total_processed += 1;
        }

        if !update_ops.is_empty() {
            if let Err(e) = storage.write_batch(&update_ops) {
                tracing::error!(
                    error = %e,
                    count = update_ops.len(),
                    "decay score update WriteBatch failed (non-fatal)"
                );
            }
        }

        if !candidate_ops.is_empty() {
            if let Err(e) = storage.write_batch(&candidate_ops) {
                tracing::error!(
                    error = %e,
                    count = candidate_ops.len(),
                    "auto-forget candidate WriteBatch failed (non-fatal)"
                );
            }
        }

        // Update cursor to last processed key
        if let Some((last_key, _)) = batch_entries.last() {
            current_cursor = last_key.clone();
            if let Err(e) = storage.put(ColumnFamilyName::Meta, &cursor_key, &current_cursor) {
                tracing::error!(
                    error = %e,
                    "decay cursor update failed (non-fatal)"
                );
            }
        }

        // If we processed fewer than batch_size, we've reached the end
        if batch_entries.len() < config.batch_size {
            current_cursor = Vec::new();
            if let Err(e) = storage.put(ColumnFamilyName::Meta, &cursor_key, &current_cursor) {
                tracing::error!(
                    error = %e,
                    "decay cursor reset failed (non-fatal)"
                );
            }
            break;
        }
    }
}

/// Read auto-forget candidate IDs from the meta CF.
///
/// Returns the memory IDs (as raw bytes) of all candidates whose decay
/// scores fell below the threshold during the most recent sweep.
pub fn read_auto_forget_candidates(storage: &dyn StorageBackend) -> Vec<Vec<u8>> {
    let prefix = AUTO_FORGET_PREFIX.as_bytes().to_vec();
    let entries = match storage.prefix_iterator(ColumnFamilyName::Meta, &prefix) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(
                error = %e,
                "read auto-forget candidates failed (non-fatal)"
            );
            return Vec::new();
        }
    };

    let mut candidates = Vec::with_capacity(entries.len());
    for (key, _value) in entries {
        let key_str = match std::str::from_utf8(&key) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Some(hex_id) = key_str.strip_prefix(AUTO_FORGET_PREFIX) {
            if let Ok(id_bytes) = hex::decode(hex_id) {
                candidates.push(id_bytes);
            }
        }
    }

    candidates
}

/// Clear all auto-forget candidates from the meta CF.
pub fn clear_auto_forget_candidates(storage: &dyn StorageBackend) {
    let prefix = AUTO_FORGET_PREFIX.as_bytes().to_vec();
    if let Ok(entries) = storage.prefix_iterator(ColumnFamilyName::Meta, &prefix) {
        let ops: Vec<BatchOperation> = entries
            .into_iter()
            .map(|(key, _)| BatchOperation::Delete {
                cf: ColumnFamilyName::Meta,
                key,
            })
            .collect();
        if !ops.is_empty() {
            if let Err(e) = storage.write_batch(&ops) {
                tracing::error!(
                    error = %e,
                    count = ops.len(),
                    "clear auto-forget candidates WriteBatch failed (non-fatal)"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_score_zero_access_nonzero() {
        let score = compute_decay_score(0.8, 1_000_000, 0, 1_000_000, DEFAULT_HALF_LIFE_US, 100);
        assert!(
            score > 0.0,
            "zero access_count should still produce positive score, got {}",
            score
        );
        assert!(
            (score - 0.8).abs() < 0.01,
            "at age=0 with no accesses, score should be near importance (0.8), got {}",
            score
        );
    }

    #[test]
    fn decay_score_half_life() {
        let now = DEFAULT_HALF_LIFE_US + 1_000_000;
        let score = compute_decay_score(1.0, 1_000_000, 0, now, DEFAULT_HALF_LIFE_US, 100);
        // At exactly one half-life, time_factor = 0.5, reinforcement = 1.0
        // score ≈ 1.0 * 0.5 * 1.0 = 0.5
        assert!(
            (score - 0.5).abs() < 0.01,
            "at one half-life, score should be ~0.5, got {}",
            score
        );
    }

    #[test]
    fn decay_score_double_half_life() {
        let now = 2 * DEFAULT_HALF_LIFE_US + 1_000_000;
        let score = compute_decay_score(1.0, 1_000_000, 0, now, DEFAULT_HALF_LIFE_US, 100);
        // At 2 half-lives, time_factor = 0.25
        assert!(
            (score - 0.25).abs() < 0.01,
            "at two half-lives, score should be ~0.25, got {}",
            score
        );
    }

    #[test]
    fn decay_score_reinforcement_increases() {
        let now = 1_000_000;
        let score_0 = compute_decay_score(1.0, now, 0, now, DEFAULT_HALF_LIFE_US, 100);
        let score_10 = compute_decay_score(1.0, now, 10, now, DEFAULT_HALF_LIFE_US, 100);
        let score_100 = compute_decay_score(1.0, now, 100, now, DEFAULT_HALF_LIFE_US, 100);

        assert!(
            score_10 > score_0,
            "10 accesses should score higher than 0: {} vs {}",
            score_10,
            score_0
        );
        assert!(
            score_100 > score_10,
            "100 accesses should score higher than 10: {} vs {}",
            score_100,
            score_10
        );
    }

    #[test]
    fn decay_score_importance_scales() {
        let now = 1_000_000;
        let score_low = compute_decay_score(0.2, now, 5, now, DEFAULT_HALF_LIFE_US, 100);
        let score_high = compute_decay_score(0.9, now, 5, now, DEFAULT_HALF_LIFE_US, 100);

        assert!(
            score_high > score_low,
            "higher importance should produce higher score: {} vs {}",
            score_high,
            score_low
        );
    }

    #[test]
    fn decay_score_zero_importance() {
        let score = compute_decay_score(0.0, 1_000_000, 50, 1_000_000, DEFAULT_HALF_LIFE_US, 100);
        assert_eq!(score, 0.0, "zero importance always produces zero score");
    }

    #[test]
    fn decay_score_capped_reinforcement() {
        let now = 1_000_000;
        let score_at_cap = compute_decay_score(1.0, now, 100, now, DEFAULT_HALF_LIFE_US, 100);
        let score_above_cap = compute_decay_score(1.0, now, 10_000, now, DEFAULT_HALF_LIFE_US, 100);

        assert!(
            (score_at_cap - score_above_cap).abs() < 0.001,
            "access_count above cap should not increase score: {} vs {}",
            score_at_cap,
            score_above_cap
        );
    }

    #[test]
    fn decay_config_validation() {
        let config = DecayConfig {
            half_life_us: 0,
            sweep_interval_us: 100,
            batch_size: 10,
            max_batches_per_sweep: 0,
            auto_forget_threshold: 2.0,
            epsilon: -1.0,
            reinforcement_cap: 0,
            enabled: true,
        };

        let validated = config.validated();
        assert!(validated.half_life_us >= 1);
        assert!(validated.sweep_interval_us >= 1_000_000);
        assert!(validated.batch_size >= 100);
        assert!(validated.max_batches_per_sweep >= 1);
        assert!(validated.auto_forget_threshold <= 1.0);
        assert!(validated.epsilon > 0.0);
        assert!(validated.reinforcement_cap >= 1);
    }
}
