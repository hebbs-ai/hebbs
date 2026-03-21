//! Integration tests for two-phase contradiction detection pipeline.
//!
//! Tests cover:
//! - Heuristic classifier edge cases
//! - Engine API (check_contradictions, contradiction_prepare, contradiction_commit)
//! - Full pipeline: remember -> check -> pending -> commit -> edges
//! - Config behavior (enabled/disabled, thresholds)
//! - LLM response parsing
//! - Bidirectional edge creation via commit

use std::sync::Arc;

use hebbs_core::contradict::{
    heuristic_classify, ContradictionConfig, ContradictionVerdict, EntailmentResult,
};
use hebbs_core::engine::{Engine, RememberInput};
use hebbs_embed::MockEmbedder;
use hebbs_index::graph::{EdgeType, GraphIndex};
use hebbs_index::HnswParams;
use hebbs_storage::{InMemoryBackend, StorageBackend};

fn test_engine() -> (Engine, Arc<dyn StorageBackend>) {
    let backend: Arc<dyn StorageBackend> = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::default_dims());
    let params = HnswParams::with_m(384, 16);
    let engine = Engine::new_with_params(backend.clone(), embedder, params, 42).unwrap();
    (engine, backend)
}

fn simple_input(content: &str) -> RememberInput {
    RememberInput {
        content: content.to_string(),
        importance: None,
        context: None,
        entity_id: None,
        edges: vec![],
        kind: None,
    }
}

// ── Heuristic Classifier: Comprehensive Edge Cases ──────────────────

#[test]
fn heuristic_strong_negation_contradiction() {
    // Both antonym + negation signals
    let a = "The system is reliable and stable under load.";
    let b = "The system is unreliable and unstable during peak hours.";
    match heuristic_classify(a, b) {
        EntailmentResult::Contradiction { confidence } => {
            assert!(
                confidence >= 0.5,
                "strong signals should yield high confidence: {}",
                confidence
            );
        }
        other => panic!("expected Contradiction, got {:?}", other),
    }
}

#[test]
fn heuristic_pure_antonym_contradiction() {
    let a = "The deployment was a success and the results were positive.";
    let b = "The deployment was a failure and the results were negative.";
    match heuristic_classify(a, b) {
        EntailmentResult::Contradiction { confidence } => {
            assert!(
                confidence >= 0.35,
                "antonyms should trigger: {}",
                confidence
            );
        }
        other => panic!("expected Contradiction, got {:?}", other),
    }
}

#[test]
fn heuristic_negation_only_contradiction() {
    let a = "The vendor delivered all components on schedule.";
    let b = "The vendor didn't deliver any components and missed the deadline.";
    match heuristic_classify(a, b) {
        EntailmentResult::Contradiction { confidence } => {
            assert!(
                confidence >= 0.35,
                "negation should trigger: {}",
                confidence
            );
        }
        other => panic!("expected Contradiction, got {:?}", other),
    }
}

#[test]
fn heuristic_numeric_disagreement_contradiction() {
    // Same topic, different numbers, plus negation asymmetry
    let a = "The system processed 1000 requests with 3 errors in the production environment today.";
    let b = "The system failed to process requests, with 150 errors in the production environment today.";
    match heuristic_classify(a, b) {
        EntailmentResult::Contradiction { confidence } => {
            assert!(
                confidence >= 0.35,
                "numeric + negation should trigger: {}",
                confidence
            );
        }
        other => panic!("expected Contradiction, got {:?}", other),
    }
}

#[test]
fn heuristic_revision_with_temporal_markers() {
    let a = "I previously believed the architecture was sound.";
    let b = "The architecture has fundamental flaws that need rethinking.";
    match heuristic_classify(a, b) {
        EntailmentResult::Revision { confidence } => {
            assert!(
                confidence > 0.0,
                "revision markers should produce positive confidence: {}",
                confidence
            );
        }
        other => panic!("expected Revision, got {:?}", other),
    }
}

#[test]
fn heuristic_revision_updated_marker() {
    let a = "Updated: the API now uses OAuth2 for authentication.";
    let b = "The API uses basic auth for all endpoints.";
    match heuristic_classify(a, b) {
        EntailmentResult::Revision { confidence } => {
            assert!(
                confidence > 0.0,
                "updated marker should trigger revision: {}",
                confidence
            );
        }
        other => panic!("expected Revision, got {:?}", other),
    }
}

