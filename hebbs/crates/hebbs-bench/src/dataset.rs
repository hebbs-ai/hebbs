use std::collections::HashMap;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use hebbs_core::engine::RememberInput;

const DOMAINS: &[&str] = &["sales", "engineering", "support"];
const ACTIONS: &[&str] = &[
    "discussed",
    "reported",
    "resolved",
    "escalated",
    "requested",
    "proposed",
    "completed",
    "reviewed",
    "approved",
    "scheduled",
];
const TOPICS: &[&str] = &[
    "Q4 budget",
    "server migration",
    "client onboarding",
    "API redesign",
    "security audit",
    "performance tuning",
    "data pipeline",
    "feature release",
    "compliance review",
    "team restructuring",
    "product launch",
    "contract renewal",
    "infrastructure upgrade",
    "customer feedback",
    "roadmap planning",
];
const ENTITIES: &[&str] = &[
    "entity_alpha",
    "entity_beta",
    "entity_gamma",
    "entity_delta",
    "entity_epsilon",
];

pub fn generate_memories(count: usize, seed: u64) -> Vec<RememberInput> {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut memories = Vec::with_capacity(count);

    for i in 0..count {
        let domain = DOMAINS[rng.gen_range(0..DOMAINS.len())];
        let action = ACTIONS[rng.gen_range(0..ACTIONS.len())];
        let topic = TOPICS[rng.gen_range(0..TOPICS.len())];
        let entity = ENTITIES[rng.gen_range(0..ENTITIES.len())];

        let content = format!(
            "{} team {} {} during sprint {} meeting. Priority level: {}. Follow-up needed by end of week.",
            domain,
            action,
            topic,
            rng.gen_range(1..100),
            rng.gen_range(1..5)
        );

        let importance = 0.1 + rng.gen::<f32>() * 0.9;

        let mut context = HashMap::new();
        context.insert("domain".to_string(), serde_json::json!(domain));
        context.insert("index".to_string(), serde_json::json!(i));
        context.insert(
            "priority".to_string(),
            serde_json::json!(rng.gen_range(1..5)),
        );

        memories.push(RememberInput {
            content,
            importance: Some(importance),
            context: Some(context),
            entity_id: Some(entity.to_string()),
            edges: Vec::new(),
            kind: None,
        });
    }

    memories
}
