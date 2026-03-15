use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use globset::{Glob, GlobSet, GlobSetBuilder};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use hebbs_core::engine::Engine;
use hebbs_embed::Embedder;

use crate::config::VaultConfig;
use crate::error::{Result, VaultError};
use crate::ingest::{phase1_delete, phase1_ingest, phase2_ingest};
use crate::manifest::Manifest;

/// Accumulated stats from the watcher lifecycle.
#[derive(Debug, Default)]
pub struct WatcherStats {
    pub events_received: usize,
    pub phase1_runs: usize,
    pub phase2_runs: usize,
    pub burst_detections: usize,
}

/// File watcher daemon: watches a vault directory and runs the two-phase
/// ingest pipeline on file changes.
///
/// Architecture:
///   FS events -> debounce -> phase 1 (parse + manifest) -> debounce -> phase 2 (embed + index)
pub async fn watch_vault(
    vault_root: PathBuf,
    engine: Arc<Engine>,
    embedder: Arc<dyn Embedder>,
    cancel: CancellationToken,
) -> Result<WatcherStats> {
    let hebbs_dir = vault_root.join(".hebbs");
    if !hebbs_dir.exists() {
        return Err(VaultError::NotInitialized {
            path: vault_root.clone(),
        });
    }

    let config = VaultConfig::load(&hebbs_dir)?;
    let mut manifest = Manifest::load(&hebbs_dir)?;

    // Build ignore glob set
    let ignore_set = build_ignore_set(&config.effective_ignore_patterns())?;

    // Startup catch-up: index any files changed since last run
    info!("running startup catch-up");
    let changed_files = find_changed_files(&vault_root, &manifest, &ignore_set)?;
    if !changed_files.is_empty() {
        info!(
            "catch-up: {} files changed since last run",
            changed_files.len()
        );
        let p1_stats = phase1_ingest(&changed_files, &vault_root, &mut manifest, &config)?;
        manifest.save(&hebbs_dir)?;
        info!(
            "catch-up phase 1: {} processed, {} new, {} modified",
            p1_stats.files_processed, p1_stats.sections_new, p1_stats.sections_modified
        );

        let p2_stats =
            phase2_ingest(&vault_root, &mut manifest, &engine, &embedder, &config).await?;
        manifest.save(&hebbs_dir)?;
        info!(
            "catch-up phase 2: {} embedded, {} remembered, {} revised",
            p2_stats.sections_embedded, p2_stats.sections_remembered, p2_stats.sections_revised
        );
    }

    // Set up file watcher
    let (tx, mut rx) = mpsc::channel::<WatchEvent>(1000);

    let watcher_tx = tx.clone();
    let mut watcher = notify::recommended_watcher(
        move |res: std::result::Result<Event, notify::Error>| match res {
            Ok(event) => {
                let _ = watcher_tx.blocking_send(WatchEvent::FsEvent(event));
            }
            Err(e) => {
                warn!("watcher error: {}", e);
            }
        },
    )
    .map_err(|e| VaultError::Watcher {
        reason: format!("failed to create watcher: {e}"),
    })?;

    watcher
        .watch(&vault_root, RecursiveMode::Recursive)
        .map_err(|e| VaultError::Watcher {
            reason: format!("failed to watch {}: {e}", vault_root.display()),
        })?;

    info!("watching {} for .md file changes", vault_root.display());

    let mut stats = WatcherStats::default();
    let mut pending_creates: HashSet<PathBuf> = HashSet::new();
    let mut pending_deletes: HashSet<PathBuf> = HashSet::new();
    let mut events_in_window = 0usize;

    let phase1_duration = Duration::from_millis(config.watch.phase1_debounce_ms);
    let phase2_duration = Duration::from_millis(config.watch.phase2_debounce_ms);
    let burst_duration = Duration::from_millis(config.watch.burst_debounce_ms);

    let mut phase1_timer = tokio::time::interval(phase1_duration);
    phase1_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut phase1_armed = false;

    let mut phase2_deadline: Option<tokio::time::Instant> = None;
    let mut has_stale_sections = false;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("watcher shutdown requested");
                break;
            }

            event = rx.recv() => {
                match event {
                    Some(WatchEvent::FsEvent(fs_event)) => {
                        for path in &fs_event.paths {
                            // Filter: only .md files, skip ignored patterns
                            if !is_relevant_md(path, &vault_root, &ignore_set) {
                                continue;
                            }

                            stats.events_received += 1;
                            events_in_window += 1;

                            match fs_event.kind {
                                EventKind::Create(_) | EventKind::Modify(_) => {
                                    pending_deletes.remove(path);
                                    pending_creates.insert(path.clone());
                                }
                                EventKind::Remove(_) => {
                                    pending_creates.remove(path);
                                    pending_deletes.insert(path.clone());
                                }
                                _ => {}
                            }

                            phase1_armed = true;
                        }
                    }
                    None => break,
                }
            }

            _ = phase1_timer.tick(), if phase1_armed => {
                if pending_creates.is_empty() && pending_deletes.is_empty() {
                    phase1_armed = false;
                    continue;
                }

                // Run phase 1
                let creates: Vec<PathBuf> = pending_creates.drain().collect();
                let deletes: Vec<PathBuf> = pending_deletes.drain().collect();

                // Phase 1: parse creates/modifies
                if !creates.is_empty() {
                    match phase1_ingest(&creates, &vault_root, &mut manifest, &config) {
                        Ok(p1) => {
                            info!(
                                "[phase1] parsed {} files ({} new, {} modified, {} unchanged)",
                                p1.files_processed, p1.sections_new, p1.sections_modified, p1.sections_unchanged
                            );
                            stats.phase1_runs += 1;
                        }
                        Err(e) => warn!("phase 1 error: {}", e),
                    }
                }

                // Phase 1: handle deletes
                for path in &deletes {
                    match phase1_delete(path, &vault_root, &mut manifest) {
                        Ok(n) => {
                            if n > 0 {
                                info!("[phase1] deleted {}: {} sections orphaned", path.display(), n);
                            }
                        }
                        Err(e) => warn!("phase 1 delete error for {}: {}", path.display(), e),
                    }
                }

                // Save manifest after phase 1
                if let Err(e) = manifest.save(&hebbs_dir) {
                    warn!("failed to save manifest: {}", e);
                }

                // Arm phase 2 timer
                let is_burst = events_in_window > config.watch.burst_threshold;
                if is_burst {
                    stats.burst_detections += 1;
                    info!(
                        "burst detected ({} events), extending phase 2 debounce to {}ms",
                        events_in_window, config.watch.burst_debounce_ms
                    );
                    phase2_deadline = Some(tokio::time::Instant::now() + burst_duration);
                } else {
                    phase2_deadline = Some(tokio::time::Instant::now() + phase2_duration);
                }
                has_stale_sections = true;
                events_in_window = 0;
                phase1_armed = false;
            }

            _ = async {
                match phase2_deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending().await,
                }
            }, if has_stale_sections => {
                // Run phase 2
                info!("[phase2] starting embed + index");
                match phase2_ingest(&vault_root, &mut manifest, &engine, &embedder, &config).await {
                    Ok(p2) => {
                        info!(
                            "[phase2] complete: {} embedded, {} remembered, {} revised, {} forgotten",
                            p2.sections_embedded, p2.sections_remembered, p2.sections_revised, p2.sections_forgotten
                        );
                        stats.phase2_runs += 1;
                    }
                    Err(e) => warn!("phase 2 error: {}", e),
                }

                if let Err(e) = manifest.save(&hebbs_dir) {
                    warn!("failed to save manifest after phase 2: {}", e);
                }

                phase2_deadline = None;
                has_stale_sections = false;
            }
        }
    }

    // Graceful shutdown: save manifest
    info!("saving manifest on shutdown");
    manifest.save(&hebbs_dir)?;

    Ok(stats)
}

