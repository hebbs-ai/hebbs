use std::fmt;
use std::time::Duration;

use tokio_stream::{Stream, StreamExt};
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::Channel;
use tracing::instrument;
use ulid::Ulid;

use hebbs_proto::generated as pb;
use pb::health_service_client::HealthServiceClient;
use pb::memory_service_client::MemoryServiceClient;
use pb::reflect_service_client::ReflectServiceClient;
use pb::subscribe_service_client::SubscribeServiceClient;

use crate::convert;
use crate::error::{self, ClientError};
use crate::retry::{self, RetryPolicy};
use crate::types::*;

/// Interceptor that optionally injects a `Bearer` authorization header.
#[derive(Clone)]
struct AuthInterceptor {
    header_value: Option<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>,
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(ref val) = self.header_value {
            req.metadata_mut().insert("authorization", val.clone());
        }
        Ok(req)
    }
}

type AuthChannel = InterceptedService<Channel, AuthInterceptor>;

/// Async client for HEBBS cognitive memory engine.
///
/// All operations are async and communicate with a running `hebbs-server`
/// via gRPC. Memory IDs are `Ulid` throughout; proto conversion is internal.
///
/// Construct via `HebbsClient::builder()`.
#[derive(Clone)]
pub struct HebbsClient {
    memory: MemoryServiceClient<AuthChannel>,
    subscribe_svc: SubscribeServiceClient<AuthChannel>,
    reflect: ReflectServiceClient<AuthChannel>,
    health: HealthServiceClient<AuthChannel>,
    endpoint: String,
    timeout: Duration,
    retry_policy: RetryPolicy,
    api_key: Option<String>,
}

impl fmt::Debug for HebbsClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HebbsClient")
            .field("endpoint", &self.endpoint)
            .field("timeout", &self.timeout)
            .field("retry_policy", &self.retry_policy)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Builder for `HebbsClient`.
pub struct ClientBuilder {
    endpoint: String,
    timeout: Duration,
    connect_timeout: Duration,
    retry_policy: RetryPolicy,
    keepalive_interval: Option<Duration>,
    connect_lazy: bool,
    user_agent: String,
    api_key: Option<String>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:6380".to_string(),
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            retry_policy: RetryPolicy::default(),
            keepalive_interval: Some(Duration::from_secs(30)),
            connect_lazy: true,
            user_agent: format!("hebbs-client-rust/{}", env!("CARGO_PKG_VERSION")),
            api_key: None,
        }
    }
}

impl ClientBuilder {
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub fn retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub fn keepalive_interval(mut self, interval: Duration) -> Self {
        self.keepalive_interval = Some(interval);
        self
    }

    pub fn connect_lazy(mut self, lazy: bool) -> Self {
        self.connect_lazy = lazy;
        self
    }

    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Build the client and optionally connect to the server.
    ///
    /// If `connect_lazy` is true (default), the TCP connection is deferred
    /// to the first RPC call. If false, validates connectivity immediately.
    pub async fn build(self) -> crate::Result<HebbsClient> {
        if self.timeout.is_zero() {
            return Err(ClientError::InvalidConfig {
                message: "timeout must be > 0".to_string(),
            });
        }
        if self.endpoint.is_empty() {
            return Err(ClientError::InvalidConfig {
                message: "endpoint must not be empty".to_string(),
            });
        }

        let mut ep = tonic::transport::Endpoint::from_shared(self.endpoint.clone())
            .map_err(|e| ClientError::InvalidConfig {
                message: format!("invalid endpoint: {}", e),
            })?
            .timeout(self.timeout)
            .connect_timeout(self.connect_timeout)
            .user_agent(self.user_agent)
            .map_err(|e| ClientError::InvalidConfig {
                message: format!("invalid user agent: {}", e),
            })?;

        if let Some(ka) = self.keepalive_interval {
            ep = ep
                .http2_keep_alive_interval(ka)
                .keep_alive_timeout(Duration::from_secs(10))
                .keep_alive_while_idle(true);
        }

        let channel = if self.connect_lazy {
            ep.connect_lazy()
        } else {
            ep.connect()
                .await
                .map_err(|e| ClientError::ConnectionFailed {
                    endpoint: self.endpoint.clone(),
                    reason: e.to_string(),
                })?
        };

        let header_value = match self.api_key {
            Some(ref key) => {
                let bearer = format!("Bearer {}", key);
                let val = bearer.parse().map_err(|_| ClientError::InvalidConfig {
                    message: "api_key contains invalid characters for a gRPC header".to_string(),
                })?;
                Some(val)
            }
            None => None,
        };
        let interceptor = AuthInterceptor { header_value };
        let auth_channel = InterceptedService::new(channel, interceptor);

        Ok(HebbsClient {
            memory: MemoryServiceClient::new(auth_channel.clone()),
            subscribe_svc: SubscribeServiceClient::new(auth_channel.clone()),
            reflect: ReflectServiceClient::new(auth_channel.clone()),
            health: HealthServiceClient::new(auth_channel),
            endpoint: self.endpoint,
            timeout: self.timeout,
            retry_policy: self.retry_policy,
            api_key: self.api_key,
        })
    }
}

