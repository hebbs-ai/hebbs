use std::collections::HashMap;

use crate::cluster::{cluster_embeddings, ClusterConfig};
use crate::error::{ReflectError, Result};
use crate::llm::{LlmProvider, LlmRequest, ResponseFormat};
use crate::prompt::{build_proposal_prompt, build_validation_prompt};
use crate::types::*;

/// The four-stage reflection pipeline.
///
/// Stateless: accepts a set of memories and produces insights.
/// Does NOT access storage, indexes, or the Engine.
pub struct ReflectPipeline;

impl ReflectPipeline {
    /// Run the full four-stage pipeline.
    ///
    /// Stage 1: Cluster memories by embedding similarity.
    /// Stage 2: For each cluster, propose candidate insights via LLM.
    /// Stage 3: Validate candidates against source memories + existing insights.
    /// Stage 4: Return validated insights ready for consolidation.
    pub fn run(
        input: ReflectInput,
        proposal_provider: &dyn LlmProvider,
        validation_provider: &dyn LlmProvider,
    ) -> Result<ReflectOutput> {
        if input.memories.is_empty() {
            return Ok(ReflectOutput {
                insights: Vec::new(),
                clusters: Vec::new(),
            });
        }

        // ── Stage 1: Clustering ──────────────────────────────────────
        // Cluster in associative space (relational roles); fall back to content embedding for
        // legacy memories that have no assoc_embedding yet.
        let embeddings: Vec<Vec<f32>> = input
            .memories
            .iter()
            .map(|m| {
                if m.assoc_embedding.is_empty() {
                    m.embedding.clone()
                } else {
                    m.assoc_embedding.clone()
                }
            })
            .collect();

        let cluster_config = ClusterConfig {
            min_cluster_size: input.config.min_cluster_size,
            max_clusters: input.config.max_clusters,
            seed: input.config.clustering_seed,
            max_iterations: input.config.max_iterations,
            silhouette_subsample: 500,
        };

        let raw_clusters = match cluster_embeddings(&embeddings, &cluster_config) {
            Ok(c) => c,
            Err(ReflectError::InsufficientMemories { have, need }) => {
                return Err(ReflectError::InsufficientMemories { have, need });
            }
            Err(e) => {
                return Err(ReflectError::Pipeline {
                    stage: "clustering".into(),
                    message: format!("{e}"),
                });
            }
        };

        let mut all_insights: Vec<ProducedInsight> = Vec::new();
        let mut cluster_infos: Vec<ClusterInfo> = Vec::new();

        // ── Stages 2-3: Per-cluster proposal + validation ────────────
        for cluster in &raw_clusters {
            let cluster_memories: Vec<&MemoryEntry> = cluster
                .member_indices
                .iter()
                .map(|&i| &input.memories[i])
                .collect();

            let info = match process_cluster(
                cluster,
                &cluster_memories,
                &input.existing_insights,
                &input.config,
                proposal_provider,
                validation_provider,
            ) {
                Ok((insights, status)) => {
                    let count = insights.len();
                    all_insights.extend(insights);
                    ClusterInfo {
                        cluster_id: cluster.cluster_id,
                        member_count: cluster.member_indices.len(),
                        centroid: cluster.centroid.clone(),
                        status: if count > 0 {
                            ClusterStatus::Success {
                                insight_count: count,
                            }
                        } else {
                            status
                        },
                    }
                }
                Err(e) => ClusterInfo {
                    cluster_id: cluster.cluster_id,
                    member_count: cluster.member_indices.len(),
                    centroid: cluster.centroid.clone(),
                    status: ClusterStatus::Failed {
                        error: format!("{e}"),
                    },
                },
            };
            cluster_infos.push(info);
        }

        Ok(ReflectOutput {
            insights: all_insights,
            clusters: cluster_infos,
        })
    }
}