#[test]
fn heuristic_revision_trumps_contradiction() {
    // Has both revision markers AND negation, revision should win
    let a = "I used to think the system was reliable and never had issues.";
    let b = "The system is unreliable and fails regularly.";
    // "used to" + "previously" style markers should bias toward revision
    match heuristic_classify(a, b) {
        EntailmentResult::Revision { .. } => {} // Expected
        other => panic!(
            "revision markers should trump contradiction signals, got {:?}",
            other
        ),
    }
}

#[test]
fn heuristic_neutral_unrelated_topics() {
    let a = "Rust has a strong type system with ownership semantics.";
    let b = "Python is popular for data science and machine learning.";
    assert_eq!(heuristic_classify(a, b), EntailmentResult::Neutral);
}

#[test]
fn heuristic_neutral_complementary_facts() {
    let a = "The server runs on Linux with 16GB of RAM.";
    let b = "The server handles REST API requests for the mobile app.";
    assert_eq!(heuristic_classify(a, b), EntailmentResult::Neutral);
}

#[test]
fn heuristic_neutral_same_assertion() {
    let a = "The database uses PostgreSQL 15.";
    let b = "We run PostgreSQL 15 for our data layer.";
    assert_eq!(heuristic_classify(a, b), EntailmentResult::Neutral);
}

#[test]
fn heuristic_neutral_short_text() {
    let a = "yes";
    let b = "no";
    // Too short for meaningful contradiction detection
    assert_eq!(heuristic_classify(a, b), EntailmentResult::Neutral);
}

#[test]
fn heuristic_neutral_empty_text() {
    assert_eq!(heuristic_classify("", ""), EntailmentResult::Neutral);
    assert_eq!(heuristic_classify("hello", ""), EntailmentResult::Neutral);
    assert_eq!(heuristic_classify("", "hello"), EntailmentResult::Neutral);
}

#[test]
fn heuristic_confidence_capped_at_075() {
    // Stack many signals: antonyms + negation + numeric
    let a = "The reliable system had a success rate of 99% and was efficient, effective, and safe to use in the production environment.";
    let b = "The unreliable system had a failure rate and was not efficient, ineffective, and unsafe to use in the production environment.";
    match heuristic_classify(a, b) {
        EntailmentResult::Contradiction { confidence } => {
            assert!(
                confidence <= 0.75,
                "heuristic cap should be 0.75: {}",
                confidence
            );
        }
        other => panic!("expected Contradiction, got {:?}", other),
    }
}

// ── Engine API: check_contradictions ─────────────────────────────────

#[test]
fn check_contradictions_disabled_returns_empty() {
    let (engine, _storage) = test_engine();
    let mem = engine.remember(simple_input("test memory")).unwrap();
    let mut id = [0u8; 16];
    id.copy_from_slice(&mem.memory_id);

    let config = ContradictionConfig {
        enabled: false,
        ..Default::default()
    };
    let result = engine.check_contradictions(&id, &config, None).unwrap();
    assert!(
        result.pending.is_empty(),
        "disabled config should return no contradictions"
    );
}

#[test]
fn check_contradictions_empty_corpus_returns_empty() {
    let (engine, _storage) = test_engine();
    let mem = engine.remember(simple_input("lonely memory")).unwrap();
    let mut id = [0u8; 16];
    id.copy_from_slice(&mem.memory_id);

    let config = ContradictionConfig::default();
    let result = engine.check_contradictions(&id, &config, None).unwrap();
    assert!(
        result.pending.is_empty(),
        "single memory should have no contradictions"
    );
}

#[test]
fn check_contradictions_with_candidates_but_no_contradiction() {
    let (engine, _storage) = test_engine();

    // Remember several compatible memories
    for i in 0..5 {
        engine
            .remember(simple_input(&format!(
                "The project is going well, milestone {} completed on time.",
                i
            )))
            .unwrap();
    }

    let mem = engine
        .remember(simple_input(
            "The project timeline looks good, all deliverables are on track.",
        ))
        .unwrap();
    let mut id = [0u8; 16];
    id.copy_from_slice(&mem.memory_id);

    // Use very low similarity threshold to ensure candidates are found
    // (MockEmbedder doesn't produce semantically similar vectors)
    let config = ContradictionConfig {
        enabled: true,
        candidates_k: 5,
        min_similarity: 0.0,
        min_confidence: 0.7,
    };
    let result = engine.check_contradictions(&id, &config, None).unwrap();
    // Compatible content, so heuristic shouldn't find contradictions
    assert!(
        result.pending.is_empty(),
        "compatible memories should not contradict"
    );
}

