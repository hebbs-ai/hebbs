use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::config::EmbedderConfig;
use crate::error::{EmbedError, Result};

/// Resolved paths for a model's files on disk.
#[derive(Debug)]
pub struct ModelPaths {
    pub model_onnx: PathBuf,
    /// External data file for large ONNX models (e.g. EmbeddingGemma-300M).
    /// Must be in the same directory as `model.onnx` for ONNX Runtime to find it.
    pub model_onnx_data: PathBuf,
    pub tokenizer_json: PathBuf,
    pub config_json: PathBuf,
}

impl ModelPaths {
    pub fn from_dir(dir: &Path) -> Self {
        Self {
            model_onnx: dir.join("model.onnx"),
            model_onnx_data: dir.join("model.onnx_data"),
            tokenizer_json: dir.join("tokenizer.json"),
            config_json: dir.join("config.json"),
        }
    }

    /// Check whether all required model files are present on disk.
    ///
    /// Note: `model.onnx_data` is only required for models with external data.
    /// Use `all_exist_with_external_data` for those models.
    pub fn all_exist(&self) -> bool {
        self.model_onnx.exists() && self.tokenizer_json.exists()
    }

    /// Check whether all files including external data are present.
    pub fn all_exist_with_external_data(&self) -> bool {
        self.all_exist() && self.model_onnx_data.exists()
    }
}

/// Ensure model files are present, downloading if necessary and permitted.
///
/// Returns the resolved paths to all model files.
pub fn ensure_model_files(config: &EmbedderConfig) -> Result<ModelPaths> {
    let paths = ModelPaths::from_dir(&config.model_dir);
    let has_external = config.model_config.has_external_data;

    let all_present = if has_external {
        paths.all_exist_with_external_data()
    } else {
        paths.all_exist()
    };

    if all_present {
        return Ok(paths);
    }

    if !config.auto_download {
        return Err(EmbedError::ModelLoad {
            message: format!(
                "model files not found at {} and auto_download is disabled — \
                 pre-place model.onnx and tokenizer.json in the model directory",
                config.model_dir.display()
            ),
        });
    }

    fs::create_dir_all(&config.model_dir).map_err(|e| EmbedError::ModelLoad {
        message: format!(
            "failed to create model directory {}: {}",
            config.model_dir.display(),
            e
        ),
    })?;

    if !paths.model_onnx.exists() {
        let url = format!("{}/onnx/model.onnx", config.download_base_url);
        download_file(&url, &paths.model_onnx)?;
    }

    // Download external data file if model requires it
    if has_external && !paths.model_onnx_data.exists() {
        let url = format!("{}/onnx/model.onnx_data", config.download_base_url);
        download_file(&url, &paths.model_onnx_data)?;
    }

    if !paths.tokenizer_json.exists() {
        let url = format!("{}/tokenizer.json", config.download_base_url);
        download_file(&url, &paths.tokenizer_json)?;
    }

    // Write config.json alongside model files
    if !paths.config_json.exists() {
        let config_json =
            serde_json::to_string_pretty(&config.model_config).map_err(|e| EmbedError::Config {
                message: format!("failed to serialize model config: {}", e),
            })?;
        fs::write(&paths.config_json, config_json).map_err(|e| EmbedError::ModelLoad {
            message: format!("failed to write config.json: {}", e),
        })?;
    }

    Ok(paths)
}

