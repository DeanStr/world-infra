//! Rate-limit backend mechanics.

use std::{
    collections::HashMap,
    error::Error,
    fmt,
    num::NonZeroU32,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Product-neutral limit specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitSpec {
    /// Fixed-window limit.
    Fixed {
        /// Maximum allowed requests per window.
        count: NonZeroU32,
        /// Window duration.
        window: Duration,
    },
    /// No limit. Product adapters decide how config maps to this.
    Unlimited,
}

impl LimitSpec {
    /// Return true when this limit is unlimited.
    #[must_use]
    pub const fn is_unlimited(self) -> bool {
        matches!(self, Self::Unlimited)
    }
}

/// Stable namespace wrapper.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Namespace(String);

impl Namespace {
    /// Construct a namespace.
    ///
    /// # Errors
    ///
    /// Returns [`RateLimitError::InvalidKey`] for blank or unsafe names.
    pub fn new(value: impl AsRef<str>) -> Result<Self, RateLimitError> {
        validate_key_part(value.as_ref()).map(|value| Self(value.to_owned()))
    }

    /// Access the namespace.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Rate-limit key under a namespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RateLimitKey(String);

impl RateLimitKey {
    /// Construct a key.
    ///
    /// # Errors
    ///
    /// Returns [`RateLimitError::InvalidKey`] for blank or unsafe names.
    pub fn new(value: impl AsRef<str>) -> Result<Self, RateLimitError> {
        validate_key_part(value.as_ref()).map(|value| Self(value.to_owned()))
    }

    /// Access the key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_key_part(value: &str) -> Result<&str, RateLimitError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 256
        || value
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/')))
    {
        return Err(RateLimitError::InvalidKey);
    }
    Ok(value)
}

/// Backend health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitHealth {
    /// Backend is ready.
    Healthy,
    /// Backend is degraded but adapter policy may allow fallback.
    Degraded,
    /// Backend is unavailable.
    Unavailable,
}

/// Failure policy selected by product adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePolicy {
    /// Allow request when the backend fails.
    FailOpen,
    /// Reject request when the backend fails.
    FailClosed,
}

/// Rate-limit decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitDecision {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// Requests remaining in the active window.
    pub remaining: u32,
    /// Retry-after hint when rejected.
    pub retry_after: Option<Duration>,
    /// Backend health observed while making the decision.
    pub health: RateLimitHealth,
}

impl RateLimitDecision {
    /// Unlimited allow decision.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            allowed: true,
            remaining: u32::MAX,
            retry_after: None,
            health: RateLimitHealth::Healthy,
        }
    }

    /// Decision to apply after backend failure and product failure policy.
    #[must_use]
    pub const fn from_failure(policy: FailurePolicy) -> Self {
        Self {
            allowed: matches!(policy, FailurePolicy::FailOpen),
            remaining: 0,
            retry_after: None,
            health: RateLimitHealth::Unavailable,
        }
    }
}

/// Rate-limit error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitError {
    /// Namespace or key was invalid.
    InvalidKey,
    /// Fixed-window duration was zero.
    ZeroWindow,
    /// Backend failed.
    Backend(String),
}

impl fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey => f.write_str("rate-limit key is invalid"),
            Self::ZeroWindow => f.write_str("rate-limit window must be non-zero"),
            Self::Backend(error) => write!(f, "rate-limit backend failed: {error}"),
        }
    }
}

impl Error for RateLimitError {}

/// Hooks products can use to observe backend mechanics.
pub trait RateLimitHooks {
    /// Called after each decision.
    fn decision(&self, _namespace: &Namespace, _key: &RateLimitKey, _decision: &RateLimitDecision) {
    }

    /// Called after backend errors.
    fn backend_error(&self, _namespace: &Namespace, _key: &RateLimitKey, _error: &RateLimitError) {}
}

impl RateLimitHooks for () {}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    window_start: u128,
    window_end: u128,
    count: u32,
}

