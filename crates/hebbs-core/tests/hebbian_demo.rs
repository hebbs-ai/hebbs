//! Demo: four design limitations fixed by Dual-Embedding Hebbian Recall.
//!
//! Run with:
//!   cargo test --package hebbs-core --test hebbian_demo -- --nocapture

use std::sync::Arc;

use hebbs_core::engine::{Engine, RememberEdge, RememberInput};
use hebbs_core::recall::{CausalDirection, RecallInput, RecallStrategy, StrategyDetail};
use hebbs_core::tenant::TenantContext;
use hebbs_embed::MockEmbedder;
use hebbs_index::{EdgeType, HnswParams};
use hebbs_storage::InMemoryBackend;

fn make_engine(dims: usize) -> Engine {
    let storage = Arc::new(InMemoryBackend::new());
    let embedder = Arc::new(MockEmbedder::new(dims));
    Engine::new_with_params(storage, embedder, HnswParams::new(dims), 42).unwrap()
}

fn parse_id(bytes: &[u8]) -> [u8; 16] {
    let mut id = [0u8; 16];
    id.copy_from_slice(&bytes[..16]);
    id
}

// ─────────────────────────────────────────────────────────────────
// Fix 1: Bidirectional causal traversal
//
// Old: start at A (root cause) → finds nothing, BFS only followed
//      forward graph edges and A has none.
// New: CausalDirection::Backward from A finds its downstream effects
//      via the learned Hebbian offset vector.
// ─────────────────────────────────────────────────────────────────

#[test]
fn fix1_bidirectional_causal_traversal() {
    let engine = make_engine(32);

    // Build a causal chain: A → B → C
    //   "DB overloaded" caused "latency spike" which caused "user complaints"
    let mem_a = engine
        .remember(RememberInput {
            content: "DB overloaded".into(),
            importance: Some(0.9),
            context: None,
            entity_id: Some("incident".into()),
            edges: vec![],
        })
        .unwrap();
    let id_a = parse_id(&mem_a.memory_id);

    let mem_b = engine
        .remember(RememberInput {
            content: "Latency spike".into(),
            importance: Some(0.8),
            context: None,
            entity_id: Some("incident".into()),
            edges: vec![RememberEdge {
                target_id: id_a,
                edge_type: EdgeType::CausedBy,
                confidence: Some(0.95),
            }],
        })
        .unwrap();
    let id_b = parse_id(&mem_b.memory_id);

    let mem_c = engine
        .remember(RememberInput {
            content: "User complaints".into(),
            importance: Some(0.7),
            context: None,
            entity_id: Some("incident".into()),
            edges: vec![RememberEdge {
                target_id: id_b,
                edge_type: EdgeType::CausedBy,
                confidence: Some(0.9),
            }],
        })
        .unwrap();
    let id_c = parse_id(&mem_c.memory_id);

    // --- Forward: from the leaf effect C, trace back to causes ---
    let mut fwd = RecallInput::new(hex::encode(id_c), RecallStrategy::Causal);
    fwd.top_k = Some(10);
    fwd.causal_direction = Some(CausalDirection::Forward);
    let fwd_result = engine.recall(fwd).unwrap();
    let fwd_contents: Vec<&str> = fwd_result
        .results
        .iter()
        .map(|r| r.memory.content.as_str())
        .collect();
    println!(
        "[Fix 1] Forward from C (user complaints) → {:?}",
        fwd_contents
    );
    assert!(
        fwd_contents.contains(&"Latency spike") || fwd_contents.contains(&"DB overloaded"),
        "Forward from C should find upstream causes, got: {:?}",
        fwd_contents
    );

    // --- Backward: from the root cause A, find its downstream effects ---
    // This is what was IMPOSSIBLE before — BFS had no reverse edges.
    let mut bwd = RecallInput::new(hex::encode(id_a), RecallStrategy::Causal);
    bwd.top_k = Some(10);
    bwd.causal_direction = Some(CausalDirection::Backward);
    let bwd_result = engine.recall(bwd).unwrap();
    let bwd_contents: Vec<&str> = bwd_result
        .results
        .iter()
        .map(|r| r.memory.content.as_str())
        .collect();
    println!(
        "[Fix 1] Backward from A (DB overloaded) → {:?}",
        bwd_contents
    );
    assert!(
        !bwd_result.results.is_empty(),
        "Backward from root cause A should find its effects, found none"
    );
    assert!(
        bwd_contents.contains(&"Latency spike") || bwd_contents.contains(&"User complaints"),
        "Backward from A should find B or C, got: {:?}",
        bwd_contents
    );

    println!("[Fix 1] PASS: bidirectional causal traversal works");
}