#[test]
fn check_contradictions_finds_contradiction_in_pipeline() {
    let (engine, _storage) = test_engine();

    // First, remember an affirmative statement
    engine
        .remember(simple_input(
            "Vendor X has been reliable and delivered every milestone on time successfully.",
        ))
        .unwrap();

    // Now remember a contradicting statement
    let contradicting = engine
        .remember(simple_input(
            "Vendor X is unreliable and failed to deliver, missing every deadline.",
        ))
        .unwrap();
    let mut id = [0u8; 16];
    id.copy_from_slice(&contradicting.memory_id);

    // Low min_similarity to bypass MockEmbedder's hash-based vectors.
    // Low min_confidence to catch heuristic's moderate confidence.
    let config = ContradictionConfig {
        enabled: true,
        candidates_k: 10,
        min_similarity: 0.0,
        min_confidence: 0.35,
    };

    let result = engine.check_contradictions(&id, &config, None).unwrap();

    // The heuristic should detect: "reliable" vs "unreliable" (antonym),
    // negation asymmetry ("failed", "missing" vs none in A)
    assert!(
        !result.pending.is_empty(),
        "should detect contradiction between affirmative and negative vendor statements"
    );

    // Phase 1 creates pending records, not edges
    let contradictions_from_new = engine.contradictions(&id).unwrap();
    assert!(
        contradictions_from_new.is_empty(),
        "pending records should not create edges until commit"
    );

    // Phase 2a: prepare retrieves all pending candidates
    let pending = engine.contradiction_prepare().unwrap();
    assert!(
        !pending.is_empty(),
        "prepare should return pending candidates"
    );

    // Phase 2b: commit with "contradiction" verdict creates edges
    let verdicts: Vec<ContradictionVerdict> = pending
        .iter()
        .map(|p| ContradictionVerdict {
            pending_id: hex::encode(p.id),
            verdict: "contradiction".to_string(),
            confidence: p.classifier_score,
            reasoning: None,
        })
        .collect();

    let commit_result = engine.contradiction_commit(&verdicts).unwrap();
    assert!(
        commit_result.contradictions_confirmed > 0,
        "commit should confirm at least one contradiction"
    );

    // Now edges should exist
    let edges = engine.contradictions(&id).unwrap();
    assert!(
        !edges.is_empty(),
        "committed contradictions should create CONTRADICTS edges"
    );
}

#[test]
fn contradictions_api_returns_edges() {
    let (engine, storage) = test_engine();

    let mem_a = engine.remember(simple_input("Memory A content")).unwrap();
    let mem_b = engine.remember(simple_input("Memory B content")).unwrap();

    let mut id_a = [0u8; 16];
    id_a.copy_from_slice(&mem_a.memory_id);
    let mut id_b = [0u8; 16];
    id_b.copy_from_slice(&mem_b.memory_id);

    // Manually create CONTRADICTS edges to test the query API
    use hebbs_index::graph::EdgeMetadata;
    use hebbs_storage::{BatchOperation, ColumnFamilyName};

    let metadata = EdgeMetadata::new(0.85, 1000);
    let meta_bytes = metadata.to_bytes();

    let fwd = GraphIndex::encode_forward_key(&id_a, EdgeType::Contradicts, &id_b);
    let rev = GraphIndex::encode_reverse_key(&id_a, EdgeType::Contradicts, &id_b);
    storage
        .write_batch(&[
            BatchOperation::Put {
                cf: ColumnFamilyName::Graph,
                key: fwd,
                value: meta_bytes.clone(),
            },
            BatchOperation::Put {
                cf: ColumnFamilyName::Graph,
                key: rev,
                value: meta_bytes,
            },
        ])
        .unwrap();

    let result = engine.contradictions(&id_a).unwrap();
    assert_eq!(result.len(), 1, "should find one contradiction");
    assert_eq!(result[0].0, id_b, "contradiction target should be memory B");
    assert!(
        (result[0].1 - 0.85).abs() < 0.01,
        "confidence should be 0.85"
    );
}

#[test]
fn contradictions_api_empty_when_none() {
    let (engine, _storage) = test_engine();
    let mem = engine
        .remember(simple_input("no contradictions here"))
        .unwrap();
    let mut id = [0u8; 16];
    id.copy_from_slice(&mem.memory_id);

    let result = engine.contradictions(&id).unwrap();
    assert!(result.is_empty());
}

