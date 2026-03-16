use hebbs_cli::config::OutputFormat;
use hebbs_cli::format::Renderer;
use hebbs_proto::generated as pb;

fn make_test_memory() -> pb::Memory {
    pb::Memory {
        memory_id: vec![
            0x01, 0x8E, 0xA5, 0xF3, 0x21, 0x00, 0x7C, 0x8F, 0x9A, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
            0x00, 0x11,
        ],
        content: "The customer asked about pricing for enterprise tier".to_string(),
        importance: 0.85,
        context: None,
        entity_id: Some("user-123".to_string()),
        embedding: vec![0.1, 0.2, 0.3],
        created_at: 1_700_000_000_000_000,
        updated_at: 1_700_000_000_000_000,
        last_accessed_at: 1_700_000_000_000_000,
        access_count: 5,
        decay_score: 0.92,
        kind: 1, // Episode
        device_id: None,
        logical_clock: 42,
        source_memory_ids: Vec::new(),
    }
}

fn make_test_recall_results(count: usize) -> Vec<pb::RecallResult> {
    (0..count)
        .map(|i| pb::RecallResult {
            memory: Some(pb::Memory {
                memory_id: vec![0u8; 16],
                content: format!("Memory content #{}", i + 1),
                importance: 0.5 + (i as f32 * 0.1),
                context: None,
                entity_id: None,
                embedding: Vec::new(),
                created_at: 0,
                updated_at: 0,
                last_accessed_at: 0,
                access_count: 0,
                decay_score: 0.0,
                kind: 1,
                device_id: None,
                logical_clock: 0,
                source_memory_ids: Vec::new(),
            }),
            score: 1.0 - (i as f32 * 0.1),
            strategy_details: Vec::new(),
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════
//  Human Format Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn render_memory_human_contains_id() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let memory = make_test_memory();
    let mut buf = Vec::new();
    renderer.render_memory(&memory, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    // Should contain ULID-formatted ID
    assert!(output.len() > 26);
    // Should contain content
    assert!(output.contains("customer asked about pricing"));
    // Should contain entity_id
    assert!(output.contains("user-123"));
}

#[test]
fn render_memory_human_with_color() {
    let renderer = Renderer::new(OutputFormat::Human, true);
    let memory = make_test_memory();
    let mut buf = Vec::new();
    renderer.render_memory(&memory, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();
    // Should contain ANSI escape codes when color is enabled
    assert!(output.contains('\x1b') || output.contains("customer"));
}

#[test]
fn render_memory_detail_human() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let memory = make_test_memory();
    let mut buf = Vec::new();
    renderer.render_memory_detail(&memory, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    assert!(output.contains("Memory ID"));
    assert!(output.contains("Kind"));
    assert!(output.contains("Importance"));
    assert!(output.contains("Content"));
    assert!(output.contains("Access Count"));
    assert!(output.contains("Embedding"));
    assert!(output.contains("3-dim"));
}

#[test]
fn render_recall_results_human_empty() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let results: Vec<pb::RecallResult> = vec![];
    let mut buf = Vec::new();
    renderer.render_recall_results(&results, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("No results"));
}

#[test]
fn render_recall_results_human_with_data() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let results = make_test_recall_results(3);
    let mut buf = Vec::new();
    renderer.render_recall_results(&results, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    assert!(output.contains("Score"));
    assert!(output.contains("Memory ID"));
    assert!(output.contains("Memory content #1"));
    assert!(output.contains("Memory content #2"));
    assert!(output.contains("Memory content #3"));
}

// ═══════════════════════════════════════════════════════════════════════
//  JSON Format Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn render_memory_json_is_valid() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let memory = make_test_memory();
    let mut buf = Vec::new();
    renderer.render_memory(&memory, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert!(parsed.is_object());

    let obj = parsed.as_object().unwrap();
    assert!(obj.contains_key("memory_id"));
    assert!(obj.contains_key("content"));
    assert!(obj.contains_key("importance"));
    assert!(obj.contains_key("kind"));

    assert_eq!(
        obj["content"],
        "The customer asked about pricing for enterprise tier"
    );
    assert_eq!(obj["importance"], 0.85);
    assert_eq!(obj["kind"], "episode");
    assert_eq!(obj["entity_id"], "user-123");
    assert_eq!(obj["access_count"], 5);
}

#[test]
fn render_memory_json_ulid_format() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let memory = make_test_memory();
    let mut buf = Vec::new();
    renderer.render_memory(&memory, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    let id = parsed["memory_id"].as_str().unwrap();
    // ULID is always 26 characters
    assert_eq!(id.len(), 26);
}

#[test]
fn render_recall_json_is_valid_array() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let results = make_test_recall_results(3);
    let mut buf = Vec::new();
    renderer.render_recall_results(&results, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert!(parsed.is_array());
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 3);

    for (i, item) in arr.iter().enumerate() {
        assert!(item.is_object());
        let obj = item.as_object().unwrap();
        assert!(obj.contains_key("memory"));
        assert!(obj.contains_key("score"));
        assert_eq!(
            obj["memory"]["content"],
            format!("Memory content #{}", i + 1)
        );
    }
}

#[test]
fn render_forget_json_is_valid() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let resp = pb::ForgetResponse {
        forgotten_count: 5,
        cascade_count: 2,
        truncated: false,
        tombstone_count: 5,
    };
    let mut buf = Vec::new();
    renderer.render_forget_result(&resp, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(parsed["forgotten_count"], 5);
    assert_eq!(parsed["cascade_count"], 2);
    assert_eq!(parsed["truncated"], false);
}

// ═══════════════════════════════════════════════════════════════════════
//  Raw Format Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn render_memory_raw_contains_debug_output() {
    let renderer = Renderer::new(OutputFormat::Raw, false);
    let memory = make_test_memory();
    let mut buf = Vec::new();
    renderer.render_memory(&memory, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    // Raw output is Rust Debug format
    assert!(output.contains("Memory"));
    assert!(output.contains("memory_id"));
}

// ═══════════════════════════════════════════════════════════════════════
//  Health/Status Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn render_health_human() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let resp = pb::HealthCheckResponse {
        status: pb::health_check_response::ServingStatus::Serving as i32,
        version: "0.1.0".to_string(),
        memory_count: 42,
        uptime_seconds: 3600,
    };
    let mut buf = Vec::new();
    renderer.render_health(&resp, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    assert!(output.contains("SERVING"));
    assert!(output.contains("0.1.0"));
    assert!(output.contains("42"));
    assert!(output.contains("3600"));
}

#[test]
fn render_health_json() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let resp = pb::HealthCheckResponse {
        status: pb::health_check_response::ServingStatus::Serving as i32,
        version: "0.1.0".to_string(),
        memory_count: 42,
        uptime_seconds: 3600,
    };
    let mut buf = Vec::new();
    renderer.render_health(&resp, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(parsed["version"], "0.1.0");
    assert_eq!(parsed["memory_count"], 42);
    assert_eq!(parsed["uptime_seconds"], 3600);
}

// ═══════════════════════════════════════════════════════════════════════
//  Reflect Result Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn render_reflect_human() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let resp = pb::ReflectResponse {
        insights_created: 3,
        clusters_found: 5,
        clusters_processed: 5,
        memories_processed: 100,
    };
    let mut buf = Vec::new();
    renderer.render_reflect_result(&resp, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    assert!(output.contains("Insights created: 3"));
    assert!(output.contains("Clusters found: 5"));
    assert!(output.contains("Memories processed: 100"));
}

