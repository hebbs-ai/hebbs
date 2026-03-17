//! Contradiction detection pipeline.
//!
//! Two modes, auto-selected based on config:
//! - **LLM mode**: High-quality entailment classification via LLM provider.
//! - **Heuristic mode**: Embedding similarity + lexical signals, no external calls.
//!
//! ## Complexity
//! Per new memory: O(log n) HNSW search + O(K) classification calls.
//! K is bounded by `candidates_k` (default 10).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hebbs_index::graph::{EdgeMetadata, EdgeType, GraphIndex};
use hebbs_index::IndexManager;
use hebbs_storage::{BatchOperation, ColumnFamilyName, StorageBackend};
use serde::{Deserialize, Serialize};

use crate::error::{HebbsError, Result};
use crate::keys;
use crate::memory::Memory;
use crate::tenant::TenantContext;

// ── Types ─────────────────────────────────────────────────────────────

/// Result of classifying the relationship between two memories.
#[derive(Debug, Clone, PartialEq)]
pub enum EntailmentResult {
    /// The two memories assert opposing or incompatible facts.
    Contradiction { confidence: f32 },
    /// Memory B updates or supersedes memory A (evolution of thinking).
    Revision { confidence: f32 },
    /// The memories are compatible or unrelated.
    Neutral,
}

/// A detected contradiction between two memories.
#[derive(Debug, Clone)]
pub struct Contradiction {
    /// The source memory that was checked.
    pub memory_id_a: [u8; 16],
    /// The contradicting memory.
    pub memory_id_b: [u8; 16],
    /// Classification confidence [0.0, 1.0].
    pub confidence: f32,
    /// Which classifier produced this result.
    pub method: ClassifierMethod,
}

/// Which classifier produced the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClassifierMethod {
    Heuristic,
    Llm,
}

/// Configuration for contradiction detection.
#[derive(Debug, Clone)]
pub struct ContradictionConfig {
    /// Maximum neighbors to check per memory. Default: 10.
    pub candidates_k: usize,
    /// Minimum similarity to consider a pair. Default: 0.7.
    pub min_similarity: f32,
    /// Minimum confidence to create a CONTRADICTS edge. Default: 0.7.
    pub min_confidence: f32,
    /// Whether contradiction detection is enabled. Default: true.
    pub enabled: bool,
}

impl Default for ContradictionConfig {
    fn default() -> Self {
        Self {
            candidates_k: 10,
            min_similarity: 0.5,
            min_confidence: 0.35,
            enabled: true,
        }
    }
}

/// Output of a contradiction scan.
#[derive(Debug, Clone)]
pub struct ContradictionScanOutput {
    /// Number of candidate pairs evaluated.
    pub pairs_checked: usize,
    /// Contradictions found and stored as edges.
    pub contradictions: Vec<Contradiction>,
    /// Number of revisions detected (not stored as contradictions).
    pub revisions_detected: usize,
}

/// Output of `check_memory_contradictions`.
///
/// When an LLM provider is available, contradictions and revisions are resolved
/// immediately (edges created in the graph). The `resolved_contradictions` field
/// contains the confirmed contradictions for downstream use (e.g. file writing).
///
/// When no LLM provider is available, the heuristic classifier creates pending
/// records in the `Pending` CF for later AI review via `prepare_contradictions`
/// / `commit_contradictions`.
#[derive(Debug, Clone, Default)]
pub struct ContradictionCheckOutput {
    /// Pending contradiction candidates (heuristic path only).
    pub pending: Vec<PendingContradiction>,
    /// Contradictions resolved immediately by the LLM path.
    /// Each entry contains (memory_id_a, memory_id_b, confidence, content_a, content_b).
    pub resolved_contradictions: Vec<ResolvedContradiction>,
    /// Number of revisions resolved immediately by the LLM path.
    pub revisions_resolved: usize,
}

/// A contradiction resolved immediately by the LLM classifier.
#[derive(Debug, Clone)]
pub struct ResolvedContradiction {
    pub memory_id_a: [u8; 16],
    pub memory_id_b: [u8; 16],
    pub confidence: f32,
    pub content_a: String,
    pub content_b: String,
}

