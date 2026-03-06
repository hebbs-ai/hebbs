use std::collections::HashMap;
use std::time::Instant;

use parking_lot::Mutex;

/// Operation classes for rate limiting.
/// Limits are per (tenant, class) pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationClass {
    Write, // remember, revise, forget
    Read,  // recall, prime, subscribe, insights, get
    Admin, // reflect, reflect_policy, key management
}

impl OperationClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Read => "read",
            Self::Admin => "admin",
        }
    }

    /// Map an operation name to its class.
    pub fn from_operation(op: &str) -> Self {
        match op {
            "remember" | "revise" | "forget" => Self::Write,
            "recall" | "prime" | "subscribe" | "insights" | "get" | "feed" => Self::Read,
            "reflect" | "reflect_policy" | "set_policy" | "key_create" | "key_revoke"
            | "key_list" => Self::Admin,
            _ => Self::Read,
        }
    }
}

/// Configuration for rate limiting.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub write_rate: f64,
    pub write_burst: u32,
    pub read_rate: f64,
    pub read_burst: u32,
    pub admin_rate: f64,
    pub admin_burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            write_rate: 1000.0,
            write_burst: 5000,
            read_rate: 5000.0,
            read_burst: 10000,
            admin_rate: 10.0,
            admin_burst: 20,
        }
    }
}

impl RateLimitConfig {
    pub fn rate_for_class(&self, class: OperationClass) -> f64 {
        match class {
            OperationClass::Write => self.write_rate,
            OperationClass::Read => self.read_rate,
            OperationClass::Admin => self.admin_rate,
        }
    }

    pub fn burst_for_class(&self, class: OperationClass) -> f64 {
        match class {
            OperationClass::Write => self.write_burst as f64,
            OperationClass::Read => self.read_burst as f64,
            OperationClass::Admin => self.admin_burst as f64,
        }
    }
}

/// Token bucket state for a single (tenant, operation_class) pair.
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Try to consume one token. Returns Ok(remaining) or Err(retry_after_ms).
    fn try_acquire(&mut self) -> Result<f64, u64> {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(self.tokens)
        } else {
            let deficit = 1.0 - self.tokens;
            let wait_secs = deficit / self.refill_rate;
            Err((wait_secs * 1000.0).ceil() as u64)
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
            self.last_refill = now;
        }
    }

    fn remaining(&mut self) -> f64 {
        self.refill();
        self.tokens
    }
}

/// Composite key for per-tenant, per-class rate limiting.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BucketKey {
    tenant_id: String,
    class: OperationClass,
}

/// Rate limiter managing token buckets for all (tenant, operation_class) pairs.
///
/// Thread-safe via interior Mutex. The Mutex protects the bucket map;
/// individual bucket operations are fast (no I/O), so contention is minimal.
pub struct RateLimiter {
    buckets: Mutex<HashMap<BucketKey, TokenBucket>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// Check if a request should be allowed for the given tenant and operation.
    /// Returns Ok(remaining_tokens) or Err(retry_after_ms).
    pub fn check(&self, tenant_id: &str, operation: &str) -> Result<f64, u64> {
        if !self.config.enabled {
            return Ok(f64::MAX);
        }

        let class = OperationClass::from_operation(operation);
        let key = BucketKey {
            tenant_id: tenant_id.to_string(),
            class,
        };

        let mut buckets = self.buckets.lock();
        let bucket = buckets.entry(key).or_insert_with(|| {
            TokenBucket::new(
                self.config.burst_for_class(class),
                self.config.rate_for_class(class),
            )
        });

        bucket.try_acquire()
    }

    /// Get the current limit and remaining tokens for a (tenant, class) pair.
    /// Used for rate limit response headers.
    pub fn status(&self, tenant_id: &str, operation: &str) -> (f64, f64, u64) {
        let class = OperationClass::from_operation(operation);
        let key = BucketKey {
            tenant_id: tenant_id.to_string(),
            class,
        };

        let mut buckets = self.buckets.lock();
        let bucket = buckets.entry(key).or_insert_with(|| {
            TokenBucket::new(
                self.config.burst_for_class(class),
                self.config.rate_for_class(class),
            )
        });

        let remaining = bucket.remaining();
        let limit = bucket.max_tokens;
        let refill_time = if remaining < limit {
            let deficit = limit - remaining;
            (deficit / bucket.refill_rate).ceil() as u64
        } else {
            0
        };

        (limit, remaining, refill_time)
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_rate_limiting() {
        let config = RateLimitConfig {
            enabled: true,
            write_rate: 10.0,
            write_burst: 5,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);

        for _ in 0..5 {
            assert!(limiter.check("tenant_a", "remember").is_ok());
        }

        let result = limiter.check("tenant_a", "remember");
        assert!(result.is_err());
    }

    #[test]
    fn tenant_isolation() {
        let config = RateLimitConfig {
            enabled: true,
            write_rate: 10.0,
            write_burst: 3,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);

        for _ in 0..3 {
            assert!(limiter.check("tenant_a", "remember").is_ok());
        }
        assert!(limiter.check("tenant_a", "remember").is_err());

        assert!(limiter.check("tenant_b", "remember").is_ok());
    }

    #[test]
    fn operation_class_separation() {
        let config = RateLimitConfig {
            enabled: true,
            write_rate: 10.0,
            write_burst: 2,
            read_rate: 100.0,
            read_burst: 2,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);

        for _ in 0..2 {
            assert!(limiter.check("t", "remember").is_ok());
        }
        assert!(limiter.check("t", "remember").is_err());

        assert!(limiter.check("t", "recall").is_ok());
    }

    #[test]
    fn disabled_limiter_always_allows() {
        let config = RateLimitConfig {
            enabled: false,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);

        for _ in 0..10000 {
            assert!(limiter.check("t", "remember").is_ok());
        }
    }

    #[test]
    fn retry_after_is_positive() {
        let config = RateLimitConfig {
            enabled: true,
            write_rate: 1.0,
            write_burst: 1,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);

        assert!(limiter.check("t", "remember").is_ok());
        let result = limiter.check("t", "remember");
        match result {
            Err(retry_ms) => assert!(retry_ms > 0),
            Ok(_) => panic!("expected rate limit"),
        }
    }

    #[test]
    fn status_returns_correct_values() {
        let config = RateLimitConfig {
            enabled: true,
            write_rate: 100.0,
            write_burst: 10,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);

        let (limit, remaining, _reset) = limiter.status("t", "remember");
        assert!((limit - 10.0).abs() < 0.01);
        assert!((remaining - 10.0).abs() < 0.01);
    }

    #[test]
    fn operation_class_mapping() {
        assert_eq!(
            OperationClass::from_operation("remember"),
            OperationClass::Write
        );
        assert_eq!(
            OperationClass::from_operation("revise"),
            OperationClass::Write
        );
        assert_eq!(
            OperationClass::from_operation("forget"),
            OperationClass::Write
        );
        assert_eq!(
            OperationClass::from_operation("recall"),
            OperationClass::Read
        );
        assert_eq!(
            OperationClass::from_operation("prime"),
            OperationClass::Read
        );
        assert_eq!(OperationClass::from_operation("get"), OperationClass::Read);
        assert_eq!(
            OperationClass::from_operation("reflect"),
            OperationClass::Admin
        );
    }
}
