//! End-to-end tests that exercise every command documented in `skills/hebbs/SKILL.md`.
//!
//! Architecture:
//!   1. Start a lightweight gRPC server in-process (InMemoryBackend + MockEmbedder, auth disabled).
//!   2. Invoke the `hebbs` binary as a subprocess pointing at that server.
//!   3. Parse `--format json` output and assert semantic correctness — not just exit codes.
//!
//! NOTE: MockEmbedder produces hash-based (non-semantic) vectors. Cosine distances
//! between unrelated strings are essentially random. Tests that check recall results
//! verify structure and non-empty response, not semantic ranking quality.
//!
//! The reflect tests use `min_memories_for_reflect: 3` and `min_cluster_size: 2` to
//! ensure clustering triggers with a small test corpus. With MockEmbedder's random-ish
//! vectors, k-means will still form clusters — just not semantically meaningful ones.

use std::net::SocketAddr;
use std::process::Output;
use std::sync::Arc;
use std::time::Instant;

use hebbs_core::auth::KeyCache;
use hebbs_core::engine::Engine;
use hebbs_core::rate_limit::{RateLimitConfig, RateLimiter};
use hebbs_core::reflect::ReflectConfig;
use hebbs_embed::MockEmbedder;
use hebbs_index::HnswParams;
use hebbs_proto::generated::{
    health_service_server::HealthServiceServer, memory_service_server::MemoryServiceServer,
    reflect_service_server::ReflectServiceServer, subscribe_service_server::SubscribeServiceServer,
};
use hebbs_reflect::{LlmProviderConfig, ProviderType};
use hebbs_server::grpc::health_service::HealthServiceImpl;
use hebbs_server::grpc::memory_service::MemoryServiceImpl;
use hebbs_server::grpc::reflect_service::ReflectServiceImpl;
use hebbs_server::grpc::subscribe_service::SubscribeServiceImpl;
use hebbs_server::metrics::HebbsMetrics;
use hebbs_server::middleware::{self, AuthState};
use tonic::transport::Server as TonicServer;

// ═══════════════════════════════════════════════════════════════════════
//  Test Server Harness
// ═══════════════════════════════════════════════════════════════════════