/// A pending contradiction candidate awaiting AI review.
///
/// Created during Phase 1 (detect) by either the heuristic or LLM classifier.
/// Stored in `ColumnFamilyName::Pending` with key prefix `ctr:`.
/// Consumed by `prepare_contradictions` / `commit_contradictions` in Phase 2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingContradiction {
    pub id: [u8; 16],
    pub memory_id_a: [u8; 16],
    pub memory_id_b: [u8; 16],
    pub content_a_snippet: String,
    pub content_b_snippet: String,
    /// Confidence score from the Phase 1 classifier (heuristic or LLM).
    pub classifier_score: f32,
    /// Which classifier produced this candidate.
    pub classifier_method: ClassifierMethod,
    pub similarity: f32,
    pub created_at: u64,
}

impl PendingContradiction {
    /// Serialize to JSON bytes for storage.
    ///
    /// Complexity: O(n) where n = serialized size.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| HebbsError::Serialization {
            message: format!("failed to serialize PendingContradiction: {}", e),
        })
    }

    /// Deserialize from JSON bytes.
    ///
    /// Complexity: O(n) where n = byte length.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(|e| HebbsError::Serialization {
            message: format!("failed to deserialize PendingContradiction: {}", e),
        })
    }
}

/// An AI reviewer's verdict on a pending contradiction candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContradictionVerdict {
    /// Hex-encoded pending ID.
    pub pending_id: String,
    /// One of "contradiction", "revision", "dismiss".
    pub verdict: String,
    /// AI confidence in the verdict [0.0, 1.0].
    pub confidence: f32,
    /// Optional reasoning from the AI reviewer.
    pub reasoning: Option<String>,
}

/// Result of committing contradiction verdicts.
pub struct ContradictionCommitResult {
    /// Number of candidates confirmed as contradictions.
    pub contradictions_confirmed: usize,
    /// Number of candidates classified as revisions.
    pub revisions_created: usize,
    /// Number of candidates dismissed.
    pub dismissed: usize,
    /// Details of confirmed contradictions for file writing.
    pub confirmed: Vec<Contradiction>,
}

// ── Heuristic Classifier ──────────────────────────────────────────────

/// Negation markers that indicate a statement negates a claim.
const NEGATION_MARKERS: &[&str] = &[
    "not",
    "no longer",
    "never",
    "failed",
    "missed",
    "stopped",
    "unable",
    "cannot",
    "can't",
    "didn't",
    "doesn't",
    "won't",
    "isn't",
    "aren't",
    "wasn't",
    "weren't",
    "shouldn't",
    "unreliable",
    "unsuccessful",
    "inadequate",
    "insufficient",
    "declined",
    "dropped",
    "decreased",
    "reduced",
    "lost",
];

/// Temporal/revision markers that suggest evolution rather than contradiction.
const REVISION_MARKERS: &[&str] = &[
    "used to",
    "previously",
    "updated",
    "changed",
    "now",
    "revised",
    "corrected",
    "no longer think",
    "reconsidered",
    "on second thought",
    "after further",
    "in retrospect",
];

/// Antonym pairs where presence of one in A and the other in B suggests contradiction.
const ANTONYM_PAIRS: &[(&str, &str)] = &[
    ("reliable", "unreliable"),
    ("success", "failure"),
    ("increase", "decrease"),
    ("improve", "worsen"),
    ("accept", "reject"),
    ("approve", "disapprove"),
    ("agree", "disagree"),
    ("correct", "incorrect"),
    ("efficient", "inefficient"),
    ("effective", "ineffective"),
    ("possible", "impossible"),
    ("complete", "incomplete"),
    ("consistent", "inconsistent"),
    ("stable", "unstable"),
    ("safe", "unsafe"),
    ("valid", "invalid"),
    ("available", "unavailable"),
    ("sufficient", "insufficient"),
    ("accurate", "inaccurate"),
    ("positive", "negative"),
    ("fast", "slow"),
    ("good", "bad"),
    ("high", "low"),
    ("strong", "weak"),
    ("easy", "difficult"),
];

