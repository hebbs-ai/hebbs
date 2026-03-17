use std::path::Path;

use ndarray::{Array2, ArrayViewD, Axis, Ix3};
use ort::session::Session;
use ort::value::TensorRef;
use parking_lot::Mutex;
use tokenizers::Tokenizer;

use crate::config::{EmbedderConfig, ModelConfig, PoolingStrategy};
use crate::error::{EmbedError, Result};
use crate::model::{ensure_model_files, ModelPaths};
use crate::normalize::l2_normalize;
use crate::traits::Embedder;

/// Maximum batch size for a single ONNX inference call.
/// Larger batches are chunked internally. (Principle 4: bounded resources)
///
/// 256 texts × 512 tokens × 4 bytes × 2 (IDs + mask) ≈ 1 MB of input tensors.
const MAX_BATCH_SIZE: usize = 256;

/// ONNX Runtime-backed embedder.
///
/// Loads a transformer model and tokenizer from disk, runs inference
/// locally with hardware acceleration auto-detection.
///
/// Thread-safe: `Session` is wrapped in `Mutex` because `Session::run`
/// requires `&mut self`. The `Tokenizer` is internally thread-safe.
/// Multiple threads can call `embed()` concurrently; inference is
/// serialized through the Mutex (ONNX Runtime's internal parallelism
/// still applies within a single inference call).
pub struct OnnxEmbedder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    config: ModelConfig,
}

impl OnnxEmbedder {
    /// Create a new ONNX embedder from the given configuration.
    ///
    /// This is an expensive operation (~100–500 ms): model parsing,
    /// graph optimization, memory allocation. Call once at startup.
    pub fn new(config: EmbedderConfig) -> Result<Self> {
        let paths = ensure_model_files(&config)?;
        Self::from_paths(&paths, config.model_config)
    }

