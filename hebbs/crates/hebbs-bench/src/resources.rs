use std::path::Path;
use std::sync::Arc;

use hebbs_core::engine::{Engine, RememberInput};
use hebbs_embed::MockEmbedder;
use hebbs_storage::RocksDbBackend;

use crate::dataset;
use crate::Tier;

#[derive(serde::Serialize)]
pub struct ResourceResults {
    pub measurements: Vec<ResourceMeasurement>,
}

#[derive(serde::Serialize)]
pub struct ResourceMeasurement {
    pub memory_count: usize,
    pub disk_bytes: u64,
    pub bytes_per_memory: u64,
    pub rss_bytes: u64,
}

#[allow(deprecated)]
fn get_rss_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        use std::mem;
        let mut info: libc::mach_task_basic_info = unsafe { mem::zeroed() };
        let mut count = (mem::size_of::<libc::mach_task_basic_info>()
            / mem::size_of::<libc::natural_t>()) as u32;
        let kr = unsafe {
            libc::task_info(
                libc::mach_task_self(),
                libc::MACH_TASK_BASIC_INFO,
                &mut info as *mut _ as *mut i32,
                &mut count,
            )
        };
        if kr == libc::KERN_SUCCESS {
            return info.resident_size;
        }
        0
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(pages) = content.split_whitespace().nth(1) {
                if let Ok(p) = pages.parse::<u64>() {
                    return p * 4096;
                }
            }
        }
        0
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            } else if p.is_dir() {
                total += dir_size(&p);
            }
        }
    }
    total
}

pub fn run(tier: &Tier, data_dir: Option<&Path>, seed: u64) -> ResourceResults {
    let scale_points: Vec<usize> = match tier {
        Tier::Quick => vec![1_000, 5_000, 10_000],
        Tier::Standard => vec![1_000, 10_000, 50_000, 100_000],
        Tier::Full => vec![1_000, 10_000, 100_000, 500_000, 1_000_000],
    };

    let max_scale = *scale_points.last().unwrap();
    let dims = 8;

    let dir = match data_dir {
        Some(p) => {
            std::fs::create_dir_all(p).ok();
            tempfile::tempdir_in(p).expect("failed to create temp dir")
        }
        None => tempfile::tempdir().expect("failed to create temp dir"),
    };

    let rss_before = get_rss_bytes();

    let storage: Arc<dyn hebbs_storage::StorageBackend> =
        Arc::new(RocksDbBackend::open(dir.path().to_str().unwrap()).unwrap());
    let embedder: Arc<dyn hebbs_embed::Embedder> = Arc::new(MockEmbedder::new(dims));
    let engine = Engine::new(storage, embedder).unwrap();

    let inputs = dataset::generate_memories(max_scale, seed);
    let mut inserted = 0;
    let mut results = Vec::new();

    for &target in &scale_points {
        while inserted < target && inserted < inputs.len() {
            engine
                .remember(RememberInput {
                    content: inputs[inserted].content.clone(),
                    importance: inputs[inserted].importance,
                    context: inputs[inserted].context.clone(),
                    entity_id: inputs[inserted].entity_id.clone(),
                    edges: Vec::new(),
            kind: None,
        })
                .unwrap();
            inserted += 1;
        }

        let disk = dir_size(dir.path());
        let rss = get_rss_bytes();
        let bytes_per = if inserted > 0 {
            disk / inserted as u64
        } else {
            0
        };

        println!(
            "  {} memories: disk={}MB, RSS={}MB, bytes/mem={}",
            inserted,
            disk / 1_048_576,
            rss / 1_048_576,
            bytes_per,
        );

        results.push(ResourceMeasurement {
            memory_count: inserted,
            disk_bytes: disk,
            bytes_per_memory: bytes_per,
            rss_bytes: rss.saturating_sub(rss_before),
        });
    }

    ResourceResults {
        measurements: results,
    }
}
