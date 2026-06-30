//! Volatile idempotency claim stores for service-edge work.
//!
//! This crate owns in-memory and Redis pending/completed claim mechanics. It
//! does not parse or migrate durable product keys, canonicalize request bodies,
//! define API response shapes, or replace product-owned SQL idempotency ledgers.

use std::{
    collections::HashMap,
    error::Error,
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Claim result for idempotent work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkClaim {
    /// This caller created the pending claim and may perform the work.
    Fresh,
    /// Another caller already has a pending claim.
    Pending,
    /// The work was previously completed.
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryState {
    Marker,
    Pending,
    Completed,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    state: EntryState,
    expires_at: std::time::Instant,
}

#[derive(Debug, Default)]
struct InMemoryState {
    entries: HashMap<String, Entry>,
    next_cleanup_at: Option<std::time::Instant>,
}

impl InMemoryState {
    fn maybe_cleanup(&mut self, now: std::time::Instant) {
        if self.next_cleanup_at.is_some_and(|next| next > now) {
            return;
        }
        self.entries.retain(|_, entry| entry.expires_at > now);
        self.next_cleanup_at = self.entries.values().map(|entry| entry.expires_at).min();
    }

    fn note_expiry(&mut self, expires_at: std::time::Instant) {
        self.next_cleanup_at = Some(
            self.next_cleanup_at
                .map(|current| current.min(expires_at))
                .unwrap_or(expires_at),
        );
    }
}

/// Runtime namespace prepended to volatile store keys.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdempotencyNamespace(String);

impl IdempotencyNamespace {
    /// Construct a namespace.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError::InvalidNamespace`] for blank or
    /// unsafe namespaces.
    pub fn new(value: impl AsRef<str>) -> Result<Self, IdempotencyRuntimeError> {
        let value =
            validate_part(value.as_ref()).map_err(|_| IdempotencyRuntimeError::InvalidNamespace)?;
        Ok(Self(value.to_owned()))
    }

    /// Access the namespace.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Runtime idempotency error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyRuntimeError {
    /// Namespace was blank or unsafe.
    InvalidNamespace,
    /// Key was blank or unsafe.
    InvalidKey,
    /// Backend command timed out.
    Timeout,
    /// Backend returned an unsupported marker.
    UnsupportedMarker,
    /// Backend failed.
    Backend(String),
}

impl fmt::Display for IdempotencyRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidNamespace => f.write_str("idempotency namespace is invalid"),
            Self::InvalidKey => f.write_str("idempotency key is invalid"),
            Self::Timeout => f.write_str("idempotency backend timed out"),
            Self::UnsupportedMarker => f.write_str("idempotency key has unsupported marker value"),
            Self::Backend(error) => write!(f, "idempotency backend failed: {error}"),
        }
    }
}

impl Error for IdempotencyRuntimeError {}

fn validate_part(value: &str) -> Result<&str, IdempotencyRuntimeError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 512
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(IdempotencyRuntimeError::InvalidKey);
    }
    Ok(value)
}

fn ttl_duration(ttl: Duration) -> Duration {
    ttl.max(Duration::from_millis(1))
}

#[cfg(feature = "redis")]
fn ttl_secs_ceil(ttl: Duration) -> u64 {
    let ttl = ttl_duration(ttl);
    ttl.as_secs()
        .saturating_add(u64::from(ttl.subsec_nanos() != 0))
        .max(1)
}

fn namespaced_key(
    namespace: Option<&IdempotencyNamespace>,
    key: &str,
) -> Result<String, IdempotencyRuntimeError> {
    let key = validate_part(key)?;
    Ok(match namespace {
        Some(namespace) => format!(
            "idem:{}:{}:{}:{key}",
            namespace.as_str().len(),
            namespace.as_str(),
            key.len()
        ),
        None => format!("idem:0::{}:{key}", key.len()),
    })
}

/// In-memory idempotency store with TTL cleanup.
#[derive(Debug, Clone, Default)]
pub struct InMemoryIdempotencyStore {
    namespace: Option<IdempotencyNamespace>,
    state: Arc<Mutex<InMemoryState>>,
}

impl InMemoryIdempotencyStore {
    /// Construct an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct an empty in-memory store with a namespace.
    #[must_use]
    pub fn with_namespace(namespace: IdempotencyNamespace) -> Self {
        Self {
            namespace: Some(namespace),
            state: Arc::new(Mutex::new(InMemoryState::default())),
        }
    }

