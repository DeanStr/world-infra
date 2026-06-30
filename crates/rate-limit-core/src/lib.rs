//! Rate-limit backend mechanics.

use std::{
    collections::HashMap,
    error::Error,
    fmt,
    num::NonZeroU32,
    str::FromStr,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sha2::Digest as _;

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
    /// Construct a fixed limit.
    ///
    /// # Errors
    ///
    /// Returns [`LimitParseError::InvalidCount`] for a zero count or
    /// [`LimitParseError::InvalidWindow`] for a zero window.
    pub fn fixed(count: u32, window: Duration) -> Result<Self, LimitParseError> {
        if window.is_zero() {
            return Err(LimitParseError::InvalidWindow);
        }
        Ok(Self::Fixed {
            count: NonZeroU32::new(count).ok_or(LimitParseError::InvalidCount)?,
            window,
        })
    }

    /// Return true when this limit is unlimited.
    #[must_use]
    pub const fn is_unlimited(self) -> bool {
        matches!(self, Self::Unlimited)
    }
}

impl FromStr for LimitSpec {
    type Err = LimitParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_limit_spec(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowParseMode {
    PermissiveBareSeconds,
    StrictSuffix,
}

/// Error returned when parsing a limit specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitParseError {
    /// Input was blank.
    Empty,
    /// Count was missing, malformed, or zero.
    InvalidCount,
    /// Window was missing or malformed.
    InvalidWindow,
}

impl fmt::Display for LimitParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("limit spec is empty"),
            Self::InvalidCount => f.write_str("limit count is invalid"),
            Self::InvalidWindow => f.write_str("limit window is invalid"),
        }
    }
}

impl Error for LimitParseError {}

/// Parse a limit specification such as `10/min`, `100/60s`, or `unlimited`.
///
/// # Errors
///
/// Returns [`LimitParseError`] for malformed specifications.
pub fn parse_limit_spec(value: impl AsRef<str>) -> Result<LimitSpec, LimitParseError> {
    parse_limit_spec_with_mode(value, WindowParseMode::PermissiveBareSeconds)
}

/// Parse a limit specification without accepting bare numeric windows.
///
/// This is useful for externally supplied configuration where `10/60` should
/// not silently mean `10/60s`.
///
/// # Errors
///
/// Returns [`LimitParseError`] for malformed specifications.
pub fn parse_limit_spec_strict(value: impl AsRef<str>) -> Result<LimitSpec, LimitParseError> {
    parse_limit_spec_with_mode(value, WindowParseMode::StrictSuffix)
}

fn parse_limit_spec_with_mode(
    value: impl AsRef<str>,
    mode: WindowParseMode,
) -> Result<LimitSpec, LimitParseError> {
    let value = value.as_ref().trim();
    if value.is_empty() {
        return Err(LimitParseError::Empty);
    }
    if matches!(
        value.to_ascii_lowercase().as_str(),
        "unlimited" | "none" | "off"
    ) {
        return Ok(LimitSpec::Unlimited);
    }
    let (count, window) = value
        .split_once('/')
        .ok_or(LimitParseError::InvalidWindow)?;
    let count = count
        .trim()
        .parse::<u32>()
        .map_err(|_| LimitParseError::InvalidCount)?;
    let count = NonZeroU32::new(count).ok_or(LimitParseError::InvalidCount)?;
    Ok(LimitSpec::Fixed {
        count,
        window: parse_window(window, mode)?,
    })
}

fn parse_window(value: &str, mode: WindowParseMode) -> Result<Duration, LimitParseError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "s" | "sec" | "second" | "seconds" => return Ok(Duration::from_secs(1)),
        "m" | "min" | "minute" | "minutes" => return Ok(Duration::from_secs(60)),
        "h" | "hr" | "hour" | "hours" => return Ok(Duration::from_secs(3600)),
        "d" | "day" | "days" => return Ok(Duration::from_secs(86_400)),
        _ => {}
    }
    let (number, scale) = if let Some(number) = value.strip_suffix("ms") {
        (number, Duration::from_millis(1))
    } else if let Some(number) = value.strip_suffix('s') {
        (number, Duration::from_secs(1))
    } else if let Some(number) = value.strip_suffix('m') {
        (number, Duration::from_secs(60))
    } else if let Some(number) = value.strip_suffix('h') {
        (number, Duration::from_secs(3600))
    } else if let Some(number) = value.strip_suffix('d') {
        (number, Duration::from_secs(86_400))
    } else {
        if mode == WindowParseMode::StrictSuffix {
            return Err(LimitParseError::InvalidWindow);
        }
        (value.as_str(), Duration::from_secs(1))
    };
    let units = number
        .trim()
        .parse::<u64>()
        .map_err(|_| LimitParseError::InvalidWindow)?;
    if units == 0 {
        return Err(LimitParseError::InvalidWindow);
    }
    Ok(scale.saturating_mul(u32::try_from(units).unwrap_or(u32::MAX)))
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

