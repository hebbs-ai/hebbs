use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use ulid::Ulid;

use hebbs_core::forget::ForgetOutput;
use hebbs_core::memory::{Memory, MemoryKind};
use hebbs_core::recall::{RecallOutput, RecallResult, StrategyDetail};

/// Convert a Memory struct to a Python dict.
///
/// All fields are converted to Python-native types:
/// - memory_id: str (26-char ULID)
/// - context: dict
/// - kind: str ("episode" | "insight" | "revision")
/// - embedding: list[float] | None
/// - timestamps: int (microseconds since epoch)
pub fn memory_to_py(py: Python<'_>, m: &Memory) -> PyResult<PyObject> {
    let dict = PyDict::new(py);

    let ulid_str = if m.memory_id.len() == 16 {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&m.memory_id);
        Ulid::from_bytes(bytes).to_string()
    } else {
        hex::encode(&m.memory_id)
    };

    dict.set_item("id", ulid_str)?;
    dict.set_item("content", &m.content)?;
    dict.set_item("importance", m.importance)?;

    let context = m.context().unwrap_or_default();
    let ctx_dict = hashmap_to_py(py, &context)?;
    dict.set_item("context", ctx_dict)?;

    dict.set_item("entity_id", m.entity_id.as_deref())?;

    match &m.embedding {
        Some(emb) => {
            let py_list = PyList::new(py, emb.iter())?;
            dict.set_item("embedding", py_list)?;
        }
        None => {
            dict.set_item("embedding", py.None())?;
        }
    }

    dict.set_item("created_at", m.created_at)?;
    dict.set_item("updated_at", m.updated_at)?;
    dict.set_item("last_accessed_at", m.last_accessed_at)?;
    dict.set_item("access_count", m.access_count)?;
    dict.set_item("decay_score", m.decay_score)?;

    let kind_str = match m.kind {
        MemoryKind::Episode => "episode",
        MemoryKind::Insight => "insight",
        MemoryKind::Revision => "revision",
        MemoryKind::Document => "document",
        MemoryKind::Proposition => "proposition",
    };
    dict.set_item("kind", kind_str)?;
    dict.set_item("device_id", m.device_id.as_deref())?;
    dict.set_item("logical_clock", m.logical_clock)?;

    Ok(dict.into())
}

/// Convert a RecallOutput to a Python dict.
pub fn recall_output_to_py(py: Python<'_>, output: &RecallOutput) -> PyResult<PyObject> {
    let dict = PyDict::new(py);

    let results = PyList::empty(py);
    for r in &output.results {
        results.append(recall_result_to_py(py, r)?)?;
    }
    dict.set_item("results", results)?;

    let errors = PyList::empty(py);
    for e in &output.strategy_errors {
        let err_dict = PyDict::new(py);
        err_dict.set_item("strategy", format!("{:?}", e.strategy))?;
        err_dict.set_item("message", &e.message)?;
        errors.append(err_dict)?;
    }
    dict.set_item("strategy_errors", errors)?;

    match output.embed_duration_us {
        Some(us) => dict.set_item("embed_ms", us as f64 / 1000.0)?,
        None => dict.set_item("embed_ms", py.None())?,
    }

    Ok(dict.into())
}

fn recall_result_to_py(py: Python<'_>, r: &RecallResult) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item("memory", memory_to_py(py, &r.memory)?)?;
    dict.set_item("score", r.score)?;

    let details = PyList::empty(py);
    for d in &r.strategy_details {
        details.append(strategy_detail_to_py(py, d)?)?;
    }
    dict.set_item("strategy_details", details)?;

    Ok(dict.into())
}

