use std::io::{self, IsTerminal, Read, Write};
use std::time::Instant;

use hebbs_proto::generated as pb;
use tokio_stream::StreamExt;

use crate::cli::{Commands, ContextModeArg, KindArg, StrategyArg};
use crate::connection::ConnectionManager;
use crate::error::CliError;
use crate::format::{self, Renderer};

/// Execute a parsed command against the server.
/// Returns the exit code.
pub async fn execute(
    cmd: Commands,
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    http_port: u16,
    tenant_id: Option<&str>,
) -> i32 {
    let start = Instant::now();
    let mut stdout = io::stdout();

    let result = match cmd {
        Commands::Remember {
            content,
            importance,
            context,
            entity_id,
            edge,
        } => {
            execute_remember(
                conn,
                renderer,
                content,
                importance,
                context,
                entity_id,
                edge,
                tenant_id,
                &mut stdout,
            )
            .await
        }

        Commands::Get { id } => execute_get(conn, renderer, &id, tenant_id, &mut stdout).await,

        Commands::Recall {
            cue,
            strategy,
            top_k,
            entity_id,
            max_depth,
            seed,
            weights,
            ef_search,
            edge_types,
            time_range,
            analogical_alpha,
            context,
        } => {
            execute_recall(
                conn,
                renderer,
                cue,
                strategy,
                top_k,
                entity_id,
                max_depth,
                seed,
                weights,
                ef_search,
                edge_types,
                time_range,
                analogical_alpha,
                context,
                tenant_id,
                &mut stdout,
            )
            .await
        }

        Commands::Revise {
            id,
            content,
            importance,
            context,
            context_mode,
            entity_id,
            edge,
        } => {
            execute_revise(
                conn,
                renderer,
                &id,
                content,
                importance,
                context,
                context_mode,
                entity_id,
                edge,
                tenant_id,
                &mut stdout,
            )
            .await
        }

        Commands::Forget {
            ids,
            entity_id,
            staleness_us,
            access_floor,
            kind,
            decay_floor,
        } => {
            execute_forget(
                conn,
                renderer,
                ids,
                entity_id,
                staleness_us,
                access_floor,
                kind,
                decay_floor,
                tenant_id,
                &mut stdout,
            )
            .await
        }

        Commands::Prime {
            entity_id,
            context,
            max_memories,
            recency_us,
            similarity_cue,
        } => {
            execute_prime(
                conn,
                renderer,
                entity_id,
                context,
                max_memories,
                recency_us,
                similarity_cue,
                tenant_id,
                &mut stdout,
            )
            .await
        }

        Commands::Subscribe {
            entity_id,
            confidence,
        } => {
            execute_subscribe(
                conn,
                renderer,
                entity_id,
                confidence,
                tenant_id,
                &mut stdout,
            )
            .await
        }

        Commands::Feed {
            subscription_id,
            text,
        } => {
            execute_feed(
                conn,
                renderer,
                subscription_id,
                text,
                tenant_id,
                &mut stdout,
            )
            .await
        }

        Commands::Reflect {
            entity_id,
            since_us,
        } => execute_reflect(conn, renderer, entity_id, since_us, tenant_id, &mut stdout).await,

        Commands::ReflectPrepare {
            entity_id,
            since_us,
        } => {
            execute_reflect_prepare(conn, renderer, entity_id, since_us, tenant_id, &mut stdout)
                .await
        }

        Commands::ReflectCommit {
            session_id,
            insights,
        } => {
            execute_reflect_commit(
                conn,
                renderer,
                &session_id,
                &insights,
                tenant_id,
                &mut stdout,
            )
            .await
        }

        Commands::ContradictionPrepare => {
            execute_contradiction_prepare(conn, renderer, tenant_id, &mut stdout).await
        }

        Commands::ContradictionCommit { verdicts } => {
            execute_contradiction_commit(conn, renderer, &verdicts, tenant_id, &mut stdout).await
        }

        Commands::Insights {
            entity_id,
            min_confidence,
            max_results,
        } => {
            execute_insights(
                conn,
                renderer,
                entity_id,
                min_confidence,
                max_results,
                tenant_id,
                &mut stdout,
            )
            .await
        }

        Commands::Status => execute_status(conn, renderer, &mut stdout).await,

        Commands::Inspect { id } => {
            execute_inspect(conn, renderer, &id, tenant_id, &mut stdout).await
        }

        Commands::Export { entity_id, limit } => {
            execute_export(conn, renderer, entity_id, limit, tenant_id, &mut stdout).await
        }

        Commands::Metrics => execute_metrics(conn, renderer, http_port, &mut stdout).await,

        Commands::Version => {
            writeln!(
                stdout,
                "hebbs-cli {} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH
            )
            .ok();
            Ok(())
        }
    };

    let elapsed = start.elapsed();
    match result {
        Ok(()) => {
            let stderr = io::stderr();
            let is_pipe = !io::stdout().is_terminal();
            let mut timing_out: Box<dyn Write> = if is_pipe {
                Box::new(stderr)
            } else {
                Box::new(&mut stdout)
            };
            if renderer.format == crate::config::OutputFormat::Human {
                writeln!(timing_out, "\n({})", format::format_elapsed(elapsed)).ok();
            }
            0
        }
        Err(e) => {
            let code = e.exit_code();
            let _ = renderer.render_error(&e, &mut io::stderr());
            if let CliError::ServerUnavailable { .. } | CliError::ConnectionFailed { .. } = &e {
                conn.mark_disconnected();
            }
            code
        }
    }
}