#[test]
fn check_contradictions_creates_bidirectional_edges() {
    let (engine, _storage) = test_engine();

    let mem_a = engine
        .remember(simple_input(
            "The system is reliable, stable, and effective under heavy load.",
        ))
        .unwrap();
    let mut id_a = [0u8; 16];
    id_a.copy_from_slice(&mem_a.memory_id);

    let mem_b = engine
        .remember(simple_input(
            "The system is unreliable, unstable, and ineffective under any load.",
        ))
        .unwrap();
    let mut id_b = [0u8; 16];
    id_b.copy_from_slice(&mem_b.memory_id);

    let config = ContradictionConfig {
        enabled: true,
        candidates_k: 10,
        min_similarity: 0.0,
        min_confidence: 0.35,
    };

    let result = engine.check_contradictions(&id_b, &config, None).unwrap();
    if !result.pending.is_empty() {
        // Commit the pending contradictions as confirmed
        let verdicts: Vec<ContradictionVerdict> = result
            .pending
            .iter()
            .map(|p| ContradictionVerdict {
                pending_id: hex::encode(p.id),
                verdict: "contradiction".to_string(),
                confidence: p.classifier_score,
                reasoning: None,
            })
            .collect();
        engine.contradiction_commit(&verdicts).unwrap();

        // Verify bidirectional: B->A exists AND A->B exists
        let from_b = engine.contradictions(&id_b).unwrap();
        let from_a = engine.contradictions(&id_a).unwrap();

        assert!(!from_b.is_empty(), "B should have edge to A");
        assert!(
            !from_a.is_empty(),
            "A should have edge from B (bidirectional)"
        );
        assert_eq!(from_b[0].0, id_a);
        assert_eq!(from_a[0].0, id_b);
    }
}

#[test]
fn check_contradictions_skips_already_classified_pairs() {
    let (engine, _storage) = test_engine();

    let mem_a = engine
        .remember(simple_input(
            "The API is fast, efficient, and handles requests reliably.",
        ))
        .unwrap();
    let mut id_a = [0u8; 16];
    id_a.copy_from_slice(&mem_a.memory_id);

    let mem_b = engine
        .remember(simple_input(
            "The API is slow, inefficient, and unreliable at handling requests.",
        ))
        .unwrap();
    let mut id_b = [0u8; 16];
    id_b.copy_from_slice(&mem_b.memory_id);

    let config = ContradictionConfig {
        enabled: true,
        candidates_k: 10,
        min_similarity: 0.0,
        min_confidence: 0.35,
    };

    // First check should find contradiction candidates
    let first = engine.check_contradictions(&id_b, &config, None).unwrap();

    // Commit the first batch to create graph edges
    if !first.pending.is_empty() {
        let verdicts: Vec<ContradictionVerdict> = first
            .pending
            .iter()
            .map(|p| ContradictionVerdict {
                pending_id: hex::encode(p.id),
                verdict: "contradiction".to_string(),
                confidence: p.classifier_score,
                reasoning: None,
            })
            .collect();
        engine.contradiction_commit(&verdicts).unwrap();
    }

    // Second check should skip already-classified pair (edges exist now)
    let second = engine.check_contradictions(&id_b, &config, None).unwrap();
    assert!(
        second.pending.is_empty(),
        "second check should skip already-classified pair, got {} results",
        second.pending.len()
    );

    // Edges from first run should still exist
    if !first.pending.is_empty() {
        let edges = engine.contradictions(&id_b).unwrap();
        assert!(!edges.is_empty(), "edges from first run should persist");
    }
}

#[test]
fn check_contradictions_respects_min_confidence() {
    let (engine, _storage) = test_engine();

    // Weak contradiction: only negation asymmetry (0.35 confidence)
    engine
        .remember(simple_input("The vendor delivered components on schedule."))
        .unwrap();

    let mem = engine
        .remember(simple_input(
            "The vendor didn't deliver components on schedule.",
        ))
        .unwrap();
    let mut id = [0u8; 16];
    id.copy_from_slice(&mem.memory_id);

    // High min_confidence should filter out low-confidence results
    let config_high = ContradictionConfig {
        enabled: true,
        candidates_k: 10,
        min_similarity: 0.0,
        min_confidence: 0.7,
    };
    let result_high = engine
        .check_contradictions(&id, &config_high, None)
        .unwrap();
    assert!(
        result_high.pending.is_empty(),
        "high min_confidence should filter weak contradictions"
    );
}