/// Download a file from a URL to a local path with atomic rename.
///
/// Resumes interrupted downloads using HTTP Range headers if a `.download.tmp`
/// file already exists. Shows progress on stderr for files > 1 MB.
fn download_file(url: &str, dest: &Path) -> Result<()> {
    let tmp_path = dest.with_extension("download.tmp");

    let file_name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("file");

    // Check for a partial download to resume
    let already_downloaded: u64 = tmp_path.metadata().map(|m| m.len()).unwrap_or(0);

    let (response, resume_offset) = if already_downloaded > 0 {
        // Try a Range request to resume
        let range = format!("bytes={}-", already_downloaded);
        match ureq::get(url).header("Range", &range).call() {
            Ok(resp) if resp.status() == 206 => {
                eprintln!(
                    "  Resuming {} from {:.1} MB...",
                    file_name,
                    already_downloaded as f64 / 1_048_576.0
                );
                (resp, already_downloaded)
            }
            _ => {
                // Server doesn't support range requests — start over
                (
                    ureq::get(url).call().map_err(|e| EmbedError::Download {
                        message: format!("HTTP request to {} failed: {}", url, e),
                    })?,
                    0,
                )
            }
        }
    } else {
        (
            ureq::get(url).call().map_err(|e| EmbedError::Download {
                message: format!("HTTP request to {} failed: {}", url, e),
            })?,
            0,
        )
    };

    // Content-Length from this response (bytes remaining, not total)
    let remaining: Option<u64> = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let total_length: Option<u64> = remaining.map(|r| r + resume_offset);

    // Open file for append (resume) or create (fresh start)
    let mut file = if resume_offset > 0 {
        fs::OpenOptions::new()
            .append(true)
            .open(&tmp_path)
            .map_err(|e| EmbedError::Download {
                message: format!(
                    "failed to open tmp file for append {}: {}",
                    tmp_path.display(),
                    e
                ),
            })?
    } else {
        fs::File::create(&tmp_path).map_err(|e| EmbedError::Download {
            message: format!("failed to create temp file {}: {}", tmp_path.display(), e),
        })?
    };

    // Stream to disk in 256 KB chunks
    let mut reader = response.into_body().into_reader();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut session_bytes: u64 = 0;
    let mut last_report = std::time::Instant::now();
    let start = std::time::Instant::now();
    let show_progress = total_length.is_some_and(|l| l > 1_000_000);

    loop {
        let n = reader.read(&mut buffer).map_err(|e| EmbedError::Download {
            message: format!("failed reading response body for {}: {}", url, e),
        })?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])
            .map_err(|e| EmbedError::Download {
                message: format!("failed writing to {}: {}", tmp_path.display(), e),
            })?;
        session_bytes += n as u64;

        // Progress report every 500ms
        if show_progress && last_report.elapsed() >= std::time::Duration::from_millis(500) {
            let downloaded = resume_offset + session_bytes;
            let elapsed = start.elapsed().as_secs_f64();
            let speed_mb = if elapsed > 0.0 {
                (session_bytes as f64 / 1_048_576.0) / elapsed
            } else {
                0.0
            };
            if let Some(total) = total_length {
                let pct = (downloaded as f64 / total as f64 * 100.0).min(100.0);
                let eta = if speed_mb > 0.01 {
                    let remaining_mb = (total - downloaded) as f64 / 1_048_576.0;
                    let secs = (remaining_mb / speed_mb) as u64;
                    if secs >= 60 {
                        format!("~{}m{}s left", secs / 60, secs % 60)
                    } else {
                        format!("~{}s left", secs)
                    }
                } else {
                    String::new()
                };
                // Visual progress bar (20 chars wide)
                let filled = (pct / 5.0) as usize;
                let bar: String = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(20 - filled);
                eprint!(
                    "\r  Downloading {} {} {:.0}% ({:.1}/{:.1} MB, {:.1} MB/s) {}  ",
                    file_name,
                    bar,
                    pct,
                    downloaded as f64 / 1_048_576.0,
                    total as f64 / 1_048_576.0,
                    speed_mb,
                    eta,
                );
            } else {
                eprint!(
                    "\r  Downloading {} ... {:.1} MB ({:.1} MB/s)  ",
                    file_name,
                    (resume_offset + session_bytes) as f64 / 1_048_576.0,
                    speed_mb,
                );
            }
            last_report = std::time::Instant::now();
        }
    }

    // Final progress line
    if show_progress {
        let total_downloaded = resume_offset + session_bytes;
        let elapsed = start.elapsed().as_secs_f64();
        eprintln!(
            "\r  Downloaded {} ({:.1} MB total, {:.1}s this session)                    ",
            file_name,
            total_downloaded as f64 / 1_048_576.0,
            elapsed,
        );
    }

    file.flush().map_err(|e| EmbedError::Download {
        message: format!("failed to flush {}: {}", tmp_path.display(), e),
    })?;
    drop(file);

    fs::rename(&tmp_path, dest).map_err(|e| EmbedError::Download {
        message: format!(
            "failed to rename {} → {}: {}",
            tmp_path.display(),
            dest.display(),
            e
        ),
    })?;

    Ok(())
}

/// Compute the SHA-256 hash of a file and return hex-encoded digest.
pub fn sha256_file(path: &Path) -> Result<String> {
    let data = fs::read(path).map_err(|e| EmbedError::ModelLoad {
        message: format!("failed to read file for checksum {}: {}", path.display(), e),
    })?;
    let hash = Sha256::digest(&data);
    Ok(hex::encode(hash))
}

/// Verify that a file's SHA-256 checksum matches the expected value.
pub fn verify_checksum(path: &Path, expected: &str) -> Result<()> {
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(EmbedError::ChecksumMismatch {
            file: path.display().to_string(),
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_paths_from_dir() {
        let paths = ModelPaths::from_dir(Path::new("/data/models/bge"));
        assert_eq!(
            paths.model_onnx,
            PathBuf::from("/data/models/bge/model.onnx")
        );
        assert_eq!(
            paths.tokenizer_json,
            PathBuf::from("/data/models/bge/tokenizer.json")
        );
        assert_eq!(
            paths.config_json,
            PathBuf::from("/data/models/bge/config.json")
        );
    }

    #[test]
    fn all_exist_false_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ModelPaths::from_dir(dir.path());
        assert!(!paths.all_exist());
    }

    #[test]
    fn sha256_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, b"hello world").unwrap();
        let hash = sha256_file(&file).unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn verify_checksum_correct() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, b"hello world").unwrap();
        verify_checksum(
            &file,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        )
        .unwrap();
    }

    #[test]
    fn verify_checksum_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, b"hello world").unwrap();
        let err = verify_checksum(&file, "0000000000000000").unwrap_err();
        assert!(matches!(err, EmbedError::ChecksumMismatch { .. }));
    }

    #[test]
    fn ensure_model_files_no_download() {
        let dir = tempfile::tempdir().unwrap();
        let config = EmbedderConfig {
            model_dir: dir.path().to_path_buf(),
            model_config: crate::config::ModelConfig::bge_small_en_v1_5(),
            download_base_url: "http://localhost:0".to_string(),
            auto_download: false,
        };
        let err = ensure_model_files(&config).unwrap_err();
        assert!(matches!(err, EmbedError::ModelLoad { .. }));
    }
}
