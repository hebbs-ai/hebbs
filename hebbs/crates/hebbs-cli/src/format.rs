use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use comfy_table::{presets, Cell, CellAlignment, Color, ContentArrangement, Table};
use owo_colors::OwoColorize;
use serde::Serialize;

use hebbs_proto::generated as pb;

use crate::config::OutputFormat;

// ═══════════════════════════════════════════════════════════════════════
//  ULID Formatting
// ═══════════════════════════════════════════════════════════════════════

/// Encode 16 raw bytes as a 26-character Crockford Base32 ULID string.
pub fn ulid_to_string(bytes: &[u8]) -> String {
    if bytes.len() != 16 {
        return hex::encode(bytes);
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(bytes);
    let val = u128::from_be_bytes(arr);
    ulid::Ulid::from(val).to_string()
}

/// Parse a ULID string (26 chars Crockford Base32) or hex string (32 chars) into 16 bytes.
pub fn parse_memory_id(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();

    // Try ULID format (26 chars)
    if trimmed.len() == 26 {
        if let Ok(ulid) = ulid::Ulid::from_string(trimmed) {
            return Ok(ulid.0.to_be_bytes().to_vec());
        }
    }

    // Try hex format (32 chars)
    if trimmed.len() == 32 {
        if let Ok(bytes) = hex::decode(trimmed) {
            if bytes.len() == 16 {
                return Ok(bytes);
            }
        }
    }

    Err(format!(
        "Invalid memory ID '{}'. Expected 26-char ULID or 32-char hex string.",
        trimmed
    ))
}

// ═══════════════════════════════════════════════════════════════════════
//  Timestamp Formatting
// ═══════════════════════════════════════════════════════════════════════

/// Format a microsecond timestamp as a relative time string (e.g., "2m ago").
pub fn format_relative_time(timestamp_us: u64) -> String {
    let now_us = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    if timestamp_us == 0 {
        return "never".to_string();
    }

    if timestamp_us > now_us {
        return "just now".to_string();
    }

    let diff_us = now_us - timestamp_us;
    let secs = diff_us / 1_000_000;

    if secs < 60 {
        return format!("{}s ago", secs);
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{}m ago", mins);
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{}h ago", hours);
    }
    let days = hours / 24;
    if days < 30 {
        return format!("{}d ago", days);
    }
    let months = days / 30;
    if months < 12 {
        return format!("{}mo ago", months);
    }
    let years = months / 12;
    format!("{}y ago", years)
}

/// Format a microsecond timestamp as ISO-8601.
pub fn format_absolute_time(timestamp_us: u64) -> String {
    if timestamp_us == 0 {
        return "N/A".to_string();
    }
    let secs = timestamp_us / 1_000_000;
    let subsec = (timestamp_us % 1_000_000) / 1000;

    chrono_minimal(secs, subsec as u32)
}

fn chrono_minimal(secs: u64, millis: u32) -> String {
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let mins = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days_since_epoch);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hours, mins, seconds, millis
    )
}