/// Process a single cluster through Stages 2 and 3.
fn process_cluster(
    cluster: &Cluster,
    cluster_memories: &[&MemoryEntry],
    existing_insights: &[MemoryEntry],
    config: &PipelineConfig,
    proposal_provider: &dyn LlmProvider,
    validation_provider: &dyn LlmProvider,
) -> Result<(Vec<ProducedInsight>, ClusterStatus)> {
    // ── Stage 2: Proposal ────────────────────────────────────────
    let memory_ids_hex: Vec<String> = cluster_memories.iter().map(|m| hex::encode(m.id)).collect();

    let first_words: String = cluster_memories
        .first()
        .map(|m| {
            m.content
                .split_whitespace()
                .take(5)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    let (sys_prompt, user_prompt) = build_proposal_prompt(
        cluster_memories,
        &cluster.centroid,
        config.proposal_max_tokens,
    );

    let mut metadata = HashMap::new();
    metadata.insert("stage".into(), "proposal".into());
    metadata.insert("memory_ids".into(), memory_ids_hex.join(","));
    metadata.insert("cluster_topic".into(), first_words);

    let proposal_resp = proposal_provider.complete(LlmRequest {
        system_message: sys_prompt,
        user_message: user_prompt,
        max_tokens: config.proposal_max_tokens,
        temperature: 0.3,
        response_format: ResponseFormat::Json,
        metadata,
    })?;

    let candidates = parse_proposal_response(&proposal_resp.content)?;
    if candidates.is_empty() {
        return Ok((Vec::new(), ClusterStatus::NoInsights));
    }

    // ── Stage 3: Validation ──────────────────────────────────────
    let source_refs: Vec<&MemoryEntry> = cluster_memories.to_vec();
    let existing_refs: Vec<&MemoryEntry> = existing_insights.iter().collect();

    let (val_sys, val_user) = build_validation_prompt(
        &candidates,
        &source_refs,
        &existing_refs,
        config.validation_max_tokens,
    );

    let mut val_metadata = HashMap::new();
    val_metadata.insert("stage".into(), "validation".into());
    val_metadata.insert("candidate_count".into(), candidates.len().to_string());

    let val_resp = validation_provider.complete(LlmRequest {
        system_message: val_sys,
        user_message: val_user,
        max_tokens: config.validation_max_tokens,
        temperature: 0.1,
        response_format: ResponseFormat::Json,
        metadata: val_metadata,
    })?;

    let validated = parse_validation_response(&val_resp.content)?;

    // ── Stage 4: Build ProducedInsights from accepted/revised ────
    let valid_ids: std::collections::HashSet<[u8; 16]> =
        cluster_memories.iter().map(|m| m.id).collect();

    let mean_importance: f32 = if cluster_memories.is_empty() {
        0.5
    } else {
        cluster_memories.iter().map(|m| m.importance).sum::<f32>() / cluster_memories.len() as f32
    };

    let mut produced = Vec::new();
    for entry in &validated {
        if entry.candidate_index >= candidates.len() {
            continue;
        }
        let candidate = &candidates[entry.candidate_index];

        let (content, confidence) = match &entry.verdict {
            InsightVerdict::Accepted => (candidate.content.clone(), entry.confidence),
            InsightVerdict::Revised { revised_content } => {
                (revised_content.clone(), entry.confidence)
            }
            InsightVerdict::Rejected { .. } | InsightVerdict::MergedWithExisting { .. } => {
                continue;
            }
        };

        let source_ids: Vec<[u8; 16]> = candidate
            .source_memory_ids
            .iter()
            .filter_map(|hex_id| {
                let bytes = hex::decode(hex_id).ok()?;
                if bytes.len() != 16 {
                    return None;
                }
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&bytes);
                if valid_ids.contains(&arr) {
                    Some(arr)
                } else {
                    None
                }
            })
            .collect();

        let final_source_ids = if source_ids.is_empty() {
            cluster_memories.iter().map(|m| m.id).collect()
        } else {
            source_ids
        };

        produced.push(ProducedInsight {
            content,
            confidence: compute_insight_importance(
                mean_importance,
                confidence,
                final_source_ids.len(),
                config.insight_importance_weight,
            ),
            source_memory_ids: final_source_ids,
            tags: candidate.tags.clone(),
            cluster_id: cluster.cluster_id,
        });
    }

    let status = if produced.is_empty() {
        ClusterStatus::NoInsights
    } else {
        ClusterStatus::Success {
            insight_count: produced.len(),
        }
    };

    Ok((produced, status))
}

/// Compute insight importance from source memory importance, LLM confidence,
/// and source count. Larger clusters (more memories converging on the same
/// insight) get a logarithmic boost — diminishing returns so a cluster of 100
/// isn't 10x more important than a cluster of 10.
fn compute_insight_importance(
    mean_source_importance: f32,
    llm_confidence: f32,
    source_count: usize,
    weight: f32,
) -> f32 {
    let base = weight * mean_source_importance + (1.0 - weight) * llm_confidence;
    // ln(1 + n) / 10 gives: 1 source → 0.069, 3 → 0.139, 10 → 0.240, 50 → 0.392
    let freq_boost = (1.0 + source_count as f32).ln() / 10.0;
    (base + freq_boost).clamp(0.0, 1.0)
}

fn parse_proposal_response(content: &str) -> Result<Vec<CandidateInsight>> {
    let trimmed = content.trim();
    let parsed: ProposalResponse =
        serde_json::from_str(trimmed).map_err(|e| ReflectError::ResponseParse {
            message: format!("failed to parse proposal response: {e}\nraw: {trimmed}"),
        })?;
    Ok(parsed.insights)
}

fn parse_validation_response(content: &str) -> Result<Vec<ValidatedInsightEntry>> {
    let trimmed = content.trim();
    let parsed: ValidationResponse =
        serde_json::from_str(trimmed).map_err(|e| ReflectError::ResponseParse {
            message: format!("failed to parse validation response: {e}\nraw: {trimmed}"),
        })?;
    Ok(parsed.results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmProvider;

    fn make_memories(n: usize, d: usize) -> Vec<MemoryEntry> {
        use rand::prelude::*;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        (0..n)
            .map(|i| {
                let mut emb: Vec<f32> = (0..d).map(|_| rng.gen::<f32>() - 0.5).collect();
                let norm: f64 = emb
                    .iter()
                    .map(|&x| x as f64 * x as f64)
                    .sum::<f64>()
                    .sqrt()
                    .max(1e-12);
                for v in &mut emb {
                    *v = (*v as f64 / norm) as f32;
                }
                let mut id = [0u8; 16];
                id[0] = (i >> 8) as u8;
                id[1] = i as u8;
                MemoryEntry {
                    id,
                    content: format!("Memory about topic {} with detail {}", i % 3, i),
                    importance: 0.5 + (i % 5) as f32 * 0.1,
                    entity_id: Some("test_entity".into()),
                    embedding: emb.clone(),
                    created_at: 1_000_000 * i as u64,
                    assoc_embedding: emb,
                }
            })
            .collect()
    }

    fn make_clustered_memories(
        cluster_count: usize,
        per_cluster: usize,
        d: usize,
    ) -> Vec<MemoryEntry> {
        use rand::prelude::*;
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut memories = Vec::new();
        let mut idx = 0u16;
        for c in 0..cluster_count {
            let mut center = vec![0.0f32; d];
            center[c % d] = 1.0;
            for _ in 0..per_cluster {
                let mut emb: Vec<f32> = center
                    .iter()
                    .map(|&x| x + (rng.gen::<f32>() - 0.5) * 0.3)
                    .collect();
                let norm: f64 = emb
                    .iter()
                    .map(|&x| x as f64 * x as f64)
                    .sum::<f64>()
                    .sqrt()
                    .max(1e-12);
                for v in &mut emb {
                    *v = (*v as f64 / norm) as f32;
                }
                let mut id = [0u8; 16];
                id[0] = (idx >> 8) as u8;
                id[1] = idx as u8;
                memories.push(MemoryEntry {
                    id,
                    content: format!("Cluster {c} memory about topic {}", idx),
                    importance: 0.6,
                    entity_id: Some("entity_a".into()),
                    embedding: emb.clone(),
                    created_at: 1_000_000 * idx as u64,
                    assoc_embedding: emb,
                });
                idx += 1;
            }
        }
        memories
    }

    #[test]
    fn pipeline_produces_insights_from_clustered_data() {
        let memories = make_clustered_memories(3, 10, 16);
        let input = ReflectInput {
            memories,
            existing_insights: Vec::new(),
            config: PipelineConfig {
                min_cluster_size: 3,
                max_clusters: 10,
                clustering_seed: 42,
                max_iterations: 30,
                proposal_max_tokens: 4000,
                validation_max_tokens: 6000,
                insight_importance_weight: 0.7,
            },
        };
        let mock = MockLlmProvider::new();
        let output = ReflectPipeline::run(input, &mock, &mock).unwrap();

        assert!(
            !output.insights.is_empty(),
            "pipeline should produce insights"
        );
        assert!(
            !output.clusters.is_empty(),
            "pipeline should produce clusters"
        );

        for insight in &output.insights {
            assert!(!insight.content.is_empty());
            assert!(insight.confidence > 0.0 && insight.confidence <= 1.0);
            assert!(!insight.source_memory_ids.is_empty());
        }
    }

    #[test]
    fn pipeline_handles_empty_input() {
        let input = ReflectInput {
            memories: Vec::new(),
            existing_insights: Vec::new(),
            config: PipelineConfig::default(),
        };
        let mock = MockLlmProvider::new();
        let output = ReflectPipeline::run(input, &mock, &mock).unwrap();
        assert!(output.insights.is_empty());
        assert!(output.clusters.is_empty());
    }

    #[test]
    fn pipeline_validates_source_memory_ids() {
        let memories = make_clustered_memories(2, 8, 16);
        let input = ReflectInput {
            memories,
            existing_insights: Vec::new(),
            config: PipelineConfig {
                min_cluster_size: 3,
                ..PipelineConfig::default()
            },
        };
        let mock = MockLlmProvider::new();
        let output = ReflectPipeline::run(input, &mock, &mock).unwrap();

        for insight in &output.insights {
            assert!(
                !insight.source_memory_ids.is_empty(),
                "every insight must have source memory IDs"
            );
        }
    }

    #[test]
    fn pipeline_with_random_data() {
        let memories = make_memories(50, 16);
        let input = ReflectInput {
            memories,
            existing_insights: Vec::new(),
            config: PipelineConfig {
                min_cluster_size: 3,
                max_clusters: 10,
                ..PipelineConfig::default()
            },
        };
        let mock = MockLlmProvider::new();
        let output = ReflectPipeline::run(input, &mock, &mock).unwrap();
        assert!(!output.clusters.is_empty());
    }

    #[test]
    fn insight_importance_in_valid_range() {
        // With 1 source: freq_boost = ln(2)/10 ≈ 0.0693
        let base = 0.5 * 0.7 + 0.8 * 0.3;
        let boost_1 = (2.0_f32).ln() / 10.0;
        assert!((compute_insight_importance(0.5, 0.8, 1, 0.7) - (base + boost_1)).abs() < 1e-6);
        assert_eq!(compute_insight_importance(1.0, 1.0, 1, 0.7), 1.0); // clamped
                                                                       // With 0 sources: freq_boost = ln(1)/10 = 0
        assert_eq!(compute_insight_importance(0.0, 0.0, 0, 0.7), 0.0);
    }

    #[test]
    fn larger_cluster_boosts_importance() {
        let small = compute_insight_importance(0.5, 0.5, 3, 0.7);
        let large = compute_insight_importance(0.5, 0.5, 20, 0.7);
        assert!(
            large > small,
            "larger cluster should produce higher importance"
        );
    }

    #[test]
    fn parse_proposal_valid() {
        let json = r#"{"insights":[{"content":"test","confidence":0.8,"source_memory_ids":["aa"],"tags":["t"]}]}"#;
        let result = parse_proposal_response(json).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "test");
    }

    #[test]
    fn parse_proposal_empty() {
        let json = r#"{"insights":[]}"#;
        let result = parse_proposal_response(json).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_validation_accepted() {
        let json = r#"{"results":[{"candidate_index":0,"verdict":"accepted","confidence":0.9}]}"#;
        let result = parse_validation_response(json).unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].verdict, InsightVerdict::Accepted));
    }
}