impl HebbsClient {
    /// Start building a client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The endpoint this client is connected to.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    // ── Remember ────────────────────────────────────────────────────

    /// Store a new memory with minimal parameters.
    #[instrument(skip_all, fields(operation = "remember", endpoint = %self.endpoint))]
    pub async fn remember(
        &self,
        content: impl Into<String>,
        importance: f32,
    ) -> crate::Result<Memory> {
        let opts = RememberOptions::new(content).importance(importance);
        self.remember_with(opts).await
    }

    /// Store a new memory with full options.
    #[instrument(skip_all, fields(operation = "remember", endpoint = %self.endpoint))]
    pub async fn remember_with(&self, opts: RememberOptions) -> crate::Result<Memory> {
        let req = convert::remember_options_to_proto(&opts);
        let resp = self
            .unary_call("remember", |mut c| {
                let r = req.clone();
                async move { c.remember(r).await }
            })
            .await?;
        let memory = resp
            .memory
            .as_ref()
            .ok_or_else(|| ClientError::Serialization {
                message: "response missing memory".to_string(),
            })?;
        convert::proto_memory_to_domain(memory)
            .map_err(|e| ClientError::Serialization { message: e })
    }

    // ── Get ──────────────────────────────────────────────────────────

    /// Retrieve a single memory by ID.
    #[instrument(skip_all, fields(operation = "get", endpoint = %self.endpoint))]
    pub async fn get(&self, memory_id: Ulid) -> crate::Result<Memory> {
        let req = pb::GetRequest {
            memory_id: memory_id.to_bytes().to_vec(),
            tenant_id: None,
        };
        let resp = self
            .retryable_call("get", |mut c| {
                let r = req.clone();
                async move { c.get(r).await }
            })
            .await?;
        let memory = resp
            .memory
            .as_ref()
            .ok_or_else(|| ClientError::Serialization {
                message: "response missing memory".to_string(),
            })?;
        convert::proto_memory_to_domain(memory)
            .map_err(|e| ClientError::Serialization { message: e })
    }

    // ── Recall ───────────────────────────────────────────────────────

    /// Recall memories with a cue using default similarity strategy.
    #[instrument(skip_all, fields(operation = "recall", endpoint = %self.endpoint))]
    pub async fn recall(&self, cue: impl Into<String>) -> crate::Result<RecallOutput> {
        let opts = RecallOptions::new(cue);
        self.recall_with(opts).await
    }

    /// Recall memories with full options.
    #[instrument(skip_all, fields(operation = "recall", endpoint = %self.endpoint))]
    pub async fn recall_with(&self, opts: RecallOptions) -> crate::Result<RecallOutput> {
        let req = convert::recall_options_to_proto(&opts);
        let resp = self
            .retryable_call("recall", |mut c| {
                let r = req.clone();
                async move { c.recall(r).await }
            })
            .await?;

        let results = resp
            .results
            .iter()
            .map(convert::proto_recall_result_to_domain)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ClientError::Serialization { message: e })?;

        let strategy_errors = resp
            .strategy_errors
            .into_iter()
            .map(|e| {
                let strategy = proto_to_recall_strategy(e.strategy);
                StrategyError {
                    strategy,
                    message: e.message,
                }
            })
            .collect();

