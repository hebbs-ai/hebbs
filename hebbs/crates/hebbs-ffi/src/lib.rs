// Copyright 2025-2026 Parag Arora. Licensed under Apache-2.0.

//! C-ABI FFI layer for HEBBS cognitive memory engine.
//!
//! Provides an opaque handle wrapping `hebbs_core::Engine` for embedded
//! (no-server) usage from C, Python (PyO3), and other languages.
//!
//! # Safety
//!
//! All FFI functions follow these invariants:
//! - Handles returned by `hebbs_open` are valid until passed to `hebbs_close`.
//! - String pointers must be valid UTF-8 with the specified length.
//! - Result pointers are allocated by the FFI layer and freed via `hebbs_free_string`.
//! - Thread-local errors are retrievable via `hebbs_last_error`.
//! - The handle is `Send + Sync` and safe to share across threads.

mod convert;
mod error;
mod handle;
mod subscribe;

pub use error::{hebbs_free_string, hebbs_last_error};
pub use handle::{hebbs_close, hebbs_count, hebbs_open};

use std::os::raw::c_char;
use std::slice;

use crate::error::{
    set_last_error, FFI_INTERNAL, FFI_INVALID_ARG, FFI_INVALID_HANDLE, FFI_NOT_FOUND, FFI_OK,
    FFI_STORAGE,
};
use crate::handle::HandleRef;

// ═══════════════════════════════════════════════════════════════════════
//  Remember
// ═══════════════════════════════════════════════════════════════════════

/// Create a new memory with minimal parameters.
///
/// Returns 0 on success, negative on error. On success, `result_out` is set
/// to a JSON string that must be freed with `hebbs_free_string`.
///
/// # Safety
///
/// - `handle` must be a valid handle from `hebbs_open`.
/// - `content` must be a valid UTF-8 pointer with `content_len` bytes.
/// - `result_out` must be a valid pointer to a `*mut c_char`.
/// - `result_len_out` must be a valid pointer to a `u32`.
#[no_mangle]
pub unsafe extern "C" fn hebbs_remember(
    handle: *mut libc::c_void,
    content: *const c_char,
    content_len: u32,
    importance: f32,
    result_out: *mut *mut c_char,
    result_len_out: *mut u32,
) -> i32 {
    let engine = match HandleRef::from_raw(handle) {
        Some(e) => e,
        None => return FFI_INVALID_HANDLE,
    };

    let content_str = match ptr_to_str(content, content_len) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let input = hebbs_core::engine::RememberInput {
        content: content_str.to_string(),
        importance: Some(importance),
        context: None,
        entity_id: None,
        edges: Vec::new(),
            kind: None,
        };

    match engine.remember(input) {
        Ok(memory) => write_memory_result(&memory, result_out, result_len_out),
        Err(e) => map_engine_error(e),
    }
}