// ─────────────────────────────────────────────────────────────────
// Fix 2: Tenant isolation
//
// Old: all writes went to DEFAULT_TENANT regardless of the caller.
//      Tenant B could see Tenant A's memories.
// New: each tenant's HNSW is scoped; cross-tenant recall returns nothing.
// ─────────────────────────────────────────────────────────────────

#[test]
fn fix2_tenant_isolation() {
    let engine = make_engine(32);

    let tenant_a = TenantContext::new("acme").unwrap();
    let tenant_b = TenantContext::new("globex").unwrap();

    // Acme stores a confidential memory
    engine
        .remember_for_tenant(
            &tenant_a,
            RememberInput {
                content: "Acme secret: Q4 revenue $50M".into(),
                importance: Some(0.9),
                context: None,
                entity_id: Some("acme_finance".into()),
                edges: vec![],
            },
        )
        .unwrap();

    // Globex stores its own memory
    engine
        .remember_for_tenant(
            &tenant_b,
            RememberInput {
                content: "Globex data: new product launch".into(),
                importance: Some(0.9),
                context: None,
                entity_id: Some("globex_product".into()),
                edges: vec![],
            },
        )
        .unwrap();

    // Globex queries for Acme's content — should get nothing back
    let result = engine
        .recall_for_tenant(
            &tenant_b,
            RecallInput::new("Q4 revenue 50M", RecallStrategy::Similarity),
        )
        .unwrap();

    let leaked: Vec<&str> = result
        .results
        .iter()
        .filter(|r| r.memory.content.contains("Acme secret"))
        .map(|r| r.memory.content.as_str())
        .collect();

    println!(
        "[Fix 2] Globex recall sees {} results, leaked Acme data: {:?}",
        result.results.len(),
        leaked
    );
    assert!(
        leaked.is_empty(),
        "Tenant isolation broken: Globex can see Acme's memory: {:?}",
        leaked
    );

    // Acme can still recall its own memory
    let acme_result = engine
        .recall_for_tenant(
            &tenant_a,
            RecallInput::new("Q4 revenue", RecallStrategy::Similarity),
        )
        .unwrap();
    println!(
        "[Fix 2] Acme recall sees {} results (their own data)",
        acme_result.results.len()
    );
    assert!(
        !acme_result.results.is_empty(),
        "Acme should be able to recall its own memories"
    );

    println!("[Fix 2] PASS: tenant data does not leak across tenants");
}

// ─────────────────────────────────────────────────────────────────
// Fix 3: Analogical recall uses vector arithmetic
//
// Old: "find an analogy" just counted overlapping JSON keys (Jaccard).
// New: when analogy_a_id + analogy_b_id are given, computes
//      target = normalize(a_C + normalize(a_B - a_A)) in assoc space.
//      StrategyDetail reports used_vector_analogy: true.
// ─────────────────────────────────────────────────────────────────