        Ok(RecallOutput {
            results,
            strategy_errors,
        })
    }

    // ── Prime ────────────────────────────────────────────────────────

    /// Pre-load relevant memories for an entity.
    #[instrument(skip_all, fields(operation = "prime", endpoint = %self.endpoint))]
    pub async fn prime(&self, entity_id: impl Into<String>) -> crate::Result<PrimeOutput> {
        let opts = PrimeOptions::new(entity_id);
        self.prime_with(opts).await
    }

    /// Pre-load with full options.
    #[instrument(skip_all, fields(operation = "prime", endpoint = %self.endpoint))]
    pub async fn prime_with(&self, opts: PrimeOptions) -> crate::Result<PrimeOutput> {
        let req = convert::prime_options_to_proto(&opts);
        let resp = self
            .retryable_call("prime", |mut c| {
                let r = req.clone();
                async move { c.prime(r).await }
            })
            .await?;

        let results = resp
            .results
            .iter()
            .map(convert::proto_recall_result_to_domain)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ClientError::Serialization { message: e })?;

        Ok(PrimeOutput {
            results,
            temporal_count: resp.temporal_count,
            similarity_count: resp.similarity_count,
        })
    }

    // ── Revise ───────────────────────────────────────────────────────

    /// Revise an existing memory.
    #[instrument(skip_all, fields(operation = "revise", endpoint = %self.endpoint))]
    pub async fn revise(&self, opts: ReviseOptions) -> crate::Result<Memory> {
        let req = convert::revise_options_to_proto(&opts);
        let resp = self
            .unary_call("revise", |mut c| {
                let r = req.clone();
                async move { c.revise(r).await }
            })
            .await?;
        let memory = resp
            .memory
            .as_ref()
            .ok_or_else(|| ClientError::Serialization {
                message: "response missing memory".to_string(),
            })?;
        convert::proto_memory_to_domain(memory)
            .map_err(|e| ClientError::Serialization { message: e })
    }

    // ── Forget ───────────────────────────────────────────────────────

    /// Forget a single memory by ID.
    #[instrument(skip_all, fields(operation = "forget", endpoint = %self.endpoint))]
    pub async fn forget(&self, memory_id: Ulid) -> crate::Result<ForgetOutput> {
        let criteria = ForgetCriteria::by_id(memory_id);
        self.forget_with(criteria).await
    }

    /// Forget memories matching criteria.
    #[instrument(skip_all, fields(operation = "forget", endpoint = %self.endpoint))]
    pub async fn forget_with(&self, criteria: ForgetCriteria) -> crate::Result<ForgetOutput> {
        let req = convert::forget_criteria_to_proto(&criteria);
        let resp = self
            .unary_call("forget", |mut c| {
                let r = req.clone();
                async move { c.forget(r).await }
            })
            .await?;

        Ok(ForgetOutput {
            forgotten_count: resp.forgotten_count,
            cascade_count: resp.cascade_count,
            truncated: resp.truncated,
            tombstone_count: resp.tombstone_count,
        })
    }

    // ── Subscribe ────────────────────────────────────────────────────

    /// Start a subscription that streams relevant memories.
    #[instrument(skip_all, fields(operation = "subscribe", endpoint = %self.endpoint))]
    pub async fn subscribe(&self, opts: SubscribeOptions) -> crate::Result<SubscribeHandle> {
        let req = convert::subscribe_options_to_proto(&opts);

        let mut client = self.subscribe_svc.clone();
        let response = client
            .subscribe(req)
            .await
            .map_err(|s| error::from_status(s, "subscribe"))?;

        let stream = response.into_inner();
        let subscription_id = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let sub_id = subscription_id.clone();

        let mapped_stream = stream.map(move |item| match item {
            Ok(push) => {
                sub_id.store(push.subscription_id, std::sync::atomic::Ordering::Relaxed);
                convert::proto_push_to_domain(&push)
                    .map_err(|e| ClientError::Serialization { message: e })
            }
            Err(status) => Err(ClientError::SubscriptionClosed {
                reason: status.message().to_string(),
            }),
        });

        Ok(SubscribeHandle {
            stream: Box::pin(mapped_stream),
            subscribe_client: client,
            subscription_id,
        })
    }

    // ── Reflect ──────────────────────────────────────────────────────

    /// Trigger reflection over a scope.
    #[instrument(skip_all, fields(operation = "reflect", endpoint = %self.endpoint))]
    pub async fn reflect(&self, scope: crate::types::ReflectScope) -> crate::Result<ReflectOutput> {
        let req = convert::reflect_scope_to_proto(&scope);
        let mut client = self.reflect.clone();
        let resp = tokio::time::timeout(self.timeout, client.reflect(req))
            .await
            .map_err(|_| ClientError::Timeout {
                operation: "reflect",
                elapsed: self.timeout,
            })?
            .map_err(|s| error::from_status(s, "reflect"))?
            .into_inner();

        Ok(ReflectOutput {
            insights_created: resp.insights_created,
            clusters_found: resp.clusters_found,
            clusters_processed: resp.clusters_processed,
            memories_processed: resp.memories_processed,
        })
    }

    // ── Insights ─────────────────────────────────────────────────────

    /// Query stored insights.
    #[instrument(skip_all, fields(operation = "insights", endpoint = %self.endpoint))]
    pub async fn insights(&self, filter: InsightsFilter) -> crate::Result<Vec<Memory>> {
        let req = convert::insights_filter_to_proto(&filter);
        let mut client = self.reflect.clone();
        let resp = tokio::time::timeout(self.timeout, client.get_insights(req))
            .await
            .map_err(|_| ClientError::Timeout {
                operation: "insights",
                elapsed: self.timeout,
            })?
            .map_err(|s| error::from_status(s, "insights"))?
            .into_inner();

        resp.insights
            .iter()
            .map(convert::proto_memory_to_domain)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ClientError::Serialization { message: e })
    }

    // ── Health ────────────────────────────────────────────────────────

    /// Check server health.
    #[instrument(skip_all, fields(operation = "health", endpoint = %self.endpoint))]
    pub async fn health(&self) -> crate::Result<HealthStatus> {
        let req = pb::HealthCheckRequest {};
        let mut client = self.health.clone();
        let resp = tokio::time::timeout(self.timeout, client.check(req))
            .await
            .map_err(|_| ClientError::Timeout {
                operation: "health",
                elapsed: self.timeout,
            })?
            .map_err(|s| error::from_status(s, "health"))?
            .into_inner();

        let status = match resp.status {
            1 => ServingStatus::Serving,
            2 => ServingStatus::NotServing,
            _ => ServingStatus::Unknown,
        };

        Ok(HealthStatus {
            status,
            version: resp.version,
            memory_count: resp.memory_count,
            uptime_seconds: resp.uptime_seconds,
        })
    }

    // ── Internal dispatch ────────────────────────────────────────────

    /// Execute a unary (non-retryable) call against MemoryService.
    async fn unary_call<F, Fut, R>(&self, operation: &'static str, f: F) -> crate::Result<R>
    where
        F: Fn(MemoryServiceClient<AuthChannel>) -> Fut,
        Fut: std::future::Future<Output = Result<tonic::Response<R>, tonic::Status>>,
    {
        let result = tokio::time::timeout(self.timeout, f(self.memory.clone()))
            .await
            .map_err(|_| ClientError::Timeout {
                operation,
                elapsed: self.timeout,
            })?
            .map_err(|s| error::from_status(s, operation))?;

        Ok(result.into_inner())
    }

    /// Execute a retryable call against MemoryService with exponential backoff.
    async fn retryable_call<F, Fut, R>(&self, operation: &'static str, f: F) -> crate::Result<R>
    where
        F: Fn(MemoryServiceClient<AuthChannel>) -> Fut,
        Fut: std::future::Future<Output = Result<tonic::Response<R>, tonic::Status>>,
    {
        if !retry::is_idempotent(operation) || self.retry_policy.max_retries == 0 {
            return self.unary_call(operation, f).await;
        }

        let mut last_error = None;
        let max_attempts = self.retry_policy.max_retries + 1;

        for attempt in 0..max_attempts {
            match tokio::time::timeout(self.timeout, f(self.memory.clone())).await {
                Ok(Ok(response)) => return Ok(response.into_inner()),
                Ok(Err(status)) => {
                    if attempt < self.retry_policy.max_retries
                        && error::is_retryable_status(status.code())
                    {
                        let delay = self.retry_policy.delay_for_attempt(attempt);
                        tracing::debug!(
                            operation,
                            attempt,
                            code = %status.code(),
                            delay_ms = delay.as_millis() as u64,
                            "retrying after transient error"
                        );
                        tokio::time::sleep(delay).await;
                        last_error = Some(status);
                        continue;
                    }
                    return Err(error::from_status(status, operation));
                }
                Err(_) => {
                    if attempt < self.retry_policy.max_retries {
                        let delay = self.retry_policy.delay_for_attempt(attempt);
                        tracing::debug!(
                            operation,
                            attempt,
                            delay_ms = delay.as_millis() as u64,
                            "retrying after timeout"
                        );
                        tokio::time::sleep(delay).await;
                        last_error = Some(tonic::Status::deadline_exceeded("timeout"));
                        continue;
                    }
                    return Err(ClientError::Timeout {
                        operation,
                        elapsed: self.timeout,
                    });
                }
            }
        }

        match last_error {
            Some(status) => {
                let mut err = error::from_status(status, operation);
                if let ClientError::Unavailable {
                    ref mut attempts, ..
                } = err
                {
                    *attempts = max_attempts;
                }
                Err(err)
            }
            None => Err(ClientError::ServerError {
                message: "no attempts made".to_string(),
            }),
        }
    }
}

/// Handle to an active subscription stream.
///
/// Provides an async stream of pushes plus methods to feed text
/// and close the subscription.
pub struct SubscribeHandle {
    stream: std::pin::Pin<Box<dyn Stream<Item = crate::Result<SubscribePush>> + Send>>,
    subscribe_client: SubscribeServiceClient<AuthChannel>,
    subscription_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl SubscribeHandle {
    /// Get the next push from the subscription.
    pub async fn next(&mut self) -> Option<crate::Result<SubscribePush>> {
        self.stream.next().await
    }

    /// Feed text to the subscription for matching.
    pub async fn feed(&mut self, text: impl Into<String>) -> crate::Result<()> {
        let sub_id = self
            .subscription_id
            .load(std::sync::atomic::Ordering::Relaxed);
        let req = pb::FeedRequest {
            subscription_id: sub_id,
            text: text.into(),
            tenant_id: None,
        };
        self.subscribe_client
            .feed(req)
            .await
            .map_err(|s| error::from_status(s, "feed"))?;
        Ok(())
    }

    /// Close the subscription.
    pub async fn close(&mut self) -> crate::Result<()> {
        let sub_id = self
            .subscription_id
            .load(std::sync::atomic::Ordering::Relaxed);
        let req = pb::CloseSubscriptionRequest {
            subscription_id: sub_id,
            tenant_id: None,
        };
        let _ = self.subscribe_client.close_subscription(req).await;
        Ok(())
    }

    /// Get the subscription ID (available after first push).
    pub fn subscription_id(&self) -> u64 {
        self.subscription_id
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

fn proto_to_recall_strategy(v: i32) -> RecallStrategy {
    match pb::RecallStrategyType::try_from(v) {
        Ok(pb::RecallStrategyType::Similarity) => RecallStrategy::Similarity,
        Ok(pb::RecallStrategyType::Temporal) => RecallStrategy::Temporal,
        Ok(pb::RecallStrategyType::Causal) => RecallStrategy::Causal,
        Ok(pb::RecallStrategyType::Analogical) => RecallStrategy::Analogical,
        _ => RecallStrategy::Similarity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_defaults() {
        let b = ClientBuilder::default();
        assert_eq!(b.endpoint, "http://localhost:6380");
        assert_eq!(b.timeout, Duration::from_secs(30));
        assert_eq!(b.connect_timeout, Duration::from_secs(5));
        assert!(b.connect_lazy);
    }

    #[tokio::test]
    async fn builder_rejects_zero_timeout() {
        let result = HebbsClient::builder().timeout(Duration::ZERO).build().await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ClientError::InvalidConfig { .. }));
    }

    #[tokio::test]
    async fn builder_rejects_empty_endpoint() {
        let result = HebbsClient::builder().endpoint("").build().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn builder_lazy_connect_succeeds() {
        let result = HebbsClient::builder()
            .endpoint("http://localhost:19999")
            .connect_lazy(true)
            .build()
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn builder_eager_connect_fails_for_bad_endpoint() {
        let result = HebbsClient::builder()
            .endpoint("http://localhost:19999")
            .connect_lazy(false)
            .connect_timeout(Duration::from_millis(100))
            .build()
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn proto_to_recall_strategy_mapping() {
        assert_eq!(
            proto_to_recall_strategy(pb::RecallStrategyType::Similarity as i32),
            RecallStrategy::Similarity
        );
        assert_eq!(
            proto_to_recall_strategy(pb::RecallStrategyType::Temporal as i32),
            RecallStrategy::Temporal
        );
        assert_eq!(
            proto_to_recall_strategy(pb::RecallStrategyType::Causal as i32),
            RecallStrategy::Causal
        );
        assert_eq!(
            proto_to_recall_strategy(pb::RecallStrategyType::Analogical as i32),
            RecallStrategy::Analogical
        );
        assert_eq!(proto_to_recall_strategy(99), RecallStrategy::Similarity);
    }
}