enum WatchEvent {
    FsEvent(Event),
}

/// Check if a path is a relevant .md file (not ignored).
pub fn is_relevant_md(path: &Path, vault_root: &Path, ignore_set: &GlobSet) -> bool {
    // Must be .md
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "md" {
        return false;
    }

    // Check against ignore patterns
    if let Ok(rel) = path.strip_prefix(vault_root) {
        let rel_str = rel.to_string_lossy();
        if ignore_set.is_match(rel_str.as_ref()) {
            return false;
        }
        // Also check each path component
        for component in rel.components() {
            let comp_str = component.as_os_str().to_string_lossy();
            if comp_str.starts_with('.') && comp_str != "." {
                // Skip hidden directories (except vault root)
                let with_slash = format!("{}/", comp_str);
                if ignore_set.is_match(&with_slash) || ignore_set.is_match(comp_str.as_ref()) {
                    return false;
                }
            }
        }
    }

    true
}

/// Build a GlobSet from ignore patterns.
pub fn build_ignore_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|e| VaultError::Config {
            reason: format!("invalid ignore pattern '{}': {}", pattern, e),
        })?;
        builder.add(glob);
        // Also add pattern matching anywhere in path
        if !pattern.starts_with("**/") {
            if let Ok(g) = Glob::new(&format!("**/{}", pattern)) {
                builder.add(g);
            }
        }
    }
    builder.build().map_err(|e| VaultError::Config {
        reason: format!("failed to build ignore set: {e}"),
    })
}

/// Walk the vault directory and find all .md files that have changed since last manifest update.
pub fn find_changed_files(
    vault_root: &Path,
    manifest: &Manifest,
    ignore_set: &GlobSet,
) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();

    walk_md_files(vault_root, vault_root, ignore_set, &mut |path| {
        let rel = path
            .strip_prefix(vault_root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");

        let file_bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => return,
        };
        let checksum = crate::manifest::sha256_checksum(&file_bytes);

        let needs_index = manifest
            .files
            .get(&rel)
            .map(|entry| entry.checksum != checksum)
            .unwrap_or(true); // not in manifest = needs indexing

        if needs_index {
            changed.push(path);
        }
    })?;

    Ok(changed)
}

/// Recursively walk a directory for .md files, respecting ignore patterns.
fn walk_md_files(
    root: &Path,
    dir: &Path,
    ignore_set: &GlobSet,
    callback: &mut dyn FnMut(PathBuf),
) -> Result<()> {
    let entries = std::fs::read_dir(dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Check if directory should be ignored
            if let Ok(rel) = path.strip_prefix(root) {
                let rel_str = format!("{}/", rel.to_string_lossy().replace('\\', "/"));
                if ignore_set.is_match(&rel_str) {
                    continue;
                }
            }
            walk_md_files(root, &path, ignore_set, callback)?;
        } else if is_relevant_md(&path, root, ignore_set) {
            callback(path);
        }
    }

    Ok(())
}

/// Walk all .md files in the vault. Public API for index command.
pub fn collect_md_files(vault_root: &Path, config: &VaultConfig) -> Result<Vec<PathBuf>> {
    let ignore_set = build_ignore_set(&config.effective_ignore_patterns())?;
    let mut files = Vec::new();
    walk_md_files(vault_root, vault_root, &ignore_set, &mut |path| {
        files.push(path);
    })?;
    files.sort();
    Ok(files)
}