/// In-process fixed-window backend.
#[derive(Debug, Default)]
pub struct InProcessFixedWindow {
    buckets: Mutex<HashMap<(String, String), Bucket>>,
}

impl InProcessFixedWindow {
    /// Create an empty backend.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Check and increment a bucket.
    ///
    /// # Errors
    ///
    /// Returns [`RateLimitError::ZeroWindow`] for zero fixed windows.
    pub fn check(
        &self,
        namespace: &Namespace,
        key: &RateLimitKey,
        limit: LimitSpec,
        now: SystemTime,
    ) -> Result<RateLimitDecision, RateLimitError> {
        let LimitSpec::Fixed { count, window } = limit else {
            return Ok(RateLimitDecision::unlimited());
        };
        if window.is_zero() {
            return Err(RateLimitError::ZeroWindow);
        }
        let window_millis = window_millis(window);
        let now_millis = unix_millis(now);
        let window_start = now_millis - (now_millis % window_millis);
        let window_end = window_start.saturating_add(window_millis);
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|error| RateLimitError::Backend(error.to_string()))?;
        let bucket = buckets
            .entry((namespace.0.clone(), key.0.clone()))
            .or_insert(Bucket {
                window_start,
                window_end,
                count: 0,
            });
        if bucket.window_start != window_start {
            *bucket = Bucket {
                window_start,
                window_end,
                count: 0,
            };
        }
        bucket.count = bucket.count.saturating_add(1);
        let allowed = bucket.count <= count.get();
        let remaining = count.get().saturating_sub(bucket.count);
        Ok(RateLimitDecision {
            allowed,
            remaining,
            retry_after: (!allowed).then(|| {
                let next_window = window_start.saturating_add(window_millis);
                duration_from_millis(next_window.saturating_sub(now_millis).max(1))
            }),
            health: RateLimitHealth::Healthy,
        })
    }

    /// Remove buckets whose windows ended before `now`.
    pub fn cleanup(&self, now: SystemTime, older_than: Duration) -> Result<usize, RateLimitError> {
        let cutoff = unix_millis(now).saturating_sub(duration_millis_ceil(older_than));
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|error| RateLimitError::Backend(error.to_string()))?;
        let before = buckets.len();
        buckets.retain(|_, bucket| bucket.window_end > cutoff);
        Ok(before - buckets.len())
    }

    /// Return the number of active in-process buckets.
    ///
    /// # Errors
    ///
    /// Returns [`RateLimitError::Backend`] if the mutex is poisoned.
    pub fn active_bucket_count(&self) -> Result<usize, RateLimitError> {
        self.buckets
            .lock()
            .map(|buckets| buckets.len())
            .map_err(|error| RateLimitError::Backend(error.to_string()))
    }
}

fn unix_millis(now: SystemTime) -> u128 {
    now.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}

fn window_millis(window: Duration) -> u128 {
    duration_millis_ceil(window).max(1)
}

fn duration_millis_ceil(duration: Duration) -> u128 {
    duration
        .as_millis()
        .saturating_add(u128::from(duration.subsec_nanos() % 1_000_000 != 0))
}

fn duration_from_millis(millis: u128) -> Duration {
    Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX))
}

/// Redis fixed-window backend helper.
#[cfg(feature = "redis")]
pub mod redis_backend {
    use super::*;