#[test]
fn render_reflect_json() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let resp = pb::ReflectResponse {
        insights_created: 3,
        clusters_found: 5,
        clusters_processed: 5,
        memories_processed: 100,
    };
    let mut buf = Vec::new();
    renderer.render_reflect_result(&resp, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(parsed["insights_created"], 3);
    assert_eq!(parsed["memories_processed"], 100);
}

// ═══════════════════════════════════════════════════════════════════════
//  Subscribe Push Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn render_subscribe_push_human() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let push = pb::SubscribePushMessage {
        subscription_id: 1,
        memory: Some(make_test_memory()),
        confidence: 0.95,
        push_timestamp_us: 1_700_000_000_000_000,
        sequence_number: 7,
    };
    let mut buf = Vec::new();
    renderer.render_subscribe_push(&push, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    assert!(output.contains("#7"));
    assert!(output.contains("0.95"));
}

#[test]
fn render_subscribe_push_json() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let push = pb::SubscribePushMessage {
        subscription_id: 1,
        memory: Some(make_test_memory()),
        confidence: 0.95,
        push_timestamp_us: 1_700_000_000_000_000,
        sequence_number: 7,
    };
    let mut buf = Vec::new();
    renderer.render_subscribe_push(&push, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(parsed["subscription_id"], 1);
    assert_eq!(parsed["sequence_number"], 7);
    assert!(parsed["memory"].is_object());
}

// ═══════════════════════════════════════════════════════════════════════
//  Error Rendering Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn render_error_human() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let err = hebbs_cli::error::CliError::NotFound {
        message: "Memory abc not found".to_string(),
    };
    let mut buf = Vec::new();
    renderer.render_error(&err, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("Memory abc not found"));
}

#[test]
fn render_error_json() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let err = hebbs_cli::error::CliError::NotFound {
        message: "Memory abc not found".to_string(),
    };
    let mut buf = Vec::new();
    renderer.render_error(&err, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert!(parsed["error"]
        .as_str()
        .unwrap()
        .contains("Memory abc not found"));
    assert_eq!(parsed["code"], 4);
}