    /// Return the configured namespace.
    #[must_use]
    pub fn namespace(&self) -> Option<&IdempotencyNamespace> {
        self.namespace.as_ref()
    }

    /// Set a marker once.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for invalid keys or lock failures.
    pub async fn set_once(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<bool, IdempotencyRuntimeError> {
        let key = namespaced_key(self.namespace.as_ref(), key)?;
        let now = std::time::Instant::now();
        let expires_at = now + ttl_duration(ttl);
        let mut state = self
            .state
            .lock()
            .map_err(|error| IdempotencyRuntimeError::Backend(error.to_string()))?;
        state.maybe_cleanup(now);
        if state
            .entries
            .get(&key)
            .is_some_and(|entry| entry.expires_at > now)
        {
            return Ok(false);
        }
        state.entries.insert(
            key,
            Entry {
                state: EntryState::Marker,
                expires_at,
            },
        );
        state.note_expiry(expires_at);
        Ok(true)
    }

    /// Claim pending work.
    ///
    /// Existing `marker` entries created by [`InMemoryIdempotencyStore::set_once`]
    /// are reported as [`WorkClaim::Completed`], matching Redis store semantics.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for invalid keys or lock failures.
    pub async fn claim_pending(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<WorkClaim, IdempotencyRuntimeError> {
        let key = namespaced_key(self.namespace.as_ref(), key)?;
        let now = std::time::Instant::now();
        let expires_at = now + ttl_duration(ttl);
        let mut state = self
            .state
            .lock()
            .map_err(|error| IdempotencyRuntimeError::Backend(error.to_string()))?;
        state.maybe_cleanup(now);
        match state.entries.get(&key).copied() {
            Some(Entry {
                state: EntryState::Pending,
                expires_at,
            }) if expires_at > now => Ok(WorkClaim::Pending),
            Some(Entry { expires_at, .. }) if expires_at > now => Ok(WorkClaim::Completed),
            _ => {
                state.entries.insert(
                    key,
                    Entry {
                        state: EntryState::Pending,
                        expires_at,
                    },
                );
                state.note_expiry(expires_at);
                Ok(WorkClaim::Fresh)
            }
        }
    }

    /// Mark work completed.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for invalid keys or lock failures.
    pub async fn mark_completed(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<(), IdempotencyRuntimeError> {
        let key = namespaced_key(self.namespace.as_ref(), key)?;
        let expires_at = std::time::Instant::now() + ttl_duration(ttl);
        let mut state = self
            .state
            .lock()
            .map_err(|error| IdempotencyRuntimeError::Backend(error.to_string()))?;
        state.entries.insert(
            key,
            Entry {
                state: EntryState::Completed,
                expires_at,
            },
        );
        state.note_expiry(expires_at);
        Ok(())
    }

    /// Delete a key.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for invalid keys or lock failures.
    pub async fn delete(&self, key: &str) -> Result<(), IdempotencyRuntimeError> {
        let key = namespaced_key(self.namespace.as_ref(), key)?;
        let mut state = self
            .state
            .lock()
            .map_err(|error| IdempotencyRuntimeError::Backend(error.to_string()))?;
        state.entries.remove(&key);
        Ok(())
    }

    /// Return whether a key exists and has not expired.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for invalid keys or lock failures.
    pub async fn exists(&self, key: &str) -> Result<bool, IdempotencyRuntimeError> {
        let key = namespaced_key(self.namespace.as_ref(), key)?;
        let now = std::time::Instant::now();
        let mut state = self
            .state
            .lock()
            .map_err(|error| IdempotencyRuntimeError::Backend(error.to_string()))?;
        state.maybe_cleanup(now);
        Ok(state
            .entries
            .get(&key)
            .is_some_and(|entry| entry.expires_at > now))
    }
}

/// Redis idempotency store with cached multiplexed connection.
#[cfg(feature = "redis")]
#[derive(Clone)]
pub struct RedisIdempotencyStore {
    client: redis::Client,
    namespace: Option<IdempotencyNamespace>,
    command_timeout: Duration,
    connection: Arc<Mutex<Option<redis::aio::MultiplexedConnection>>>,
}

#[cfg(feature = "redis")]
impl fmt::Debug for RedisIdempotencyStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisIdempotencyStore")
            .field("namespace", &self.namespace)
            .field("command_timeout", &self.command_timeout)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "redis")]
impl RedisIdempotencyStore {
    /// Construct a Redis store.
    #[must_use]
    pub fn new(client: redis::Client, command_timeout: Duration) -> Self {
        Self {
            client,
            namespace: None,
            command_timeout: command_timeout.max(Duration::from_millis(1)),
            connection: Arc::new(Mutex::new(None)),
        }
    }

