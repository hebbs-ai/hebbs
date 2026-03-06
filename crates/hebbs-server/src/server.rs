use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::signal;
use tonic::transport::Server as TonicServer;
use tracing::{error, info, warn};

use hebbs_core::auth::KeyCache;
use hebbs_core::engine::Engine;
use hebbs_core::rate_limit::RateLimiter;
use hebbs_core::reflect::ReflectConfig;
use hebbs_embed::MockEmbedder;
use hebbs_reflect::{LlmProviderConfig, ProviderType};
use hebbs_proto::generated::{
    health_service_server::HealthServiceServer, memory_service_server::MemoryServiceServer,
    reflect_service_server::ReflectServiceServer, subscribe_service_server::SubscribeServiceServer,
};
use hebbs_storage::RocksDbBackend;

use crate::config::HebbsConfig;
use crate::grpc::health_service::HealthServiceImpl;
use crate::grpc::memory_service::MemoryServiceImpl;
use crate::grpc::reflect_service::ReflectServiceImpl;
use crate::grpc::subscribe_service::SubscribeServiceImpl;
use crate::metrics::HebbsMetrics;
use crate::middleware::{self, AuthState};
use crate::rest;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn run(config: HebbsConfig) -> Result<(), Box<dyn std::error::Error>> {
    let start_time = Instant::now();
    let metrics = Arc::new(HebbsMetrics::new());

    info!(
        data_dir = %config.storage.data_dir,
        "opening storage"
    );
    let storage = Arc::new(
        RocksDbBackend::open(&config.storage.data_dir)
            .map_err(|e| format!("failed to open storage: {}", e))?,
    );

    info!("initializing embedder (mock for Phase 8)");
    let embedder = Arc::new(MockEmbedder::new(config.embedding.dimensions));

    info!("initializing engine");
    let engine = Arc::new(
        Engine::new(storage.clone(), embedder)
            .map_err(|e| format!("failed to create engine: {}", e))?,
    );

    // Update initial memory count metric
    if let Ok(count) = engine.count() {
        metrics.memory_count.set(count as i64);
    }

    // ── Auth & rate limiting ────────────────────────────────────────────
    let key_cache = Arc::new(KeyCache::new());
    match key_cache.load_from_storage(storage.as_ref()) {
        Ok(n) => info!(key_count = n, "loaded API keys from storage"),
        Err(e) => warn!(error = %e, "failed to load API keys (auth will reject all requests)"),
    }

    let rate_limiter = Arc::new(RateLimiter::new(config.rate_limit.clone()));
    let auth_state = Arc::new(AuthState {
        key_cache,
        rate_limiter,
        auth_enabled: config.auth.enabled,
    });

    info!(
        auth_enabled = config.auth.enabled,
        rate_limit_enabled = config.rate_limit.enabled,
        "middleware configured"
    );

    // ── Addresses ───────────────────────────────────────────────────────
    let grpc_addr: SocketAddr =
        format!("{}:{}", config.server.bind_address, config.server.grpc_port)
            .parse()
            .map_err(|e| format!("invalid gRPC address: {}", e))?;

    let http_addr: SocketAddr =
        format!("{}:{}", config.server.bind_address, config.server.http_port)
            .parse()
            .map_err(|e| format!("invalid HTTP address: {}", e))?;

    // ── gRPC services ───────────────────────────────────────────────────
    let memory_svc = MemoryServiceImpl {
        engine: engine.clone(),
        metrics: metrics.clone(),
        auth_state: auth_state.clone(),
    };

    let subscribe_svc =
        SubscribeServiceImpl::new(engine.clone(), metrics.clone(), auth_state.clone());

    let proposal_provider_config = build_llm_provider_config(
        &config.reflect.proposal_provider,
        &config.reflect.proposal_model,
    );
    let validation_provider_config = build_llm_provider_config(
        &config.reflect.validation_provider,
        &config.reflect.validation_model,
    );
    if proposal_provider_config.provider_type != ProviderType::Mock {
        info!(
            provider = %config.reflect.proposal_provider,
            model = %config.reflect.proposal_model,
            "reflect proposal LLM configured"
        );
    }
    let reflect_config = ReflectConfig {
        max_memories_per_reflect: config.reflect.max_memories_per_reflect,
        min_memories_for_reflect: config.reflect.min_memories_for_reflect,
        threshold_trigger_count: config.reflect.threshold_trigger_count,
        schedule_trigger_interval_us: config.reflect.schedule_trigger_interval_secs
            .saturating_mul(1_000_000),
        trigger_check_interval_us: config.reflect.trigger_check_interval_secs
            .saturating_mul(1_000_000),
        enabled: config.reflect.enabled,
        proposal_provider_config,
        validation_provider_config,
        ..ReflectConfig::default()
    }
    .validated();
    let proposal_provider = Arc::from(
        hebbs_reflect::create_provider(&reflect_config.proposal_provider_config)
            .expect("failed to create reflect proposal LLM provider"),
    );
    let validation_provider = Arc::from(
        hebbs_reflect::create_provider(&reflect_config.validation_provider_config)
            .expect("failed to create reflect validation LLM provider"),
    );
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
        start_time,
        version: VERSION.to_string(),
    };

    let grpc_interceptor = middleware::grpc_auth_interceptor(auth_state.clone());

    // ── REST router ─────────────────────────────────────────────────────
    let app_state = rest::AppState {
        engine: engine.clone(),
        metrics: metrics.clone(),
        start_time,
        version: VERSION.to_string(),
        sse_subscriptions: std::sync::Arc::new(parking_lot::Mutex::new(
            std::collections::HashMap::new(),
        )),
    };

    let http_router = rest::create_router(app_state)
        .layer(axum::middleware::from_fn(middleware::rate_limit_middleware))
        .layer(axum::middleware::from_fn(middleware::rest_auth_middleware))
        .layer(axum::Extension(auth_state.clone()));

    info!(addr = %grpc_addr, "starting gRPC server");
    info!(addr = %http_addr, "starting HTTP server");

    let grpc_server = TonicServer::builder()
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
        .serve_with_shutdown(grpc_addr, shutdown_signal());

    let http_listener = tokio::net::TcpListener::bind(http_addr).await?;
    let http_server =
        axum::serve(http_listener, http_router).with_graceful_shutdown(shutdown_signal());

    info!(
        version = VERSION,
        grpc_port = config.server.grpc_port,
        http_port = config.server.http_port,
        "HEBBS server ready"
    );

    tokio::select! {
        result = grpc_server => {
            if let Err(e) = result {
                error!(error = %e, "gRPC server error");
            }
        }
        result = http_server => {
            if let Err(e) = result {
                error!(error = %e, "HTTP server error");
            }
        }
    }

    info!("shutting down background workers");
    engine.stop_decay();
    engine.stop_reflect();

    info!(
        elapsed_ms = start_time.elapsed().as_millis() as u64,
        "HEBBS server stopped cleanly"
    );

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received");
}

fn build_llm_provider_config(provider_name: &str, model: &str) -> LlmProviderConfig {
    let provider_type = ProviderType::from_name(provider_name);
    let api_key = match provider_type {
        ProviderType::OpenAi => std::env::var("OPENAI_API_KEY").ok(),
        ProviderType::Anthropic => std::env::var("ANTHROPIC_API_KEY").ok(),
        _ => None,
    };
    LlmProviderConfig {
        provider_type,
        api_key,
        base_url: None,
        model: model.to_string(),
        timeout_secs: 120,
        max_retries: 2,
        retry_backoff_ms: 1000,
    }
}
