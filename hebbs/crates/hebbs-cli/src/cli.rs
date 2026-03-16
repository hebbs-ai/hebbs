use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "hebbs-cli",
    version,
    about = "HEBBS interactive CLI client -- the cli of cognitive memory"
)]
pub struct Cli {
    /// Server gRPC endpoint
    #[arg(long, global = true, env = "HEBBS_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Server HTTP port (for metrics)
    #[arg(long, global = true, env = "HEBBS_HTTP_PORT")]
    pub http_port: Option<u16>,

    /// Request timeout in milliseconds
    #[arg(long, global = true, env = "HEBBS_TIMEOUT")]
    pub timeout: Option<u64>,

    /// Output format
    #[arg(long, global = true, value_enum)]
    pub format: Option<FormatArg>,

    /// Color output
    #[arg(long, global = true, value_enum)]
    pub color: Option<ColorArg>,

    /// API key for authentication (can also be set via HEBBS_API_KEY env var)
    #[arg(long, global = true, env = "HEBBS_API_KEY")]
    pub api_key: Option<String>,

    /// Explicit tenant ID (optional, normally derived from API key)
    #[arg(long, global = true, env = "HEBBS_TENANT")]
    pub tenant: Option<String>,

    /// Verbose mode (-v for debug, -vv for trace)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Clone, ValueEnum)]
pub enum FormatArg {
    Human,
    Json,
    Raw,
}

#[derive(Clone, ValueEnum)]
pub enum ColorArg {
    Always,
    Never,
    Auto,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Store a new memory
    Remember {
        /// Content text (reads from stdin if not provided and stdin is a pipe)
        content: Option<String>,

        /// Importance score (0.0 to 1.0)
        #[arg(short, long)]
        importance: Option<f32>,

        /// Context as JSON object
        #[arg(short, long)]
        context: Option<String>,

        /// Entity ID for scoping
        #[arg(short, long)]
        entity_id: Option<String>,

        /// Graph edges (repeatable). Format: TARGET_ID:EDGE_TYPE[:CONFIDENCE]
        /// Types: caused_by, related_to, followed_by, revised_from, insight_from
        #[arg(long)]
        edge: Vec<String>,
    },

    /// Retrieve a memory by ID
    Get {
        /// Memory ID (ULID string or hex)
        id: String,
    },

    /// Recall memories using one or more strategies
    Recall {
        /// Search cue text
        cue: Option<String>,

        /// Recall strategy
        #[arg(short, long, value_enum)]
        strategy: Option<StrategyArg>,

        /// Maximum results to return
        #[arg(short = 'k', long, default_value = "10")]
        top_k: u32,

        /// Entity ID (required for temporal strategy)
        #[arg(short, long)]
        entity_id: Option<String>,

        /// Maximum graph traversal depth (causal strategy)
        #[arg(long)]
        max_depth: Option<u32>,

        /// Seed memory ID for causal strategy
        #[arg(long)]
        seed: Option<String>,

        /// Scoring weights as "relevance:recency:importance:reinforcement" (e.g. "1:0:0:0" for pure relevance)
        #[arg(short, long, value_name = "R:T:I:F")]
        weights: Option<String>,

        /// Override HNSW ef_search for this query (default: 50). Higher values increase recall accuracy but add latency.
        #[arg(long)]
        ef_search: Option<u32>,

        /// Edge types to follow in causal traversal (comma-separated: caused_by,followed_by,related_to,revised_from,insight_from)
        #[arg(long, value_delimiter = ',')]
        edge_types: Option<Vec<String>>,

        /// Time range for temporal strategy as START_US:END_US (microseconds since epoch)
        #[arg(long, value_name = "START:END")]
        time_range: Option<String>,

        /// Analogical alpha: blends embedding similarity (1.0) vs structural similarity (0.0). Default: 0.5.
        #[arg(long)]
        analogical_alpha: Option<f32>,

        /// Context as JSON object (for analogical structural similarity comparison)
        #[arg(short, long)]
        context: Option<String>,
    },

    /// Update an existing memory
    Revise {
        /// Memory ID to revise
        id: String,

        /// New content
        #[arg(short = 'C', long)]
        content: Option<String>,

        /// New importance
        #[arg(short, long)]
        importance: Option<f32>,

        /// Context as JSON object
        #[arg(short = 'x', long)]
        context: Option<String>,

        /// Context merge mode
        #[arg(long, value_enum, default_value = "merge")]
        context_mode: ContextModeArg,

        /// New entity ID
        #[arg(short, long)]
        entity_id: Option<String>,

        /// Graph edges (repeatable). Format: TARGET_ID:EDGE_TYPE[:CONFIDENCE]
        /// Types: caused_by, related_to, followed_by, revised_from, insight_from
        #[arg(long)]
        edge: Vec<String>,
    },