    /// Create an embedder from pre-existing model files.
    ///
    /// Use when model files are already present (offline / air-gapped
    /// deployments, CI caches).
    pub fn from_paths(paths: &ModelPaths, config: ModelConfig) -> Result<Self> {
        let session = Self::build_session(&paths.model_onnx)?;
        let tokenizer = Self::load_tokenizer(&paths.tokenizer_json, &config)?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            config,
        })
    }

    /// Build an ONNX Runtime session with hardware acceleration auto-detection.
    ///
    /// Execution provider priority:
    /// 1. CoreML   (macOS Apple Silicon) — ~1 ms
    /// 2. CUDA     (NVIDIA GPU)         — ~2 ms
    /// 3. DirectML (Windows GPU)        — ~3 ms
    /// 4. CPU      (all platforms)      — ~3–5 ms
    fn build_session(model_path: &Path) -> Result<Session> {
        let mut builder = Session::builder().map_err(|e| EmbedError::ModelLoad {
            message: format!("failed to create session builder: {}", e),
        })?;

        // Register hardware accelerators when their compile-time feature is enabled.
        // Each EP silently falls back to CPU if the hardware is unavailable at runtime.
        {
            #[allow(unused_mut)]
            let mut eps: Vec<ort::execution_providers::ExecutionProviderDispatch> = Vec::new();

            #[cfg(feature = "coreml")]
            eps.push(ort::execution_providers::CoreMLExecutionProvider::default().build());

            #[cfg(feature = "cuda")]
            eps.push(ort::execution_providers::CUDAExecutionProvider::default().build());

            #[cfg(feature = "directml")]
            eps.push(ort::execution_providers::DirectMLExecutionProvider::default().build());

            if !eps.is_empty() {
                builder =
                    builder
                        .with_execution_providers(eps)
                        .map_err(|e| EmbedError::ModelLoad {
                            message: format!("failed to register execution providers: {}", e),
                        })?;
            }
        }

        builder
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
            .map_err(|e| EmbedError::ModelLoad {
                message: format!("failed to set optimization level: {}", e),
            })?
            .with_intra_threads(
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4),
            )
            .map_err(|e| EmbedError::ModelLoad {
                message: format!("failed to set intra-op threads: {}", e),
            })?
            .commit_from_file(model_path)
            .map_err(|e| EmbedError::ModelLoad {
                message: format!(
                    "failed to load ONNX model from {}: {}",
                    model_path.display(),
                    e
                ),
            })
    }

    /// Load and configure the tokenizer.
    fn load_tokenizer(tokenizer_path: &Path, config: &ModelConfig) -> Result<Tokenizer> {
        let mut tokenizer =
            Tokenizer::from_file(tokenizer_path).map_err(|e| EmbedError::Tokenization {
                message: format!(
                    "failed to load tokenizer from {}: {}",
                    tokenizer_path.display(),
                    e
                ),
            })?;

        // Truncate inputs to model's max sequence length (silent truncation,
        // not an error — the full text is preserved in the Memory record).
        let truncation = tokenizers::TruncationParams {
            max_length: config.max_seq_length,
            ..Default::default()
        };
        tokenizer
            .with_truncation(Some(truncation))
            .map_err(|e| EmbedError::Tokenization {
                message: format!("failed to configure truncation: {}", e),
            })?;

        // Disable built-in padding — we pad per-batch to the longest
        // sequence in that batch, not to max_seq_length (saves compute).
        tokenizer.with_padding(None);

        Ok(tokenizer)
    }

    /// Tokenize, pad, and build input tensors for a batch of texts.
    ///
    /// Returns `(input_ids, attention_mask, token_type_ids)` as 2D arrays
    /// of shape `[batch_size, max_seq_len_in_batch]`.
    /// `token_type_ids` is all zeros when the model doesn't use them (but
    /// is still returned for uniform handling; callers check `uses_token_type_ids`).
    fn prepare_inputs(&self, texts: &[&str]) -> Result<(Array2<i64>, Array2<i64>, Array2<i64>)> {
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| EmbedError::Tokenization {
                message: format!("batch tokenization failed: {}", e),
            })?;

        let batch_size = encodings.len();
        let max_len = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(0);

        if max_len == 0 {
            return Err(EmbedError::Tokenization {
                message: "all inputs produced empty tokenizations".to_string(),
            });
        }

        // Pre-allocate padded tensors — O(batch_size × max_len)
        let mut input_ids = Array2::<i64>::zeros((batch_size, max_len));
        let mut attention_mask = Array2::<i64>::zeros((batch_size, max_len));
        let mut token_type_ids = Array2::<i64>::zeros((batch_size, max_len));

        for (i, encoding) in encodings.iter().enumerate() {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();
            let type_ids = encoding.get_type_ids();
            let seq_len = ids.len();

            for j in 0..seq_len {
                input_ids[[i, j]] = ids[j] as i64;
                attention_mask[[i, j]] = mask[j] as i64;
                token_type_ids[[i, j]] = type_ids[j] as i64;
            }
        }

        Ok((input_ids, attention_mask, token_type_ids))
    }

    /// Run ONNX inference and apply pooling + normalization.
    ///
    /// Returns one L2-normalized embedding vector per input text.
    fn embed_batch_inner(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let (input_ids, attention_mask, token_type_ids) = self.prepare_inputs(texts)?;

        // Build TensorRef inputs from ndarray arrays
        let input_ids_ref =
            TensorRef::from_array_view(&input_ids).map_err(|e| EmbedError::Inference {
                message: format!("failed to create input_ids tensor: {}", e),
            })?;
        let attention_mask_ref =
            TensorRef::from_array_view(&attention_mask).map_err(|e| EmbedError::Inference {
                message: format!("failed to create attention_mask tensor: {}", e),
            })?;

        // Run inference and extract output inside the Mutex scope.
        // We copy the result to an owned ndarray so the session lock
        // is released before the CPU-bound pooling step.
        let output_owned = {
            let mut session = self.session.lock();

            let outputs = if self.config.uses_token_type_ids {
                let token_type_ids_ref = TensorRef::from_array_view(&token_type_ids)
                    .map_err(|e| EmbedError::Inference {
                        message: format!("failed to create token_type_ids tensor: {}", e),
                    })?;
                session
                    .run(ort::inputs![
                        "input_ids" => input_ids_ref,
                        "attention_mask" => attention_mask_ref,
                        "token_type_ids" => token_type_ids_ref,
                    ])
                    .map_err(|e| EmbedError::Inference {
                        message: format!("ONNX inference failed: {}", e),
                    })?
            } else {
                session
                    .run(ort::inputs![
                        "input_ids" => input_ids_ref,
                        "attention_mask" => attention_mask_ref,
                    ])
                    .map_err(|e| EmbedError::Inference {
                        message: format!("ONNX inference failed: {}", e),
                    })?
            };

            // Extract the right output tensor.
            // Pre-pooled models (EmbeddingGemma) have a "sentence_embedding" output
            // at index 1. BERT-family models have "last_hidden_state" at index 0.
            let output_idx = if self.config.pooling_strategy == PoolingStrategy::None
                && outputs.len() > 1
            {
                1 // sentence_embedding (2D)
            } else {
                0 // last_hidden_state (3D) or only output
            };

            let output_dyn: ArrayViewD<f32> =
                outputs[output_idx]
                    .try_extract_array::<f32>()
                    .map_err(|e| EmbedError::Inference {
                        message: format!("failed to extract output array: {}", e),
                    })?;

            output_dyn.to_owned()
        };

        let batch_size = texts.len();
        let ndim = output_owned.ndim();

        // Models like EmbeddingGemma output pre-pooled 2D [batch, hidden].
        // BERT-family models output 3D [batch, seq_len, hidden] needing pooling.
        let pooled = if ndim == 2 || self.config.pooling_strategy == PoolingStrategy::None {
            output_owned
                .into_dimensionality::<ndarray::Ix2>()
                .map_err(|e| EmbedError::Inference {
                    message: format!("expected 2D output from pre-pooled model: {}", e),
                })?
        } else {
            let output_3d =
                output_owned
                    .into_dimensionality::<Ix3>()
                    .map_err(|e| EmbedError::Inference {
                        message: format!("output tensor is not 3-dimensional: {}", e),
                    })?;

            match self.config.pooling_strategy {
                PoolingStrategy::Mean => mean_pool(&output_3d.view(), &attention_mask),
                PoolingStrategy::Cls => cls_pool(&output_3d.view()),
                PoolingStrategy::None => unreachable!(),
            }
        };

        // L2-normalize each embedding
        let mut results = Vec::with_capacity(batch_size);
        for i in 0..batch_size {
            let mut vec = pooled.row(i).to_vec();
            l2_normalize(&mut vec);
            results.push(vec);
        }

        Ok(results)
    }
}