/// Read content from stdin when it's a pipe and no content was provided.
fn read_stdin_content(content: Option<String>) -> Result<String, CliError> {
    if let Some(c) = content {
        if c.is_empty() {
            return Err(CliError::InvalidArgument {
                message: "Content cannot be empty".to_string(),
            });
        }
        return Ok(c);
    }

    if !io::stdin().is_terminal() {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::Internal {
                message: format!("Failed to read stdin: {}", e),
            })?;
        let trimmed = buf.trim_end().to_string();
        if trimmed.is_empty() {
            return Err(CliError::InvalidArgument {
                message: "Content cannot be empty (stdin was empty)".to_string(),
            });
        }
        return Ok(trimmed);
    }

    Err(CliError::InvalidArgument {
        message: "Content is required. Provide it as an argument or pipe via stdin.".to_string(),
    })
}

fn parse_scoring_weights(s: &str) -> Result<pb::ScoringWeights, CliError> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 4 {
        return Err(CliError::InvalidArgument {
            message: format!(
                "Weights must be 4 colon-separated floats (R:T:I:F), got {} parts: \"{}\"",
                parts.len(),
                s,
            ),
        });
    }
    let parse = |part: &str, name: &str| -> Result<f32, CliError> {
        part.parse::<f32>().map_err(|_| CliError::InvalidArgument {
            message: format!("Invalid {} weight \"{}\": must be a number", name, part),
        })
    };
    Ok(pb::ScoringWeights {
        w_relevance: parse(parts[0], "relevance")?,
        w_recency: parse(parts[1], "recency")?,
        w_importance: parse(parts[2], "importance")?,
        w_reinforcement: parse(parts[3], "reinforcement")?,
        max_age_us: 0,
        reinforcement_cap: 0,
    })
}

// ═══════════════════════════════════════════════════════════════════════
//  Command Implementations
// ═══════════════════════════════════════════════════════════════════════