    /// Return the configured command timeout.
    #[must_use]
    pub const fn command_timeout(&self) -> Duration {
        self.command_timeout
    }

    /// Return a copy with a namespace.
    #[must_use]
    pub fn with_namespace(mut self, namespace: IdempotencyNamespace) -> Self {
        self.namespace = Some(namespace);
        self
    }

    async fn get_connection(
        &self,
    ) -> Result<redis::aio::MultiplexedConnection, IdempotencyRuntimeError> {
        if let Some(connection) = self
            .connection
            .lock()
            .map_err(|error| IdempotencyRuntimeError::Backend(error.to_string()))?
            .clone()
        {
            return Ok(connection);
        }
        let connection = tokio::time::timeout(
            self.command_timeout,
            self.client.get_multiplexed_async_connection(),
        )
        .await
        .map_err(|_| IdempotencyRuntimeError::Timeout)?
        .map_err(|error| IdempotencyRuntimeError::Backend(error.to_string()))?;
        *self
            .connection
            .lock()
            .map_err(|error| IdempotencyRuntimeError::Backend(error.to_string()))? =
            Some(connection.clone());
        Ok(connection)
    }

    fn clear_connection(&self) {
        if let Ok(mut connection) = self.connection.lock() {
            *connection = None;
        }
    }

    fn redis_key(&self, key: &str) -> Result<String, IdempotencyRuntimeError> {
        namespaced_key(self.namespace.as_ref(), key)
    }

    /// Ping Redis.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for timeout or backend failure.
    pub async fn ping(&self) -> Result<(), IdempotencyRuntimeError> {
        let mut connection = self.get_connection().await?;
        let result = tokio::time::timeout(
            self.command_timeout,
            redis::cmd("PING").query_async::<String>(&mut connection),
        )
        .await
        .map_err(|_| {
            self.clear_connection();
            IdempotencyRuntimeError::Timeout
        })?;
        result.map_err(|error| {
            if error.is_connection_dropped() {
                self.clear_connection();
            }
            IdempotencyRuntimeError::Backend(error.to_string())
        })?;
        Ok(())
    }