/// Classify the relationship between two memory contents using heuristic signals.
///
/// Combines negation asymmetry, antonym detection, and numeric disagreement.
/// Confidence capped at 0.75 to reflect reduced accuracy vs LLM.
///
/// In two-phase mode, this serves as a candidate finder (high recall).
/// AI review via `contradiction prepare`/`contradiction commit` provides
/// the final classification.
///
/// Complexity: O(|content_a| + |content_b|) for tokenization + O(1) for signal checks.
pub fn heuristic_classify(content_a: &str, content_b: &str) -> EntailmentResult {
    let lower_a = content_a.to_lowercase();
    let lower_b = content_b.to_lowercase();

    // Check for revision markers first (trumps contradiction)
    let revision_score = revision_signal(&lower_a, &lower_b);
    if revision_score > 0.2 {
        return EntailmentResult::Revision {
            confidence: (revision_score * 0.75).min(0.75),
        };
    }

    let mut contradiction_score: f32 = 0.0;

    // Signal 1: Negation asymmetry
    let neg_a = negation_count(&lower_a);
    let neg_b = negation_count(&lower_b);
    if (neg_a == 0 && neg_b > 0) || (neg_a > 0 && neg_b == 0) {
        contradiction_score += 0.35;
    }

    // Signal 2: Antonym presence
    let antonym_hits = antonym_signal(&lower_a, &lower_b);
    contradiction_score += (antonym_hits as f32) * 0.25;

    // Signal 3: Numeric disagreement (same context, different numbers)
    if has_numeric_disagreement(&lower_a, &lower_b) {
        contradiction_score += 0.2;
    }

    // Cap at 0.75 for heuristic mode
    let confidence = contradiction_score.min(0.75);

    if confidence >= 0.35 {
        EntailmentResult::Contradiction { confidence }
    } else {
        EntailmentResult::Neutral
    }
}

/// Count negation markers in text.
fn negation_count(text: &str) -> usize {
    NEGATION_MARKERS
        .iter()
        .filter(|m| text.contains(**m))
        .count()
}

/// Check for revision markers in either text.
fn revision_signal(text_a: &str, text_b: &str) -> f32 {
    let count = REVISION_MARKERS
        .iter()
        .filter(|m| text_a.contains(**m) || text_b.contains(**m))
        .count();
    (count as f32) * 0.3
}

/// Count antonym pair hits between two texts.
fn antonym_signal(text_a: &str, text_b: &str) -> usize {
    ANTONYM_PAIRS
        .iter()
        .filter(|(pos, neg)| {
            (text_a.contains(pos) && text_b.contains(neg))
                || (text_a.contains(neg) && text_b.contains(pos))
        })
        .count()
}

/// Check for numeric disagreement: overlapping context words but different numbers.
fn has_numeric_disagreement(text_a: &str, text_b: &str) -> bool {
    let nums_a: Vec<&str> = text_a
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_ascii_digit()))
        .collect();
    let nums_b: Vec<&str> = text_b
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_ascii_digit()))
        .collect();

    if nums_a.is_empty() || nums_b.is_empty() {
        return false;
    }

    // Check if there are context words in common (suggesting same topic)
    let words_a: HashSet<&str> = text_a.split_whitespace().collect();
    let words_b: HashSet<&str> = text_b.split_whitespace().collect();
    let overlap = words_a.intersection(&words_b).count();

    if overlap < 3 {
        return false;
    }

    // Different numbers in similar context
    let num_set_a: HashSet<&str> = nums_a.into_iter().collect();
    let num_set_b: HashSet<&str> = nums_b.into_iter().collect();
    !num_set_a.is_empty() && !num_set_b.is_empty() && num_set_a.is_disjoint(&num_set_b)
}

// ── Pipeline ──────────────────────────────────────────────────────────