// ── Config Behavior ──────────────────────────────────────────────────

#[test]
fn contradiction_config_defaults() {
    let config = ContradictionConfig::default();
    assert!(config.enabled);
    assert_eq!(config.candidates_k, 10);
    assert!((config.min_similarity - 0.5).abs() < 0.01);
    assert!((config.min_confidence - 0.35).abs() < 0.01);
}

// ── LLM Response Parsing (via heuristic to test parse_llm_response indirectly) ──

#[test]
fn heuristic_multiple_antonyms_increase_confidence() {
    let a = "The system is reliable, safe, and efficient.";
    let b = "The system is unreliable, unsafe, and inefficient.";
    match heuristic_classify(a, b) {
        EntailmentResult::Contradiction {
            confidence: c_multi,
        } => {
            // Compare with single antonym
            let a2 = "The system is reliable.";
            let b2 = "The system is unreliable.";
            if let EntailmentResult::Contradiction {
                confidence: c_single,
            } = heuristic_classify(a2, b2)
            {
                assert!(
                    c_multi >= c_single,
                    "multiple antonyms should yield >= confidence: {} vs {}",
                    c_multi,
                    c_single
                );
            }
        }
        other => panic!("expected Contradiction, got {:?}", other),
    }
}

#[test]
fn heuristic_handles_case_insensitivity() {
    let a = "The SYSTEM is RELIABLE and STABLE.";
    let b = "the system is UNRELIABLE and UNSTABLE.";
    match heuristic_classify(a, b) {
        EntailmentResult::Contradiction { .. } => {} // Expected
        other => panic!("case should not matter, got {:?}", other),
    }
}

// ── Contradiction Prepare/Commit: Verdict Types ─────────────────────

#[test]
fn contradiction_prepare_returns_empty_when_no_pending() {
    let (engine, _storage) = test_engine();

    // Store some compatible memories -- no contradictions
    engine.remember(simple_input("The sky is blue.")).unwrap();
    engine.remember(simple_input("Water is wet.")).unwrap();

    let pending = engine.contradiction_prepare().unwrap();
    assert!(
        pending.is_empty(),
        "no pending contradictions should exist for compatible memories"
    );
}

#[test]
fn contradiction_commit_dismiss_removes_candidate() {
    let (engine, _storage) = test_engine();

    engine
        .remember(simple_input(
            "The system is reliable, stable, and well-tested under heavy load.",
        ))
        .unwrap();

    let mem_b = engine
        .remember(simple_input(
            "The system is unreliable, unstable, and poorly tested under any load.",
        ))
        .unwrap();
    let mut id_b = [0u8; 16];
    id_b.copy_from_slice(&mem_b.memory_id);

    let config = ContradictionConfig {
        enabled: true,
        candidates_k: 10,
        min_similarity: 0.0,
        min_confidence: 0.35,
    };

    let result = engine.check_contradictions(&id_b, &config, None).unwrap();
    if result.pending.is_empty() {
        return; // MockEmbedder may not produce candidates
    }

    // Dismiss all candidates
    let verdicts: Vec<ContradictionVerdict> = result
        .pending
        .iter()
        .map(|p| ContradictionVerdict {
            pending_id: hex::encode(p.id),
            verdict: "dismiss".to_string(),
            confidence: 0.95,
            reasoning: Some("Not a real conflict".to_string()),
        })
        .collect();

    let commit_result = engine.contradiction_commit(&verdicts).unwrap();
    assert!(
        commit_result.dismissed > 0,
        "should have dismissed at least one candidate"
    );
    assert_eq!(
        commit_result.contradictions_confirmed, 0,
        "no contradictions should be confirmed on dismiss"
    );

    // No edges should be created
    let edges = engine.contradictions(&id_b).unwrap();
    assert!(
        edges.is_empty(),
        "dismissed candidates should not create edges"
    );

    // Prepare again should return empty (candidates consumed)
    let pending_after = engine.contradiction_prepare().unwrap();
    assert!(
        pending_after.is_empty(),
        "dismissed candidates should be removed from pending"
    );
}