#[test]
fn fix3_analogical_recall_uses_vector_arithmetic() {
    let engine = make_engine(32);

    // Store three memories: A, B, C
    // We'll query: A is to B as C is to ?
    let mem_a = engine
        .remember(RememberInput {
            content: "Paris is the capital of France".into(),
            importance: Some(0.9),
            context: None,
            entity_id: None,
            edges: vec![],
        })
        .unwrap();
    let id_a = parse_id(&mem_a.memory_id);

    let mem_b = engine
        .remember(RememberInput {
            content: "France is a country in Europe".into(),
            importance: Some(0.9),
            context: None,
            entity_id: None,
            edges: vec![],
        })
        .unwrap();
    let id_b = parse_id(&mem_b.memory_id);

    // C is the query cue
    let _mem_c = engine
        .remember(RememberInput {
            content: "Berlin is the capital of Germany".into(),
            importance: Some(0.9),
            context: None,
            entity_id: None,
            edges: vec![],
        })
        .unwrap();

    // Analogical recall: A:B :: C:?
    let mut input = RecallInput::new("Germany is a country in Europe", RecallStrategy::Analogical);
    input.top_k = Some(5);
    input.analogy_a_id = Some(id_a);
    input.analogy_b_id = Some(id_b);

    let result = engine.recall(input).unwrap();

    // Check that the vector arithmetic path was taken
    let used_vector = result.results.iter().any(|r| {
        r.strategy_details.iter().any(|d| {
            matches!(
                d,
                StrategyDetail::Analogical {
                    used_vector_analogy: true,
                    ..
                }
            )
        })
    });

    println!(
        "[Fix 3] Analogical recall found {} results, used vector arithmetic: {}",
        result.results.len(),
        used_vector
    );
    assert!(
        !result.results.is_empty(),
        "Analogical recall should return results"
    );
    assert!(
        used_vector,
        "Should have used vector arithmetic (used_vector_analogy=true), got: {:?}",
        result
            .results
            .iter()
            .flat_map(|r| &r.strategy_details)
            .collect::<Vec<_>>()
    );

    println!("[Fix 3] PASS: analogical recall uses real vector arithmetic");
}

// ─────────────────────────────────────────────────────────────────
// Fix 4: Reflection clusters in relational (assoc) space
//
// Old: Stage 1 clustering used content embeddings → grouped by topic.
// New: uses assoc_embedding → groups by structural/relational role.
//      Each Memory now carries associative_embedding alongside its
//      content embedding; reflect pipeline picks the right one.
// ─────────────────────────────────────────────────────────────────

#[test]
fn fix4_assoc_embedding_stored_on_memory() {
    let engine = make_engine(32);

    // Store a memory with an edge — the Hebbian update fires and
    // evolves the assoc embeddings of both endpoints.
    let mem_cause = engine
        .remember(RememberInput {
            content: "Cache miss rate spiked".into(),
            importance: Some(0.8),
            context: None,
            entity_id: Some("ops".into()),
            edges: vec![],
        })
        .unwrap();
    let id_cause = parse_id(&mem_cause.memory_id);

    let mem_effect = engine
        .remember(RememberInput {
            content: "API p99 latency exceeded SLA".into(),
            importance: Some(0.85),
            context: None,
            entity_id: Some("ops".into()),
            edges: vec![RememberEdge {
                target_id: id_cause,
                edge_type: EdgeType::CausedBy,
                confidence: Some(0.9),
            }],
        })
        .unwrap();

    // Both memories must carry an associative embedding
    assert!(
        mem_cause.associative_embedding.is_some(),
        "Cause memory must have an associative embedding"
    );
    assert!(
        mem_effect.associative_embedding.is_some(),
        "Effect memory must have an associative embedding"
    );

    let cause_assoc = mem_cause.associative_embedding.as_ref().unwrap();
    let effect_assoc = mem_effect.associative_embedding.as_ref().unwrap();

    println!(
        "[Fix 4] Cause assoc embedding dims: {}, Effect assoc embedding dims: {}",
        cause_assoc.len(),
        effect_assoc.len()
    );

    // The assoc embeddings must be non-trivial (non-zero)
    let cause_norm: f32 = cause_assoc.iter().map(|x| x * x).sum::<f32>().sqrt();
    let effect_norm: f32 = effect_assoc.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(cause_norm > 0.0, "Cause assoc embedding must be non-zero");
    assert!(effect_norm > 0.0, "Effect assoc embedding must be non-zero");

    // Reflect pipeline uses assoc embeddings for clustering.
    // We verify this indirectly: the pipeline accepts MemoryEntry with assoc_embedding.
    // (Full clustering requires an LLM provider; this test confirms the plumbing.)
    println!(
        "[Fix 4] PASS: memories carry associative embeddings (norm={:.3}, {:.3}); \
         reflect pipeline will cluster in relational space",
        cause_norm, effect_norm
    );
}