/// Check a single memory against its nearest neighbors for contradictions.
///
/// Two paths depending on whether an LLM provider is available:
///
/// **LLM path** (primary): Uses `hebbs_llm::contradiction::llm_classify_contradiction`
/// for high-quality classification. Contradiction and Revision verdicts create graph
/// edges immediately (no pending queue). Dismiss verdicts are skipped.
///
/// **Heuristic path** (fallback): When no LLM provider is given, uses the heuristic
/// classifier and writes `PendingContradiction` records to `ColumnFamilyName::Pending`
/// for later AI review via `prepare_contradictions` / `commit_contradictions`.
///
/// Both paths share:
/// 1. HNSW search for top-K similar memories -- O(log n)
/// 2. Filter out pairs that already have CONTRADICTS or REVISED_FROM edges
/// 3. Classify each candidate pair -- O(K) calls
///
/// Returns a `ContradictionCheckOutput` containing pending records (heuristic path)
/// or resolved contradictions (LLM path).
pub fn check_memory_contradictions(
    memory_id: &[u8; 16],
    storage: Arc<dyn StorageBackend>,
    index_manager: &IndexManager,
    tenant: &TenantContext,
    config: &ContradictionConfig,
    llm_provider: Option<&dyn hebbs_reflect::LlmProvider>,
) -> Result<ContradictionCheckOutput> {
    if !config.enabled {
        return Ok(ContradictionCheckOutput::default());
    }

    // Load the memory
    let memory = load_memory(storage.as_ref(), memory_id)?;
    let content_a = &memory.content;

    if content_a.trim().is_empty() {
        return Ok(ContradictionCheckOutput::default());
    }

    // Get embedding for HNSW search
    let embedding = match &memory.embedding {
        Some(e) => e,
        None => return Ok(ContradictionCheckOutput::default()),
    };

    // Find top-K similar memories -- O(log n)
    let candidates = index_manager.search_vector_for_tenant(
        tenant.tenant_id(),
        embedding,
        config.candidates_k,
        Some(64),
    )?;

    // Load existing edges to skip already-classified pairs -- O(K)
    let graph = GraphIndex::new(storage.clone());
    let existing_edges = graph.outgoing_edges(memory_id).unwrap_or_default();
    let existing_targets: HashSet<[u8; 16]> = existing_edges
        .iter()
        .filter(|(et, _, _)| *et == EdgeType::Contradicts || *et == EdgeType::RevisedFrom)
        .map(|(_, target, _)| *target)
        .collect();

    let now_us = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    let mut output = ContradictionCheckOutput::default();
    let mut batch_ops = Vec::new();

    for (candidate_id, distance) in &candidates {
        let similarity = 1.0 - distance.min(2.0) / 2.0;
        if similarity < config.min_similarity {
            continue;
        }

        // Skip self
        if candidate_id == memory_id {
            continue;
        }

        // Skip already-classified pairs
        if existing_targets.contains(candidate_id) {
            continue;
        }

        // Load candidate memory
        let candidate = match load_memory(storage.as_ref(), candidate_id) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if candidate.content.trim().is_empty() {
            continue;
        }

        if let Some(provider) = llm_provider {
            // LLM path: classify with hebbs_llm and create edges immediately
            let classification = match hebbs_llm::contradiction::llm_classify_contradiction(
                provider,
                content_a,
                &candidate.content,
            ) {
                Ok(c) => c,
                Err(_) => continue,
            };

            match classification.verdict {
                hebbs_llm::contradiction::ContradictionVerdict::Contradiction => {
                    if classification.confidence < config.min_confidence {
                        continue;
                    }

                    let metadata = EdgeMetadata::new(classification.confidence, now_us);
                    let meta_bytes = metadata.to_bytes();

                    // Bidirectional CONTRADICTS edges: A->B and B->A
                    let fwd_ab = GraphIndex::encode_forward_key(
                        memory_id,
                        EdgeType::Contradicts,
                        candidate_id,
                    );
                    let rev_ab = GraphIndex::encode_reverse_key(
                        memory_id,
                        EdgeType::Contradicts,
                        candidate_id,
                    );
                    let fwd_ba = GraphIndex::encode_forward_key(
                        candidate_id,
                        EdgeType::Contradicts,
                        memory_id,
                    );
                    let rev_ba = GraphIndex::encode_reverse_key(
                        candidate_id,
                        EdgeType::Contradicts,
                        memory_id,
                    );

                    batch_ops.push(BatchOperation::Put {
                        cf: ColumnFamilyName::Graph,
                        key: fwd_ab,
                        value: meta_bytes.clone(),
                    });
                    batch_ops.push(BatchOperation::Put {
                        cf: ColumnFamilyName::Graph,
                        key: rev_ab,
                        value: meta_bytes.clone(),
                    });
                    batch_ops.push(BatchOperation::Put {
                        cf: ColumnFamilyName::Graph,
                        key: fwd_ba,
                        value: meta_bytes.clone(),
                    });
                    batch_ops.push(BatchOperation::Put {
                        cf: ColumnFamilyName::Graph,
                        key: rev_ba,
                        value: meta_bytes,
                    });

                    output.resolved_contradictions.push(ResolvedContradiction {
                        memory_id_a: *memory_id,
                        memory_id_b: *candidate_id,
                        confidence: classification.confidence,
                        content_a: content_a.clone(),
                        content_b: candidate.content.clone(),
                    });
                }
                hebbs_llm::contradiction::ContradictionVerdict::Revision => {
                    if classification.confidence < config.min_confidence {
                        continue;
                    }

                    let metadata = EdgeMetadata::new(classification.confidence, now_us);
                    let meta_bytes = metadata.to_bytes();

                    // REVISED_FROM edge: B revised from A (B supersedes A)
                    let fwd_key = GraphIndex::encode_forward_key(
                        candidate_id,
                        EdgeType::RevisedFrom,
                        memory_id,
                    );
                    let rev_key = GraphIndex::encode_reverse_key(
                        candidate_id,
                        EdgeType::RevisedFrom,
                        memory_id,
                    );

                    batch_ops.push(BatchOperation::Put {
                        cf: ColumnFamilyName::Graph,
                        key: fwd_key,
                        value: meta_bytes.clone(),
                    });
                    batch_ops.push(BatchOperation::Put {
                        cf: ColumnFamilyName::Graph,
                        key: rev_key,
                        value: meta_bytes,
                    });

                    output.revisions_resolved += 1;
                }
                hebbs_llm::contradiction::ContradictionVerdict::Dismiss => {
                    // Skip dismissed pairs
                }
            }
        } else {
            // Heuristic fallback: classify and create pending records
            let result = heuristic_classify(content_a, &candidate.content);

            match result {
                EntailmentResult::Contradiction { confidence }
                    if confidence >= config.min_confidence =>
                {
                    let pending_id = ulid::Ulid::new().to_bytes();
                    let snippet_a = truncate_snippet(content_a, 200);
                    let snippet_b = truncate_snippet(&candidate.content, 200);

                    let record = PendingContradiction {
                        id: pending_id,
                        memory_id_a: *memory_id,
                        memory_id_b: *candidate_id,
                        content_a_snippet: snippet_a,
                        content_b_snippet: snippet_b,
                        classifier_score: confidence,
                        classifier_method: ClassifierMethod::Heuristic,
                        similarity,
                        created_at: now_us,
                    };

                    let key = keys::encode_pending_contradiction_key(&pending_id);
                    let value = record.to_bytes()?;

                    batch_ops.push(BatchOperation::Put {
                        cf: ColumnFamilyName::Pending,
                        key,
                        value,
                    });

                    output.pending.push(record);
                }
                _ => {}
            }
        }
    }

    // Write all graph edges or pending records atomically
    if !batch_ops.is_empty() {
        storage.write_batch(&batch_ops)?;
    }

    Ok(output)
}

