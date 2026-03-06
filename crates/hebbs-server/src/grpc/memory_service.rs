use std::sync::Arc;

use tonic::{Request, Response, Status};
use tracing::instrument;

use hebbs_core::auth::{PERM_ADMIN, PERM_READ, PERM_WRITE};
use hebbs_core::engine::Engine;
use hebbs_proto::generated as pb;
use pb::memory_service_server::MemoryService;

use crate::convert;
use crate::metrics::HebbsMetrics;
use crate::middleware::{self, AuthState};

pub struct MemoryServiceImpl {
    pub engine: Arc<Engine>,
    pub metrics: Arc<HebbsMetrics>,
    pub auth_state: Arc<AuthState>,
}

#[tonic::async_trait]
impl MemoryService for MemoryServiceImpl {
    #[instrument(skip_all, fields(operation = "remember"))]
    async fn remember(
        &self,
        request: Request<pb::RememberRequest>,
    ) -> Result<Response<pb::RememberResponse>, Status> {
        let start = std::time::Instant::now();
        let tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_WRITE)?;
        middleware::check_rate_limit(&self.auth_state, &tenant, "remember")?;
        let req = request.into_inner();

        let input = convert::proto_to_remember_input(req).map_err(Status::invalid_argument)?;

        let engine = self.engine.clone();
        let result =
            tokio::task::spawn_blocking(move || engine.remember_for_tenant(&tenant, input))
                .await
                .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(memory) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("remember", "ok", elapsed);
                self.metrics.memory_count.inc();
                Ok(Response::new(pb::RememberResponse {
                    memory: Some(convert::memory_to_proto(&memory)),
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("remember", "error", elapsed);
                self.metrics.record_error("remember", &error_type_label(&e));
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }

    #[instrument(skip_all, fields(operation = "get"))]
    async fn get(
        &self,
        request: Request<pb::GetRequest>,
    ) -> Result<Response<pb::GetResponse>, Status> {
        let start = std::time::Instant::now();
        let tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_READ)?;
        middleware::check_rate_limit(&self.auth_state, &tenant, "get")?;
        let req = request.into_inner();

        if req.memory_id.len() != 16 {
            return Err(Status::invalid_argument(format!(
                "memory_id must be 16 bytes, got {}",
                req.memory_id.len()
            )));
        }

        let engine = self.engine.clone();
        let memory_id = req.memory_id;
        let result =
            tokio::task::spawn_blocking(move || engine.get_for_tenant(&tenant, &memory_id))
                .await
                .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(memory) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("get", "ok", elapsed);
                Ok(Response::new(pb::GetResponse {
                    memory: Some(convert::memory_to_proto(&memory)),
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("get", "error", elapsed);
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }

    #[instrument(skip_all, fields(operation = "recall"))]
    async fn recall(
        &self,
        request: Request<pb::RecallRequest>,
    ) -> Result<Response<pb::RecallResponse>, Status> {
        let start = std::time::Instant::now();
        let tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_READ)?;
        middleware::check_rate_limit(&self.auth_state, &tenant, "recall")?;
        let req = request.into_inner();

        let input = convert::proto_to_recall_input(req).map_err(Status::invalid_argument)?;

        let engine = self.engine.clone();
        let result = tokio::task::spawn_blocking(move || engine.recall_for_tenant(&tenant, input))
            .await
            .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(output) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("recall", "ok", elapsed);

                let results: Vec<pb::RecallResult> = output
                    .results
                    .iter()
                    .map(convert::recall_result_to_proto)
                    .collect();

                let strategy_errors: Vec<pb::StrategyErrorMessage> = output
                    .strategy_errors
                    .into_iter()
                    .map(|e| pb::StrategyErrorMessage {
                        strategy: recall_strategy_to_proto_i32(&e.strategy),
                        message: e.message,
                    })
                    .collect();

                Ok(Response::new(pb::RecallResponse {
                    results,
                    strategy_errors,
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("recall", "error", elapsed);
                self.metrics.record_error("recall", &error_type_label(&e));
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }

    #[instrument(skip_all, fields(operation = "prime"))]
    async fn prime(
        &self,
        request: Request<pb::PrimeRequest>,
    ) -> Result<Response<pb::PrimeResponse>, Status> {
        let start = std::time::Instant::now();
        let tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_READ)?;
        middleware::check_rate_limit(&self.auth_state, &tenant, "prime")?;
        let req = request.into_inner();

        let input = convert::proto_to_prime_input(req).map_err(Status::invalid_argument)?;

        let engine = self.engine.clone();
        let result = tokio::task::spawn_blocking(move || engine.prime_for_tenant(&tenant, input))
            .await
            .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(output) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("prime", "ok", elapsed);

                let results: Vec<pb::RecallResult> = output
                    .results
                    .iter()
                    .map(convert::recall_result_to_proto)
                    .collect();

                Ok(Response::new(pb::PrimeResponse {
                    results,
                    temporal_count: output.temporal_count as u32,
                    similarity_count: output.similarity_count as u32,
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("prime", "error", elapsed);
                self.metrics.record_error("prime", &error_type_label(&e));
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }

    #[instrument(skip_all, fields(operation = "revise"))]
    async fn revise(
        &self,
        request: Request<pb::ReviseRequest>,
    ) -> Result<Response<pb::ReviseResponse>, Status> {
        let start = std::time::Instant::now();
        let tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_WRITE)?;
        middleware::check_rate_limit(&self.auth_state, &tenant, "revise")?;
        let req = request.into_inner();

        let input = convert::proto_to_revise_input(req).map_err(Status::invalid_argument)?;

        let engine = self.engine.clone();
        let result = tokio::task::spawn_blocking(move || engine.revise_for_tenant(&tenant, input))
            .await
            .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(memory) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("revise", "ok", elapsed);
                Ok(Response::new(pb::ReviseResponse {
                    memory: Some(convert::memory_to_proto(&memory)),
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("revise", "error", elapsed);
                self.metrics.record_error("revise", &error_type_label(&e));
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }

    #[instrument(skip_all, fields(operation = "forget"))]
    async fn forget(
        &self,
        request: Request<pb::ForgetRequest>,
    ) -> Result<Response<pb::ForgetResponse>, Status> {
        let start = std::time::Instant::now();
        let tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_WRITE)?;
        middleware::check_rate_limit(&self.auth_state, &tenant, "forget")?;
        let req = request.into_inner();

        let criteria = convert::proto_to_forget_criteria(req).map_err(Status::invalid_argument)?;

        let engine = self.engine.clone();
        let result =
            tokio::task::spawn_blocking(move || engine.forget_for_tenant(&tenant, criteria))
                .await
                .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(output) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("forget", "ok", elapsed);
                self.metrics.memory_count.sub(output.forgotten_count as i64);
                Ok(Response::new(pb::ForgetResponse {
                    forgotten_count: output.forgotten_count as u64,
                    cascade_count: output.cascade_count as u64,
                    truncated: output.truncated,
                    tombstone_count: output.tombstone_count as u64,
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("forget", "error", elapsed);
                self.metrics.record_error("forget", &error_type_label(&e));
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }

    #[instrument(skip_all, fields(operation = "set_policy"))]
    async fn set_policy(
        &self,
        request: Request<pb::SetPolicyRequest>,
    ) -> Result<Response<pb::SetPolicyResponse>, Status> {
        middleware::check_permission(&request, PERM_ADMIN)?;
        let _req = request.into_inner();

        // Policy enforcement is a Phase 13 stub — always acknowledges.
        Ok(Response::new(pb::SetPolicyResponse { applied: true }))
    }
}

fn recall_strategy_to_proto_i32(s: &hebbs_core::recall::RecallStrategy) -> i32 {
    use hebbs_core::recall::RecallStrategy;
    match s {
        RecallStrategy::Similarity => pb::RecallStrategyType::Similarity as i32,
        RecallStrategy::Temporal => pb::RecallStrategyType::Temporal as i32,
        RecallStrategy::Causal => pb::RecallStrategyType::Causal as i32,
        RecallStrategy::Analogical => pb::RecallStrategyType::Analogical as i32,
    }
}

fn error_type_label(e: &hebbs_core::error::HebbsError) -> String {
    use hebbs_core::error::HebbsError;
    match e {
        HebbsError::MemoryNotFound { .. } => "not_found".to_string(),
        HebbsError::InvalidInput { .. } => "invalid_input".to_string(),
        HebbsError::Storage(_) => "storage".to_string(),
        HebbsError::Embedding(_) => "embedding".to_string(),
        HebbsError::Index(_) => "index".to_string(),
        HebbsError::Reflect(_) => "reflect".to_string(),
        HebbsError::Serialization { .. } => "serialization".to_string(),
        HebbsError::Internal { .. } => "internal".to_string(),
        HebbsError::Unauthorized { .. } => "unauthorized".to_string(),
        HebbsError::Forbidden { .. } => "forbidden".to_string(),
        HebbsError::RateLimited { .. } => "rate_limited".to_string(),
        HebbsError::TenantNotFound { .. } => "tenant_not_found".to_string(),
        _ => "unknown".to_string(),
    }
}