    /// Remove memories matching criteria
    Forget {
        /// Specific memory IDs to forget
        #[arg(short, long)]
        ids: Vec<String>,

        /// Entity ID scope
        #[arg(short, long)]
        entity_id: Option<String>,

        /// Staleness threshold in microseconds
        #[arg(long)]
        staleness_us: Option<u64>,

        /// Minimum access count floor
        #[arg(long)]
        access_floor: Option<u64>,

        /// Memory kind filter
        #[arg(long, value_enum)]
        kind: Option<KindArg>,

        /// Decay score floor
        #[arg(long)]
        decay_floor: Option<f32>,
    },

    /// Pre-load context for an entity
    Prime {
        /// Entity ID
        entity_id: String,

        /// Context as JSON object
        #[arg(short, long)]
        context: Option<String>,

        /// Maximum memories to return
        #[arg(short, long)]
        max_memories: Option<u32>,

        /// Recency window in microseconds
        #[arg(long)]
        recency_us: Option<u64>,

        /// Similarity cue text
        #[arg(long)]
        similarity_cue: Option<String>,
    },

    /// Start a subscription stream
    Subscribe {
        /// Entity ID scope
        #[arg(short, long)]
        entity_id: Option<String>,

        /// Confidence threshold (0.0 to 1.0)
        #[arg(short, long, default_value = "0.5")]
        confidence: f32,
    },

    /// Feed text to an active subscription
    Feed {
        /// Subscription ID
        subscription_id: u64,

        /// Text to feed
        text: String,
    },

    /// Trigger reflection pipeline
    Reflect {
        /// Entity ID scope (omit for global)
        #[arg(short, long)]
        entity_id: Option<String>,

        /// Only process memories since this timestamp (microseconds)
        #[arg(long)]
        since_us: Option<u64>,
    },

    /// Prepare reflection data for agent-driven two-step reflect (no LLM call)
    ReflectPrepare {
        /// Entity ID scope (omit for global)
        #[arg(short, long)]
        entity_id: Option<String>,

        /// Only process memories since this timestamp (microseconds)
        #[arg(long)]
        since_us: Option<u64>,
    },

    /// Commit agent-produced insights from a previous reflect-prepare session
    ReflectCommit {
        /// Session ID from reflect-prepare output
        #[arg(short, long)]
        session_id: String,

        /// JSON array of insights to commit
        #[arg(short, long)]
        insights: String,
    },

    /// Retrieve pending contradiction candidates for agent review
    ContradictionPrepare,

    /// Commit agent-reviewed contradiction verdicts
    ContradictionCommit {
        /// JSON array of verdicts to commit
        #[arg(short, long)]
        verdicts: String,
    },

    /// Query consolidated insights
    Insights {
        /// Entity ID filter
        #[arg(short, long)]
        entity_id: Option<String>,

        /// Minimum confidence threshold
        #[arg(short, long)]
        min_confidence: Option<f32>,

        /// Maximum results
        #[arg(short = 'k', long)]
        max_results: Option<u32>,
    },

    /// Server health and status
    Status,

    /// Inspect a memory (detail + graph edges + neighbors)
    Inspect {
        /// Memory ID
        id: String,
    },

    /// Export memories as JSONL
    Export {
        /// Entity ID scope
        #[arg(short, long)]
        entity_id: Option<String>,

        /// Maximum memories to export
        #[arg(short, long, default_value = "1000")]
        limit: u32,
    },

    /// Display server metrics
    Metrics,

    /// Print version
    Version,
}

#[derive(Clone, ValueEnum)]
pub enum StrategyArg {
    Similarity,
    Temporal,
    Causal,
    Analogical,
}

#[derive(Clone, ValueEnum)]
pub enum ContextModeArg {
    Merge,
    Replace,
}

#[derive(Clone, ValueEnum)]
pub enum KindArg {
    Episode,
    Insight,
    Revision,
}

/// Build the clap Command for REPL mode parsing (without binary name requirement).
pub fn build_command() -> clap::Command {
    Cli::command()
}