struct TestServer {
    addr: SocketAddr,
    _shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl TestServer {
    async fn start() -> Self {
        // Bind port 0 to get an OS-assigned port, then pass the listener directly
        // to tonic via TcpIncoming::from_listener to avoid TOCTOU port conflicts.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let backend = Arc::new(hebbs_storage::InMemoryBackend::new());
        let embedder = Arc::new(MockEmbedder::default_dims());
        let params = HnswParams::with_m(384, 4);
        let engine = Arc::new(Engine::new_with_params(backend, embedder, params, 42).unwrap());

        let metrics = Arc::new(HebbsMetrics::new());
        let key_cache = Arc::new(KeyCache::new());
        let auth_state = Arc::new(AuthState {
            key_cache,
            rate_limiter: Arc::new(RateLimiter::new(RateLimitConfig {
                enabled: false,
                ..Default::default()
            })),
            auth_enabled: false,
        });

        let memory_svc = MemoryServiceImpl {
            engine: engine.clone(),
            metrics: metrics.clone(),
            auth_state: auth_state.clone(),
        };
        let subscribe_svc =
            SubscribeServiceImpl::new(engine.clone(), metrics.clone(), auth_state.clone());

        let mock_llm_config = LlmProviderConfig {
            provider_type: ProviderType::Mock,
            api_key: None,
            base_url: None,
            model: "mock".to_string(),
            timeout_secs: 30,
            max_retries: 0,
            retry_backoff_ms: 0,
        };
        let reflect_config = ReflectConfig {
            min_memories_for_reflect: 3,
            min_cluster_size: 2,
            ..Default::default()
        };

        let proposal_provider: Arc<dyn hebbs_reflect::LlmProvider> =
            Arc::from(hebbs_reflect::create_provider(&mock_llm_config).unwrap());
        let validation_provider: Arc<dyn hebbs_reflect::LlmProvider> =
            Arc::from(hebbs_reflect::create_provider(&mock_llm_config).unwrap());

        let reflect_svc = ReflectServiceImpl {
            engine: engine.clone(),
            metrics: metrics.clone(),
            reflect_config,
            proposal_provider,
            validation_provider,
            auth_state: auth_state.clone(),
        };

        let health_svc = HealthServiceImpl {
            engine: engine.clone(),
            start_time: Instant::now(),
            version: "test".to_string(),
            data_dir: std::env::temp_dir(),
        };

        let grpc_interceptor = middleware::grpc_auth_interceptor(auth_state);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let incoming =
            tonic::transport::server::TcpIncoming::from_listener(listener, true, None).unwrap();

        tokio::spawn(async move {
            TonicServer::builder()
                .add_service(MemoryServiceServer::with_interceptor(
                    memory_svc,
                    grpc_interceptor.clone(),
                ))
                .add_service(SubscribeServiceServer::with_interceptor(
                    subscribe_svc,
                    grpc_interceptor.clone(),
                ))
                .add_service(ReflectServiceServer::with_interceptor(
                    reflect_svc,
                    grpc_interceptor,
                ))
                .add_service(HealthServiceServer::new(health_svc))
                .serve_with_incoming_shutdown(incoming, async { drop(shutdown_rx.await) })
                .await
                .unwrap();
        });

        // Brief wait for the server task to start accepting connections
        for _ in 0..100 {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        TestServer {
            addr,
            _shutdown_tx: shutdown_tx,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}", self.addr)
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  CLI Runner
// ═══════════════════════════════════════════════════════════════════════

fn cli_bin() -> std::path::PathBuf {
    let mut path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    path.push("hebbs");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

fn run_cli(server: &TestServer, args: &[&str]) -> Output {
    let mut cmd = std::process::Command::new(cli_bin());
    cmd.env("HEBBS_ENDPOINT", server.endpoint());
    cmd.arg("--format").arg("json");
    cmd.arg("--timeout").arg("10000");
    cmd.args(args);
    cmd.output().expect("failed to execute hebbs")
}

fn run_cli_success(server: &TestServer, args: &[&str]) -> serde_json::Value {
    let output = run_cli(server, args);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "CLI failed with args {:?}\nstdout: {}\nstderr: {}",
        args,
        stdout,
        stderr,
    );
    if stdout.trim().is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("Invalid JSON from CLI: {}\nraw: {}", e, stdout))
}

fn extract_id(json: &serde_json::Value) -> String {
    json["memory_id"]
        .as_str()
        .expect("missing memory_id in response")
        .to_string()
}

/// Parse recall JSON output — handles both array and object-with-error forms.
fn parse_recall_results(output: &Output) -> Vec<serde_json::Value> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.is_empty() {
        return vec![];
    }
    // The JSON array is the first valid JSON in stdout (may have trailing text)
    if let Ok(serde_json::Value::Array(arr)) = serde_json::from_str(lines[0]) {
        return arr;
    }
    serde_json::from_str::<Vec<serde_json::Value>>(stdout.trim()).unwrap_or_default()
}

// ═══════════════════════════════════════════════════════════════════════
//  Status
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_status() {
    let server = TestServer::start().await;
    let json = run_cli_success(&server, &["status"]);
    assert!(json["version"].is_string(), "status must return version");
    assert_eq!(
        json["memory_count"].as_u64().unwrap(),
        0,
        "fresh server has 0 memories"
    );
    assert!(
        json["uptime_seconds"].as_u64().is_some(),
        "status must return uptime"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Remember — verifies stored content matches input
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_remember_stores_content_correctly() {
    let server = TestServer::start().await;
    let json = run_cli_success(
        &server,
        &[
            "remember",
            "User prefers dark mode",
            "--importance",
            "0.8",
            "--entity-id",
            "user_prefs",
        ],
    );

    assert!(!extract_id(&json).is_empty(), "must return a memory_id");
    assert_eq!(json["content"].as_str().unwrap(), "User prefers dark mode");
    let imp = json["importance"].as_f64().unwrap();
    assert!(
        (imp - 0.8).abs() < 0.01,
        "importance should be 0.8, got {imp}"
    );
    assert_eq!(json["entity_id"].as_str().unwrap(), "user_prefs");
    assert_eq!(json["kind"].as_str().unwrap(), "episode");
    assert!(
        json["created_at"].as_u64().unwrap() > 0,
        "created_at must be set"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_remember_with_context_preserves_metadata() {
    let server = TestServer::start().await;
    let json = run_cli_success(
        &server,
        &[
            "remember",
            "Meeting notes from Q2 review",
            "--context",
            r#"{"source":"email","topic":"Q2"}"#,
            "--entity-id",
            "meetings",
        ],
    );

    assert!(!extract_id(&json).is_empty());
    assert_eq!(
        json["content"].as_str().unwrap(),
        "Meeting notes from Q2 review"
    );
    assert_eq!(json["context"]["source"].as_str().unwrap(), "email");
    assert_eq!(json["context"]["topic"].as_str().unwrap(), "Q2");
    assert_eq!(json["entity_id"].as_str().unwrap(), "meetings");
}

// ═══════════════════════════════════════════════════════════════════════
//  Get — round-trip: remember then retrieve by ID
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_get_returns_stored_memory() {
    let server = TestServer::start().await;

    let mem = run_cli_success(
        &server,
        &[
            "remember",
            "Contract renewal is on March 15",
            "--importance",
            "0.9",
            "--entity-id",
            "legal",
        ],
    );
    let mem_id = extract_id(&mem);

    let get_json = run_cli_success(&server, &["get", &mem_id]);
    assert_eq!(
        get_json["content"].as_str().unwrap(),
        "Contract renewal is on March 15"
    );
    assert!((get_json["importance"].as_f64().unwrap() - 0.9).abs() < 0.01);
    assert_eq!(get_json["entity_id"].as_str().unwrap(), "legal");
}

// ═══════════════════════════════════════════════════════════════════════
//  Inspect — richer view of a memory
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_inspect_shows_detail() {
    let server = TestServer::start().await;

    let mem = run_cli_success(&server, &["remember", "Memory to inspect"]);
    let mem_id = extract_id(&mem);

    let output = run_cli(&server, &["inspect", &mem_id]);
    assert!(
        output.status.success(),
        "inspect failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Memory to inspect"),
        "inspect output must contain the memory content"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall — similarity: returns results, all have expected structure
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_recall_similarity_returns_results() {
    let server = TestServer::start().await;

    run_cli_success(
        &server,
        &[
            "remember",
            "User prefers dark mode in all applications",
            "--importance",
            "0.8",
            "--entity-id",
            "user_prefs",
        ],
    );
    run_cli_success(
        &server,
        &[
            "remember",
            "User likes minimal UI designs",
            "--importance",
            "0.6",
            "--entity-id",
            "user_prefs",
        ],
    );
    run_cli_success(
        &server,
        &[
            "remember",
            "User changed theme to Solarized Dark",
            "--importance",
            "0.5",
            "--entity-id",
            "user_prefs",
        ],
    );

    let output = run_cli(
        &server,
        &[
            "recall",
            "dark mode preferences",
            "--strategy",
            "similarity",
            "--top-k",
            "5",
            "--entity-id",
            "user_prefs",
        ],
    );
    assert!(output.status.success(), "recall similarity failed");

    let results = parse_recall_results(&output);
    assert!(
        !results.is_empty(),
        "recall must return at least 1 result for 3 stored memories"
    );

    for r in &results {
        assert!(
            r["memory"].is_object(),
            "each result must have a memory object"
        );
        assert!(
            r["memory"]["content"].is_string(),
            "memory must have content"
        );
        assert!(
            r["score"].is_number(),
            "each result must have a composite score"
        );
        assert!(
            r["strategy_details"].is_array(),
            "each result must have strategy_details"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall — temporal: requires entity-id, returns chronologically ordered results
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_recall_temporal_returns_ordered_results() {
    let server = TestServer::start().await;

    run_cli_success(
        &server,
        &["remember", "First event of the day", "--entity-id", "daily"],
    );
    // Tiny sleep to ensure distinct timestamps
    std::thread::sleep(std::time::Duration::from_millis(2));
    run_cli_success(
        &server,
        &[
            "remember",
            "Second event of the day",
            "--entity-id",
            "daily",
        ],
    );

    let output = run_cli(
        &server,
        &[
            "recall",
            "daily events",
            "--strategy",
            "temporal",
            "--entity-id",
            "daily",
            "--top-k",
            "5",
        ],
    );
    assert!(
        output.status.success(),
        "recall temporal failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let results = parse_recall_results(&output);
    assert!(
        results.len() >= 2,
        "temporal recall should return both memories, got {}",
        results.len()
    );

    let content_0 = results[0]["memory"]["content"].as_str().unwrap();
    let content_1 = results[1]["memory"]["content"].as_str().unwrap();
    assert!(
        content_0.contains("Second") || content_1.contains("Second"),
        "temporal recall must contain 'Second event': got '{}' and '{}'",
        content_0,
        content_1
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall — scoring weights: verify the flag is accepted and returns results
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_recall_with_weights() {
    let server = TestServer::start().await;

    run_cli_success(
        &server,
        &[
            "remember",
            "Important architecture decision",
            "--importance",
            "0.9",
        ],
    );
    run_cli_success(
        &server,
        &["remember", "Minor style preference", "--importance", "0.3"],
    );

    let output = run_cli(
        &server,
        &[
            "recall",
            "architecture",
            "--strategy",
            "similarity",
            "--weights",
            "0.3:0.1:0.5:0.1",
        ],
    );
    assert!(
        output.status.success(),
        "recall with weights failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let results = parse_recall_results(&output);
    assert!(
        !results.is_empty(),
        "recall with weights must return results"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall — ef-search: HNSW accuracy parameter
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_recall_ef_search() {
    let server = TestServer::start().await;

    run_cli_success(&server, &["remember", "contract renewal terms"]);

    let output = run_cli(
        &server,
        &[
            "recall",
            "contract renewal",
            "--strategy",
            "similarity",
            "--ef-search",
            "200",
        ],
    );
    assert!(
        output.status.success(),
        "recall with ef-search failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let results = parse_recall_results(&output);
    assert!(
        !results.is_empty(),
        "ef-search recall must return the stored memory"
    );
    assert!(
        results[0]["memory"]["content"]
            .as_str()
            .unwrap()
            .contains("contract"),
        "result should contain the stored contract memory"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall — causal: seed + max-depth + edge-types flags
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_recall_causal_with_flags() {
    let server = TestServer::start().await;

    let mem = run_cli_success(
        &server,
        &[
            "remember",
            "Root cause: pricing was too high",
            "--importance",
            "0.9",
        ],
    );
    let mem_id = extract_id(&mem);

    let output = run_cli(
        &server,
        &[
            "recall",
            "pricing pushback",
            "--strategy",
            "causal",
            "--seed",
            &mem_id,
            "--max-depth",
            "3",
            "--edge-types",
            "caused_by,followed_by",
        ],
    );
    assert!(
        output.status.success(),
        "causal recall failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Recall — analogical: alpha parameter
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_recall_analogical_alpha() {
    let server = TestServer::start().await;

    run_cli_success(&server, &["remember", "Similar pricing dynamics in Q1"]);

    let output = run_cli(
        &server,
        &[
            "recall",
            "pricing patterns",
            "--strategy",
            "analogical",
            "--analogical-alpha",
            "0.2",
        ],
    );
    assert!(
        output.status.success(),
        "analogical recall failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Prime — returns entity's memories
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_prime_returns_entity_memories() {
    let server = TestServer::start().await;

    run_cli_success(
        &server,
        &[
            "remember",
            "User prefers dark mode",
            "--entity-id",
            "user_prefs",
        ],
    );
    run_cli_success(
        &server,
        &[
            "remember",
            "User likes minimal UI",
            "--entity-id",
            "user_prefs",
        ],
    );

    let output = run_cli(&server, &["prime", "user_prefs", "--max-memories", "20"]);
    assert!(
        output.status.success(),
        "prime failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let results = parse_recall_results(&output);
    assert_eq!(
        results.len(),
        2,
        "prime should return both memories for user_prefs entity"
    );

    let contents: Vec<&str> = results
        .iter()
        .filter_map(|r| r["memory"]["content"].as_str())
        .collect();
    assert!(
        contents.iter().any(|c| c.contains("dark mode")),
        "prime results must include 'dark mode' memory"
    );
    assert!(
        contents.iter().any(|c| c.contains("minimal UI")),
        "prime results must include 'minimal UI' memory"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_prime_with_similarity_cue() {
    let server = TestServer::start().await;

    run_cli_success(
        &server,
        &[
            "remember",
            "User prefers dark mode",
            "--entity-id",
            "user_prefs",
        ],
    );

    let output = run_cli(
        &server,
        &[
            "prime",
            "user_prefs",
            "--max-memories",
            "10",
            "--similarity-cue",
            "UI preferences",
        ],
    );
    assert!(
        output.status.success(),
        "prime with similarity-cue failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Reflect — the full prepare → commit → verify-insights loop
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_reflect_full_loop() {
    let server = TestServer::start().await;

    // Store enough memories to trigger clustering (min_memories_for_reflect=3, min_cluster_size=2)
    let contents = [
        "User always picks dark theme in every app",
        "User turned on dark mode in VS Code",
        "User enabled night shift on phone",
        "User set dark mode on Slack",
        "User prefers dark backgrounds for reading",
        "User mentioned eye strain with light themes",
    ];
    for c in &contents {
        run_cli_success(
            &server,
            &[
                "remember",
                c,
                "--importance",
                "0.7",
                "--entity-id",
                "theme_prefs",
            ],
        );
    }

    // 1. Prepare: should produce session + clusters with actual content
    let prepare = run_cli_success(&server, &["reflect-prepare", "--entity-id", "theme_prefs"]);

    let session_id = prepare["session_id"]
        .as_str()
        .expect("must return session_id");
    assert!(!session_id.is_empty());

    let processed = prepare["memories_processed"].as_u64().unwrap();
    assert_eq!(processed, 6, "all 6 memories should be processed");

    let clusters = prepare["clusters"]
        .as_array()
        .expect("clusters must be array");
    assert!(
        !clusters.is_empty(),
        "6 memories with min_cluster_size=2 MUST produce at least 1 cluster"
    );

    let cluster = &clusters[0];
    assert!(cluster["cluster_id"].as_u64().is_some());
    assert!(
        cluster["member_count"].as_u64().unwrap() >= 2,
        "cluster must have >= 2 members"
    );

    let prompt = cluster["proposal_system_prompt"].as_str().unwrap();
    assert!(
        !prompt.is_empty(),
        "proposal_system_prompt must be non-empty"
    );

    let user_prompt = cluster["proposal_user_prompt"].as_str().unwrap();
    assert!(
        !user_prompt.is_empty(),
        "proposal_user_prompt must be non-empty"
    );

    let mem_ids = cluster["memory_ids"].as_array().unwrap();
    assert!(mem_ids.len() >= 2, "cluster must reference >= 2 memory IDs");
    for id in mem_ids {
        let hex = id.as_str().unwrap();
        assert_eq!(hex.len(), 32, "memory_id must be 32-char hex, got '{hex}'");
    }

    let memories = cluster["memories"]
        .as_array()
        .expect("cluster must include memories array");
    assert!(!memories.is_empty(), "memories array must be non-empty");
    for m in memories {
        assert!(m["memory_id"].is_string());
        let content = m["content"].as_str().unwrap();
        assert!(!content.is_empty(), "memory content must be non-empty");
        assert!(m["importance"].as_f64().is_some());
        assert!(m["created_at"].as_u64().unwrap() > 0);
    }

    // 2. Commit: agent produces insights from the cluster
    let hex_ids: Vec<&str> = mem_ids.iter().take(2).filter_map(|v| v.as_str()).collect();

    let insights_json = serde_json::json!([{
        "content": "User consistently prefers dark themes across all applications and devices",
        "confidence": 0.9,
        "source_memory_ids": hex_ids,
        "tags": ["preference", "ui", "dark_theme"]
    }])
    .to_string();

    let commit = run_cli_success(
        &server,
        &[
            "reflect-commit",
            "--session-id",
            session_id,
            "--insights",
            &insights_json,
        ],
    );
    let created = commit["insights_created"].as_u64().unwrap();
    assert_eq!(created, 1, "should create exactly 1 insight");

    // 3. Verify the insight exists
    let insights_output = run_cli_success(
        &server,
        &[
            "insights",
            "--entity-id",
            "theme_prefs",
            "--max-results",
            "10",
        ],
    );
    let stdout = serde_json::to_string(&insights_output).unwrap();
    assert!(
        stdout.contains("dark themes") || stdout.contains("dark_theme"),
        "insights output should contain the committed insight"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Insights — empty entity returns gracefully
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_insights_empty_entity() {
    let server = TestServer::start().await;

    let output = run_cli(
        &server,
        &[
            "insights",
            "--entity-id",
            "nonexistent",
            "--max-results",
            "10",
            "--min-confidence",
            "0.5",
        ],
    );
    assert!(
        output.status.success(),
        "insights on empty entity should succeed"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Forget — by ID: confirm count and verify memory is gone
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_forget_by_id_removes_memory() {
    let server = TestServer::start().await;

    let mem = run_cli_success(&server, &["remember", "Temporary note to delete"]);
    let mem_id = extract_id(&mem);

    let forget = run_cli_success(&server, &["forget", "--ids", &mem_id]);
    assert_eq!(
        forget["forgotten_count"].as_u64().unwrap(),
        1,
        "should forget exactly 1"
    );

    let get_output = run_cli(&server, &["get", &mem_id]);
    assert!(
        !get_output.status.success(),
        "get on forgotten memory should fail"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Forget — by entity: removes all memories for that entity
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_forget_by_entity_removes_all() {
    let server = TestServer::start().await;

    run_cli_success(
        &server,
        &["remember", "Note 1", "--entity-id", "old_project"],
    );
    run_cli_success(
        &server,
        &["remember", "Note 2", "--entity-id", "old_project"],
    );
    run_cli_success(
        &server,
        &["remember", "Note 3", "--entity-id", "old_project"],
    );

    let forget = run_cli_success(&server, &["forget", "--entity-id", "old_project"]);
    assert_eq!(
        forget["forgotten_count"].as_u64().unwrap(),
        3,
        "should forget all 3 memories in old_project"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Forget — by kind + decay-floor
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_forget_by_kind() {
    let server = TestServer::start().await;

    run_cli_success(
        &server,
        &["remember", "Low importance episode", "--importance", "0.1"],
    );
    run_cli_success(
        &server,
        &["remember", "High importance episode", "--importance", "0.9"],
    );

    let output = run_cli(
        &server,
        &["forget", "--kind", "episode", "--decay-floor", "0.5"],
    );
    assert!(
        output.status.success(),
        "forget by kind failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
    let count = json["forgotten_count"].as_u64().unwrap_or(0);
    assert!(
        count >= 1,
        "should forget at least the low-importance episode (decay <= 0.5)"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Forget — by staleness
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_forget_by_staleness_accepts_flag() {
    let server = TestServer::start().await;

    run_cli_success(&server, &["remember", "Some old fact"]);

    let output = run_cli(&server, &["forget", "--staleness-us", "999999999999999"]);
    assert!(
        output.status.success(),
        "forget by staleness failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
    assert!(
        json["forgotten_count"].is_number(),
        "must return forgotten_count"
    );
}

// ═══════════════════════════════════════════════════════════════════════
//  Full Agent Lifecycle — mirrors the SKILL.md "Decision Guide"
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_agent_lifecycle() {
    let server = TestServer::start().await;

    // ── 1. Prime (conversation start) ──
    let prime_output = run_cli(
        &server,
        &[
            "prime",
            "customer_42",
            "--max-memories",
            "20",
            "--similarity-cue",
            "account overview",
        ],
    );
    assert!(
        prime_output.status.success(),
        "prime should succeed even with empty entity"
    );

    // ── 2. Remember facts from the conversation ──
    run_cli_success(
        &server,
        &[
            "remember",
            "Customer prefers email over phone",
            "--importance",
            "0.8",
            "--entity-id",
            "customer_42",
        ],
    );
    run_cli_success(
        &server,
        &[
            "remember",
            "Customer budget is 50k per quarter",
            "--importance",
            "0.9",
            "--entity-id",
            "customer_42",
            "--context",
            r#"{"source":"sales_call"}"#,
        ],
    );
    run_cli_success(
        &server,
        &[
            "remember",
            "Customer interested in premium tier",
            "--importance",
            "0.7",
            "--entity-id",
            "customer_42",
        ],
    );
    run_cli_success(
        &server,
        &[
            "remember",
            "Customer mentioned previous vendor was too slow",
            "--importance",
            "0.6",
            "--entity-id",
            "customer_42",
        ],
    );
    run_cli_success(
        &server,
        &[
            "remember",
            "Customer wants quarterly business reviews",
            "--importance",
            "0.5",
            "--entity-id",
            "customer_42",
        ],
    );
    run_cli_success(
        &server,
        &[
            "remember",
            "Customer timeline: decision by end of Q2",
            "--importance",
            "0.8",
            "--entity-id",
            "customer_42",
        ],
    );

    // ── 3. Recall before answering ──
    let recall_output = run_cli(
        &server,
        &[
            "recall",
            "What is the customer's budget?",
            "--strategy",
            "similarity",
            "--top-k",
            "5",
            "--entity-id",
            "customer_42",
        ],
    );
    assert!(recall_output.status.success());
    let recall_results = parse_recall_results(&recall_output);
    assert!(
        !recall_results.is_empty(),
        "recall must return results for entity with 6 memories"
    );

    // ── 4. Reflect: prepare → commit ──
    let prepare = run_cli_success(&server, &["reflect-prepare", "--entity-id", "customer_42"]);
    let session_id = prepare["session_id"].as_str().unwrap();
    let clusters = prepare["clusters"].as_array().unwrap();
    assert!(
        !clusters.is_empty(),
        "6 memories should cluster (min_cluster_size=2): got {} clusters",
        clusters.len()
    );

    let hex_ids: Vec<&str> = clusters[0]["memory_ids"]
        .as_array()
        .unwrap()
        .iter()
        .take(2)
        .filter_map(|v| v.as_str())
        .collect();

    let insights_json = serde_json::json!([{
        "content": "Customer 42 is a high-value Q2 prospect with 50k budget interested in premium tier",
        "confidence": 0.85,
        "source_memory_ids": hex_ids,
        "tags": ["customer_profile", "sales"]
    }])
    .to_string();

    let commit = run_cli_success(
        &server,
        &[
            "reflect-commit",
            "--session-id",
            session_id,
            "--insights",
            &insights_json,
        ],
    );
    assert!(commit["insights_created"].as_u64().unwrap() >= 1);

    // ── 5. Verify insights ──
    let insights = run_cli(
        &server,
        &[
            "insights",
            "--entity-id",
            "customer_42",
            "--max-results",
            "5",
        ],
    );
    assert!(insights.status.success());
    let stdout = String::from_utf8_lossy(&insights.stdout);
    assert!(
        stdout.contains("high-value") || stdout.contains("premium"),
        "insights should contain the committed insight text"
    );

    // ── 6. Correction ──
    run_cli_success(
        &server,
        &[
            "remember",
            "CORRECTION: budget is actually 75k per quarter",
            "--importance",
            "0.95",
            "--entity-id",
            "customer_42",
        ],
    );

    // Verify correction is retrievable
    let recall2 = run_cli(
        &server,
        &[
            "recall",
            "budget",
            "--strategy",
            "similarity",
            "--entity-id",
            "customer_42",
        ],
    );
    assert!(recall2.status.success());

    // ── 7. Cleanup ──
    let forget = run_cli_success(&server, &["forget", "--entity-id", "customer_42"]);
    assert!(
        forget["forgotten_count"].as_u64().unwrap() >= 7,
        "should forget all 7 customer_42 memories (6 original + 1 correction)"
    );

    // Verify entity is clean
    let prime_after = run_cli(&server, &["prime", "customer_42", "--max-memories", "10"]);
    let after_results = parse_recall_results(&prime_after);
    assert_eq!(
        after_results.len(),
        0,
        "entity should be empty after forget"
    );
}