// ── Two-Phase Commit API ──────────────────────────────────────────────

/// Phase 2a: retrieve all pending contradiction candidates for AI review.
///
/// Prefix-scans `ColumnFamilyName::Pending` for keys starting with `ctr:`.
///
/// Complexity: O(log n) seek + O(k) scan where k = pending records.
pub fn prepare_contradictions(
    storage: Arc<dyn StorageBackend>,
) -> Result<Vec<PendingContradiction>> {
    let entries = storage.prefix_iterator(
        ColumnFamilyName::Pending,
        keys::PENDING_CONTRADICTION_PREFIX,
    )?;

    let mut results = Vec::with_capacity(entries.len());
    for (_key, value) in &entries {
        let record = PendingContradiction::from_bytes(value)?;
        results.push(record);
    }

    Ok(results)
}

/// Phase 2b: commit AI-reviewed verdicts, creating graph edges and
/// cleaning up pending records.
///
/// For verdict "contradiction": creates bidirectional CONTRADICTS edges.
/// For verdict "revision": creates REVISED_FROM edges (A revised from B).
/// For all verdicts: deletes the pending record from the Pending CF.
///
/// Complexity: O(k) where k = number of verdicts.
pub fn commit_contradictions(
    storage: Arc<dyn StorageBackend>,
    verdicts: &[ContradictionVerdict],
) -> Result<ContradictionCommitResult> {
    let now_us = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    let mut batch_ops = Vec::new();
    let mut contradictions_confirmed: usize = 0;
    let mut revisions_created: usize = 0;
    let mut dismissed: usize = 0;
    let mut confirmed = Vec::new();

    for verdict in verdicts {
        // Decode the hex pending ID
        let id_bytes = hex::decode(&verdict.pending_id).map_err(|e| HebbsError::Serialization {
            message: format!("invalid pending_id hex: {}", e),
        })?;
        if id_bytes.len() != 16 {
            return Err(HebbsError::Serialization {
                message: format!("pending_id must be 16 bytes, got {}", id_bytes.len()),
            });
        }
        let mut pending_id = [0u8; 16];
        pending_id.copy_from_slice(&id_bytes);

        // Load the pending record to get memory IDs
        let pending_key = keys::encode_pending_contradiction_key(&pending_id);
        let pending_bytes = storage
            .get(ColumnFamilyName::Pending, &pending_key)?
            .ok_or_else(|| HebbsError::Serialization {
                message: format!("pending contradiction {} not found", verdict.pending_id),
            })?;
        let record = PendingContradiction::from_bytes(&pending_bytes)?;

        // Delete the pending record regardless of verdict
        batch_ops.push(BatchOperation::Delete {
            cf: ColumnFamilyName::Pending,
            key: pending_key,
        });

        match verdict.verdict.as_str() {
            "contradiction" => {
                let metadata = EdgeMetadata::new(verdict.confidence, now_us);
                let meta_bytes = metadata.to_bytes();

                // Bidirectional CONTRADICTS edges: A->B and B->A
                // Forward and reverse index entries for each direction.
                let fwd_ab = GraphIndex::encode_forward_key(
                    &record.memory_id_a,
                    EdgeType::Contradicts,
                    &record.memory_id_b,
                );
                let rev_ab = GraphIndex::encode_reverse_key(
                    &record.memory_id_a,
                    EdgeType::Contradicts,
                    &record.memory_id_b,
                );
                let fwd_ba = GraphIndex::encode_forward_key(
                    &record.memory_id_b,
                    EdgeType::Contradicts,
                    &record.memory_id_a,
                );
                let rev_ba = GraphIndex::encode_reverse_key(
                    &record.memory_id_b,
                    EdgeType::Contradicts,
                    &record.memory_id_a,
                );

                batch_ops.push(BatchOperation::Put {
                    cf: ColumnFamilyName::Graph,
                    key: fwd_ab,
                    value: meta_bytes.clone(),
                });
                batch_ops.push(BatchOperation::Put {
                    cf: ColumnFamilyName::Graph,
                    key: rev_ab,
                    value: meta_bytes.clone(),
                });
                batch_ops.push(BatchOperation::Put {
                    cf: ColumnFamilyName::Graph,
                    key: fwd_ba,
                    value: meta_bytes.clone(),
                });
                batch_ops.push(BatchOperation::Put {
                    cf: ColumnFamilyName::Graph,
                    key: rev_ba,
                    value: meta_bytes,
                });

                confirmed.push(Contradiction {
                    memory_id_a: record.memory_id_a,
                    memory_id_b: record.memory_id_b,
                    confidence: verdict.confidence,
                    method: ClassifierMethod::Heuristic,
                });
                contradictions_confirmed += 1;
            }
            "revision" => {
                let metadata = EdgeMetadata::new(verdict.confidence, now_us);
                let meta_bytes = metadata.to_bytes();

                // REVISED_FROM edge: B revised from A (B supersedes A)
                let fwd_key = GraphIndex::encode_forward_key(
                    &record.memory_id_b,
                    EdgeType::RevisedFrom,
                    &record.memory_id_a,
                );
                let rev_key = GraphIndex::encode_reverse_key(
                    &record.memory_id_b,
                    EdgeType::RevisedFrom,
                    &record.memory_id_a,
                );

                batch_ops.push(BatchOperation::Put {
                    cf: ColumnFamilyName::Graph,
                    key: fwd_key,
                    value: meta_bytes.clone(),
                });
                batch_ops.push(BatchOperation::Put {
                    cf: ColumnFamilyName::Graph,
                    key: rev_key,
                    value: meta_bytes,
                });

                revisions_created += 1;
            }
            _ => {
                // "dismiss" or any unknown verdict: just delete the pending record
                dismissed += 1;
            }
        }
    }

    // Write all operations atomically
    if !batch_ops.is_empty() {
        storage.write_batch(&batch_ops)?;
    }

    Ok(ContradictionCommitResult {
        contradictions_confirmed,
        revisions_created,
        dismissed,
        confirmed,
    })
}