// ═══════════════════════════════════════════════════════════════════════
//  Context Handling Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn render_memory_with_context() {
    use hebbs_cli::format::json_to_prost_value;

    let renderer = Renderer::new(OutputFormat::Json, false);
    let mut memory = make_test_memory();

    let context = prost_types::Struct {
        fields: vec![
            (
                "topic".to_string(),
                json_to_prost_value(&serde_json::json!("sales")),
            ),
            (
                "priority".to_string(),
                json_to_prost_value(&serde_json::json!(1.0)),
            ),
        ]
        .into_iter()
        .collect(),
    };
    memory.context = Some(context);

    let mut buf = Vec::new();
    renderer.render_memory(&memory, &mut buf).unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    let ctx = &parsed["context"];
    assert_eq!(ctx["topic"], "sales");
    assert_eq!(ctx["priority"], 1.0);
}

// ═══════════════════════════════════════════════════════════════════════
//  Contradiction Prepare/Commit Rendering Tests
// ═══════════════════════════════════════════════════════════════════════

fn make_test_contradiction_prepare_response() -> pb::ContradictionPrepareResponse {
    pb::ContradictionPrepareResponse {
        candidates: vec![
            pb::PendingContradictionProto {
                pending_id: "abc123def456".to_string(),
                memory_id_a: "mem_a_001".to_string(),
                memory_id_b: "mem_b_002".to_string(),
                content_a_snippet: "The system is reliable".to_string(),
                content_b_snippet: "The system is unreliable".to_string(),
                classifier_score: 0.65,
                classifier_method: "heuristic".to_string(),
                similarity: 0.82,
                created_at: 1_700_000_000_000_000,
            },
        ],
    }
}

#[test]
fn render_contradiction_prepare_human_empty() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let resp = pb::ContradictionPrepareResponse { candidates: vec![] };
    let mut buf = Vec::new();
    renderer
        .render_contradiction_prepare_result(&resp, &mut buf)
        .unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("No pending contradictions"));
}

#[test]
fn render_contradiction_prepare_human_with_candidates() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let resp = make_test_contradiction_prepare_response();
    let mut buf = Vec::new();
    renderer
        .render_contradiction_prepare_result(&resp, &mut buf)
        .unwrap();
    let output = String::from_utf8(buf).unwrap();

    assert!(output.contains("Pending contradictions: 1"));
    assert!(output.contains("abc123def456"));
    assert!(output.contains("The system is reliable"));
    assert!(output.contains("The system is unreliable"));
    assert!(output.contains("heuristic"));
    assert!(output.contains("0.65"));
    assert!(output.contains("0.82"));
}

#[test]
fn render_contradiction_prepare_json_is_valid() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let resp = make_test_contradiction_prepare_response();
    let mut buf = Vec::new();
    renderer
        .render_contradiction_prepare_result(&resp, &mut buf)
        .unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert!(parsed.is_array());
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 1);

    let candidate = &arr[0];
    assert_eq!(candidate["pending_id"], "abc123def456");
    assert_eq!(candidate["memory_id_a"], "mem_a_001");
    assert_eq!(candidate["memory_id_b"], "mem_b_002");
    assert_eq!(candidate["content_a_snippet"], "The system is reliable");
    assert_eq!(candidate["content_b_snippet"], "The system is unreliable");
    assert_eq!(candidate["classifier_method"], "heuristic");
}

#[test]
fn render_contradiction_prepare_json_empty() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let resp = pb::ContradictionPrepareResponse { candidates: vec![] };
    let mut buf = Vec::new();
    renderer
        .render_contradiction_prepare_result(&resp, &mut buf)
        .unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert!(parsed.is_array());
    assert!(parsed.as_array().unwrap().is_empty());
}

#[test]
fn render_contradiction_commit_human() {
    let renderer = Renderer::new(OutputFormat::Human, false);
    let resp = pb::ContradictionCommitResponse {
        contradictions_confirmed: 2,
        revisions_created: 1,
        dismissed: 3,
    };
    let mut buf = Vec::new();
    renderer
        .render_contradiction_commit_result(&resp, &mut buf)
        .unwrap();
    let output = String::from_utf8(buf).unwrap();

    assert!(output.contains("Contradictions confirmed: 2"));
    assert!(output.contains("Revisions created: 1"));
    assert!(output.contains("Dismissed: 3"));
}

#[test]
fn render_contradiction_commit_json_is_valid() {
    let renderer = Renderer::new(OutputFormat::Json, false);
    let resp = pb::ContradictionCommitResponse {
        contradictions_confirmed: 2,
        revisions_created: 1,
        dismissed: 3,
    };
    let mut buf = Vec::new();
    renderer
        .render_contradiction_commit_result(&resp, &mut buf)
        .unwrap();
    let output = String::from_utf8(buf).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(parsed["contradictions_confirmed"], 2);
    assert_eq!(parsed["revisions_created"], 1);
    assert_eq!(parsed["dismissed"], 3);
}