/// Build a stable key from a prefix, label, and subject.
///
/// If the subject contains characters that are unsafe for shared rate-limit
/// keys, it is replaced with a SHA-256 digest while preserving prefix and label.
///
/// # Errors
///
/// Returns [`RateLimitError::InvalidKey`] if prefix, label, or subject are
/// blank or if prefix/label are otherwise unsafe.
pub fn subject_key(
    prefix: &str,
    label: &str,
    subject: &str,
) -> Result<RateLimitKey, RateLimitError> {
    let prefix = validate_key_part(prefix)?;
    let label = validate_key_part(label)?;
    let subject = subject.trim();
    if subject.is_empty() {
        return Err(RateLimitError::InvalidKey);
    }
    if validate_key_part(subject).is_ok() {
        let raw = length_prefixed_subject_key(prefix, label, "raw", subject);
        if let Ok(key) = RateLimitKey::new(&raw) {
            return Ok(key);
        }
    }

    let digest = format!("{:x}", sha2::Sha256::digest(subject.as_bytes()));
    RateLimitKey::new(length_prefixed_subject_key(
        prefix, label, "sha256", &digest,
    ))
}

fn length_prefixed_subject_key(
    prefix: &str,
    label: &str,
    subject_kind: &str,
    subject: &str,
) -> String {
    format!(
        "subject:v2:p{}:{prefix}:l{}:{label}:{subject_kind}:s{}:{subject}",
        prefix.len(),
        label.len(),
        subject.len()
    )
}

/// Product-neutral limit catalog keyed by product-owned labels.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LimitCatalog {
    limits: HashMap<String, LimitSpec>,
}

impl LimitCatalog {
    /// Construct an empty catalog.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a catalog from label/spec pairs.
    ///
    /// # Errors
    ///
    /// Returns [`RateLimitError::InvalidKey`] if a label is unsafe.
    pub fn from_pairs<I, K>(pairs: I) -> Result<Self, RateLimitError>
    where
        I: IntoIterator<Item = (K, LimitSpec)>,
        K: Into<String>,
    {
        let mut catalog = Self::new();
        for (label, spec) in pairs {
            catalog.insert(label, spec)?;
        }
        Ok(catalog)
    }

    /// Insert or replace a limit.
    ///
    /// # Errors
    ///
    /// Returns [`RateLimitError::InvalidKey`] if the label is unsafe.
    pub fn insert(
        &mut self,
        label: impl Into<String>,
        spec: LimitSpec,
    ) -> Result<Option<LimitSpec>, RateLimitError> {
        let label = label.into();
        let label = validate_key_part(&label)?.to_owned();
        Ok(self.limits.insert(label, spec))
    }

    /// Return a limit by label.
    #[must_use]
    pub fn get(&self, label: &str) -> Option<LimitSpec> {
        let label = validate_key_part(label).ok()?;
        self.limits.get(label).copied()
    }

    /// Merge another catalog, replacing existing labels.
    pub fn merge(&mut self, other: LimitCatalog) {
        self.limits.extend(other.limits);
    }

    /// Iterate over catalog entries.
    pub fn iter(&self) -> impl Iterator<Item = (&str, LimitSpec)> {
        self.limits
            .iter()
            .map(|(label, spec)| (label.as_str(), *spec))
    }

    /// Number of labels in the catalog.
    #[must_use]
    pub fn len(&self) -> usize {
        self.limits.len()
    }

    /// Return whether the catalog is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.limits.is_empty()
    }
}

/// Error returned when parsing a limit catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitCatalogError {
    /// JSON was malformed.
    InvalidJson(String),
    /// Label was unsafe.
    InvalidLabel(String),
    /// Limit spec was malformed.
    InvalidSpec(String),
}

impl fmt::Display for LimitCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(error) => write!(f, "limit catalog JSON is invalid: {error}"),
            Self::InvalidLabel(label) => write!(f, "limit label is invalid: {label}"),
            Self::InvalidSpec(label) => write!(f, "limit spec is invalid for label {label}"),
        }
    }
}