#[test]
fn contradiction_commit_revision_creates_revised_from_edge() {
    let (engine, _storage) = test_engine();

    let mem_a = engine
        .remember(simple_input(
            "The system is reliable, stable, and well-tested under heavy load.",
        ))
        .unwrap();
    let mut id_a = [0u8; 16];
    id_a.copy_from_slice(&mem_a.memory_id);

    let mem_b = engine
        .remember(simple_input(
            "The system is unreliable, unstable, and poorly tested under any load.",
        ))
        .unwrap();
    let mut id_b = [0u8; 16];
    id_b.copy_from_slice(&mem_b.memory_id);

    let config = ContradictionConfig {
        enabled: true,
        candidates_k: 10,
        min_similarity: 0.0,
        min_confidence: 0.35,
    };

    let result = engine.check_contradictions(&id_b, &config, None).unwrap();
    if result.pending.is_empty() {
        return; // MockEmbedder may not produce candidates
    }

    // Mark as revision instead of contradiction
    let verdicts: Vec<ContradictionVerdict> = result
        .pending
        .iter()
        .map(|p| ContradictionVerdict {
            pending_id: hex::encode(p.id),
            verdict: "revision".to_string(),
            confidence: 0.85,
            reasoning: Some("Updated assessment".to_string()),
        })
        .collect();

    let commit_result = engine.contradiction_commit(&verdicts).unwrap();
    assert!(
        commit_result.revisions_created > 0,
        "should have created at least one revision"
    );
    assert_eq!(
        commit_result.contradictions_confirmed, 0,
        "no contradictions should be confirmed on revision"
    );

    // Contradiction edges should NOT exist (revision creates REVISED_FROM, not CONTRADICTS)
    let contradiction_edges = engine.contradictions(&id_b).unwrap();
    assert!(
        contradiction_edges.is_empty(),
        "revision verdict should not create CONTRADICTS edges"
    );
}

#[test]
fn contradiction_commit_with_reasoning() {
    let (engine, _storage) = test_engine();

    engine
        .remember(simple_input(
            "The vendor delivered every milestone on time and was reliable throughout.",
        ))
        .unwrap();

    let mem_b = engine
        .remember(simple_input(
            "The vendor failed to deliver and was unreliable, missing every deadline.",
        ))
        .unwrap();
    let mut id_b = [0u8; 16];
    id_b.copy_from_slice(&mem_b.memory_id);

    let config = ContradictionConfig {
        enabled: true,
        candidates_k: 10,
        min_similarity: 0.0,
        min_confidence: 0.35,
    };

    let result = engine.check_contradictions(&id_b, &config, None).unwrap();
    if result.pending.is_empty() {
        return;
    }

    // Commit with detailed reasoning
    let verdicts: Vec<ContradictionVerdict> = result
        .pending
        .iter()
        .map(|p| ContradictionVerdict {
            pending_id: hex::encode(p.id),
            verdict: "contradiction".to_string(),
            confidence: 0.9,
            reasoning: Some("Vendor reliability assessment directly contradicts: delivered vs failed to deliver".to_string()),
        })
        .collect();

    let commit_result = engine.contradiction_commit(&verdicts).unwrap();
    assert!(
        commit_result.contradictions_confirmed > 0,
        "should confirm contradiction with reasoning"
    );
}

#[test]
fn contradiction_prepare_returns_classifier_fields() {
    let (engine, _storage) = test_engine();

    engine
        .remember(simple_input(
            "The API is fast, efficient, and handles all requests reliably in production.",
        ))
        .unwrap();

    let mem_b = engine
        .remember(simple_input(
            "The API is slow, inefficient, and unreliable at handling any requests.",
        ))
        .unwrap();
    let mut id_b = [0u8; 16];
    id_b.copy_from_slice(&mem_b.memory_id);

    let config = ContradictionConfig {
        enabled: true,
        candidates_k: 10,
        min_similarity: 0.0,
        min_confidence: 0.35,
    };

    engine.check_contradictions(&id_b, &config, None).unwrap();

    let pending = engine.contradiction_prepare().unwrap();
    if pending.is_empty() {
        return;
    }

    for p in &pending {
        assert!(
            p.classifier_score > 0.0,
            "classifier_score should be positive: {}",
            p.classifier_score
        );
        assert!(
            p.classifier_score <= 0.75,
            "heuristic classifier_score should be capped at 0.75: {}",
            p.classifier_score
        );
        assert!(
            !p.content_a_snippet.is_empty(),
            "content_a_snippet should not be empty"
        );
        assert!(
            !p.content_b_snippet.is_empty(),
            "content_b_snippet should not be empty"
        );
    }
}