impl Embedder for OnnxEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed_batch_inner(&[text])?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| EmbedError::Inference {
                message: "inference produced no output for single input".to_string(),
            })
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Chunk into MAX_BATCH_SIZE groups (Principle 4: bounded resources)
        if texts.len() <= MAX_BATCH_SIZE {
            return self.embed_batch_inner(texts);
        }

        let mut all_results = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(MAX_BATCH_SIZE) {
            let chunk_results = self.embed_batch_inner(chunk)?;
            all_results.extend(chunk_results);
        }
        Ok(all_results)
    }

    fn dimensions(&self) -> usize {
        self.config.dimensions
    }
}

/// Mean pooling: average token embeddings, excluding padding tokens.
///
/// output shape:         [batch_size, seq_len, hidden_size]
/// attention_mask shape: [batch_size, seq_len]
/// result shape:         [batch_size, hidden_size]
///
/// Complexity: O(batch_size × seq_len × hidden_size)
fn mean_pool(output: &ndarray::ArrayView3<f32>, attention_mask: &Array2<i64>) -> Array2<f32> {
    let batch_size = output.len_of(Axis(0));
    let seq_len = output.len_of(Axis(1));
    let hidden_size = output.len_of(Axis(2));

    let mut result = Array2::<f32>::zeros((batch_size, hidden_size));

    for b in 0..batch_size {
        let mut token_count: f32 = 0.0;
        for s in 0..seq_len {
            let mask_val = attention_mask[[b, s]] as f32;
            if mask_val > 0.0 {
                token_count += 1.0;
                for h in 0..hidden_size {
                    result[[b, h]] += output[[b, s, h]] * mask_val;
                }
            }
        }
        if token_count > 0.0 {
            let inv = 1.0 / token_count;
            for h in 0..hidden_size {
                result[[b, h]] *= inv;
            }
        }
    }

    result
}

