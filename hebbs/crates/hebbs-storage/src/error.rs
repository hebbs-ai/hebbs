use std::fmt;

/// All errors produced by the HEBBS storage layer.
///
/// This taxonomy covers the full project (not just Phase 1) so that
/// downstream crates can depend on a stable, additive error surface.
/// Marked `#[non_exhaustive]` so new variants can be added without
/// breaking downstream match arms.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StorageError {
    /// RocksDB I/O failure — disk full, WAL write failure, corruption.
    /// May be retryable after freeing resources.
    #[error("storage I/O error in {operation}: {message}")]
    Io {
        operation: &'static str,
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Requested key does not exist in the specified column family.
    #[error("not found: {entity_kind} with key {key} in column family '{cf}'")]
    NotFound {
        entity_kind: &'static str,
        key: String,
        cf: ColumnFamilyName,
    },

    /// Caller provided invalid data — content too long, out-of-range
    /// values, malformed keys.
    #[error("invalid input for {operation}: {message} (got {actual}, limit {limit})")]
    InvalidInput {
        operation: &'static str,
        message: String,
        actual: String,
        limit: String,
    },

    /// Serialization or deserialization failed — corrupt data on disk
    /// or incompatible schema version.
    #[error("serialization error in {operation}: {message}")]
    Serialization {
        operation: &'static str,
        message: String,
    },

    /// A bounded resource hit its configured limit — max memories,
    /// max batch size, etc.
    #[error("capacity exceeded in {operation}: {message} (current {current}, max {max})")]
    CapacityExceeded {
        operation: &'static str,
        message: String,
        current: u64,
        max: u64,
    },

    /// Invariant violation — a bug in HEBBS itself.
    #[error("internal error in {operation}: {message}")]
    Internal {
        operation: &'static str,
        message: String,
    },
}

/// Strongly-typed column family names.
///
/// Using an enum instead of raw strings prevents typos in CF references
/// and makes the set of column families exhaustively known at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnFamilyName {
    Default,
    Temporal,
    Vectors,
    Graph,
    Meta,
    VectorsAssociative,
    QueryLog,
}

impl ColumnFamilyName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Temporal => "temporal",
            Self::Vectors => "vectors",
            Self::Graph => "graph",
            Self::Meta => "meta",
            Self::VectorsAssociative => "vectors_associative",
            Self::QueryLog => "query_log",
        }
    }

    /// All column families in creation order.
    pub fn all() -> &'static [ColumnFamilyName] {
        &[
            Self::Default,
            Self::Temporal,
            Self::Vectors,
            Self::Graph,
            Self::Meta,
            Self::VectorsAssociative,
            Self::QueryLog,
        ]
    }

    /// Non-default column families (RocksDB always creates "default"
    /// implicitly, so these are the ones we must explicitly create).
    pub fn non_default() -> &'static [ColumnFamilyName] {
        &[
            Self::Temporal,
            Self::Vectors,
            Self::Graph,
            Self::Meta,
            Self::VectorsAssociative,
            Self::QueryLog,
        ]
    }
}

impl fmt::Display for ColumnFamilyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub type Result<T> = std::result::Result<T, StorageError>;