fn days_to_ymd(days: u64) -> (i32, u32, u32) {
    // Civil calendar algorithm from Howard Hinnant
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

/// Format elapsed duration in a human-readable way.
pub fn format_elapsed(elapsed: std::time::Duration) -> String {
    let us = elapsed.as_micros();
    if us < 1000 {
        format!("{}µs", us)
    } else if us < 1_000_000 {
        format!("{:.1}ms", us as f64 / 1000.0)
    } else {
        format!("{:.2}s", elapsed.as_secs_f64())
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Memory Display
// ═══════════════════════════════════════════════════════════════════════

/// JSON-serializable memory representation.
#[derive(Serialize)]
pub struct MemoryJson {
    pub memory_id: String,
    pub content: String,
    pub importance: f32,
    pub context: serde_json::Value,
    pub entity_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub last_accessed_at: u64,
    pub access_count: u64,
    pub decay_score: f32,
    pub kind: String,
    pub logical_clock: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_dimensions: Option<usize>,
}

pub fn proto_memory_to_json(m: &pb::Memory) -> MemoryJson {
    let context = proto_struct_to_json(&m.context);

    MemoryJson {
        memory_id: ulid_to_string(&m.memory_id),
        content: m.content.clone(),
        importance: m.importance,
        context,
        entity_id: m.entity_id.clone(),
        created_at: m.created_at,
        updated_at: m.updated_at,
        last_accessed_at: m.last_accessed_at,
        access_count: m.access_count,
        decay_score: m.decay_score,
        kind: format_kind(m.kind),
        logical_clock: m.logical_clock,
        embedding_dimensions: if m.embedding.is_empty() {
            None
        } else {
            Some(m.embedding.len())
        },
    }
}

pub fn format_kind(kind_i32: i32) -> String {
    match pb::MemoryKind::try_from(kind_i32) {
        Ok(pb::MemoryKind::Episode) => "episode".to_string(),
        Ok(pb::MemoryKind::Insight) => "insight".to_string(),
        Ok(pb::MemoryKind::Revision) => "revision".to_string(),
        Ok(pb::MemoryKind::Document) => "document".to_string(),
        Ok(pb::MemoryKind::Proposition) => "proposition".to_string(),
        _ => "unknown".to_string(),
    }
}

pub fn proto_struct_to_json(s: &Option<prost_types::Struct>) -> serde_json::Value {
    match s {
        Some(st) => {
            let map: serde_json::Map<String, serde_json::Value> = st
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), prost_value_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        None => serde_json::Value::Object(serde_json::Map::new()),
    }
}

fn prost_value_to_json(v: &prost_types::Value) -> serde_json::Value {
    use prost_types::value::Kind;
    match &v.kind {
        Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::NumberValue(n)) => serde_json::json!(*n),
        Some(Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(Kind::StructValue(s)) => proto_struct_to_json(&Some(s.clone())),
        Some(Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.iter().map(prost_value_to_json).collect())
        }
        None => serde_json::Value::Null,
    }
}

fn json_to_prost_struct(map: &serde_json::Map<String, serde_json::Value>) -> prost_types::Struct {
    prost_types::Struct {
        fields: map
            .iter()
            .map(|(k, v)| (k.clone(), json_to_prost_value(v)))
            .collect(),
    }
}

pub fn json_to_prost_value(v: &serde_json::Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match v {
        serde_json::Value::Null => Kind::NullValue(0),
        serde_json::Value::Bool(b) => Kind::BoolValue(*b),
        serde_json::Value::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Kind::StringValue(s.clone()),
        serde_json::Value::Array(arr) => Kind::ListValue(prost_types::ListValue {
            values: arr.iter().map(json_to_prost_value).collect(),
        }),
        serde_json::Value::Object(obj) => Kind::StructValue(json_to_prost_struct(obj)),
    };
    prost_types::Value { kind: Some(kind) }
}

/// Parse a JSON string into a prost_types::Struct for proto context fields.
pub fn parse_context_json(json_str: &str) -> Result<prost_types::Struct, String> {
    let val: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Invalid JSON context: {}", e))?;

    match val {
        serde_json::Value::Object(map) => Ok(json_to_prost_struct(&map)),
        _ => Err("Context must be a JSON object".to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Edge Parsing
// ═══════════════════════════════════════════════════════════════════════

/// Parse an edge specification string into a proto Edge.
///
/// Format: `TARGET_ID:EDGE_TYPE[:CONFIDENCE]`
///
/// - TARGET_ID: 26-char ULID or 32-char hex
/// - EDGE_TYPE: caused_by | related_to | followed_by | revised_from | insight_from
/// - CONFIDENCE: optional float 0.0-1.0 (defaults to None)
///
/// Examples:
///   `01ARZ3NDEKTSV4RRFFQ69G5FAV:caused_by`
///   `01ARZ3NDEKTSV4RRFFQ69G5FAV:related_to:0.9`
pub fn parse_edge(input: &str) -> Result<pb::Edge, String> {
    let parts: Vec<&str> = input.splitn(3, ':').collect();

    if parts.len() < 2 {
        return Err(format!(
            "Invalid edge '{}'. Expected TARGET_ID:EDGE_TYPE[:CONFIDENCE]",
            input
        ));
    }

    let target_id = parse_memory_id(parts[0])?;

    let edge_type = parse_edge_type(parts[1])?;

    let confidence = if parts.len() == 3 {
        let c: f32 = parts[2].parse().map_err(|_| {
            format!(
                "Invalid confidence '{}'. Expected a float 0.0-1.0",
                parts[2]
            )
        })?;
        if !(0.0..=1.0).contains(&c) {
            return Err(format!("Confidence {} out of range. Must be 0.0-1.0", c));
        }
        Some(c)
    } else {
        None
    };

    Ok(pb::Edge {
        target_id,
        edge_type: edge_type as i32,
        confidence,
    })
}

/// Parse multiple edge specifications.
pub fn parse_edges(inputs: &[String]) -> Result<Vec<pb::Edge>, String> {
    inputs.iter().map(|s| parse_edge(s)).collect()
}

fn parse_edge_type(s: &str) -> Result<pb::EdgeType, String> {
    match s.to_lowercase().as_str() {
        "caused_by" => Ok(pb::EdgeType::CausedBy),
        "related_to" => Ok(pb::EdgeType::RelatedTo),
        "followed_by" => Ok(pb::EdgeType::FollowedBy),
        "revised_from" => Ok(pb::EdgeType::RevisedFrom),
        "insight_from" => Ok(pb::EdgeType::InsightFrom),
        "contradicts" => Ok(pb::EdgeType::Contradicts),
        "has_entity" => Ok(pb::EdgeType::HasEntity),
        "entity_relation" => Ok(pb::EdgeType::EntityRelation),
        "proposition_of" => Ok(pb::EdgeType::PropositionOf),
        _ => Err(format!(
            "Unknown edge type '{}'. Valid types: caused_by, related_to, followed_by, revised_from, insight_from, contradicts, has_entity, entity_relation, proposition_of",
            s
        )),
    }
}

pub fn format_edge_type(et: i32) -> &'static str {
    match pb::EdgeType::try_from(et) {
        Ok(pb::EdgeType::CausedBy) => "caused_by",
        Ok(pb::EdgeType::RelatedTo) => "related_to",
        Ok(pb::EdgeType::FollowedBy) => "followed_by",
        Ok(pb::EdgeType::RevisedFrom) => "revised_from",
        Ok(pb::EdgeType::InsightFrom) => "insight_from",
        Ok(pb::EdgeType::Contradicts) => "contradicts",
        Ok(pb::EdgeType::HasEntity) => "has_entity",
        Ok(pb::EdgeType::EntityRelation) => "entity_relation",
        Ok(pb::EdgeType::PropositionOf) => "proposition_of",
        _ => "unknown",
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Output Rendering
// ═══════════════════════════════════════════════════════════════════════

pub struct Renderer {
    pub format: OutputFormat,
    pub use_color: bool,
}

impl Renderer {
    pub fn new(format: OutputFormat, use_color: bool) -> Self {
        Self { format, use_color }
    }

    pub fn render_memory(&self, m: &pb::Memory, w: &mut dyn Write) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => self.render_memory_human(m, w),
            OutputFormat::Json => self.render_memory_json(m, w),
            OutputFormat::Raw => writeln!(w, "{:?}", m),
        }
    }

    fn render_memory_human(&self, m: &pb::Memory, w: &mut dyn Write) -> std::io::Result<()> {
        let id = ulid_to_string(&m.memory_id);
        let kind = format_kind(m.kind);
        let created = format_relative_time(m.created_at);
        let content_preview = truncate_content(&m.content, 80);

        if self.use_color {
            writeln!(
                w,
                "{}  {}  imp={}  {}",
                id.cyan(),
                kind.dimmed(),
                format_importance_colored(m.importance),
                created.dimmed()
            )?;
        } else {
            writeln!(w, "{}  {}  imp={:.2}  {}", id, kind, m.importance, created)?;
        }

        writeln!(w, "  {}", content_preview)?;

        if let Some(ref eid) = m.entity_id {
            writeln!(w, "  entity: {}", eid)?;
        }

        Ok(())
    }

    fn render_memory_json(&self, m: &pb::Memory, w: &mut dyn Write) -> std::io::Result<()> {
        let json = proto_memory_to_json(m);
        let s = serde_json::to_string(&json).unwrap_or_default();
        writeln!(w, "{}", s)
    }

    /// Render a list of memories. JSON format outputs a JSON array.
    pub fn render_memories(
        &self,
        memories: &[pb::Memory],
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Json => {
                let json_values: Vec<MemoryJson> =
                    memories.iter().map(proto_memory_to_json).collect();
                let s = serde_json::to_string(&json_values).unwrap_or_default();
                writeln!(w, "{}", s)
            }
            _ => {
                for m in memories {
                    self.render_memory(m, w)?;
                }
                Ok(())
            }
        }
    }

    pub fn render_memory_detail(&self, m: &pb::Memory, w: &mut dyn Write) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => self.render_memory_detail_human(m, w),
            OutputFormat::Json => self.render_memory_json(m, w),
            OutputFormat::Raw => writeln!(w, "{:?}", m),
        }
    }

    fn render_memory_detail_human(&self, m: &pb::Memory, w: &mut dyn Write) -> std::io::Result<()> {
        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_BORDERS_ONLY)
            .set_content_arrangement(ContentArrangement::Dynamic);

        let id = ulid_to_string(&m.memory_id);
        let rows = [
            ("Memory ID", id),
            ("Kind", format_kind(m.kind)),
            ("Importance", format!("{:.4}", m.importance)),
            ("Content", m.content.clone()),
            ("Entity ID", m.entity_id.clone().unwrap_or_default()),
            ("Created", format_timestamp_full(m.created_at)),
            ("Updated", format_timestamp_full(m.updated_at)),
            ("Last Accessed", format_timestamp_full(m.last_accessed_at)),
            ("Access Count", m.access_count.to_string()),
            ("Decay Score", format!("{:.4}", m.decay_score)),
            ("Logical Clock", m.logical_clock.to_string()),
            (
                "Embedding",
                if m.embedding.is_empty() {
                    "none".to_string()
                } else {
                    format!(
                        "{}-dim, norm={:.4}",
                        m.embedding.len(),
                        embedding_norm(&m.embedding)
                    )
                },
            ),
            ("Context", {
                let ctx = proto_struct_to_json(&m.context);
                serde_json::to_string_pretty(&ctx).unwrap_or_default()
            }),
        ];

        for (label, value) in &rows {
            if self.use_color {
                table.add_row(vec![Cell::new(label).fg(Color::Cyan), Cell::new(value)]);
            } else {
                table.add_row(vec![Cell::new(label), Cell::new(value)]);
            }
        }

        writeln!(w, "{}", table)
    }

    pub fn render_recall_results(
        &self,
        results: &[pb::RecallResult],
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => self.render_recall_human(results, w),
            OutputFormat::Json => {
                let json_results: Vec<RecallResultJson> =
                    results.iter().map(recall_result_to_json).collect();
                let s = serde_json::to_string(&json_results).unwrap_or_default();
                writeln!(w, "{}", s)
            }
            OutputFormat::Raw => writeln!(w, "{:?}", results),
        }
    }

    fn render_recall_human(
        &self,
        results: &[pb::RecallResult],
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        if results.is_empty() {
            writeln!(w, "No results.")?;
            return Ok(());
        }

        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_BORDERS_ONLY)
            .set_content_arrangement(ContentArrangement::Dynamic);

        table.set_header(vec![
            Cell::new("#").set_alignment(CellAlignment::Right),
            Cell::new("Score"),
            Cell::new("Relevance"),
            Cell::new("Memory ID"),
            Cell::new("Kind"),
            Cell::new("Content"),
        ]);

        for (i, r) in results.iter().enumerate() {
            if let Some(ref m) = r.memory {
                let id = ulid_to_string(&m.memory_id);
                let content = truncate_content(&m.content, 60);
                let kind = format_kind(m.kind);
                let relevance = r
                    .strategy_details
                    .first()
                    .map(|d| format!("{:.4}", d.relevance))
                    .unwrap_or_else(|| "--".to_string());

                table.add_row(vec![
                    Cell::new(i + 1).set_alignment(CellAlignment::Right),
                    Cell::new(format!("{:.4}", r.score)),
                    Cell::new(&relevance),
                    Cell::new(&id),
                    Cell::new(&kind),
                    Cell::new(&content),
                ]);
            }
        }

        writeln!(w, "{}", table)
    }

    pub fn render_forget_result(
        &self,
        resp: &pb::ForgetResponse,
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => {
                writeln!(
                    w,
                    "Forgotten: {}, Cascaded: {}, Tombstones: {}{}",
                    resp.forgotten_count,
                    resp.cascade_count,
                    resp.tombstone_count,
                    if resp.truncated { " (truncated)" } else { "" }
                )
            }
            OutputFormat::Json => {
                let json = serde_json::json!({
                    "forgotten_count": resp.forgotten_count,
                    "cascade_count": resp.cascade_count,
                    "tombstone_count": resp.tombstone_count,
                    "truncated": resp.truncated,
                });
                writeln!(w, "{}", serde_json::to_string(&json).unwrap_or_default())
            }
            OutputFormat::Raw => writeln!(w, "{:?}", resp),
        }
    }

    pub fn render_health(
        &self,
        resp: &pb::HealthCheckResponse,
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => {
                let status_str = match resp.status() {
                    pb::health_check_response::ServingStatus::Serving => "SERVING",
                    pb::health_check_response::ServingStatus::NotServing => "NOT SERVING",
                    _ => "UNKNOWN",
                };
                writeln!(w, "Status:       {}", status_str)?;
                writeln!(w, "Version:      {}", resp.version)?;
                writeln!(w, "Memory Count: {}", resp.memory_count)?;
                writeln!(w, "Uptime:       {}s", resp.uptime_seconds)?;
                Ok(())
            }
            OutputFormat::Json => {
                let json = serde_json::json!({
                    "status": format!("{:?}", resp.status()),
                    "version": resp.version,
                    "memory_count": resp.memory_count,
                    "uptime_seconds": resp.uptime_seconds,
                });
                writeln!(w, "{}", serde_json::to_string(&json).unwrap_or_default())
            }
            OutputFormat::Raw => writeln!(w, "{:?}", resp),
        }
    }

    pub fn render_reflect_result(
        &self,
        resp: &pb::ReflectResponse,
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => {
                writeln!(
                    w,
                    "Insights created: {}, Clusters found: {}, Clusters processed: {}, Memories processed: {}",
                    resp.insights_created,
                    resp.clusters_found,
                    resp.clusters_processed,
                    resp.memories_processed
                )
            }
            OutputFormat::Json => {
                let json = serde_json::json!({
                    "insights_created": resp.insights_created,
                    "clusters_found": resp.clusters_found,
                    "clusters_processed": resp.clusters_processed,
                    "memories_processed": resp.memories_processed,
                });
                writeln!(w, "{}", serde_json::to_string(&json).unwrap_or_default())
            }
            OutputFormat::Raw => writeln!(w, "{:?}", resp),
        }
    }

    pub fn render_reflect_prepare_result(
        &self,
        resp: &pb::ReflectPrepareResponse,
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => {
                writeln!(
                    w,
                    "Session: {}\nMemories processed: {}\nClusters: {}\nExisting insights: {}",
                    resp.session_id,
                    resp.memories_processed,
                    resp.clusters.len(),
                    resp.existing_insight_count
                )?;
                for c in &resp.clusters {
                    writeln!(
                        w,
                        "\n  Cluster {}: {} members",
                        c.cluster_id, c.member_count,
                    )?;
                    for m in &c.memories {
                        let preview = truncate_content(&m.content, 60);
                        writeln!(w, "    [{:.2}] {} — {}", m.importance, m.memory_id, preview,)?;
                    }
                }
                Ok(())
            }
            OutputFormat::Json => {
                let clusters: Vec<serde_json::Value> = resp
                    .clusters
                    .iter()
                    .map(|c| {
                        let memories: Vec<serde_json::Value> = c.memories.iter().map(|m| {
                            serde_json::json!({
                                "memory_id": m.memory_id,
                                "content": m.content,
                                "importance": m.importance,
                                "entity_id": m.entity_id,
                                "created_at": m.created_at,
                            })
                        }).collect();
                        serde_json::json!({
                            "cluster_id": c.cluster_id,
                            "member_count": c.member_count,
                            "proposal_system_prompt": c.proposal_system_prompt,
                            "proposal_user_prompt": c.proposal_user_prompt,
                            "memory_ids": c.memory_ids,
                            "validation_context": serde_json::from_str::<serde_json::Value>(&c.validation_context).unwrap_or(serde_json::Value::Null),
                            "memories": memories,
                        })
                    })
                    .collect();
                let json = serde_json::json!({
                    "session_id": resp.session_id,
                    "memories_processed": resp.memories_processed,
                    "clusters": clusters,
                    "existing_insight_count": resp.existing_insight_count,
                });
                writeln!(
                    w,
                    "{}",
                    serde_json::to_string_pretty(&json).unwrap_or_default()
                )
            }
            OutputFormat::Raw => writeln!(w, "{:?}", resp),
        }
    }

    pub fn render_reflect_commit_result(
        &self,
        resp: &pb::ReflectCommitResponse,
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => {
                writeln!(w, "Insights committed: {}", resp.insights_created)
            }
            OutputFormat::Json => {
                let json = serde_json::json!({
                    "insights_created": resp.insights_created,
                });
                writeln!(w, "{}", serde_json::to_string(&json).unwrap_or_default())
            }
            OutputFormat::Raw => writeln!(w, "{:?}", resp),
        }
    }

    pub fn render_contradiction_prepare_result(
        &self,
        resp: &pb::ContradictionPrepareResponse,
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => {
                if resp.candidates.is_empty() {
                    writeln!(w, "No pending contradictions.")
                } else {
                    writeln!(w, "Pending contradictions: {}\n", resp.candidates.len())?;
                    for c in &resp.candidates {
                        writeln!(w, "--- {} ---", c.pending_id)?;
                        writeln!(w, "  Memory A ({}): {}", c.memory_id_a, c.content_a_snippet)?;
                        writeln!(w, "  Memory B ({}): {}", c.memory_id_b, c.content_b_snippet)?;
                        writeln!(
                            w,
                            "  Score: {:.2} ({}) | Similarity: {:.2}",
                            c.classifier_score, c.classifier_method, c.similarity
                        )?;
                    }
                    Ok(())
                }
            }
            OutputFormat::Json => {
                let json: Vec<serde_json::Value> = resp
                    .candidates
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "pending_id": c.pending_id,
                            "memory_id_a": c.memory_id_a,
                            "memory_id_b": c.memory_id_b,
                            "content_a_snippet": c.content_a_snippet,
                            "content_b_snippet": c.content_b_snippet,
                            "classifier_score": c.classifier_score,
                            "classifier_method": c.classifier_method,
                            "similarity": c.similarity,
                            "created_at": c.created_at,
                        })
                    })
                    .collect();
                writeln!(
                    w,
                    "{}",
                    serde_json::to_string_pretty(&json).unwrap_or_default()
                )
            }
            OutputFormat::Raw => writeln!(w, "{:?}", resp),
        }
    }

    pub fn render_contradiction_commit_result(
        &self,
        resp: &pb::ContradictionCommitResponse,
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => {
                writeln!(
                    w,
                    "Contradictions confirmed: {}, Revisions created: {}, Dismissed: {}",
                    resp.contradictions_confirmed, resp.revisions_created, resp.dismissed
                )
            }
            OutputFormat::Json => {
                let json = serde_json::json!({
                    "contradictions_confirmed": resp.contradictions_confirmed,
                    "revisions_created": resp.revisions_created,
                    "dismissed": resp.dismissed,
                });
                writeln!(w, "{}", serde_json::to_string(&json).unwrap_or_default())
            }
            OutputFormat::Raw => writeln!(w, "{:?}", resp),
        }
    }

    pub fn render_subscribe_push(
        &self,
        push: &pb::SubscribePushMessage,
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => {
                if let Some(ref m) = push.memory {
                    let id = ulid_to_string(&m.memory_id);
                    let content = truncate_content(&m.content, 60);
                    if self.use_color {
                        writeln!(
                            w,
                            "[#{}] {} conf={:.2}  {}",
                            push.sequence_number,
                            id.cyan(),
                            push.confidence,
                            content
                        )
                    } else {
                        writeln!(
                            w,
                            "[#{}] {} conf={:.2}  {}",
                            push.sequence_number, id, push.confidence, content
                        )
                    }
                } else {
                    writeln!(w, "[#{}] (no memory data)", push.sequence_number)
                }
            }
            OutputFormat::Json => {
                let json = subscribe_push_to_json(push);
                writeln!(w, "{}", serde_json::to_string(&json).unwrap_or_default())
            }
            OutputFormat::Raw => writeln!(w, "{:?}", push),
        }
    }

    pub fn render_error(
        &self,
        err: &crate::error::CliError,
        w: &mut dyn Write,
    ) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Json => {
                let json = serde_json::json!({
                    "error": err.to_string(),
                    "code": err.exit_code(),
                });
                writeln!(w, "{}", serde_json::to_string(&json).unwrap_or_default())
            }
            _ => {
                if self.use_color {
                    writeln!(w, "{} {}", "Error:".red().bold(), err)
                } else {
                    writeln!(w, "Error: {}", err)
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Helper Types
// ═══════════════════════════════════════════════════════════════════════

#[derive(Serialize)]
struct StrategyDetailJson {
    strategy: String,
    relevance: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    distance: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rank: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    depth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embedding_similarity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    structural_similarity: Option<f32>,
}

fn proto_strategy_detail_to_json(d: &pb::StrategyDetailMessage) -> StrategyDetailJson {
    let strategy = match d.strategy_type {
        x if x == pb::RecallStrategyType::Similarity as i32 => "similarity",
        x if x == pb::RecallStrategyType::Temporal as i32 => "temporal",
        x if x == pb::RecallStrategyType::Causal as i32 => "causal",
        x if x == pb::RecallStrategyType::Analogical as i32 => "analogical",
        _ => "unknown",
    };
    StrategyDetailJson {
        strategy: strategy.to_string(),
        relevance: d.relevance,
        distance: d.distance,
        timestamp: d.timestamp,
        rank: d.rank,
        depth: d.depth,
        embedding_similarity: d.embedding_similarity,
        structural_similarity: d.structural_similarity,
    }
}

#[derive(Serialize)]
struct RecallResultJson {
    memory: MemoryJson,
    score: f32,
    relevance: f32,
    strategy_details: Vec<StrategyDetailJson>,
}

fn recall_result_to_json(r: &pb::RecallResult) -> RecallResultJson {
    let memory = r
        .memory
        .as_ref()
        .map(proto_memory_to_json)
        .unwrap_or_else(|| MemoryJson {
            memory_id: String::new(),
            content: String::new(),
            importance: 0.0,
            context: serde_json::Value::Null,
            entity_id: None,
            created_at: 0,
            updated_at: 0,
            last_accessed_at: 0,
            access_count: 0,
            decay_score: 0.0,
            kind: "unknown".to_string(),
            logical_clock: 0,
            embedding_dimensions: None,
        });
    let relevance = r
        .strategy_details
        .first()
        .map(|d| d.relevance)
        .unwrap_or(0.0);
    RecallResultJson {
        memory,
        score: r.score,
        relevance,
        strategy_details: r
            .strategy_details
            .iter()
            .map(proto_strategy_detail_to_json)
            .collect(),
    }
}

#[derive(Serialize)]
struct SubscribePushJson {
    subscription_id: u64,
    sequence_number: u64,
    confidence: f32,
    memory: Option<MemoryJson>,
}

fn subscribe_push_to_json(push: &pb::SubscribePushMessage) -> SubscribePushJson {
    SubscribePushJson {
        subscription_id: push.subscription_id,
        sequence_number: push.sequence_number,
        confidence: push.confidence,
        memory: push.memory.as_ref().map(proto_memory_to_json),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Utility Functions
// ═══════════════════════════════════════════════════════════════════════

fn truncate_content(s: &str, max_len: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() <= max_len {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    }
}

fn format_importance_colored(importance: f32) -> String {
    let formatted = format!("{:.2}", importance);
    if importance >= 0.8 {
        formatted.bold().to_string()
    } else if importance <= 0.2 {
        formatted.dimmed().to_string()
    } else {
        formatted
    }
}

fn format_timestamp_full(us: u64) -> String {
    if us == 0 {
        return "N/A".to_string();
    }
    format!(
        "{} ({})",
        format_absolute_time(us),
        format_relative_time(us)
    )
}

fn embedding_norm(embedding: &[f32]) -> f32 {
    embedding.iter().map(|x| x * x).sum::<f32>().sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulid_round_trip() {
        let original = [
            0x01, 0x8E, 0xA5, 0xF3, 0x21, 0x00, 0x7C, 0x8F, 0x9A, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
            0x00, 0x11,
        ];
        let s = ulid_to_string(&original);
        assert_eq!(s.len(), 26);
        let back = parse_memory_id(&s).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn hex_format_accepted() {
        let bytes = vec![
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10,
        ];
        let hex_str = hex::encode(&bytes);
        assert_eq!(hex_str.len(), 32);
        let parsed = parse_memory_id(&hex_str).unwrap();
        assert_eq!(parsed, bytes);
    }

    #[test]
    fn invalid_memory_id_rejected() {
        assert!(parse_memory_id("too_short").is_err());
        assert!(parse_memory_id("").is_err());
        assert!(parse_memory_id("not-a-valid-id-at-all-nope!!").is_err());
    }

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate_content("hello", 80), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let long = "a".repeat(100);
        let result = truncate_content(&long, 20);
        assert!(result.len() <= 20);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn format_elapsed_microseconds() {
        let d = std::time::Duration::from_micros(500);
        assert_eq!(format_elapsed(d), "500µs");
    }

    #[test]
    fn format_elapsed_milliseconds() {
        let d = std::time::Duration::from_micros(2500);
        assert_eq!(format_elapsed(d), "2.5ms");
    }

    #[test]
    fn format_elapsed_seconds() {
        let d = std::time::Duration::from_secs(3);
        assert_eq!(format_elapsed(d), "3.00s");
    }

    #[test]
    fn format_kind_values() {
        assert_eq!(format_kind(1), "episode");
        assert_eq!(format_kind(2), "insight");
        assert_eq!(format_kind(3), "revision");
        assert_eq!(format_kind(0), "unknown");
        assert_eq!(format_kind(99), "unknown");
    }

    #[test]
    fn relative_time_zero_is_never() {
        assert_eq!(format_relative_time(0), "never");
    }

    #[test]
    fn embedding_norm_calculation() {
        let emb = vec![3.0, 4.0];
        assert!((embedding_norm(&emb) - 5.0).abs() < 0.001);
    }

    #[test]
    fn parse_context_json_valid() {
        let ctx = parse_context_json(r#"{"key": "value", "num": 42}"#).unwrap();
        assert_eq!(ctx.fields.len(), 2);
    }

    #[test]
    fn parse_context_json_non_object_rejected() {
        assert!(parse_context_json(r#"[1,2,3]"#).is_err());
    }

    #[test]
    fn parse_context_json_invalid_json_rejected() {
        assert!(parse_context_json("not json").is_err());
    }

    #[test]
    fn parse_edge_with_confidence() {
        let ulid = ulid::Ulid::new();
        let id_str = ulid.to_string();
        let input = format!("{}:caused_by:0.85", id_str);
        let edge = parse_edge(&input).unwrap();
        assert_eq!(edge.target_id.len(), 16);
        assert_eq!(edge.edge_type, pb::EdgeType::CausedBy as i32);
        assert_eq!(edge.confidence, Some(0.85));
    }

    #[test]
    fn parse_edge_without_confidence() {
        let ulid = ulid::Ulid::new();
        let id_str = ulid.to_string();
        let input = format!("{}:related_to", id_str);
        let edge = parse_edge(&input).unwrap();
        assert_eq!(edge.edge_type, pb::EdgeType::RelatedTo as i32);
        assert!(edge.confidence.is_none());
    }

    #[test]
    fn parse_edge_all_types() {
        let ulid = ulid::Ulid::new();
        let id_str = ulid.to_string();
        for (name, expected) in [
            ("caused_by", pb::EdgeType::CausedBy),
            ("related_to", pb::EdgeType::RelatedTo),
            ("followed_by", pb::EdgeType::FollowedBy),
            ("revised_from", pb::EdgeType::RevisedFrom),
            ("insight_from", pb::EdgeType::InsightFrom),
            ("contradicts", pb::EdgeType::Contradicts),
        ] {
            let input = format!("{}:{}", id_str, name);
            let edge = parse_edge(&input).unwrap();
            assert_eq!(edge.edge_type, expected as i32);
        }
    }

    #[test]
    fn parse_edge_hex_id() {
        let bytes = vec![0u8; 16];
        let hex_str = hex::encode(&bytes);
        let input = format!("{}:followed_by:0.5", hex_str);
        let edge = parse_edge(&input).unwrap();
        assert_eq!(edge.target_id, bytes);
        assert_eq!(edge.edge_type, pb::EdgeType::FollowedBy as i32);
        assert_eq!(edge.confidence, Some(0.5));
    }

    #[test]
    fn parse_edge_invalid_no_colon() {
        assert!(parse_edge("just_an_id").is_err());
    }

    #[test]
    fn parse_edge_invalid_type() {
        let ulid = ulid::Ulid::new();
        let input = format!("{}:nonexistent_type", ulid);
        assert!(parse_edge(&input).is_err());
    }

    #[test]
    fn parse_edge_confidence_out_of_range() {
        let ulid = ulid::Ulid::new();
        let input = format!("{}:caused_by:1.5", ulid);
        assert!(parse_edge(&input).is_err());
    }

    #[test]
    fn parse_edge_confidence_negative() {
        let ulid = ulid::Ulid::new();
        let input = format!("{}:caused_by:-0.1", ulid);
        assert!(parse_edge(&input).is_err());
    }

    #[test]
    fn parse_edges_multiple() {
        let u1 = ulid::Ulid::new();
        let u2 = ulid::Ulid::new();
        let inputs = vec![
            format!("{}:caused_by:0.9", u1),
            format!("{}:related_to", u2),
        ];
        let edges = parse_edges(&inputs).unwrap();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].edge_type, pb::EdgeType::CausedBy as i32);
        assert_eq!(edges[1].edge_type, pb::EdgeType::RelatedTo as i32);
    }

    #[test]
    fn parse_edges_empty_vec() {
        let edges = parse_edges(&[]).unwrap();
        assert!(edges.is_empty());
    }

    #[test]
    fn format_edge_type_all_values() {
        assert_eq!(format_edge_type(pb::EdgeType::CausedBy as i32), "caused_by");
        assert_eq!(
            format_edge_type(pb::EdgeType::RelatedTo as i32),
            "related_to"
        );
        assert_eq!(
            format_edge_type(pb::EdgeType::FollowedBy as i32),
            "followed_by"
        );
        assert_eq!(
            format_edge_type(pb::EdgeType::RevisedFrom as i32),
            "revised_from"
        );
        assert_eq!(
            format_edge_type(pb::EdgeType::InsightFrom as i32),
            "insight_from"
        );
        assert_eq!(
            format_edge_type(pb::EdgeType::Contradicts as i32),
            "contradicts"
        );
        assert_eq!(format_edge_type(0), "unknown");
        assert_eq!(format_edge_type(99), "unknown");
    }

    #[test]
    fn days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-01 is day 19723
        let (y, m, d) = days_to_ymd(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }
}