/// CLS pooling: use the \[CLS\] token embedding (first token, index 0).
///
/// Complexity: O(batch_size × hidden_size)
fn cls_pool(output: &ndarray::ArrayView3<f32>) -> Array2<f32> {
    let batch_size = output.len_of(Axis(0));
    let hidden_size = output.len_of(Axis(2));

    let mut result = Array2::<f32>::zeros((batch_size, hidden_size));

    for b in 0..batch_size {
        for h in 0..hidden_size {
            result[[b, h]] = output[[b, 0, h]];
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array3;

    #[test]
    fn mean_pool_excludes_padding() {
        // 1 sample, 4 tokens (2 real + 2 padding), hidden_size=3
        let output = Array3::<f32>::from_shape_vec(
            (1, 4, 3),
            vec![
                1.0, 2.0, 3.0, // token 0 (real)
                4.0, 5.0, 6.0, // token 1 (real)
                9.0, 9.0, 9.0, // token 2 (padding — excluded by mask)
                9.0, 9.0, 9.0, // token 3 (padding — excluded by mask)
            ],
        )
        .unwrap();
        let mask = Array2::from_shape_vec((1, 4), vec![1i64, 1, 0, 0]).unwrap();

        let pooled = mean_pool(&output.view(), &mask);
        assert_eq!(pooled.shape(), &[1, 3]);
        assert!((pooled[[0, 0]] - 2.5).abs() < 1e-6);
        assert!((pooled[[0, 1]] - 3.5).abs() < 1e-6);
        assert!((pooled[[0, 2]] - 4.5).abs() < 1e-6);
    }

    #[test]
    fn cls_pool_takes_first_token() {
        let output = Array3::<f32>::from_shape_vec(
            (2, 3, 2),
            vec![
                1.0, 2.0, 3.0, 4.0, 5.0, 6.0, // batch 0
                7.0, 8.0, 9.0, 10.0, 11.0, 12.0, // batch 1
            ],
        )
        .unwrap();

        let pooled = cls_pool(&output.view());
        assert_eq!(pooled.shape(), &[2, 2]);
        assert!((pooled[[0, 0]] - 1.0).abs() < 1e-6);
        assert!((pooled[[0, 1]] - 2.0).abs() < 1e-6);
        assert!((pooled[[1, 0]] - 7.0).abs() < 1e-6);
        assert!((pooled[[1, 1]] - 8.0).abs() < 1e-6);
    }

    #[test]
    fn mean_pool_batch_of_two() {
        let output = Array3::<f32>::from_shape_vec(
            (2, 2, 2),
            vec![
                1.0, 2.0, 3.0, 4.0, // batch 0
                5.0, 6.0, 7.0, 8.0, // batch 1
            ],
        )
        .unwrap();
        let mask = Array2::from_shape_vec((2, 2), vec![1i64, 1, 1, 1]).unwrap();

        let pooled = mean_pool(&output.view(), &mask);
        assert!((pooled[[0, 0]] - 2.0).abs() < 1e-6);
        assert!((pooled[[0, 1]] - 3.0).abs() < 1e-6);
        assert!((pooled[[1, 0]] - 6.0).abs() < 1e-6);
        assert!((pooled[[1, 1]] - 7.0).abs() < 1e-6);
    }
}
