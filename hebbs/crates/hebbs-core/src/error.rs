/// Domain-level errors for the HEBBS cognitive memory engine.
///
/// These wrap `StorageError`, `EmbedError`, and `IndexError` with
/// cognitive-domain context and add engine-specific error variants.
/// Downstream consumers (gRPC server, SDKs) match on these, not on
/// lower-layer errors directly.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HebbsError {
    #[error("storage error: {0}")]
    Storage(#[from] hebbs_storage::StorageError),

    #[error("embedding error: {0}")]
    Embedding(#[from] hebbs_embed::EmbedError),

    #[error("index error: {0}")]
    Index(#[from] hebbs_index::IndexError),

    #[error("reflect error: {0}")]
    Reflect(#[from] hebbs_reflect::ReflectError),

    #[error("LLM error: {0}")]
    Llm(#[from] hebbs_llm::LlmError),

    #[error("memory not found: {memory_id}")]
    MemoryNotFound { memory_id: String },

    #[error("invalid input for {operation}: {message}")]
    InvalidInput {
        operation: &'static str,
        message: String,
    },

    #[error("serialization error: {message}")]
    Serialization { message: String },

    #[error("internal error in {operation}: {message}")]
    Internal {
        operation: &'static str,
        message: String,
    },

    /// Phase 13: Missing or invalid API key. Maps to gRPC UNAUTHENTICATED / HTTP 401.
    #[error("unauthorized: {message}")]
    Unauthorized {
        endpoint: &'static str,
        message: String,
    },

    /// Phase 13: Valid key but insufficient permissions. Maps to gRPC PERMISSION_DENIED / HTTP 403.
    #[error("forbidden on {endpoint}: requires {required}, key has {actual}")]
    Forbidden {
        endpoint: &'static str,
        required: String,
        actual: String,
    },

    /// Phase 13: Per-tenant rate limit exceeded. Maps to gRPC RESOURCE_EXHAUSTED / HTTP 429.
    #[error("rate limited: retry after {retry_after_ms}ms")]
    RateLimited {
        retry_after_ms: u64,
        operation_class: String,
        tenant_id: String,
    },

    /// Phase 13: Operation references a tenant with no data.
    #[error("tenant not found: {tenant_id}")]
    TenantNotFound { tenant_id: String },
}

pub type Result<T> = std::result::Result<T, HebbsError>;