/// Truncate content to a bounded snippet for pending record storage.
///
/// Complexity: O(max_len).
fn truncate_snippet(content: &str, max_len: usize) -> String {
    if content.len() <= max_len {
        content.to_string()
    } else {
        // Find a char boundary to avoid splitting a multi-byte char
        let end = content
            .char_indices()
            .take_while(|(i, _)| *i < max_len)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        content[..end].to_string()
    }
}

/// Load a memory from storage by ID.
fn load_memory(storage: &dyn StorageBackend, memory_id: &[u8; 16]) -> Result<Memory> {
    let key = keys::encode_memory_key(memory_id);
    let bytes = storage
        .get(ColumnFamilyName::Default, &key)?
        .ok_or_else(|| HebbsError::MemoryNotFound {
            memory_id: hex::encode(memory_id),
        })?;
    Memory::from_bytes(&bytes).map_err(|e| HebbsError::Serialization { message: e })
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_detects_negation_contradiction() {
        let a = "Vendor X has been reliable and delivered on time for all milestones.";
        let b = "Vendor X failed to deliver and missed three consecutive deadlines.";
        match heuristic_classify(a, b) {
            EntailmentResult::Contradiction { confidence } => {
                assert!(confidence >= 0.35, "confidence {} too low", confidence);
                assert!(
                    confidence <= 0.75,
                    "confidence {} exceeds heuristic cap",
                    confidence
                );
            }
            other => panic!("expected Contradiction, got {:?}", other),
        }
    }

    #[test]
    fn heuristic_detects_antonym_contradiction() {
        let a = "The system is reliable and stable under load.";
        let b = "The system is unreliable and unstable during peak hours.";
        match heuristic_classify(a, b) {
            EntailmentResult::Contradiction { confidence } => {
                assert!(confidence >= 0.4, "confidence {} too low", confidence);
            }
            other => panic!("expected Contradiction, got {:?}", other),
        }
    }

    #[test]
    fn heuristic_detects_revision() {
        let a = "I used to think the API was well designed.";
        let b = "The API has poor ergonomics and needs a redesign.";
        match heuristic_classify(a, b) {
            EntailmentResult::Revision { confidence } => {
                assert!(confidence > 0.0, "confidence should be positive");
            }
            other => panic!("expected Revision, got {:?}", other),
        }
    }

    #[test]
    fn heuristic_neutral_for_unrelated() {
        let a = "Rust has a strong type system with ownership semantics.";
        let b = "Python is popular for data science and machine learning.";
        let result = heuristic_classify(a, b);
        assert_eq!(result, EntailmentResult::Neutral);
    }

    #[test]
    fn heuristic_neutral_for_compatible() {
        let a = "The server handles 1000 requests per second.";
        let b = "The server runs on Linux with 16GB of RAM.";
        let result = heuristic_classify(a, b);
        assert_eq!(result, EntailmentResult::Neutral);
    }

    #[test]
    fn numeric_disagreement_detected() {
        let a = "the system processed 1000 requests with 3 errors in the production environment";
        let b = "the system processed 500 requests with 15 errors in the production environment";
        assert!(has_numeric_disagreement(
            &a.to_lowercase(),
            &b.to_lowercase()
        ));
    }

    #[test]
    fn numeric_disagreement_same_numbers() {
        let a = "the server handles 1000 requests per second";
        let b = "we measured 1000 requests per second on the server";
        assert!(!has_numeric_disagreement(
            &a.to_lowercase(),
            &b.to_lowercase()
        ));
    }
}
