use std::sync::Arc;

use tonic::{Request, Response, Status};
use tracing::instrument;

use hebbs_core::auth::{PERM_ADMIN, PERM_READ};
use hebbs_core::engine::Engine;
use hebbs_core::reflect::ReflectConfig;
use hebbs_proto::generated as pb;
use hebbs_reflect::LlmProvider;
use pb::reflect_service_server::ReflectService;

use crate::convert;
use crate::metrics::HebbsMetrics;
use crate::middleware::{self, AuthState};

pub struct ReflectServiceImpl {
    pub engine: Arc<Engine>,
    pub metrics: Arc<HebbsMetrics>,
    pub reflect_config: ReflectConfig,
    pub proposal_provider: Arc<dyn LlmProvider>,
    pub validation_provider: Arc<dyn LlmProvider>,
    pub auth_state: Arc<AuthState>,
}

#[tonic::async_trait]
impl ReflectService for ReflectServiceImpl {
    #[instrument(skip_all, fields(operation = "reflect"))]
    async fn reflect(
        &self,
        request: Request<pb::ReflectRequest>,
    ) -> Result<Response<pb::ReflectResponse>, Status> {
        let start = std::time::Instant::now();
        let tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_ADMIN)?;
        middleware::check_rate_limit(&self.auth_state, &tenant, "reflect")?;
        let req = request.into_inner();

        let scope = convert::proto_to_reflect_scope(&req).map_err(Status::invalid_argument)?;

        let engine = self.engine.clone();
        let config = self.reflect_config.clone();
        let proposal = self.proposal_provider.clone();
        let validation = self.validation_provider.clone();

        let result = tokio::task::spawn_blocking(move || {
            engine.reflect_for_tenant(&tenant, scope, &config, &*proposal, &*validation)
        })
        .await
        .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(output) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("reflect", "ok", elapsed);
                self.metrics
                    .reflect_runs
                    .with_label_values(&["success"])
                    .inc();

                Ok(Response::new(pb::ReflectResponse {
                    insights_created: output.insights_created as u64,
                    clusters_found: output.clusters_found as u64,
                    clusters_processed: output.clusters_processed as u64,
                    memories_processed: output.memories_processed as u64,
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics.observe_operation("reflect", "error", elapsed);
                self.metrics
                    .reflect_runs
                    .with_label_values(&["failure"])
                    .inc();
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }

    #[instrument(skip_all, fields(operation = "get_insights"))]
    async fn get_insights(
        &self,
        request: Request<pb::GetInsightsRequest>,
    ) -> Result<Response<pb::GetInsightsResponse>, Status> {
        let start = std::time::Instant::now();
        let tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_READ)?;
        middleware::check_rate_limit(&self.auth_state, &tenant, "insights")?;
        let req = request.into_inner();

        let filter = convert::proto_to_insights_filter(&req);

        let engine = self.engine.clone();
        let result =
            tokio::task::spawn_blocking(move || engine.insights_for_tenant(&tenant, filter))
                .await
                .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(insights) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("get_insights", "ok", elapsed);

                let proto_insights: Vec<pb::Memory> =
                    insights.iter().map(convert::memory_to_proto).collect();

                Ok(Response::new(pb::GetInsightsResponse {
                    insights: proto_insights,
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("get_insights", "error", elapsed);
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }
}