#[allow(clippy::too_many_arguments)]
async fn execute_remember(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    content: Option<String>,
    importance: Option<f32>,
    context: Option<String>,
    entity_id: Option<String>,
    edge_specs: Vec<String>,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let content = read_stdin_content(content)?;

    let proto_context = match context {
        Some(ref c) => Some(
            format::parse_context_json(c).map_err(|e| CliError::InvalidArgument { message: e })?,
        ),
        None => None,
    };

    let edges =
        format::parse_edges(&edge_specs).map_err(|e| CliError::InvalidArgument { message: e })?;

    let req = pb::RememberRequest {
        content,
        importance,
        context: proto_context,
        entity_id,
        edges,
        tenant_id: tenant_id.map(str::to_string),
        kind: None,
    };

    let mut client = conn.memory_client().await?;
    let resp = client
        .remember(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    let inner = resp.into_inner();
    if let Some(ref m) = inner.memory {
        renderer
            .render_memory(m, w)
            .map_err(|e| CliError::Internal {
                message: e.to_string(),
            })?;
    }

    Ok(())
}

async fn execute_get(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    id: &str,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let memory_id =
        format::parse_memory_id(id).map_err(|e| CliError::InvalidArgument { message: e })?;

    let req = pb::GetRequest {
        memory_id,
        tenant_id: tenant_id.map(str::to_string),
    };
    let mut client = conn.memory_client().await?;
    let resp = client
        .get(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    let inner = resp.into_inner();
    if let Some(ref m) = inner.memory {
        renderer
            .render_memory_detail(m, w)
            .map_err(|e| CliError::Internal {
                message: e.to_string(),
            })?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_recall(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    cue: Option<String>,
    strategy: Option<StrategyArg>,
    top_k: u32,
    entity_id: Option<String>,
    max_depth: Option<u32>,
    seed: Option<String>,
    weights: Option<String>,
    ef_search: Option<u32>,
    edge_types: Option<Vec<String>>,
    time_range: Option<String>,
    analogical_alpha: Option<f32>,
    context: Option<String>,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let cue = cue.unwrap_or_default();

    let cue_context = match context {
        Some(ref c) => Some(
            format::parse_context_json(c).map_err(|e| CliError::InvalidArgument { message: e })?,
        ),
        None => None,
    };

    let strategy_type = match strategy {
        Some(StrategyArg::Similarity) => pb::RecallStrategyType::Similarity,
        Some(StrategyArg::Temporal) => pb::RecallStrategyType::Temporal,
        Some(StrategyArg::Causal) => pb::RecallStrategyType::Causal,
        Some(StrategyArg::Analogical) => pb::RecallStrategyType::Analogical,
        None => pb::RecallStrategyType::Similarity,
    };

    let seed_memory_id = match seed {
        Some(ref s) => {
            Some(format::parse_memory_id(s).map_err(|e| CliError::InvalidArgument { message: e })?)
        }
        None => None,
    };

    let scoring_weights = match weights {
        Some(ref w_str) => Some(parse_scoring_weights(w_str)?),
        None => None,
    };

    let parsed_edge_types: Vec<i32> = match edge_types {
        Some(ref types) => types
            .iter()
            .map(|t| match t.as_str() {
                "caused_by" => Ok(pb::EdgeType::CausedBy as i32),
                "followed_by" => Ok(pb::EdgeType::FollowedBy as i32),
                "related_to" => Ok(pb::EdgeType::RelatedTo as i32),
                "revised_from" => Ok(pb::EdgeType::RevisedFrom as i32),
                "insight_from" => Ok(pb::EdgeType::InsightFrom as i32),
                other => Err(CliError::InvalidArgument {
                    message: format!(
                        "Unknown edge type \"{}\". Valid types: caused_by, followed_by, related_to, revised_from, insight_from",
                        other,
                    ),
                }),
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };

    let parsed_time_range = match time_range {
        Some(ref tr) => {
            let parts: Vec<&str> = tr.split(':').collect();
            if parts.len() != 2 {
                return Err(CliError::InvalidArgument {
                    message: format!("Time range must be START_US:END_US, got \"{}\"", tr,),
                });
            }
            let start_us = parts[0]
                .parse::<u64>()
                .map_err(|_| CliError::InvalidArgument {
                    message: format!("Invalid start timestamp \"{}\": must be a u64", parts[0]),
                })?;
            let end_us = parts[1]
                .parse::<u64>()
                .map_err(|_| CliError::InvalidArgument {
                    message: format!("Invalid end timestamp \"{}\": must be a u64", parts[1]),
                })?;
            Some(pb::TimeRange { start_us, end_us })
        }
        None => None,
    };

    let strategy_config = pb::RecallStrategyConfig {
        strategy_type: strategy_type as i32,
        top_k: Some(top_k),
        ef_search,
        entity_id,
        time_range: parsed_time_range,
        seed_memory_id,
        edge_types: parsed_edge_types,
        max_depth,
        analogical_alpha,
    };

    let req = pb::RecallRequest {
        cue,
        strategies: vec![strategy_config],
        top_k: Some(top_k),
        scoring_weights,
        cue_context,
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.memory_client().await?;
    let resp = client
        .recall(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    let inner = resp.into_inner();
    renderer
        .render_recall_results(&inner.results, w)
        .map_err(|e| CliError::Internal {
            message: e.to_string(),
        })?;

    if !inner.strategy_errors.is_empty() {
        for err in &inner.strategy_errors {
            eprintln!("Strategy error: {}", err.message);
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_revise(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    id: &str,
    content: Option<String>,
    importance: Option<f32>,
    context: Option<String>,
    context_mode: ContextModeArg,
    entity_id: Option<String>,
    edge_specs: Vec<String>,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let memory_id =
        format::parse_memory_id(id).map_err(|e| CliError::InvalidArgument { message: e })?;

    let proto_context = match context {
        Some(ref c) => Some(
            format::parse_context_json(c).map_err(|e| CliError::InvalidArgument { message: e })?,
        ),
        None => None,
    };

    let cm = match context_mode {
        ContextModeArg::Merge => pb::ContextMode::Merge,
        ContextModeArg::Replace => pb::ContextMode::Replace,
    };

    let edges =
        format::parse_edges(&edge_specs).map_err(|e| CliError::InvalidArgument { message: e })?;

    let req = pb::ReviseRequest {
        memory_id,
        content,
        importance,
        context: proto_context,
        context_mode: cm as i32,
        entity_id,
        edges,
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.memory_client().await?;
    let resp = client
        .revise(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    let inner = resp.into_inner();
    if let Some(ref m) = inner.memory {
        renderer
            .render_memory(m, w)
            .map_err(|e| CliError::Internal {
                message: e.to_string(),
            })?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_forget(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    ids: Vec<String>,
    entity_id: Option<String>,
    staleness_us: Option<u64>,
    access_floor: Option<u64>,
    kind: Option<KindArg>,
    decay_floor: Option<f32>,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let memory_ids: Vec<Vec<u8>> = ids
        .iter()
        .map(|id| format::parse_memory_id(id))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CliError::InvalidArgument { message: e })?;

    if memory_ids.is_empty()
        && entity_id.is_none()
        && staleness_us.is_none()
        && access_floor.is_none()
        && kind.is_none()
        && decay_floor.is_none()
    {
        return Err(CliError::InvalidArgument {
            message: "At least one forget criteria is required (--ids, --entity-id, --staleness-us, etc.)".to_string(),
        });
    }

    let memory_kind = kind.map(|k| match k {
        KindArg::Episode => pb::MemoryKind::Episode as i32,
        KindArg::Insight => pb::MemoryKind::Insight as i32,
        KindArg::Revision => pb::MemoryKind::Revision as i32,
    });

    let req = pb::ForgetRequest {
        memory_ids,
        entity_id,
        staleness_threshold_us: staleness_us,
        access_count_floor: access_floor,
        memory_kind,
        decay_score_floor: decay_floor,
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.memory_client().await?;
    let resp = client
        .forget(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    renderer
        .render_forget_result(&resp.into_inner(), w)
        .map_err(|e| CliError::Internal {
            message: e.to_string(),
        })?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_prime(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    entity_id: String,
    context: Option<String>,
    max_memories: Option<u32>,
    recency_us: Option<u64>,
    similarity_cue: Option<String>,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let proto_context = match context {
        Some(ref c) => Some(
            format::parse_context_json(c).map_err(|e| CliError::InvalidArgument { message: e })?,
        ),
        None => None,
    };

    let req = pb::PrimeRequest {
        entity_id,
        context: proto_context,
        max_memories,
        recency_window_us: recency_us,
        similarity_cue,
        scoring_weights: None,
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.memory_client().await?;
    let resp = client
        .prime(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    let inner = resp.into_inner();
    renderer
        .render_recall_results(&inner.results, w)
        .map_err(|e| CliError::Internal {
            message: e.to_string(),
        })?;

    Ok(())
}

async fn execute_subscribe(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    entity_id: Option<String>,
    confidence: f32,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let req = pb::SubscribeRequest {
        entity_id,
        kind_filter: Vec::new(),
        confidence_threshold: confidence,
        time_scope_us: None,
        output_buffer_size: None,
        coarse_threshold: None,
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.subscribe_client().await?;
    let resp = client
        .subscribe(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    let mut stream = resp.into_inner();

    writeln!(
        w,
        "Subscribed. Waiting for matching memories (Ctrl-C to stop)..."
    )
    .ok();

    while let Some(result) = stream.next().await {
        match result {
            Ok(push) => {
                renderer
                    .render_subscribe_push(&push, w)
                    .map_err(|e| CliError::Internal {
                        message: e.to_string(),
                    })?;
            }
            Err(status) => {
                if status.code() == tonic::Code::Cancelled {
                    break;
                }
                return Err(CliError::from_status(status, conn.endpoint()));
            }
        }
    }

    Ok(())
}

async fn execute_feed(
    conn: &mut ConnectionManager,
    _renderer: &Renderer,
    subscription_id: u64,
    text: String,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let req = pb::FeedRequest {
        subscription_id,
        text,
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.subscribe_client().await?;
    client
        .feed(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    writeln!(w, "Text fed to subscription {}", subscription_id).ok();
    Ok(())
}

async fn execute_reflect(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    entity_id: Option<String>,
    since_us: Option<u64>,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let scope = match entity_id {
        Some(eid) => pb::ReflectScope {
            scope: Some(pb::reflect_scope::Scope::Entity(pb::EntityScope {
                entity_id: eid,
                since_us,
            })),
        },
        None => pb::ReflectScope {
            scope: Some(pb::reflect_scope::Scope::Global(pb::GlobalScope {
                since_us,
            })),
        },
    };

    let req = pb::ReflectRequest {
        scope: Some(scope),
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.reflect_client().await?;
    let resp = client
        .reflect(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    renderer
        .render_reflect_result(&resp.into_inner(), w)
        .map_err(|e| CliError::Internal {
            message: e.to_string(),
        })?;

    Ok(())
}

async fn execute_reflect_prepare(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    entity_id: Option<String>,
    since_us: Option<u64>,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let scope = match entity_id {
        Some(eid) => pb::ReflectScope {
            scope: Some(pb::reflect_scope::Scope::Entity(pb::EntityScope {
                entity_id: eid,
                since_us,
            })),
        },
        None => pb::ReflectScope {
            scope: Some(pb::reflect_scope::Scope::Global(pb::GlobalScope {
                since_us,
            })),
        },
    };

    let req = pb::ReflectPrepareRequest {
        scope: Some(scope),
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.reflect_client().await?;
    let resp = client
        .reflect_prepare(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    renderer
        .render_reflect_prepare_result(&resp.into_inner(), w)
        .map_err(|e| CliError::Internal {
            message: e.to_string(),
        })?;

    Ok(())
}

async fn execute_reflect_commit(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    session_id: &str,
    insights_json: &str,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(insights_json).map_err(|e| CliError::InvalidArgument {
            message: format!("invalid JSON for --insights: {e}"),
        })?;

    let insights: Vec<pb::ProducedInsightInput> = parsed
        .iter()
        .map(|v| {
            let content = v["content"].as_str().unwrap_or_default().to_string();
            let confidence = v["confidence"].as_f64().unwrap_or(0.8) as f32;
            let source_memory_ids: Vec<String> = v["source_memory_ids"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let tags: Vec<String> = v["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let cluster_id = v["cluster_id"].as_u64().map(|n| n as u32);

            pb::ProducedInsightInput {
                content,
                confidence,
                source_memory_ids,
                tags,
                cluster_id,
            }
        })
        .collect();

    let req = pb::ReflectCommitRequest {
        session_id: session_id.to_string(),
        insights,
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.reflect_client().await?;
    let resp = client
        .reflect_commit(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    renderer
        .render_reflect_commit_result(&resp.into_inner(), w)
        .map_err(|e| CliError::Internal {
            message: e.to_string(),
        })?;

    Ok(())
}

async fn execute_contradiction_prepare(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let req = pb::ContradictionPrepareRequest {
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.reflect_client().await?;
    let resp = client
        .contradiction_prepare(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    renderer
        .render_contradiction_prepare_result(&resp.into_inner(), w)
        .map_err(|e| CliError::Internal {
            message: e.to_string(),
        })?;

    Ok(())
}

async fn execute_contradiction_commit(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    verdicts_json: &str,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(verdicts_json).map_err(|e| CliError::InvalidArgument {
            message: format!("invalid JSON for --verdicts: {e}"),
        })?;

    let verdicts: Vec<pb::ContradictionVerdictInput> = parsed
        .iter()
        .map(|v| pb::ContradictionVerdictInput {
            pending_id: v["pending_id"].as_str().unwrap_or_default().to_string(),
            verdict: v["verdict"].as_str().unwrap_or("dismiss").to_string(),
            confidence: v["confidence"].as_f64().unwrap_or(0.8) as f32,
            reasoning: v["reasoning"].as_str().map(String::from),
        })
        .collect();

    let req = pb::ContradictionCommitRequest {
        verdicts,
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.reflect_client().await?;
    let resp = client
        .contradiction_commit(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    renderer
        .render_contradiction_commit_result(&resp.into_inner(), w)
        .map_err(|e| CliError::Internal {
            message: e.to_string(),
        })?;

    Ok(())
}

async fn execute_insights(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    entity_id: Option<String>,
    min_confidence: Option<f32>,
    max_results: Option<u32>,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let req = pb::GetInsightsRequest {
        entity_id,
        min_confidence,
        max_results,
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.reflect_client().await?;
    let resp = client
        .get_insights(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    let inner = resp.into_inner();
    if inner.insights.is_empty() {
        writeln!(w, "No insights found.").ok();
    } else {
        renderer
            .render_memories(&inner.insights, w)
            .map_err(|e| CliError::Internal {
                message: e.to_string(),
            })?;
    }

    Ok(())
}

async fn execute_status(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let req = pb::HealthCheckRequest {};
    let mut client = conn.health_client().await?;
    let resp = client
        .check(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    renderer
        .render_health(&resp.into_inner(), w)
        .map_err(|e| CliError::Internal {
            message: e.to_string(),
        })?;

    Ok(())
}

async fn execute_inspect(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    id: &str,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let memory_id =
        format::parse_memory_id(id).map_err(|e| CliError::InvalidArgument { message: e })?;

    // 1. Get the memory detail
    let get_req = pb::GetRequest {
        memory_id: memory_id.clone(),
        tenant_id: tenant_id.map(str::to_string),
    };
    let mut client = conn.memory_client().await?;
    let get_resp = client
        .get(get_req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    let inner = get_resp.into_inner();
    let memory = inner.memory.ok_or_else(|| CliError::NotFound {
        message: format!("Memory {} not found", id),
    })?;

    writeln!(w, "=== Memory Detail ===").ok();
    renderer
        .render_memory_detail(&memory, w)
        .map_err(|e| CliError::Internal {
            message: e.to_string(),
        })?;

    // 2. Graph edges (causal recall with depth 1)
    writeln!(w, "\n=== Graph Neighbors (depth 1) ===").ok();
    let causal_config = pb::RecallStrategyConfig {
        strategy_type: pb::RecallStrategyType::Causal as i32,
        top_k: Some(20),
        ef_search: None,
        entity_id: None,
        time_range: None,
        seed_memory_id: Some(memory_id.clone()),
        edge_types: Vec::new(),
        max_depth: Some(1),
        analogical_alpha: None,
    };
    let recall_req = pb::RecallRequest {
        cue: String::new(),
        strategies: vec![causal_config],
        top_k: Some(20),
        scoring_weights: None,
        cue_context: None,
        tenant_id: tenant_id.map(str::to_string),
    };

    let recall_resp = client.recall(recall_req).await;
    match recall_resp {
        Ok(resp) => {
            let recall_inner = resp.into_inner();
            if recall_inner.results.is_empty() {
                writeln!(w, "No graph neighbors found.").ok();
            } else {
                renderer
                    .render_recall_results(&recall_inner.results, w)
                    .map_err(|e| CliError::Internal {
                        message: e.to_string(),
                    })?;
            }
        }
        Err(_) => {
            writeln!(w, "(graph query not available)").ok();
        }
    }

    // 3. Nearest vector neighbors (similarity recall)
    if !memory.content.is_empty() {
        writeln!(w, "\n=== Nearest Vector Neighbors (top 5) ===").ok();
        let sim_config = pb::RecallStrategyConfig {
            strategy_type: pb::RecallStrategyType::Similarity as i32,
            top_k: Some(6),
            ef_search: None,
            entity_id: None,
            time_range: None,
            seed_memory_id: None,
            edge_types: Vec::new(),
            max_depth: None,
            analogical_alpha: None,
        };
        let sim_req = pb::RecallRequest {
            cue: memory.content.clone(),
            strategies: vec![sim_config],
            top_k: Some(6),
            scoring_weights: None,
            cue_context: None,
            tenant_id: tenant_id.map(str::to_string),
        };

        let sim_resp = client.recall(sim_req).await;
        match sim_resp {
            Ok(resp) => {
                let sim_inner = resp.into_inner();
                // Filter out the memory itself from results
                let filtered: Vec<pb::RecallResult> = sim_inner
                    .results
                    .into_iter()
                    .filter(|r| r.memory.as_ref().map_or(true, |m| m.memory_id != memory_id))
                    .take(5)
                    .collect();
                if filtered.is_empty() {
                    writeln!(w, "No vector neighbors found.").ok();
                } else {
                    renderer.render_recall_results(&filtered, w).map_err(|e| {
                        CliError::Internal {
                            message: e.to_string(),
                        }
                    })?;
                }
            }
            Err(_) => {
                writeln!(w, "(vector search not available)").ok();
            }
        }
    }

    Ok(())
}

async fn execute_export(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    entity_id: Option<String>,
    limit: u32,
    tenant_id: Option<&str>,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let capped_limit = limit.min(10000);

    let strategy_config = pb::RecallStrategyConfig {
        strategy_type: pb::RecallStrategyType::Temporal as i32,
        top_k: Some(capped_limit),
        ef_search: None,
        entity_id: entity_id.clone(),
        time_range: None,
        seed_memory_id: None,
        edge_types: Vec::new(),
        max_depth: None,
        analogical_alpha: None,
    };

    let req = pb::RecallRequest {
        cue: String::new(),
        strategies: vec![strategy_config],
        top_k: Some(capped_limit),
        scoring_weights: None,
        cue_context: None,
        tenant_id: tenant_id.map(str::to_string),
    };

    let mut client = conn.memory_client().await?;
    let resp = client
        .recall(req)
        .await
        .map_err(|s| CliError::from_status(s, conn.endpoint()))?;

    let inner = resp.into_inner();

    // Export always uses JSONL regardless of renderer format
    for result in &inner.results {
        if let Some(ref m) = result.memory {
            let json = format::proto_memory_to_json(m);
            let line = serde_json::to_string(&json).unwrap_or_default();
            writeln!(w, "{}", line).map_err(|e| CliError::Internal {
                message: e.to_string(),
            })?;
        }
    }

    let _ = renderer; // export always uses JSONL
    eprintln!(
        "Exported {} memories{}",
        inner.results.len(),
        if inner.results.len() as u32 >= capped_limit {
            " (limit reached)"
        } else {
            ""
        }
    );

    Ok(())
}

async fn execute_metrics(
    conn: &mut ConnectionManager,
    renderer: &Renderer,
    http_port: u16,
    w: &mut dyn Write,
) -> Result<(), CliError> {
    let endpoint = conn.endpoint().to_string();
    let host = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split(':')
        .next()
        .unwrap_or("localhost")
        .to_string();

    let http_url = format!("http://{}:{}/v1/metrics", host, http_port);
    writeln!(w, "Fetching metrics from {}...", http_url).ok();

    let host_owned = host.clone();
    let resp = tokio::task::spawn_blocking(move || {
        use std::io::{BufRead, BufReader};
        use std::net::{TcpStream, ToSocketAddrs};

        let addr_str = format!("{}:{}", host_owned, http_port);
        let addrs: Vec<_> = addr_str
            .to_socket_addrs()
            .map_err(|e| format!("DNS resolution failed for {}: {}", addr_str, e))?
            .collect();
        if addrs.is_empty() {
            return Err(format!("No addresses resolved for {}", addr_str));
        }
        let timeout = std::time::Duration::from_secs(5);
        let mut last_err = String::new();
        let mut stream_opt: Option<TcpStream> = None;
        for addr in &addrs {
            match TcpStream::connect_timeout(addr, timeout) {
                Ok(s) => {
                    stream_opt = Some(s);
                    break;
                }
                Err(e) => {
                    last_err = format!("{}: {}", addr, e);
                }
            }
        }
        let stream = stream_opt.ok_or_else(|| format!("Connection failed: {}", last_err))?;

        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();

        let request = format!(
            "GET /v1/metrics HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
            host_owned, http_port
        );
        use std::io::Write as _;
        (&stream)
            .write_all(request.as_bytes())
            .map_err(|e| format!("Write failed: {}", e))?;

        let reader = BufReader::new(&stream);
        let mut body = String::new();
        let mut in_body = false;
        for line in reader.lines() {
            let line = line.map_err(|e| format!("Read error: {}", e))?;
            if in_body {
                body.push_str(&line);
                body.push('\n');
            } else if line.is_empty() {
                in_body = true;
            }
        }
        Ok::<String, String>(body)
    })
    .await
    .map_err(|e| CliError::Internal {
        message: format!("Task error: {}", e),
    })?;

    match resp {
        Ok(body) => {
            match renderer.format {
                crate::config::OutputFormat::Json => {
                    writeln!(w, "{}", serde_json::json!({"metrics": body})).ok();
                }
                _ => {
                    // Parse and display key metrics in human-readable form
                    render_metrics_human(&body, w);
                }
            }
            Ok(())
        }
        Err(e) => Err(CliError::Internal {
            message: format!(
                "Cannot reach HTTP metrics endpoint. Use --http-port to specify the server's HTTP port. ({})",
                e
            ),
        }),
    }
}

fn render_metrics_human(body: &str, w: &mut dyn Write) {
    let mut interesting_lines: Vec<&str> = Vec::new();
    for line in body.lines() {
        if line.starts_with('#') {
            continue;
        }
        if line.contains("hebbs_") {
            interesting_lines.push(line);
        }
    }

    if interesting_lines.is_empty() {
        writeln!(w, "No HEBBS metrics found.").ok();
    } else {
        for line in interesting_lines {
            writeln!(w, "  {}", line).ok();
        }
    }
}
