use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use ulid::Ulid;

use hebbs_core::engine::{Engine, RememberInput};
use hebbs_core::forget::ForgetCriteria;
use hebbs_core::memory::MemoryKind;
use hebbs_core::recall::{PrimeInput, RecallInput, RecallStrategy};
use hebbs_core::reflect::{InsightsFilter, ReflectConfig, ReflectScope};
use hebbs_core::revise::{ContextMode, ReviseInput};
use hebbs_core::subscribe::SubscribeConfig;
use hebbs_embed::{EmbedderConfig, MockEmbedder, OnnxEmbedder};
use hebbs_storage::RocksDbBackend;

use crate::convert::{forget_output_to_py, memory_to_py, py_dict_to_hashmap, recall_output_to_py};
use crate::error::to_py_err;
use crate::subscribe::NativeSubscription;

/// Native HEBBS engine for embedded (no-server) mode.
///
/// All engine operations release the GIL during Rust computation,
/// allowing other Python threads to proceed concurrently.
#[pyclass]
pub struct NativeEngine {
    engine: Arc<Engine>,
}

#[pymethods]
impl NativeEngine {
    /// Open a HEBBS engine at the given path.
    ///
    /// Args:
    ///     data_dir: Path to the database directory.
    ///     use_mock_embedder: Use deterministic mock embedder (for testing).
    ///     embedding_dimensions: Embedding vector dimensions (default 768).
    ///     embedding_model: Embedding model name (default "embeddinggemma-300m").
    ///
    /// Returns:
    ///     NativeEngine instance.
    #[new]
    #[pyo3(signature = (data_dir, use_mock_embedder=true, embedding_dimensions=768, embedding_model="embeddinggemma-300m"))]
    fn new(data_dir: &str, use_mock_embedder: bool, embedding_dimensions: usize, embedding_model: &str) -> PyResult<Self> {
        let storage: Arc<dyn hebbs_storage::StorageBackend> =
            Arc::new(RocksDbBackend::open(data_dir).map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!(
                    "failed to open storage at '{}': {}",
                    data_dir, e
                ))
            })?);

        let embedder: Arc<dyn hebbs_embed::Embedder> = if use_mock_embedder {
            Arc::new(MockEmbedder::new(embedding_dimensions))
        } else {
            let config = EmbedderConfig::from_model_name(embedding_model, data_dir);
            let onnx = OnnxEmbedder::new(config).map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!(
                    "failed to initialize ONNX embedder: {}. \
                     Model will be downloaded (~33MB) on first use.",
                    e
                ))
            })?;
            Arc::new(onnx)
        };

        let engine = Engine::new(storage, embedder).map_err(to_py_err)?;

        Ok(Self {
            engine: Arc::new(engine),
        })
    }

    /// Store a new memory. GIL released during engine operation.
    ///
    /// Returns a dict with all memory fields plus `embed_ms` (float)
    /// indicating time spent in the embedder.
    #[pyo3(signature = (content, importance=0.5, context=None, entity_id=None))]
    fn remember(
        &self,
        py: Python<'_>,
        content: &str,
        importance: f32,
        context: Option<&Bound<'_, PyDict>>,
        entity_id: Option<&str>,
    ) -> PyResult<PyObject> {
        let ctx = match context {
            Some(d) => Some(py_dict_to_hashmap(d)?),
            None => None,
        };
        let eid = entity_id.map(|s| s.to_string());
        let content_owned = content.to_string();

        let input = RememberInput {
            content: content_owned,
            importance: Some(importance),
            context: ctx,
            entity_id: eid,
            edges: Vec::new(),
            kind: None,
        };

        let engine = self.engine.clone();
        let output = py.allow_threads(move || engine.remember_timed(input).map_err(to_py_err))?;
        let dict_obj = memory_to_py(py, &output.memory)?;
        let dict = dict_obj.downcast_bound::<PyDict>(py)?;
        dict.set_item("embed_ms", output.embed_duration_us as f64 / 1000.0)?;
        Ok(dict_obj)
    }

    /// Retrieve a memory by its 26-character ULID string.
    fn get(&self, py: Python<'_>, memory_id: &str) -> PyResult<PyObject> {
        let ulid = Ulid::from_string(memory_id).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("invalid ULID '{}': {}", memory_id, e))
        })?;
        let id_bytes = ulid.to_bytes();
        let engine = self.engine.clone();
        let memory = py.allow_threads(move || engine.get(&id_bytes).map_err(to_py_err))?;
        memory_to_py(py, &memory)
    }

    /// Recall memories matching a cue.
    ///
    /// When `strategies` is provided, runs all specified strategies in parallel
    /// with a single embedding pass and engine-side merge. Otherwise falls back
    /// to the single `strategy` parameter for backward compatibility.
    #[pyo3(signature = (cue, strategy="similarity", top_k=10, entity_id=None, max_depth=None, time_range=None, strategies=None))]
    fn recall(
        &self,
        py: Python<'_>,
        cue: &str,
        strategy: &str,
        top_k: usize,
        entity_id: Option<&str>,
        max_depth: Option<usize>,
        time_range: Option<(u64, u64)>,
        strategies: Option<Vec<String>>,
    ) -> PyResult<PyObject> {
        let strats = match strategies {
            Some(names) => {
                let mut parsed = Vec::with_capacity(names.len());
                for name in &names {
                    parsed.push(parse_strategy(name)?);
                }
                parsed
            }
            None => vec![parse_strategy(strategy)?],
        };

        let input = RecallInput {
            cue: cue.to_string(),
            strategies: strats,
            top_k: Some(top_k),
            entity_id: entity_id.map(|s| s.to_string()),
            time_range,
            edge_types: None,
            max_depth,
            ef_search: None,
            scoring_weights: None,
            cue_context: None,
            causal_direction: None,
            analogy_a_id: None,
            analogy_b_id: None,
            seed_memory_id: None,
            analogical_alpha: None,
        };

        let engine = self.engine.clone();
        let output = py.allow_threads(move || engine.recall(input).map_err(to_py_err))?;
        recall_output_to_py(py, &output)
    }

    /// Revise an existing memory.
    #[pyo3(signature = (memory_id, content=None, importance=None, context=None, context_mode="merge", entity_id=None))]
    fn revise(
        &self,
        py: Python<'_>,
        memory_id: &str,
        content: Option<&str>,
        importance: Option<f32>,
        context: Option<&Bound<'_, PyDict>>,
        context_mode: &str,
        entity_id: Option<&str>,
    ) -> PyResult<PyObject> {
        let ulid = Ulid::from_string(memory_id).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("invalid ULID '{}': {}", memory_id, e))
        })?;

        let ctx = match context {
            Some(d) => Some(py_dict_to_hashmap(d)?),
            None => None,
        };

        let mode = match context_mode {
            "replace" => ContextMode::Replace,
            _ => ContextMode::Merge,
        };

        let eid = entity_id.map(|s| Some(s.to_string()));

        let input = ReviseInput {
            memory_id: ulid.to_bytes().to_vec(),
            content: content.map(|s| s.to_string()),
            importance,
            context: ctx,
            context_mode: mode,
            entity_id: eid,
            edges: Vec::new(),
        };

        let engine = self.engine.clone();
        let memory = py.allow_threads(move || engine.revise(input).map_err(to_py_err))?;
        memory_to_py(py, &memory)
    }

    /// Forget memories by ID or criteria.
    #[pyo3(signature = (memory_ids=None, entity_id=None, staleness_threshold_us=None, access_count_floor=None, memory_kind=None, decay_score_floor=None))]
    fn forget(
        &self,
        py: Python<'_>,
        memory_ids: Option<Vec<String>>,
        entity_id: Option<&str>,
        staleness_threshold_us: Option<u64>,
        access_count_floor: Option<u64>,
        memory_kind: Option<&str>,
        decay_score_floor: Option<f32>,
    ) -> PyResult<PyObject> {
        let ids = match memory_ids {
            Some(id_strs) => {
                let mut ids = Vec::with_capacity(id_strs.len());
                for s in &id_strs {
                    let ulid = Ulid::from_string(s).map_err(|e| {
                        pyo3::exceptions::PyValueError::new_err(format!(
                            "invalid ULID '{}': {}",
                            s, e
                        ))
                    })?;
                    ids.push(ulid.to_bytes().to_vec());
                }
                ids
            }
            None => Vec::new(),
        };

        let kind = memory_kind.and_then(|k| match k.to_lowercase().as_str() {
            "episode" => Some(MemoryKind::Episode),
            "insight" => Some(MemoryKind::Insight),
            "revision" => Some(MemoryKind::Revision),
            _ => None,
        });

        let criteria = ForgetCriteria {
            memory_ids: ids,
            entity_id: entity_id.map(|s| s.to_string()),
            staleness_threshold_us,
            access_count_floor,
            memory_kind: kind,
            decay_score_floor,
        };

        let engine = self.engine.clone();
        let output = py.allow_threads(move || engine.forget(criteria).map_err(to_py_err))?;
        forget_output_to_py(py, &output)
    }

    /// Pre-load memories for an entity.
    #[pyo3(signature = (entity_id, max_memories=50, similarity_cue=None))]
    fn prime(
        &self,
        py: Python<'_>,
        entity_id: &str,
        max_memories: usize,
        similarity_cue: Option<&str>,
    ) -> PyResult<PyObject> {
        let input = PrimeInput {
            entity_id: entity_id.to_string(),
            context: None,
            max_memories: Some(max_memories),
            recency_window_us: None,
            similarity_cue: similarity_cue.map(|s| s.to_string()),
            scoring_weights: None,
        };

        let engine = self.engine.clone();
        let output = py.allow_threads(move || engine.prime(input).map_err(to_py_err))?;

        let dict = PyDict::new(py);
        let results = PyList::empty(py);
        for r in &output.results {
            let rdict = PyDict::new(py);
            rdict.set_item("memory", memory_to_py(py, &r.memory)?)?;
            rdict.set_item("score", r.score)?;
            results.append(rdict)?;
        }
        dict.set_item("results", results)?;
        dict.set_item("temporal_count", output.temporal_count)?;
        dict.set_item("similarity_count", output.similarity_count)?;

        Ok(dict.into())
    }

    /// Start a subscription for real-time memory matching.
    #[pyo3(signature = (entity_id=None, confidence_threshold=0.6))]
    fn subscribe(
        &self,
        py: Python<'_>,
        entity_id: Option<&str>,
        confidence_threshold: f32,
    ) -> PyResult<NativeSubscription> {
        let config = SubscribeConfig {
            entity_id: entity_id.map(|s| s.to_string()),
            memory_kinds: Vec::new(),
            confidence_threshold,
            time_scope_us: None,
            chunk_min_tokens: 15,
            chunk_max_wait_us: 500_000,
            hnsw_ef_search: 50,
            hnsw_top_k: 5,
            bloom_fp_rate: 0.01,
            coarse_threshold: 0.15,
            output_queue_depth: 100,
            input_queue_depth: 1_000,
            bloom_refresh_interval_us: 60_000_000,
            bloom_refresh_write_count: 100,
        };

        let engine = self.engine.clone();
        let handle = py.allow_threads(move || engine.subscribe(config).map_err(to_py_err))?;
        Ok(NativeSubscription::new(handle))
    }

    /// Trigger reflection over a scope.
    #[pyo3(signature = (entity_id=None, since_us=None))]
    fn reflect(
        &self,
        py: Python<'_>,
        entity_id: Option<&str>,
        since_us: Option<u64>,
    ) -> PyResult<PyObject> {
        let scope = match entity_id {
            Some(eid) => ReflectScope::Entity {
                entity_id: eid.to_string(),
                since_us,
            },
            None => ReflectScope::Global { since_us },
        };

        let config = ReflectConfig::default();
        let mock_provider = hebbs_reflect::MockLlmProvider::new();

        let engine = self.engine.clone();
        let output = py.allow_threads(move || {
            engine
                .reflect(scope, &config, &mock_provider, &mock_provider)
                .map_err(to_py_err)
        })?;

        let dict = PyDict::new(py);
        dict.set_item("insights_created", output.insights_created)?;
        dict.set_item("clusters_found", output.clusters_found)?;
        dict.set_item("clusters_processed", output.clusters_processed)?;
        dict.set_item("memories_processed", output.memories_processed)?;
        Ok(dict.into())
    }

    /// Query stored insights.
    #[pyo3(signature = (entity_id=None, min_confidence=None, max_results=None))]
    fn insights(
        &self,
        py: Python<'_>,
        entity_id: Option<&str>,
        min_confidence: Option<f32>,
        max_results: Option<usize>,
    ) -> PyResult<PyObject> {
        let filter = InsightsFilter {
            entity_id: entity_id.map(|s| s.to_string()),
            min_confidence,
            max_results,
        };

        let engine = self.engine.clone();
        let memories = py.allow_threads(move || engine.insights(filter).map_err(to_py_err))?;

        let list = PyList::empty(py);
        for m in &memories {
            list.append(memory_to_py(py, m)?)?;
        }
        Ok(list.into())
    }

    /// Get the number of memories in the engine.
    fn count(&self, py: Python<'_>) -> PyResult<usize> {
        let engine = self.engine.clone();
        py.allow_threads(move || engine.count().map_err(to_py_err))
    }

    /// Close the engine and release all resources.
    fn close(&mut self) -> PyResult<()> {
        Ok(())
    }
}

fn parse_strategy(s: &str) -> PyResult<RecallStrategy> {
    match s.to_lowercase().as_str() {
        "similarity" => Ok(RecallStrategy::Similarity),
        "temporal" => Ok(RecallStrategy::Temporal),
        "causal" => Ok(RecallStrategy::Causal),
        "analogical" => Ok(RecallStrategy::Analogical),
        _ => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "unknown recall strategy '{}'. Valid: similarity, temporal, causal, analogical",
            s
        ))),
    }
}
