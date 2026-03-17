use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{info, instrument};

use hebbs_core::auth::PERM_READ;
use hebbs_core::engine::Engine;
use hebbs_core::memory::MemoryKind;
use hebbs_core::subscribe::SubscribeConfig;
use hebbs_proto::generated as pb;
use pb::subscribe_service_server::SubscribeService;

use crate::convert;
use crate::metrics::HebbsMetrics;
use crate::middleware::{self, AuthState};

struct SubscriptionEntry {
    handle: hebbs_core::subscribe::SubscriptionHandle,
    #[allow(dead_code)]
    push_tx: mpsc::Sender<Result<pb::SubscribePushMessage, Status>>,
}

pub struct SubscribeServiceImpl {
    pub engine: Arc<Engine>,
    pub metrics: Arc<HebbsMetrics>,
    pub auth_state: Arc<AuthState>,
    subscriptions: Arc<Mutex<HashMap<u64, SubscriptionEntry>>>,
}

impl SubscribeServiceImpl {
    pub fn new(
        engine: Arc<Engine>,
        metrics: Arc<HebbsMetrics>,
        auth_state: Arc<AuthState>,
    ) -> Self {
        Self {
            engine,
            metrics,
            auth_state,
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[tonic::async_trait]
impl SubscribeService for SubscribeServiceImpl {
    type SubscribeStream = ReceiverStream<Result<pb::SubscribePushMessage, Status>>;

    #[instrument(skip_all, fields(operation = "subscribe"))]
    async fn subscribe(
        &self,
        request: Request<pb::SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let auth_tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_READ)?;
        middleware::check_rate_limit(&self.auth_state, &auth_tenant, "subscribe")?;
        let req = request.into_inner();
        let tenant = middleware::resolve_tenant(auth_tenant, req.tenant_id.as_deref())?;

        let explicit_kinds: Vec<MemoryKind> = req
            .kind_filter
            .iter()
            .filter_map(|k| match pb::MemoryKind::try_from(*k) {
                Ok(pb::MemoryKind::Episode) => Some(MemoryKind::Episode),
                Ok(pb::MemoryKind::Insight) => Some(MemoryKind::Insight),
                Ok(pb::MemoryKind::Revision) => Some(MemoryKind::Revision),
                Ok(pb::MemoryKind::Document) => Some(MemoryKind::Document),
                Ok(pb::MemoryKind::Proposition) => Some(MemoryKind::Proposition),
                _ => None,
            })
            .collect();

        let defaults = SubscribeConfig::default();

        let config = SubscribeConfig {
            entity_id: req.entity_id,
            memory_kinds: if explicit_kinds.is_empty() {
                defaults.memory_kinds
            } else {
                explicit_kinds
            },
            confidence_threshold: req.confidence_threshold,
            time_scope_us: req.time_scope_us,
            ..defaults
        };

        let engine = self.engine.clone();
        let handle =
            tokio::task::spawn_blocking(move || engine.subscribe_for_tenant(&tenant, config))
                .await
                .map_err(|e| Status::internal(format!("task join error: {}", e)))?
                .map_err(convert::hebbs_error_to_status)?;

        let subscription_id = handle.id();
        let (push_tx, push_rx) = mpsc::channel(128);

        self.metrics.active_subscriptions.inc();

        self.subscriptions.lock().insert(
            subscription_id,
            SubscriptionEntry {
                handle,
                push_tx: push_tx.clone(),
            },
        );

        let subscriptions = self.subscriptions.clone();
        let metrics = self.metrics.clone();

        let handshake = pb::SubscribePushMessage {
            subscription_id,
            memory: None,
            confidence: 0.0,
            push_timestamp_us: 0,
            sequence_number: 0,
        };
        let _ = push_tx.send(Ok(handshake)).await;

        tokio::spawn(async move {
            let mut sequence: u64 = 0;

            loop {
                tokio::time::sleep(Duration::from_millis(5)).await;

                let push_opt = {
                    let subs = subscriptions.lock();
                    match subs.get(&subscription_id) {
                        Some(entry) => entry.handle.try_recv(),
                        None => break,
                    }
                };

                if let Some(push) = push_opt {
                    sequence += 1;
                    let msg = pb::SubscribePushMessage {
                        subscription_id,
                        memory: Some(convert::memory_to_proto(&push.memory)),
                        confidence: push.confidence,
                        push_timestamp_us: push.push_timestamp_us,
                        sequence_number: sequence,
                    };
                    if push_tx.send(Ok(msg)).await.is_err() {
                        break;
                    }
                }

                if push_tx.is_closed() {
                    break;
                }
            }

            let mut subs = subscriptions.lock();
            if let Some(mut entry) = subs.remove(&subscription_id) {
                entry.handle.close();
            }
            metrics.active_subscriptions.dec();
        });

        info!(subscription_id, "subscribe stream opened");

        Ok(Response::new(ReceiverStream::new(push_rx)))
    }

    #[instrument(skip_all, fields(operation = "feed"))]
    async fn feed(
        &self,
        request: Request<pb::FeedRequest>,
    ) -> Result<Response<pb::FeedResponse>, Status> {
        let auth_tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_READ)?;
        middleware::check_rate_limit(&self.auth_state, &auth_tenant, "feed")?;
        let req = request.into_inner();
        let _tenant = middleware::resolve_tenant(auth_tenant, req.tenant_id.as_deref())?;

        let subs = self.subscriptions.lock();
        let entry = subs.get(&req.subscription_id).ok_or_else(|| {
            Status::not_found(format!("subscription {} not found", req.subscription_id))
        })?;

        let _ = entry.handle.feed(&req.text);

        Ok(Response::new(pb::FeedResponse {}))
    }

    #[instrument(skip_all, fields(operation = "close_subscription"))]
    async fn close_subscription(
        &self,
        request: Request<pb::CloseSubscriptionRequest>,
    ) -> Result<Response<pb::CloseSubscriptionResponse>, Status> {
        let auth_tenant = middleware::extract_tenant_from_request(&request);
        middleware::check_permission(&request, PERM_READ)?;
        let req = request.into_inner();
        let _tenant = middleware::resolve_tenant(auth_tenant, req.tenant_id.as_deref())?;

        let mut subs = self.subscriptions.lock();
        if let Some(mut entry) = subs.remove(&req.subscription_id) {
            entry.handle.close();
            self.metrics.active_subscriptions.dec();
            info!(subscription_id = req.subscription_id, "subscription closed");
        }

        Ok(Response::new(pb::CloseSubscriptionResponse {}))
    }
}

impl Drop for SubscribeServiceImpl {
    fn drop(&mut self) {
        let mut subs = self.subscriptions.lock();
        for (_, mut entry) in subs.drain() {
            entry.handle.close();
        }
    }
}
