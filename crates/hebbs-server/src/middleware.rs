use std::sync::Arc;

use axum::extract::{FromRequestParts, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use tonic::Status;
use tracing::warn;

use hebbs_core::auth::{KeyCache, KeyRecord};
use hebbs_core::rate_limit::RateLimiter;
use hebbs_core::tenant::TenantContext;

// ═══════════════════════════════════════════════════════════════════════
//  Shared auth state for middleware
// ═══════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct AuthState {
    pub key_cache: Arc<KeyCache>,
    pub rate_limiter: Arc<RateLimiter>,
    pub auth_enabled: bool,
}

// ═══════════════════════════════════════════════════════════════════════
//  gRPC interceptor
// ═══════════════════════════════════════════════════════════════════════

/// Returns a tonic interceptor closure that validates API keys and sets
/// `TenantContext` + `KeyRecord` in request extensions.
#[allow(clippy::result_large_err)]
pub fn grpc_auth_interceptor(
    auth_state: Arc<AuthState>,
) -> impl Fn(tonic::Request<()>) -> Result<tonic::Request<()>, Status> + Clone {
    move |mut req: tonic::Request<()>| {
        if !auth_state.auth_enabled {
            req.extensions_mut().insert(TenantContext::default());
            return Ok(req);
        }

        let token = req
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.strip_prefix("Bearer ").unwrap_or(s).to_string());

        let raw_key = match token {
            Some(k) => k,
            None => {
                return Err(Status::unauthenticated(
                    "missing authorization metadata (expected 'authorization: Bearer hb_...')",
                ));
            }
        };

        match auth_state.key_cache.validate(&raw_key) {
            Ok(record) => {
                let tenant = TenantContext::new_unchecked(record.tenant_id.clone());
                req.extensions_mut().insert(tenant);
                req.extensions_mut().insert(record);
                Ok(req)
            }
            Err(e) => {
                warn!(error = %e, "gRPC auth rejected");
                Err(Status::unauthenticated(format!(
                    "authentication failed: {}",
                    e
                )))
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  gRPC helpers
// ═══════════════════════════════════════════════════════════════════════

/// Extract the `TenantContext` stored by the auth interceptor, falling back to
/// the default tenant if none is present (e.g. auth disabled).
pub fn extract_tenant_from_request<T>(request: &tonic::Request<T>) -> TenantContext {
    request
        .extensions()
        .get::<TenantContext>()
        .cloned()
        .unwrap_or_default()
}

/// Verify that the authenticated key carries the required permission bits.
/// When auth is disabled no `KeyRecord` is present and the check passes.
#[allow(clippy::result_large_err)]
pub fn check_permission(
    request: &tonic::Request<impl std::any::Any>,
    required: u8,
) -> Result<(), Status> {
    if let Some(record) = request.extensions().get::<KeyRecord>() {
        if !record.has_permission(required) {
            return Err(Status::permission_denied(format!(
                "insufficient permissions: requires {}",
                permission_label(required)
            )));
        }
    }
    Ok(())
}

/// Check the per-tenant rate limiter for the given operation.
/// Returns `Status::resource_exhausted` with a retry-after hint on rejection.
#[allow(clippy::result_large_err)]
pub fn check_rate_limit(
    auth_state: &AuthState,
    tenant: &TenantContext,
    operation: &str,
) -> Result<(), Status> {
    if let Err(retry_after_ms) = auth_state.rate_limiter.check(tenant.tenant_id(), operation) {
        return Err(Status::resource_exhausted(format!(
            "rate limit exceeded, retry after {}ms",
            retry_after_ms
        )));
    }
    Ok(())
}

fn permission_label(perm: u8) -> &'static str {
    use hebbs_core::auth::*;
    match perm {
        PERM_READ => "read",
        PERM_WRITE => "write",
        PERM_ADMIN => "admin",
        _ => "unknown",
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  REST (axum) auth middleware
// ═══════════════════════════════════════════════════════════════════════

pub async fn rest_auth_middleware(
    axum::extract::Extension(auth_state): axum::extract::Extension<Arc<AuthState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();

    if path.starts_with("/v1/health") {
        request.extensions_mut().insert(TenantContext::default());
        return next.run(request).await;
    }

    if !auth_state.auth_enabled {
        request.extensions_mut().insert(TenantContext::default());
        return next.run(request).await;
    }

    let token = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    let raw_key = match token {
        Some(k) => k,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "error_code": "unauthenticated",
                    "message": "missing Authorization header (expected 'Authorization: Bearer hb_...')"
                })),
            )
                .into_response();
        }
    };

    match auth_state.key_cache.validate(&raw_key) {
        Ok(record) => {
            let tenant = TenantContext::new_unchecked(record.tenant_id.clone());
            request.extensions_mut().insert(tenant);
            request.extensions_mut().insert(record);
            next.run(request).await
        }
        Err(e) => {
            warn!(error = %e, path = %path, "REST auth rejected");
            (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "error_code": "unauthenticated",
                    "message": format!("authentication failed: {}", e)
                })),
            )
                .into_response()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  REST (axum) rate-limit middleware
// ═══════════════════════════════════════════════════════════════════════

pub async fn rate_limit_middleware(
    axum::extract::Extension(auth_state): axum::extract::Extension<Arc<AuthState>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();

    if path.starts_with("/v1/health") || path.starts_with("/v1/metrics") {
        return next.run(request).await;
    }

    let tenant = request
        .extensions()
        .get::<TenantContext>()
        .cloned()
        .unwrap_or_default();

    let method = request.method().clone();
    let operation = resolve_rest_operation(&method, &path);

    if let Err(retry_after_ms) = auth_state.rate_limiter.check(tenant.tenant_id(), operation) {
        let retry_after_secs = retry_after_ms.div_ceil(1000);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(
                http::header::RETRY_AFTER,
                http::HeaderValue::from_str(&retry_after_secs.to_string()).unwrap(),
            )],
            axum::Json(serde_json::json!({
                "error_code": "rate_limited",
                "message": format!("rate limit exceeded, retry after {}ms", retry_after_ms)
            })),
        )
            .into_response();
    }

    next.run(request).await
}

fn resolve_rest_operation(method: &http::Method, path: &str) -> &'static str {
    match (method.as_str(), path) {
        ("POST", p) if p.starts_with("/v1/memories") => "remember",
        ("GET", p) if p.starts_with("/v1/memories") => "get",
        (_, p) if p.starts_with("/v1/recall") => "recall",
        (_, p) if p.starts_with("/v1/prime") => "prime",
        (_, p) if p.starts_with("/v1/revise") => "revise",
        (_, p) if p.starts_with("/v1/forget") => "forget",
        (_, p) if p.starts_with("/v1/insights") => "insights",
        (_, p) if p.starts_with("/v1/reflect") => "reflect",
        _ => "get",
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Axum extractor for TenantContext
// ═══════════════════════════════════════════════════════════════════════

/// Axum extractor that retrieves the `TenantContext` set by the auth
/// middleware. Falls back to the default tenant if absent.
pub struct TenantExtractor(pub TenantContext);

#[tonic::async_trait]
impl<S> FromRequestParts<S> for TenantExtractor
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let tenant = parts
            .extensions
            .get::<TenantContext>()
            .cloned()
            .unwrap_or_default();
        Ok(TenantExtractor(tenant))
    }
}