/// Create a new memory with full options (JSON input).
///
/// `options_json` is a JSON object with fields: content, importance, context,
/// entity_id, edges.
///
/// # Safety
///
/// Same as `hebbs_remember`.
#[no_mangle]
pub unsafe extern "C" fn hebbs_remember_with(
    handle: *mut libc::c_void,
    options_json: *const c_char,
    options_len: u32,
    result_out: *mut *mut c_char,
    result_len_out: *mut u32,
) -> i32 {
    let engine = match HandleRef::from_raw(handle) {
        Some(e) => e,
        None => return FFI_INVALID_HANDLE,
    };

    let json_str = match ptr_to_str(options_json, options_len) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let input = match convert::json_to_remember_input(json_str) {
        Ok(i) => i,
        Err(msg) => {
            set_last_error(&msg);
            return FFI_INVALID_ARG;
        }
    };

    match engine.remember(input) {
        Ok(memory) => write_memory_result(&memory, result_out, result_len_out),
        Err(e) => map_engine_error(e),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Get
// ═══════════════════════════════════════════════════════════════════════

/// Retrieve a memory by its 26-character ULID string.
///
/// # Safety
///
/// Same pointer safety requirements as `hebbs_remember`.
#[no_mangle]
pub unsafe extern "C" fn hebbs_get(
    handle: *mut libc::c_void,
    memory_id: *const c_char,
    memory_id_len: u32,
    result_out: *mut *mut c_char,
    result_len_out: *mut u32,
) -> i32 {
    let engine = match HandleRef::from_raw(handle) {
        Some(e) => e,
        None => return FFI_INVALID_HANDLE,
    };

    let id_str = match ptr_to_str(memory_id, memory_id_len) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let ulid = match ulid::Ulid::from_string(id_str) {
        Ok(u) => u,
        Err(e) => {
            set_last_error(&format!("invalid ULID: {}", e));
            return FFI_INVALID_ARG;
        }
    };

    match engine.get(&ulid.to_bytes()) {
        Ok(memory) => write_memory_result(&memory, result_out, result_len_out),
        Err(e) => map_engine_error(e),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall
// ═══════════════════════════════════════════════════════════════════════

/// Recall memories matching a cue with options specified as JSON.
///
/// # Safety
///
/// Same pointer safety requirements.
#[no_mangle]
pub unsafe extern "C" fn hebbs_recall(
    handle: *mut libc::c_void,
    cue: *const c_char,
    cue_len: u32,
    options_json: *const c_char,
    options_len: u32,
    result_out: *mut *mut c_char,
    result_len_out: *mut u32,
) -> i32 {
    let engine = match HandleRef::from_raw(handle) {
        Some(e) => e,
        None => return FFI_INVALID_HANDLE,
    };

    let cue_str = match ptr_to_str(cue, cue_len) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let opts_str = if options_json.is_null() || options_len == 0 {
        ""
    } else {
        match ptr_to_str(options_json, options_len) {
            Ok(s) => s,
            Err(code) => return code,
        }
    };

    let input = match convert::json_to_recall_input(cue_str, opts_str) {
        Ok(i) => i,
        Err(msg) => {
            set_last_error(&msg);
            return FFI_INVALID_ARG;
        }
    };

    match engine.recall(input) {
        Ok(output) => {
            let json = convert::recall_output_to_json(&output);
            write_string_result(&json, result_out, result_len_out)
        }
        Err(e) => map_engine_error(e),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Revise
// ═══════════════════════════════════════════════════════════════════════

/// Revise an existing memory. Options are specified as JSON.
///
/// # Safety
///
/// Same pointer safety requirements.
#[no_mangle]
pub unsafe extern "C" fn hebbs_revise(
    handle: *mut libc::c_void,
    options_json: *const c_char,
    options_len: u32,
    result_out: *mut *mut c_char,
    result_len_out: *mut u32,
) -> i32 {
    let engine = match HandleRef::from_raw(handle) {
        Some(e) => e,
        None => return FFI_INVALID_HANDLE,
    };

    let json_str = match ptr_to_str(options_json, options_len) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let input = match convert::json_to_revise_input(json_str) {
        Ok(i) => i,
        Err(msg) => {
            set_last_error(&msg);
            return FFI_INVALID_ARG;
        }
    };

    match engine.revise(input) {
        Ok(memory) => write_memory_result(&memory, result_out, result_len_out),
        Err(e) => map_engine_error(e),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Forget
// ═══════════════════════════════════════════════════════════════════════

/// Forget memories matching criteria specified as JSON.
///
/// # Safety
///
/// Same pointer safety requirements.
#[no_mangle]
pub unsafe extern "C" fn hebbs_forget(
    handle: *mut libc::c_void,
    criteria_json: *const c_char,
    criteria_len: u32,
    result_out: *mut *mut c_char,
    result_len_out: *mut u32,
) -> i32 {
    let engine = match HandleRef::from_raw(handle) {
        Some(e) => e,
        None => return FFI_INVALID_HANDLE,
    };

    let json_str = match ptr_to_str(criteria_json, criteria_len) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let criteria = match convert::json_to_forget_criteria(json_str) {
        Ok(c) => c,
        Err(msg) => {
            set_last_error(&msg);
            return FFI_INVALID_ARG;
        }
    };

    match engine.forget(criteria) {
        Ok(output) => {
            let json = serde_json::json!({
                "forgotten_count": output.forgotten_count,
                "cascade_count": output.cascade_count,
                "truncated": output.truncated,
                "tombstone_count": output.tombstone_count,
            });
            write_string_result(&json.to_string(), result_out, result_len_out)
        }
        Err(e) => map_engine_error(e),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Prime
// ═══════════════════════════════════════════════════════════════════════

/// Pre-load relevant memories for an entity. Options as JSON.
///
/// # Safety
///
/// Same pointer safety requirements.
#[no_mangle]
pub unsafe extern "C" fn hebbs_prime(
    handle: *mut libc::c_void,
    options_json: *const c_char,
    options_len: u32,
    result_out: *mut *mut c_char,
    result_len_out: *mut u32,
) -> i32 {
    let engine = match HandleRef::from_raw(handle) {
        Some(e) => e,
        None => return FFI_INVALID_HANDLE,
    };

    let json_str = match ptr_to_str(options_json, options_len) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let input = match convert::json_to_prime_input(json_str) {
        Ok(i) => i,
        Err(msg) => {
            set_last_error(&msg);
            return FFI_INVALID_ARG;
        }
    };

    match engine.prime(input) {
        Ok(output) => {
            let json = convert::prime_output_to_json(&output);
            write_string_result(&json, result_out, result_len_out)
        }
        Err(e) => map_engine_error(e),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Reflect
// ═══════════════════════════════════════════════════════════════════════

/// Trigger reflection. Scope specified as JSON.
///
/// # Safety
///
/// Same pointer safety requirements.
#[no_mangle]
pub unsafe extern "C" fn hebbs_reflect(
    handle: *mut libc::c_void,
    options_json: *const c_char,
    options_len: u32,
    result_out: *mut *mut c_char,
    result_len_out: *mut u32,
) -> i32 {
    let engine = match HandleRef::from_raw(handle) {
        Some(e) => e,
        None => return FFI_INVALID_HANDLE,
    };

    let json_str = match ptr_to_str(options_json, options_len) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let (config, scope) = match convert::json_to_reflect_params(json_str) {
        Ok(v) => v,
        Err(msg) => {
            set_last_error(&msg);
            return FFI_INVALID_ARG;
        }
    };

    let mock_provider = hebbs_reflect::MockLlmProvider::new();

    match engine.reflect(scope, &config, &mock_provider, &mock_provider) {
        Ok(output) => {
            let json = serde_json::json!({
                "insights_created": output.insights_created,
                "clusters_found": output.clusters_found,
                "clusters_processed": output.clusters_processed,
                "memories_processed": output.memories_processed,
            });
            write_string_result(&json.to_string(), result_out, result_len_out)
        }
        Err(e) => map_engine_error(e),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Insights
// ═══════════════════════════════════════════════════════════════════════

/// Query insights with optional filter (JSON).
///
/// # Safety
///
/// Same pointer safety requirements.
#[no_mangle]
pub unsafe extern "C" fn hebbs_insights(
    handle: *mut libc::c_void,
    filter_json: *const c_char,
    filter_len: u32,
    result_out: *mut *mut c_char,
    result_len_out: *mut u32,
) -> i32 {
    let engine = match HandleRef::from_raw(handle) {
        Some(e) => e,
        None => return FFI_INVALID_HANDLE,
    };

    let json_str = if filter_json.is_null() || filter_len == 0 {
        "{}"
    } else {
        match ptr_to_str(filter_json, filter_len) {
            Ok(s) => s,
            Err(code) => return code,
        }
    };

    let filter = match convert::json_to_insights_filter(json_str) {
        Ok(f) => f,
        Err(msg) => {
            set_last_error(&msg);
            return FFI_INVALID_ARG;
        }
    };

    match engine.insights(filter) {
        Ok(memories) => {
            let json = convert::memories_to_json(&memories);
            write_string_result(&json, result_out, result_len_out)
        }
        Err(e) => map_engine_error(e),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Internal helpers
// ═══════════════════════════════════════════════════════════════════════

unsafe fn ptr_to_str<'a>(ptr: *const c_char, len: u32) -> Result<&'a str, i32> {
    if ptr.is_null() {
        set_last_error("null pointer for string argument");
        return Err(FFI_INVALID_ARG);
    }
    let bytes = slice::from_raw_parts(ptr as *const u8, len as usize);
    std::str::from_utf8(bytes).map_err(|e| {
        set_last_error(&format!("invalid UTF-8: {}", e));
        FFI_INVALID_ARG
    })
}

fn write_memory_result(
    memory: &hebbs_core::memory::Memory,
    result_out: *mut *mut c_char,
    result_len_out: *mut u32,
) -> i32 {
    let json = convert::memory_to_json(memory);
    write_string_result(&json, result_out, result_len_out)
}

fn write_string_result(s: &str, result_out: *mut *mut c_char, result_len_out: *mut u32) -> i32 {
    if result_out.is_null() || result_len_out.is_null() {
        return FFI_OK;
    }
    let c_string = match std::ffi::CString::new(s) {
        Ok(cs) => cs,
        Err(e) => {
            set_last_error(&format!("result contains null byte: {}", e));
            return FFI_INTERNAL;
        }
    };
    let len = c_string.as_bytes().len() as u32;
    let ptr = c_string.into_raw();
    unsafe {
        *result_out = ptr;
        *result_len_out = len;
    }
    FFI_OK
}

fn map_engine_error(e: hebbs_core::error::HebbsError) -> i32 {
    use hebbs_core::error::HebbsError;
    let msg = e.to_string();
    set_last_error(&msg);
    match e {
        HebbsError::InvalidInput { .. } => FFI_INVALID_ARG,
        HebbsError::MemoryNotFound { .. } => FFI_NOT_FOUND,
        HebbsError::Storage(_) => FFI_STORAGE,
        HebbsError::Embedding(_) => error::FFI_EMBEDDING,
        HebbsError::Serialization { .. } => FFI_INTERNAL,
        HebbsError::Internal { .. } => FFI_INTERNAL,
        HebbsError::Index(_) => FFI_INTERNAL,
        HebbsError::Reflect(_) => FFI_INTERNAL,
        _ => FFI_INTERNAL,
    }
}