    /// Set a marker once.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for invalid keys, timeout, or
    /// backend failure.
    pub async fn set_once(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<bool, IdempotencyRuntimeError> {
        let key = self.redis_key(key)?;
        let mut connection = self.get_connection().await?;
        let result = tokio::time::timeout(
            self.command_timeout,
            redis::cmd("SET")
                .arg(&key)
                .arg("marker")
                .arg("NX")
                .arg("EX")
                .arg(ttl_secs_ceil(ttl))
                .query_async::<Option<String>>(&mut connection),
        )
        .await
        .map_err(|_| {
            self.clear_connection();
            IdempotencyRuntimeError::Timeout
        })?;
        result
            .map(|value| value.is_some())
            .map_err(|error| self.redis_error(error))
    }

    /// Claim pending work.
    ///
    /// Existing `marker` entries created by [`RedisIdempotencyStore::set_once`]
    /// are reported as [`WorkClaim::Completed`], matching in-memory store
    /// semantics.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for invalid keys, timeout, or
    /// backend failure.
    pub async fn claim_pending(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<WorkClaim, IdempotencyRuntimeError> {
        let key = self.redis_key(key)?;
        let mut connection = self.get_connection().await?;
        let script = redis::Script::new(
            r#"
            local current = redis.call("GET", KEYS[1])
            if current == "completed" or current == "marker" then
                return 2
            end
            if current == "pending" then
                return 1
            end
            if current then
                return 3
            end
            redis.call("SET", KEYS[1], "pending", "EX", tonumber(ARGV[1]))
            return 0
            "#,
        );
        let result = tokio::time::timeout(
            self.command_timeout,
            script
                .key(key)
                .arg(ttl_secs_ceil(ttl))
                .invoke_async::<i32>(&mut connection),
        )
        .await
        .map_err(|_| {
            self.clear_connection();
            IdempotencyRuntimeError::Timeout
        })?;
        match result.map_err(|error| self.redis_error(error))? {
            0 => Ok(WorkClaim::Fresh),
            1 => Ok(WorkClaim::Pending),
            2 => Ok(WorkClaim::Completed),
            _ => Err(IdempotencyRuntimeError::UnsupportedMarker),
        }
    }

    /// Mark work completed.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for invalid keys, timeout, or
    /// backend failure.
    pub async fn mark_completed(
        &self,
        key: &str,
        ttl: Duration,
    ) -> Result<(), IdempotencyRuntimeError> {
        let key = self.redis_key(key)?;
        let mut connection = self.get_connection().await?;
        let result = tokio::time::timeout(
            self.command_timeout,
            redis::cmd("SET")
                .arg(&key)
                .arg("completed")
                .arg("EX")
                .arg(ttl_secs_ceil(ttl))
                .query_async::<String>(&mut connection),
        )
        .await
        .map_err(|_| {
            self.clear_connection();
            IdempotencyRuntimeError::Timeout
        })?;
        result.map(|_| ()).map_err(|error| self.redis_error(error))
    }

    /// Delete a key.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for invalid keys, timeout, or
    /// backend failure.
    pub async fn delete(&self, key: &str) -> Result<(), IdempotencyRuntimeError> {
        let key = self.redis_key(key)?;
        let mut connection = self.get_connection().await?;
        let result = tokio::time::timeout(
            self.command_timeout,
            redis::cmd("DEL")
                .arg(&key)
                .query_async::<usize>(&mut connection),
        )
        .await
        .map_err(|_| {
            self.clear_connection();
            IdempotencyRuntimeError::Timeout
        })?;
        result.map(|_| ()).map_err(|error| self.redis_error(error))
    }

    /// Return whether a key exists.
    ///
    /// # Errors
    ///
    /// Returns [`IdempotencyRuntimeError`] for invalid keys, timeout, or
    /// backend failure.
    pub async fn exists(&self, key: &str) -> Result<bool, IdempotencyRuntimeError> {
        let key = self.redis_key(key)?;
        let mut connection = self.get_connection().await?;
        let result = tokio::time::timeout(
            self.command_timeout,
            redis::cmd("EXISTS")
                .arg(&key)
                .query_async::<usize>(&mut connection),
        )
        .await
        .map_err(|_| {
            self.clear_connection();
            IdempotencyRuntimeError::Timeout
        })?;
        result
            .map(|count| count > 0)
            .map_err(|error| self.redis_error(error))
    }

    fn redis_error(&self, error: redis::RedisError) -> IdempotencyRuntimeError {
        if error.is_connection_dropped() {
            self.clear_connection();
        }
        IdempotencyRuntimeError::Backend(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "redis")]
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn in_memory_set_once_has_single_winner() {
        let store = InMemoryIdempotencyStore::new();
        assert!(store
            .set_once("key:1", Duration::from_secs(10))
            .await
            .unwrap());
        assert!(!store
            .set_once("key:1", Duration::from_secs(10))
            .await
            .unwrap());
        assert!(store
            .set_once("key:2", Duration::from_secs(10))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn in_memory_claim_pending_respects_ttl_and_completion() {
        let store = InMemoryIdempotencyStore::new();
        assert_eq!(
            store
                .claim_pending("evt:1", Duration::from_secs(5))
                .await
                .unwrap(),
            WorkClaim::Fresh
        );
        assert_eq!(
            store
                .claim_pending("evt:1", Duration::from_secs(5))
                .await
                .unwrap(),
            WorkClaim::Pending
        );
        store
            .mark_completed("evt:1", Duration::from_millis(1))
            .await
            .unwrap();
        assert_eq!(
            store
                .claim_pending("evt:1", Duration::from_secs(5))
                .await
                .unwrap(),
            WorkClaim::Completed
        );
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(
            store
                .claim_pending("evt:1", Duration::from_secs(5))
                .await
                .unwrap(),
            WorkClaim::Fresh
        );
    }

    #[tokio::test]
    async fn marker_claims_are_treated_as_completed() {
        let store = InMemoryIdempotencyStore::new();
        assert!(store
            .set_once("marker:1", Duration::from_secs(5))
            .await
            .unwrap());
        assert_eq!(
            store
                .claim_pending("marker:1", Duration::from_secs(5))
                .await
                .unwrap(),
            WorkClaim::Completed
        );
    }

    #[tokio::test]
    async fn delete_and_exists_use_namespace() {
        let namespace = IdempotencyNamespace::new("app").unwrap();
        let store = InMemoryIdempotencyStore::with_namespace(namespace);
        assert!(!store.exists("key").await.unwrap());
        store
            .mark_completed("key", Duration::from_secs(10))
            .await
            .unwrap();
        assert!(store.exists("key").await.unwrap());
        store.delete("key").await.unwrap();
        assert!(!store.exists("key").await.unwrap());
    }

    #[test]
    fn rejects_unsafe_namespaces_and_keys() {
        assert!(IdempotencyNamespace::new("airline").is_ok());
        assert!(IdempotencyNamespace::new("bad namespace").is_err());
    }

    #[test]
    fn namespaced_keys_are_length_prefixed() {
        let first_namespace = IdempotencyNamespace::new("a").unwrap();
        let second_namespace = IdempotencyNamespace::new("a:b").unwrap();
        assert_ne!(
            namespaced_key(Some(&first_namespace), "b:c").unwrap(),
            namespaced_key(Some(&second_namespace), "c").unwrap()
        );
        assert_ne!(
            namespaced_key(Some(&first_namespace), "key").unwrap(),
            namespaced_key(None, "a:key").unwrap()
        );
    }

    #[cfg(feature = "redis")]
    fn redis_url() -> Option<String> {
        std::env::var("IDEMPOTENCY_REDIS_URL")
            .or_else(|_| std::env::var("WORLD_INFRA_REDIS_URL"))
            .ok()
            .filter(|value| !value.trim().is_empty())
    }

    #[cfg(feature = "redis")]
    fn unique_namespace() -> IdempotencyNamespace {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        IdempotencyNamespace::new(format!("idempotency-runtime-core-test:{nanos}"))
            .expect("generated namespace is valid")
    }

    #[cfg(feature = "redis")]
    #[tokio::test]
    async fn redis_store_claims_markers_ttl_namespace_and_bad_markers() {
        let Some(url) = redis_url() else {
            return;
        };
        let client = redis::Client::open(url).expect("redis URL should be valid");
        let namespace = unique_namespace();
        let store = RedisIdempotencyStore::new(client.clone(), Duration::from_secs(2))
            .with_namespace(namespace.clone());
        store.ping().await.expect("redis ping should succeed");

        assert_eq!(
            store
                .claim_pending("work", Duration::from_secs(30))
                .await
                .expect("first claim should succeed"),
            WorkClaim::Fresh
        );
        assert_eq!(
            store
                .claim_pending("work", Duration::from_secs(30))
                .await
                .expect("second claim should succeed"),
            WorkClaim::Pending
        );
        store
            .mark_completed("work", Duration::from_secs(30))
            .await
            .expect("mark completed should succeed");
        assert_eq!(
            store
                .claim_pending("work", Duration::from_secs(30))
                .await
                .expect("completed claim should succeed"),
            WorkClaim::Completed
        );

        assert!(store
            .set_once("marker", Duration::from_secs(30))
            .await
            .expect("marker set should succeed"));
        assert_eq!(
            store
                .claim_pending("marker", Duration::from_secs(30))
                .await
                .expect("marker claim should succeed"),
            WorkClaim::Completed
        );

        let other = RedisIdempotencyStore::new(client.clone(), Duration::from_secs(2))
            .with_namespace(unique_namespace());
        assert_eq!(
            other
                .claim_pending("work", Duration::from_secs(30))
                .await
                .expect("other namespace claim should succeed"),
            WorkClaim::Fresh
        );

        assert!(store
            .set_once("ttl", Duration::from_millis(1))
            .await
            .expect("ttl marker should set"));
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .expect("direct redis connection should succeed");
        let ttl_key = namespaced_key(Some(&namespace), "ttl").expect("ttl key should be valid");
        let ttl: i64 = redis::cmd("TTL")
            .arg(&ttl_key)
            .query_async(&mut connection)
            .await
            .expect("ttl should query");
        assert!((0..=1).contains(&ttl), "expected short ttl, got {ttl}");

        let unsupported_key =
            namespaced_key(Some(&namespace), "unsupported").expect("key should be valid");
        redis::cmd("SET")
            .arg(&unsupported_key)
            .arg("surprise")
            .arg("EX")
            .arg(30)
            .query_async::<String>(&mut connection)
            .await
            .expect("unsupported marker should be inserted");
        assert_eq!(
            store
                .claim_pending("unsupported", Duration::from_secs(30))
                .await,
            Err(IdempotencyRuntimeError::UnsupportedMarker)
        );

        let _ = store.delete("work").await;
        let _ = store.delete("marker").await;
        let _ = store.delete("ttl").await;
        let _ = store.delete("unsupported").await;
        let _ = other.delete("work").await;
    }
}