fn strategy_detail_to_py(py: Python<'_>, d: &StrategyDetail) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    match d {
        StrategyDetail::Similarity {
            distance,
            relevance,
        } => {
            dict.set_item("strategy", "similarity")?;
            dict.set_item("distance", *distance)?;
            dict.set_item("relevance", *relevance)?;
        }
        StrategyDetail::Temporal {
            timestamp,
            rank,
            relevance,
        } => {
            dict.set_item("strategy", "temporal")?;
            dict.set_item("timestamp", *timestamp)?;
            dict.set_item("rank", *rank)?;
            dict.set_item("relevance", *relevance)?;
        }
        StrategyDetail::Causal {
            depth,
            edge_type,
            seed_id,
            relevance,
        } => {
            dict.set_item("strategy", "causal")?;
            dict.set_item("depth", *depth)?;
            dict.set_item("edge_type", format!("{:?}", edge_type))?;
            dict.set_item("seed_id", Ulid::from_bytes(*seed_id).to_string())?;
            dict.set_item("relevance", *relevance)?;
        }
        StrategyDetail::Analogical {
            embedding_similarity,
            structural_similarity,
            relevance,
            ..
        } => {
            dict.set_item("strategy", "analogical")?;
            dict.set_item("embedding_similarity", *embedding_similarity)?;
            dict.set_item("structural_similarity", *structural_similarity)?;
            dict.set_item("relevance", *relevance)?;
        }
    }
    Ok(dict.into())
}

/// Convert a ForgetOutput to a Python dict.
pub fn forget_output_to_py(py: Python<'_>, output: &ForgetOutput) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item("forgotten_count", output.forgotten_count)?;
    dict.set_item("cascade_count", output.cascade_count)?;
    dict.set_item("truncated", output.truncated)?;
    dict.set_item("tombstone_count", output.tombstone_count)?;
    Ok(dict.into())
}

/// Convert a HashMap<String, serde_json::Value> to a Python dict.
pub fn hashmap_to_py(
    py: Python<'_>,
    map: &HashMap<String, serde_json::Value>,
) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    for (key, value) in map {
        dict.set_item(key, json_value_to_py(py, value)?)?;
    }
    Ok(dict.into())
}

/// Convert a serde_json::Value to a Python object.
pub fn json_value_to_py(py: Python<'_>, v: &serde_json::Value) -> PyResult<PyObject> {
    match v {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok((*b).into_pyobject(py)?.to_owned().into_any().unbind()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any().unbind())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into_any().unbind())
            } else {
                Ok(py.None())
            }
        }
        serde_json::Value::String(s) => Ok(PyString::new(py, s).into()),
        serde_json::Value::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_value_to_py(py, item)?)?;
            }
            Ok(list.into())
        }
        serde_json::Value::Object(obj) => {
            let dict = PyDict::new(py);
            for (k, val) in obj {
                dict.set_item(k, json_value_to_py(py, val)?)?;
            }
            Ok(dict.into())
        }
    }
}

/// Convert a Python object to a serde_json::Value.
pub fn py_to_json_value(obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if obj.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if let Ok(b) = obj.extract::<bool>() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Ok(i) = obj.extract::<i64>() {
        return Ok(serde_json::json!(i));
    }
    if let Ok(f) = obj.extract::<f64>() {
        return Ok(serde_json::json!(f));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Ok(list) = obj.downcast::<PyList>() {
        let arr: Vec<serde_json::Value> = list
            .iter()
            .map(|item| py_to_json_value(&item))
            .collect::<PyResult<_>>()?;
        return Ok(serde_json::Value::Array(arr));
    }
    if let Ok(dict) = obj.downcast::<PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, py_to_json_value(&v)?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    let repr = obj.repr()?.to_string();
    Ok(serde_json::Value::String(repr))
}

/// Convert a Python dict to a HashMap<String, serde_json::Value>.
pub fn py_dict_to_hashmap(
    dict: &Bound<'_, PyDict>,
) -> PyResult<HashMap<String, serde_json::Value>> {
    let mut map = HashMap::new();
    for (k, v) in dict.iter() {
        let key: String = k.extract()?;
        map.insert(key, py_to_json_value(&v)?);
    }
    Ok(map)
}