    /// Redis fixed-window backend configuration.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RedisFixedWindow {
        namespace: Namespace,
        command_timeout: Duration,
    }

    impl RedisFixedWindow {
        /// Construct a Redis backend.
        #[must_use]
        pub const fn new(namespace: Namespace, command_timeout: Duration) -> Self {
            Self {
                namespace,
                command_timeout,
            }
        }

        /// Command timeout hint. Product adapters own timeout enforcement.
        #[must_use]
        pub const fn command_timeout(&self) -> Duration {
            self.command_timeout
        }

        /// Check and increment a Redis bucket using a Lua script.
        ///
        /// # Errors
        ///
        /// Returns [`RateLimitError`] for invalid limits or Redis command errors.
        pub async fn check<C>(
            &self,
            connection: &mut C,
            key: &RateLimitKey,
            limit: LimitSpec,
            now: SystemTime,
        ) -> Result<RateLimitDecision, RateLimitError>
        where
            C: redis::aio::ConnectionLike + Send,
        {
            let LimitSpec::Fixed { count, window } = limit else {
                return Ok(RateLimitDecision::unlimited());
            };
            if window.is_zero() {
                return Err(RateLimitError::ZeroWindow);
            }
            let window_millis = window_millis(window);
            let now_millis = unix_millis(now);
            let bucket = now_millis - (now_millis % window_millis);
            let redis_key = redis_bucket_key(&self.namespace, key, bucket);
            let ttl = i64::try_from(window_millis.saturating_add(1000)).unwrap_or(i64::MAX);
            let script = redis::Script::new(
                r"
                local current = redis.call('INCR', KEYS[1])
                if current == 1 then
                  redis.call('PEXPIRE', KEYS[1], ARGV[1])
                end
                return current
                ",
            );
            let current: i64 = script
                .key(redis_key)
                .arg(ttl)
                .invoke_async(connection)
                .await
                .map_err(|error| RateLimitError::Backend(error.to_string()))?;
            let current = u32::try_from(current).unwrap_or(u32::MAX);
            let allowed = current <= count.get();
            Ok(RateLimitDecision {
                allowed,
                remaining: count.get().saturating_sub(current),
                retry_after: (!allowed).then(|| {
                    let next_window = bucket.saturating_add(window_millis);
                    duration_from_millis(next_window.saturating_sub(now_millis).max(1))
                }),
                health: RateLimitHealth::Healthy,
            })
        }
    }

    fn redis_bucket_key(namespace: &Namespace, key: &RateLimitKey, bucket: u128) -> String {
        format!(
            "rl:{}:{}:{}:{}:{}",
            namespace.as_str().len(),
            namespace.as_str(),
            key.as_str().len(),
            key.as_str(),
            bucket
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn redis_bucket_keys_are_unambiguous_for_colon_parts() {
            let first = redis_bucket_key(
                &Namespace::new("a").unwrap(),
                &RateLimitKey::new("b:rl:c").unwrap(),
                42,
            );
            let second = redis_bucket_key(
                &Namespace::new("a:rl:b").unwrap(),
                &RateLimitKey::new("c").unwrap(),
                42,
            );

            assert_ne!(first, second);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_always_allows() {
        let backend = InProcessFixedWindow::new();
        let ns = Namespace::new("app").unwrap();
        let key = RateLimitKey::new("anonymous").unwrap();
        let decision = backend
            .check(&ns, &key, LimitSpec::Unlimited, UNIX_EPOCH)
            .unwrap();
        assert!(decision.allowed);
        assert_eq!(decision.remaining, u32::MAX);
    }

    #[test]
    fn fixed_window_rejects_and_sets_retry_after() {
        let backend = InProcessFixedWindow::new();
        let ns = Namespace::new("app").unwrap();
        let key = RateLimitKey::new("anonymous").unwrap();
        let limit = LimitSpec::Fixed {
            count: NonZeroU32::new(1).unwrap(),
            window: Duration::from_secs(60),
        };
        assert!(backend.check(&ns, &key, limit, UNIX_EPOCH).unwrap().allowed);
        let second = backend.check(&ns, &key, limit, UNIX_EPOCH).unwrap();
        assert!(!second.allowed);
        assert_eq!(second.retry_after, Some(Duration::from_secs(60)));
    }

    #[test]
    fn fixed_window_preserves_subsecond_duration() {
        let backend = InProcessFixedWindow::new();
        let ns = Namespace::new("app").unwrap();
        let key = RateLimitKey::new("anonymous").unwrap();
        let limit = LimitSpec::Fixed {
            count: NonZeroU32::new(1).unwrap(),
            window: Duration::from_millis(500),
        };

        assert!(backend.check(&ns, &key, limit, UNIX_EPOCH).unwrap().allowed);
        let second = backend.check(&ns, &key, limit, UNIX_EPOCH).unwrap();
        assert!(!second.allowed);
        assert_eq!(second.retry_after, Some(Duration::from_millis(500)));

        let after_window = UNIX_EPOCH + Duration::from_millis(500);
        assert!(
            backend
                .check(&ns, &key, limit, after_window)
                .unwrap()
                .allowed
        );
    }

    #[test]
    fn nonzero_submillisecond_window_is_not_zeroed() {
        let backend = InProcessFixedWindow::new();
        let ns = Namespace::new("app").unwrap();
        let key = RateLimitKey::new("anonymous").unwrap();
        let limit = LimitSpec::Fixed {
            count: NonZeroU32::new(1).unwrap(),
            window: Duration::from_nanos(1),
        };

        assert!(backend.check(&ns, &key, limit, UNIX_EPOCH).unwrap().allowed);
        let second = backend.check(&ns, &key, limit, UNIX_EPOCH).unwrap();
        assert!(!second.allowed);
        assert_eq!(second.retry_after, Some(Duration::from_millis(1)));
    }

    #[test]
    fn fractional_millisecond_window_rounds_up() {
        let backend = InProcessFixedWindow::new();
        let ns = Namespace::new("app").unwrap();
        let key = RateLimitKey::new("anonymous").unwrap();
        let limit = LimitSpec::Fixed {
            count: NonZeroU32::new(1).unwrap(),
            window: Duration::from_micros(1500),
        };

        assert!(backend.check(&ns, &key, limit, UNIX_EPOCH).unwrap().allowed);
        let at_one_millisecond = backend
            .check(&ns, &key, limit, UNIX_EPOCH + Duration::from_millis(1))
            .unwrap();
        assert!(!at_one_millisecond.allowed);
        assert_eq!(
            at_one_millisecond.retry_after,
            Some(Duration::from_millis(1))
        );

        assert!(
            backend
                .check(&ns, &key, limit, UNIX_EPOCH + Duration::from_millis(2))
                .unwrap()
                .allowed
        );
    }

    #[test]
    fn cleanup_preserves_active_long_windows() {
        let backend = InProcessFixedWindow::new();
        let ns = Namespace::new("app").unwrap();
        let key = RateLimitKey::new("anonymous").unwrap();
        let limit = LimitSpec::Fixed {
            count: NonZeroU32::new(1).unwrap(),
            window: Duration::from_secs(60),
        };

        assert!(backend.check(&ns, &key, limit, UNIX_EPOCH).unwrap().allowed);
        assert_eq!(
            backend
                .cleanup(UNIX_EPOCH + Duration::from_secs(1), Duration::ZERO)
                .unwrap(),
            0
        );

        let second = backend
            .check(&ns, &key, limit, UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        assert!(!second.allowed);
    }

    #[test]
    fn cleanup_removes_expired_windows() {
        let backend = InProcessFixedWindow::new();
        let ns = Namespace::new("app").unwrap();
        let key = RateLimitKey::new("anonymous").unwrap();
        let limit = LimitSpec::Fixed {
            count: NonZeroU32::new(1).unwrap(),
            window: Duration::from_secs(1),
        };

        assert!(backend.check(&ns, &key, limit, UNIX_EPOCH).unwrap().allowed);
        assert_eq!(
            backend
                .cleanup(UNIX_EPOCH + Duration::from_secs(2), Duration::ZERO)
                .unwrap(),
            1
        );
    }

    #[test]
    fn namespace_rejects_unsafe_values() {
        assert!(Namespace::new("ok.namespace").is_ok());
        assert!(Namespace::new("bad namespace").is_err());
    }
}