impl Error for LimitCatalogError {}

/// Parse JSON rate-limit overrides into a catalog.
///
/// Accepted values are strings such as `"10/min"`, arrays like `[10, 60]`,
/// or objects with `count` and `windowSecs`/`window_seconds` fields.
///
/// # Errors
///
/// Returns [`LimitCatalogError`] for malformed JSON or unsafe labels.
#[cfg(feature = "json")]
pub fn parse_limit_overrides_json(raw: &str) -> Result<LimitCatalog, LimitCatalogError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| LimitCatalogError::InvalidJson(error.to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| LimitCatalogError::InvalidJson("expected object".to_owned()))?;
    let mut catalog = LimitCatalog::new();
    for (label, value) in object {
        let spec = limit_spec_from_json_value(label, value)?;
        catalog
            .insert(label.clone(), spec)
            .map_err(|_| LimitCatalogError::InvalidLabel(label.clone()))?;
    }
    Ok(catalog)
}

#[cfg(feature = "json")]
fn limit_spec_from_json_value(
    label: &str,
    value: &serde_json::Value,
) -> Result<LimitSpec, LimitCatalogError> {
    if let Some(value) = value.as_str() {
        return parse_limit_spec_strict(value)
            .map_err(|_| LimitCatalogError::InvalidSpec(label.to_owned()));
    }
    if let Some(values) = value.as_array() {
        if values.len() != 2 {
            return Err(LimitCatalogError::InvalidSpec(label.to_owned()));
        }
        let count = values[0]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| LimitCatalogError::InvalidSpec(label.to_owned()))?;
        let window_secs = values[1]
            .as_u64()
            .ok_or_else(|| LimitCatalogError::InvalidSpec(label.to_owned()))?;
        return LimitSpec::fixed(count, Duration::from_secs(window_secs))
            .map_err(|_| LimitCatalogError::InvalidSpec(label.to_owned()));
    }
    if let Some(object) = value.as_object() {
        if object
            .get("unlimited")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(LimitSpec::Unlimited);
        }
        let count = object
            .get("count")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| LimitCatalogError::InvalidSpec(label.to_owned()))?;
        let window_secs = object
            .get("windowSecs")
            .or_else(|| object.get("window_seconds"))
            .or_else(|| object.get("window_secs"))
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| LimitCatalogError::InvalidSpec(label.to_owned()))?;
        return LimitSpec::fixed(count, Duration::from_secs(window_secs))
            .map_err(|_| LimitCatalogError::InvalidSpec(label.to_owned()));
    }
    Err(LimitCatalogError::InvalidSpec(label.to_owned()))
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

        /// Check and increment a Redis bucket with timeout enforcement.
        ///
        /// # Errors
        ///
        /// Returns [`RateLimitError::Backend`] when the timeout elapses or the
        /// Redis command fails.
        pub async fn check_with_timeout<C>(
            &self,
            connection: &mut C,
            key: &RateLimitKey,
            limit: LimitSpec,
            now: SystemTime,
        ) -> Result<RateLimitDecision, RateLimitError>
        where
            C: redis::aio::ConnectionLike + Send,
        {
            tokio::time::timeout(
                self.command_timeout,
                self.check(connection, key, limit, now),
            )
            .await
            .map_err(|_| RateLimitError::Backend("redis command timeout".to_owned()))?
        }

        /// Ping Redis with timeout enforcement.
        ///
        /// # Errors
        ///
        /// Returns [`RateLimitError::Backend`] when the timeout elapses or the
        /// Redis command fails.
        pub async fn ping_with_timeout<C>(
            &self,
            connection: &mut C,
        ) -> Result<RateLimitHealth, RateLimitError>
        where
            C: redis::aio::ConnectionLike + Send,
        {
            let result = tokio::time::timeout(
                self.command_timeout,
                redis::cmd("PING").query_async::<String>(connection),
            )
            .await
            .map_err(|_| RateLimitError::Backend("redis ping timeout".to_owned()))?;
            result
                .map(|_| RateLimitHealth::Healthy)
                .map_err(|error| RateLimitError::Backend(error.to_string()))
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
    fn parses_limit_specs() {
        assert_eq!("unlimited".parse::<LimitSpec>(), Ok(LimitSpec::Unlimited));
        assert_eq!(
            "10/min".parse::<LimitSpec>(),
            Ok(LimitSpec::Fixed {
                count: NonZeroU32::new(10).unwrap(),
                window: Duration::from_secs(60)
            })
        );
        assert_eq!(
            parse_limit_spec("5/250ms"),
            Ok(LimitSpec::Fixed {
                count: NonZeroU32::new(5).unwrap(),
                window: Duration::from_millis(250)
            })
        );
        assert_eq!(
            parse_limit_spec("2/day"),
            Ok(LimitSpec::Fixed {
                count: NonZeroU32::new(2).unwrap(),
                window: Duration::from_secs(86_400)
            })
        );
        assert_eq!(
            parse_limit_spec("2/7d"),
            Ok(LimitSpec::Fixed {
                count: NonZeroU32::new(2).unwrap(),
                window: Duration::from_secs(604_800)
            })
        );
        assert_eq!(
            parse_limit_spec("10/60"),
            Ok(LimitSpec::Fixed {
                count: NonZeroU32::new(10).unwrap(),
                window: Duration::from_secs(60)
            })
        );
        assert!(parse_limit_spec_strict("10/60").is_err());
        assert!(parse_limit_spec_strict("10/60s").is_ok());
        assert!(parse_limit_spec("0/min").is_err());
        assert!(LimitSpec::fixed(5, Duration::ZERO).is_err());
    }

    #[test]
    fn catalog_validates_labels_and_looks_up_specs() {
        let mut catalog = LimitCatalog::from_pairs([(
            "auth_login",
            LimitSpec::fixed(5, Duration::from_secs(60)).unwrap(),
        )])
        .unwrap();
        assert_eq!(
            catalog.get("auth_login"),
            Some(LimitSpec::fixed(5, Duration::from_secs(60)).unwrap())
        );
        catalog
            .insert(
                " search ",
                LimitSpec::fixed(30, Duration::from_secs(60)).unwrap(),
            )
            .unwrap();
        assert_eq!(
            catalog.get("search"),
            Some(LimitSpec::fixed(30, Duration::from_secs(60)).unwrap())
        );
        assert_eq!(
            catalog.get(" search "),
            Some(LimitSpec::fixed(30, Duration::from_secs(60)).unwrap())
        );
        assert!(catalog.iter().any(|(label, _)| label == "search"));
        assert!(!catalog.iter().any(|(label, _)| label == " search "));
        assert!(LimitCatalog::from_pairs([("bad label", LimitSpec::Unlimited)]).is_err());
    }

    #[test]
    fn subject_key_hashes_unsafe_subjects() {
        let direct = subject_key("ip", "auth", "203.0.113.1").unwrap();
        assert_eq!(
            direct.as_str(),
            "subject:v2:p2:ip:l4:auth:raw:s11:203.0.113.1"
        );
        let hashed = subject_key("ip", "auth", "user@example.com").unwrap();
        assert!(hashed
            .as_str()
            .starts_with("subject:v2:p2:ip:l4:auth:sha256:s64:"));
        assert_ne!(
            hashed.as_str(),
            "subject:v2:p2:ip:l4:auth:raw:s16:user@example.com"
        );
        assert!(subject_key("ip", "auth", " ").is_err());
    }

    #[test]
    fn subject_key_disambiguates_colon_separated_parts() {
        let first = subject_key("ip", "auth:a", "b").unwrap();
        let second = subject_key("ip", "auth", "a:b").unwrap();

        assert_ne!(first, second);
        assert_eq!(first.as_str(), "subject:v2:p2:ip:l6:auth:a:raw:s1:b");
        assert_eq!(second.as_str(), "subject:v2:p2:ip:l4:auth:raw:s3:a:b");
    }

    #[cfg(feature = "json")]
    #[test]
    fn parses_json_limit_overrides() {
        let catalog = parse_limit_overrides_json(
            r#"{
                "auth_login": "5/min",
                "search": [30, 60],
                "internal": {"unlimited": true},
                "write": {"count": 10, "windowSecs": 120}
            }"#,
        )
        .unwrap();
        assert_eq!(
            catalog.get("auth_login"),
            Some(LimitSpec::fixed(5, Duration::from_secs(60)).unwrap())
        );
        assert_eq!(
            catalog.get("search"),
            Some(LimitSpec::fixed(30, Duration::from_secs(60)).unwrap())
        );
        assert_eq!(catalog.get("internal"), Some(LimitSpec::Unlimited));
        assert_eq!(
            catalog.get("write"),
            Some(LimitSpec::fixed(10, Duration::from_secs(120)).unwrap())
        );
        assert!(parse_limit_overrides_json(r#"{"bad": "10/60"}"#).is_err());
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
