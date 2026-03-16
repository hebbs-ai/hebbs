use std::sync::Arc;

use tonic::{Request, Response, Status};
use tracing::instrument;

use hebbs_core::auth::{PERM_ADMIN, PERM_READ, PERM_WRITE};
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
        let auth_tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_ADMIN)?;
        middleware::check_rate_limit(&self.auth_state, &auth_tenant, "reflect")?;
        let req = request.into_inner();
        let tenant = middleware::resolve_tenant(auth_tenant, req.tenant_id.as_deref())?;

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
        let auth_tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_READ)?;
        middleware::check_rate_limit(&self.auth_state, &auth_tenant, "insights")?;
        let req = request.into_inner();
        let tenant = middleware::resolve_tenant(auth_tenant, req.tenant_id.as_deref())?;

        let filter = convert::proto_to_insights_filter(&req);

        let engine = self.engine.clone();
        let tenant_clone = tenant.clone();
        let result =
            tokio::task::spawn_blocking(move || engine.insights_for_tenant(&tenant, filter))
                .await
                .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(insights) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("get_insights", "ok", elapsed);

                let lineage =
                    convert::resolve_lineage_batch(&self.engine, &tenant_clone, &insights);
                let proto_insights: Vec<pb::Memory> = insights
                    .iter()
                    .map(|m| {
                        let sources = convert::get_lineage_for_memory(&lineage, &m.memory_id);
                        convert::memory_to_proto_with_lineage(m, &sources)
                    })
                    .collect();

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

    #[instrument(skip_all, fields(operation = "reflect_prepare"))]
    async fn reflect_prepare(
        &self,
        request: Request<pb::ReflectPrepareRequest>,
    ) -> Result<Response<pb::ReflectPrepareResponse>, Status> {
        let start = std::time::Instant::now();
        let auth_tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_ADMIN)?;
        middleware::check_rate_limit(&self.auth_state, &auth_tenant, "reflect")?;
        let req = request.into_inner();
        let tenant = middleware::resolve_tenant(auth_tenant, req.tenant_id.as_deref())?;

        let scope =
            convert::proto_to_reflect_prepare_scope(&req).map_err(Status::invalid_argument)?;

        let engine = self.engine.clone();
        let config = self.reflect_config.clone();

        let result = tokio::task::spawn_blocking(move || {
            engine.reflect_prepare_for_tenant(&tenant, scope, &config)
        })
        .await
        .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(output) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("reflect_prepare", "ok", elapsed);

                let clusters: Vec<pb::ClusterPrompt> = output
                    .clusters
                    .iter()
                    .map(convert::prepared_cluster_to_proto)
                    .collect();

                Ok(Response::new(pb::ReflectPrepareResponse {
                    session_id: output.session_id,
                    memories_processed: output.memories_processed as u64,
                    clusters,
                    existing_insight_count: output.existing_insight_count as u64,
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("reflect_prepare", "error", elapsed);
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }

    #[instrument(skip_all, fields(operation = "reflect_commit"))]
    async fn reflect_commit(
        &self,
        request: Request<pb::ReflectCommitRequest>,
    ) -> Result<Response<pb::ReflectCommitResponse>, Status> {
        let start = std::time::Instant::now();
        let auth_tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_ADMIN)?;
        middleware::check_rate_limit(&self.auth_state, &auth_tenant, "reflect")?;
        let req = request.into_inner();
        let tenant = middleware::resolve_tenant(auth_tenant, req.tenant_id.as_deref())?;

        let insights: Vec<hebbs_reflect::ProducedInsight> = req
            .insights
            .iter()
            .map(convert::proto_to_produced_insight)
            .collect::<Result<Vec<_>, String>>()
            .map_err(Status::invalid_argument)?;

        let engine = self.engine.clone();
        let session_id = req.session_id.clone();

        let result = tokio::task::spawn_blocking(move || {
            engine.reflect_commit_for_tenant(&tenant, &session_id, insights)
        })
        .await
        .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(output) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("reflect_commit", "ok", elapsed);

                Ok(Response::new(pb::ReflectCommitResponse {
                    insights_created: output.insights_created as u64,
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("reflect_commit", "error", elapsed);
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }

    #[instrument(skip_all, fields(operation = "contradiction_prepare"))]
    async fn contradiction_prepare(
        &self,
        request: Request<pb::ContradictionPrepareRequest>,
    ) -> Result<Response<pb::ContradictionPrepareResponse>, Status> {
        let start = std::time::Instant::now();
        let auth_tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_READ)?;
        middleware::check_rate_limit(&self.auth_state, &auth_tenant, "contradiction")?;
        let req = request.into_inner();
        let tenant = middleware::resolve_tenant(auth_tenant, req.tenant_id.as_deref())?;

        let engine = self.engine.clone();
        let result = tokio::task::spawn_blocking(move || {
            engine.contradiction_prepare_for_tenant(&tenant)
        })
        .await
        .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(pending) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("contradiction_prepare", "ok", elapsed);

                let candidates = pending
                    .iter()
                    .map(convert::pending_contradiction_to_proto)
                    .collect();

                Ok(Response::new(pb::ContradictionPrepareResponse {
                    candidates,
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("contradiction_prepare", "error", elapsed);
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }

    #[instrument(skip_all, fields(operation = "contradiction_commit"))]
    async fn contradiction_commit(
        &self,
        request: Request<pb::ContradictionCommitRequest>,
    ) -> Result<Response<pb::ContradictionCommitResponse>, Status> {
        let start = std::time::Instant::now();
        let auth_tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_WRITE)?;
        middleware::check_rate_limit(&self.auth_state, &auth_tenant, "contradiction")?;
        let req = request.into_inner();
        let tenant = middleware::resolve_tenant(auth_tenant, req.tenant_id.as_deref())?;

        let verdicts: Vec<hebbs_core::contradict::ContradictionVerdict> = req
            .verdicts
            .iter()
            .map(convert::proto_to_contradiction_verdict)
            .collect();

        let engine = self.engine.clone();
        let result = tokio::task::spawn_blocking(move || {
            engine.contradiction_commit_for_tenant(&tenant, &verdicts)
        })
        .await
        .map_err(|e| Status::internal(format!("task join error: {}", e)))?;

        match result {
            Ok(output) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("contradiction_commit", "ok", elapsed);

                Ok(Response::new(pb::ContradictionCommitResponse {
                    contradictions_confirmed: output.contradictions_confirmed as u64,
                    revisions_created: output.revisions_created as u64,
                    dismissed: output.dismissed as u64,
                }))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                self.metrics
                    .observe_operation("contradiction_commit", "error", elapsed);
                Err(convert::hebbs_error_to_status(e))
            }
        }
    }
}
