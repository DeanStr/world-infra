//! Opaque refresh-token minting and rotation stores.
//!
//! This crate shares hardened mechanics from mature product auth systems:
//! random opaque refresh tokens, SHA-256 token hashing, namespaced volatile
//! stores, TTL clamping, and single-use rotation. Products still own account
//! lookup, cookie policy, access-token signing, session-version invalidation,
//! and API response shapes.

use std::{
    collections::HashMap,
    error::Error,
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};

/// Refresh-token store error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshStoreError {
    /// Namespace was blank or unsafe.
    InvalidNamespace,
    /// Token was blank or unsafe.
    InvalidToken,
    /// Refresh key or stored-session schema configuration was unsafe.
    InvalidSchemaConfig,
    /// Session field was blank or unsafe.
    InvalidSessionField {
        /// Field name.
        field: &'static str,
    },
    /// Backend operation timed out.
    Timeout,
    /// Backend returned unreadable session data.
    InvalidStoredSession(String),
    /// Backend failed.
    Backend(String),
}

impl fmt::Display for RefreshStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidNamespace => f.write_str("refresh namespace is invalid"),
            Self::InvalidToken => f.write_str("refresh token is invalid"),
            Self::InvalidSchemaConfig => f.write_str("refresh store schema config is invalid"),
            Self::InvalidSessionField { field } => write!(f, "refresh session {field} is invalid"),
            Self::Timeout => f.write_str("refresh store timed out"),
            Self::InvalidStoredSession(error) => {
                write!(f, "refresh store contained invalid session: {error}")
            }
            Self::Backend(error) => write!(f, "refresh store failed: {error}"),
        }
    }
}

impl Error for RefreshStoreError {}

/// Redis key and hash-field schema for refresh-token stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshStoreConfig {
    key_prefix: String,
    key_version: String,
    subject_id_field: String,
    session_id_field: String,
    issued_at_unix_secs_field: String,
    session_version_field: String,
}

impl Default for RefreshStoreConfig {
    fn default() -> Self {
        Self {
            key_prefix: "rt".to_owned(),
            key_version: "v1".to_owned(),
            subject_id_field: "subject_id".to_owned(),
            session_id_field: "session_id".to_owned(),
            issued_at_unix_secs_field: "issued_at_unix_secs".to_owned(),
            session_version_field: "session_version".to_owned(),
        }
    }
}

impl RefreshStoreConfig {
    /// Construct a refresh-store schema config.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError::InvalidSchemaConfig`] for blank or unsafe
    /// key/field names.
    pub fn new(
        key_prefix: impl AsRef<str>,
        key_version: impl AsRef<str>,
        subject_id_field: impl AsRef<str>,
        session_id_field: impl AsRef<str>,
        issued_at_unix_secs_field: impl AsRef<str>,
        session_version_field: impl AsRef<str>,
    ) -> Result<Self, RefreshStoreError> {
        Ok(Self {
            key_prefix: validate_schema_part(key_prefix.as_ref())?.to_owned(),
            key_version: validate_schema_part(key_version.as_ref())?.to_owned(),
            subject_id_field: validate_schema_part(subject_id_field.as_ref())?.to_owned(),
            session_id_field: validate_schema_part(session_id_field.as_ref())?.to_owned(),
            issued_at_unix_secs_field: validate_schema_part(issued_at_unix_secs_field.as_ref())?
                .to_owned(),
            session_version_field: validate_schema_part(session_version_field.as_ref())?.to_owned(),
        })
    }

    /// Return a copy with a different key version, such as `v3`.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError::InvalidSchemaConfig`] for unsafe values.
    pub fn with_key_version(
        mut self,
        key_version: impl AsRef<str>,
    ) -> Result<Self, RefreshStoreError> {
        self.key_version = validate_schema_part(key_version.as_ref())?.to_owned();
        Ok(self)
    }

    /// Return a copy with different stored-session field names.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError::InvalidSchemaConfig`] for unsafe values.
    pub fn with_session_fields(
        mut self,
        subject_id_field: impl AsRef<str>,
        session_id_field: impl AsRef<str>,
        issued_at_unix_secs_field: impl AsRef<str>,
        session_version_field: impl AsRef<str>,
    ) -> Result<Self, RefreshStoreError> {
        self.subject_id_field = validate_schema_part(subject_id_field.as_ref())?.to_owned();
        self.session_id_field = validate_schema_part(session_id_field.as_ref())?.to_owned();
        self.issued_at_unix_secs_field =
            validate_schema_part(issued_at_unix_secs_field.as_ref())?.to_owned();
        self.session_version_field =
            validate_schema_part(session_version_field.as_ref())?.to_owned();
        Ok(self)
    }

    /// Access the key prefix.
    #[must_use]
    pub fn key_prefix(&self) -> &str {
        &self.key_prefix
    }

    /// Access the key version.
    #[must_use]
    pub fn key_version(&self) -> &str {
        &self.key_version
    }

    /// Access the subject/account field name.
    #[must_use]
    pub fn subject_id_field(&self) -> &str {
        &self.subject_id_field
    }

    /// Access the session field name.
    #[must_use]
    pub fn session_id_field(&self) -> &str {
        &self.session_id_field
    }

    /// Access the issued-at field name.
    #[must_use]
    pub fn issued_at_unix_secs_field(&self) -> &str {
        &self.issued_at_unix_secs_field
    }

    /// Access the session-version field name.
    #[must_use]
    pub fn session_version_field(&self) -> &str {
        &self.session_version_field
    }
}

/// Runtime namespace prepended to volatile refresh-token keys.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RefreshNamespace(String);

impl RefreshNamespace {
    /// Construct a refresh namespace.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError::InvalidNamespace`] for blank or unsafe
    /// namespaces.
    pub fn new(value: impl AsRef<str>) -> Result<Self, RefreshStoreError> {
        let value =
            validate_key_part(value.as_ref()).map_err(|_| RefreshStoreError::InvalidNamespace)?;
        Ok(Self(value.to_owned()))
    }

    /// Access the namespace.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Product-neutral refresh session payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshSession {
    /// Subject/account identifier.
    pub subject_id: String,
    /// Product session identifier.
    pub session_id: String,
    /// Issued-at timestamp in Unix seconds.
    pub issued_at_unix_secs: i64,
    /// Account/session version at issue time.
    pub session_version: u64,
}

impl RefreshSession {
    /// Construct a refresh session.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for blank or unsafe identifiers.
    pub fn new(
        subject_id: impl AsRef<str>,
        session_id: impl AsRef<str>,
        issued_at_unix_secs: i64,
        session_version: u64,
    ) -> Result<Self, RefreshStoreError> {
        let subject_id = validate_session_field(subject_id.as_ref(), "subject_id")?.to_owned();
        let session_id = validate_session_field(session_id.as_ref(), "session_id")?.to_owned();
        Ok(Self {
            subject_id,
            session_id,
            issued_at_unix_secs,
            session_version,
        })
    }
}

/// Mint a high-entropy URL-safe refresh token.
#[must_use]
pub fn mint_refresh_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

/// Return the SHA-256 hex hash of a refresh token.
///
/// # Errors
///
/// Returns [`RefreshStoreError::InvalidToken`] for blank/control-character
/// tokens.
pub fn hash_refresh_token(token: &str) -> Result<String, RefreshStoreError> {
    let token = validate_token(token)?;
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    Ok(hex_lower(hasher.finalize()))
}

fn hex_lower(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn validate_token(token: &str) -> Result<&str, RefreshStoreError> {
    if token.is_empty()
        || token.len() > 1024
        || token
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(RefreshStoreError::InvalidToken);
    }
    Ok(token)
}

fn validate_key_part(value: &str) -> Result<&str, RefreshStoreError> {
    if value.chars().any(char::is_control) {
        return Err(RefreshStoreError::InvalidNamespace);
    }
    let value = value.trim();
    if value.is_empty()
        || value.len() > 256
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(RefreshStoreError::InvalidNamespace);
    }
    Ok(value)
}

fn validate_schema_part(value: &str) -> Result<&str, RefreshStoreError> {
    if value.trim() != value {
        return Err(RefreshStoreError::InvalidSchemaConfig);
    }
    let value = validate_key_part(value).map_err(|_| RefreshStoreError::InvalidSchemaConfig)?;
    if value
        .bytes()
        .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    {
        return Err(RefreshStoreError::InvalidSchemaConfig);
    }
    Ok(value)
}

fn validate_session_field<'a>(
    value: &'a str,
    field: &'static str,
) -> Result<&'a str, RefreshStoreError> {
    if value.is_empty()
        || value.len() > 256
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(RefreshStoreError::InvalidSessionField { field });
    }
    Ok(value)
}

fn ttl_duration(ttl: Duration) -> Duration {
    if ttl.is_zero() {
        Duration::from_secs(1)
    } else {
        ttl
    }
}

#[cfg(feature = "redis")]
fn ttl_secs_ceil(ttl: Duration) -> u64 {
    let ttl = ttl_duration(ttl);
    ttl.as_secs()
        .saturating_add(u64::from(ttl.subsec_nanos() != 0))
        .max(1)
}

fn store_key(
    config: &RefreshStoreConfig,
    namespace: Option<&RefreshNamespace>,
    token: &str,
) -> Result<String, RefreshStoreError> {
    let token_hash = hash_refresh_token(token)?;
    Ok(match namespace {
        Some(namespace) => format!(
            "{}:{}:{}:{token_hash}",
            namespace.as_str(),
            config.key_prefix,
            config.key_version
        ),
        None => format!("{}:{}:{token_hash}", config.key_prefix, config.key_version),
    })
}

#[derive(Debug, Clone)]
struct Entry {
    session: RefreshSession,
    expires_at: std::time::Instant,
}

/// In-memory refresh-token store for tests, local development, and lightweight
/// deployments.
#[derive(Debug, Clone, Default)]
pub struct InMemoryRefreshStore {
    namespace: Option<RefreshNamespace>,
    config: RefreshStoreConfig,
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}

impl InMemoryRefreshStore {
    /// Construct an empty in-memory refresh store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct an empty in-memory refresh store with a namespace.
    #[must_use]
    pub fn with_namespace(namespace: RefreshNamespace) -> Self {
        Self {
            namespace: Some(namespace),
            config: RefreshStoreConfig::default(),
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Return a copy configured with a key/schema config.
    #[must_use]
    pub fn with_config(mut self, config: RefreshStoreConfig) -> Self {
        self.config = config;
        self
    }

    /// Return the key used for a token.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for invalid tokens.
    pub fn key_for_token(&self, token: &str) -> Result<String, RefreshStoreError> {
        store_key(&self.config, self.namespace.as_ref(), token)
    }

    /// Store a refresh session.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for invalid tokens or lock failures.
    pub async fn set(
        &self,
        token: &str,
        session: RefreshSession,
        ttl: Duration,
    ) -> Result<(), RefreshStoreError> {
        let key = store_key(&self.config, self.namespace.as_ref(), token)?;
        let expires_at = std::time::Instant::now() + ttl_duration(ttl);
        self.entries
            .lock()
            .map_err(|error| RefreshStoreError::Backend(error.to_string()))?
            .insert(
                key,
                Entry {
                    session,
                    expires_at,
                },
            );
        Ok(())
    }

    /// Load a refresh session.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for invalid tokens or lock failures.
    pub async fn get(&self, token: &str) -> Result<Option<RefreshSession>, RefreshStoreError> {
        let key = store_key(&self.config, self.namespace.as_ref(), token)?;
        let now = std::time::Instant::now();
        let mut entries = self
            .entries
            .lock()
            .map_err(|error| RefreshStoreError::Backend(error.to_string()))?;
        let Some(entry) = entries.get(&key).cloned() else {
            return Ok(None);
        };
        if entry.expires_at <= now {
            entries.remove(&key);
            return Ok(None);
        }
        Ok(Some(entry.session))
    }

    /// Rotate a refresh token if and only if the old token maps to
    /// `current_session`.
    ///
    /// A successful rotation deletes the old token and writes the new token.
    /// Replay of the old token returns `Ok(false)`.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for invalid tokens or lock failures.
    pub async fn rotate(
        &self,
        old_token: &str,
        new_token: &str,
        current_session: &RefreshSession,
        new_session: RefreshSession,
        ttl: Duration,
    ) -> Result<bool, RefreshStoreError> {
        let old_key = store_key(&self.config, self.namespace.as_ref(), old_token)?;
        let new_key = store_key(&self.config, self.namespace.as_ref(), new_token)?;
        if old_key == new_key {
            return Ok(false);
        }
        let now = std::time::Instant::now();
        let expires_at = now + ttl_duration(ttl);
        let mut entries = self
            .entries
            .lock()
            .map_err(|error| RefreshStoreError::Backend(error.to_string()))?;
        let Some(old_entry) = entries.get(&old_key).cloned() else {
            return Ok(false);
        };
        if old_entry.expires_at <= now {
            entries.remove(&old_key);
            return Ok(false);
        }
        if let Some(new_entry) = entries.get(&new_key).cloned() {
            if new_entry.expires_at > now {
                return Ok(false);
            }
            entries.remove(&new_key);
        }
        if old_entry.session != *current_session {
            return Ok(false);
        }
        entries.remove(&old_key);
        entries.insert(
            new_key,
            Entry {
                session: new_session,
                expires_at,
            },
        );
        Ok(true)
    }

    /// Delete a refresh token.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for invalid tokens or lock failures.
    pub async fn delete(&self, token: &str) -> Result<(), RefreshStoreError> {
        let key = store_key(&self.config, self.namespace.as_ref(), token)?;
        self.entries
            .lock()
            .map_err(|error| RefreshStoreError::Backend(error.to_string()))?
            .remove(&key);
        Ok(())
    }
}

/// Redis refresh-token store.
#[cfg(feature = "redis")]
#[derive(Clone)]
pub struct RedisRefreshStore {
    namespace: Option<RefreshNamespace>,
    config: RefreshStoreConfig,
    client: redis::Client,
    command_timeout: Duration,
    conn: Arc<tokio::sync::RwLock<Option<redis::aio::MultiplexedConnection>>>,
}

#[cfg(feature = "redis")]
impl RedisRefreshStore {
    /// Construct a Redis refresh store.
    #[must_use]
    pub fn new(client: redis::Client, command_timeout: Duration) -> Self {
        Self {
            namespace: None,
            config: RefreshStoreConfig::default(),
            client,
            command_timeout,
            conn: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Return a copy configured with a namespace.
    #[must_use]
    pub fn with_namespace(mut self, namespace: RefreshNamespace) -> Self {
        self.namespace = Some(namespace);
        self
    }

    /// Return a copy configured with a key/schema config.
    #[must_use]
    pub fn with_config(mut self, config: RefreshStoreConfig) -> Self {
        self.config = config;
        self
    }

    /// Return the key used for a token.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for invalid tokens.
    pub fn key_for_token(&self, token: &str) -> Result<String, RefreshStoreError> {
        store_key(&self.config, self.namespace.as_ref(), token)
    }

    async fn clear_conn(&self) {
        *self.conn.write().await = None;
    }

    async fn clear_conn_on_backend_error<T>(
        &self,
        result: Result<T, RefreshStoreError>,
    ) -> Result<T, RefreshStoreError> {
        if matches!(
            &result,
            Err(RefreshStoreError::Timeout) | Err(RefreshStoreError::Backend(_))
        ) {
            self.clear_conn().await;
        }
        result
    }

    async fn get_conn(&self) -> Result<redis::aio::MultiplexedConnection, RefreshStoreError> {
        let cached = { self.conn.read().await.clone() };
        if let Some(conn) = cached {
            return Ok(conn);
        }
        let conn = tokio::time::timeout(
            self.command_timeout,
            self.client.get_multiplexed_async_connection(),
        )
        .await
        .map_err(|_| RefreshStoreError::Timeout)?
        .map_err(redis_error)?;
        *self.conn.write().await = Some(conn.clone());
        Ok(conn)
    }

    /// Store a refresh session.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for invalid input or backend failures.
    pub async fn set(
        &self,
        token: &str,
        session: RefreshSession,
        ttl: Duration,
    ) -> Result<(), RefreshStoreError> {
        let key = store_key(&self.config, self.namespace.as_ref(), token)?;
        let mut conn = self.get_conn().await?;
        let ttl_secs = ttl_secs_ceil(ttl);
        let mut pipe = redis::pipe();
        pipe.atomic()
            .cmd("HSET")
            .arg(&key)
            .arg(&self.config.subject_id_field)
            .arg(&session.subject_id)
            .arg(&self.config.session_id_field)
            .arg(&session.session_id)
            .arg(&self.config.issued_at_unix_secs_field)
            .arg(session.issued_at_unix_secs)
            .arg(&self.config.session_version_field)
            .arg(session.session_version)
            .ignore()
            .cmd("EXPIRE")
            .arg(&key)
            .arg(ttl_secs)
            .ignore();
        self.clear_conn_on_backend_error(
            timeout_redis(self.command_timeout, pipe.query_async::<()>(&mut conn)).await,
        )
        .await?;
        Ok(())
    }

    /// Load a refresh session.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for invalid input or backend failures.
    pub async fn get(&self, token: &str) -> Result<Option<RefreshSession>, RefreshStoreError> {
        let key = store_key(&self.config, self.namespace.as_ref(), token)?;
        let mut conn = self.get_conn().await?;
        let fields = self
            .clear_conn_on_backend_error(
                timeout_redis(
                    self.command_timeout,
                    redis::cmd("HGETALL")
                        .arg(&key)
                        .query_async::<HashMap<String, String>>(&mut conn),
                )
                .await,
            )
            .await?;
        if fields.is_empty() {
            return Ok(None);
        }
        match parse_session_hash(fields, &self.config) {
            Ok(session) => Ok(Some(session)),
            Err(_error) => {
                let _ = timeout_redis(
                    self.command_timeout,
                    redis::cmd("DEL").arg(&key).query_async::<usize>(&mut conn),
                )
                .await;
                Ok(None)
            }
        }
    }

    /// Rotate a refresh token if and only if the old token maps to
    /// `current_session`.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for invalid input or backend failures.
    pub async fn rotate(
        &self,
        old_token: &str,
        new_token: &str,
        current_session: &RefreshSession,
        new_session: RefreshSession,
        ttl: Duration,
    ) -> Result<bool, RefreshStoreError> {
        let old_key = store_key(&self.config, self.namespace.as_ref(), old_token)?;
        let new_key = store_key(&self.config, self.namespace.as_ref(), new_token)?;
        if old_key == new_key {
            return Ok(false);
        }
        let mut conn = self.get_conn().await?;
        let script = redis::Script::new(
            r#"
                if redis.call("EXISTS", KEYS[1]) == 0 then
                    return 0
                end
                if redis.call("EXISTS", KEYS[2]) ~= 0 then
                    return 0
                end
                if redis.call("HGET", KEYS[1], ARGV[1]) ~= ARGV[5] then
                    return 0
                end
                if redis.call("HGET", KEYS[1], ARGV[2]) ~= ARGV[6] then
                    return 0
                end
                if redis.call("HGET", KEYS[1], ARGV[3]) ~= ARGV[7] then
                    return 0
                end
                if redis.call("HGET", KEYS[1], ARGV[4]) ~= ARGV[8] then
                    return 0
                end
                redis.call("DEL", KEYS[1])
                redis.call(
                    "HSET",
                    KEYS[2],
                    ARGV[1],
                    ARGV[9],
                    ARGV[2],
                    ARGV[10],
                    ARGV[3],
                    ARGV[11],
                    ARGV[4],
                    ARGV[12]
                )
                redis.call("EXPIRE", KEYS[2], tonumber(ARGV[13]))
                return 1
            "#,
        );
        let result = self
            .clear_conn_on_backend_error(
                timeout_redis(
                    self.command_timeout,
                    script
                        .key(old_key)
                        .key(new_key)
                        .arg(&self.config.subject_id_field)
                        .arg(&self.config.session_id_field)
                        .arg(&self.config.issued_at_unix_secs_field)
                        .arg(&self.config.session_version_field)
                        .arg(&current_session.subject_id)
                        .arg(&current_session.session_id)
                        .arg(current_session.issued_at_unix_secs.to_string())
                        .arg(current_session.session_version.to_string())
                        .arg(&new_session.subject_id)
                        .arg(&new_session.session_id)
                        .arg(new_session.issued_at_unix_secs.to_string())
                        .arg(new_session.session_version.to_string())
                        .arg(ttl_secs_ceil(ttl))
                        .invoke_async::<i32>(&mut conn),
                )
                .await,
            )
            .await?;
        Ok(result == 1)
    }

    /// Delete a refresh token.
    ///
    /// # Errors
    ///
    /// Returns [`RefreshStoreError`] for invalid input or backend failures.
    pub async fn delete(&self, token: &str) -> Result<(), RefreshStoreError> {
        let key = store_key(&self.config, self.namespace.as_ref(), token)?;
        let mut conn = self.get_conn().await?;
        self.clear_conn_on_backend_error(
            timeout_redis(
                self.command_timeout,
                redis::cmd("DEL").arg(&key).query_async::<usize>(&mut conn),
            )
            .await,
        )
        .await?;
        Ok(())
    }

    /// Ping Redis.
    #[must_use]
    pub async fn ping(&self) -> bool {
        let Ok(mut conn) = self.get_conn().await else {
            return false;
        };
        match timeout_redis(
            self.command_timeout,
            redis::cmd("PING").query_async::<String>(&mut conn),
        )
        .await
        {
            Ok(_) => true,
            Err(_) => {
                self.clear_conn().await;
                false
            }
        }
    }
}

#[cfg(feature = "redis")]
async fn timeout_redis<T>(
    timeout: Duration,
    future: impl std::future::Future<Output = redis::RedisResult<T>>,
) -> Result<T, RefreshStoreError> {
    match tokio::time::timeout(timeout, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(redis_error(error)),
        Err(_) => Err(RefreshStoreError::Timeout),
    }
}

#[cfg(feature = "redis")]
fn redis_error(error: redis::RedisError) -> RefreshStoreError {
    RefreshStoreError::Backend(error.to_string())
}

#[cfg(feature = "redis")]
fn parse_session_hash(
    fields: HashMap<String, String>,
    config: &RefreshStoreConfig,
) -> Result<RefreshSession, RefreshStoreError> {
    let subject_id = fields.get(&config.subject_id_field).ok_or_else(|| {
        RefreshStoreError::InvalidStoredSession(format!("missing {}", config.subject_id_field))
    })?;
    let session_id = fields.get(&config.session_id_field).ok_or_else(|| {
        RefreshStoreError::InvalidStoredSession(format!("missing {}", config.session_id_field))
    })?;
    let issued_at_unix_secs = fields
        .get(&config.issued_at_unix_secs_field)
        .ok_or_else(|| {
            RefreshStoreError::InvalidStoredSession(format!(
                "missing {}",
                config.issued_at_unix_secs_field
            ))
        })?
        .parse::<i64>()
        .map_err(|error| RefreshStoreError::InvalidStoredSession(error.to_string()))?;
    let session_version = fields
        .get(&config.session_version_field)
        .ok_or_else(|| {
            RefreshStoreError::InvalidStoredSession(format!(
                "missing {}",
                config.session_version_field
            ))
        })?
        .parse::<u64>()
        .map_err(|error| RefreshStoreError::InvalidStoredSession(error.to_string()))?;
    RefreshSession::new(subject_id, session_id, issued_at_unix_secs, session_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session() -> RefreshSession {
        RefreshSession::new("account-1", "session-1", 1_700_000_000, 3).unwrap()
    }

    fn v3_config() -> RefreshStoreConfig {
        RefreshStoreConfig::default()
            .with_key_version("v3")
            .unwrap()
            .with_session_fields("account_id", "session_id", "issued_at", "session_version")
            .unwrap()
    }

    #[test]
    fn refresh_tokens_are_url_safe_and_hashed() {
        let token = mint_refresh_token();
        assert_eq!(token.len(), 43);
        assert!(token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'));
        let hash = hash_refresh_token(&token).unwrap();
        assert_eq!(hash.len(), 64);
        assert_ne!(hash, token);
    }

    #[test]
    fn refresh_session_validates_identifiers() {
        assert!(RefreshSession::new("account", "session", 0, 0).is_ok());
        assert!(matches!(
            RefreshSession::new("", "session", 0, 0),
            Err(RefreshStoreError::InvalidSessionField {
                field: "subject_id"
            })
        ));
        assert!(matches!(
            RefreshNamespace::new("bad namespace"),
            Err(RefreshStoreError::InvalidNamespace)
        ));
        assert!(matches!(
            hash_refresh_token(" token "),
            Err(RefreshStoreError::InvalidToken)
        ));
        assert!(matches!(
            RefreshSession::new("account 1", "session", 0, 0),
            Err(RefreshStoreError::InvalidSessionField {
                field: "subject_id"
            })
        ));
    }

    #[test]
    fn refresh_store_config_controls_key_version_and_fields() {
        let config = v3_config();
        assert_eq!(config.key_version(), "v3");
        assert_eq!(config.subject_id_field(), "account_id");
        assert_eq!(config.issued_at_unix_secs_field(), "issued_at");
        assert_eq!(
            RefreshStoreConfig::default().with_key_version("bad:version"),
            Err(RefreshStoreError::InvalidSchemaConfig)
        );
        assert_eq!(
            RefreshStoreConfig::default().with_session_fields(
                "account id",
                "session_id",
                "issued_at",
                "session_version",
            ),
            Err(RefreshStoreError::InvalidSchemaConfig)
        );
        assert_eq!(
            RefreshStoreConfig::default().with_key_version(" v3 "),
            Err(RefreshStoreError::InvalidSchemaConfig)
        );

        let store = InMemoryRefreshStore::with_namespace(RefreshNamespace::new("app").unwrap())
            .with_config(config);
        let key = store.key_for_token("token").unwrap();
        assert!(key.starts_with("app:rt:v3:"));
    }

    #[tokio::test]
    async fn in_memory_roundtrip_delete_and_expiry() {
        let store = InMemoryRefreshStore::new();
        let token = mint_refresh_token();
        let session = sample_session();
        store
            .set(&token, session.clone(), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(store.get(&token).await.unwrap(), Some(session));
        store.delete(&token).await.unwrap();
        assert_eq!(store.get(&token).await.unwrap(), None);

        store
            .set(&token, sample_session(), Duration::from_millis(1))
            .await
            .unwrap();
        std::thread::sleep(Duration::from_millis(3));
        assert_eq!(store.get(&token).await.unwrap(), None);
    }

    #[tokio::test]
    async fn in_memory_rotate_is_single_use() {
        let store =
            InMemoryRefreshStore::with_namespace(RefreshNamespace::new("chairman").unwrap());
        let token = mint_refresh_token();
        let session = sample_session();
        store
            .set(&token, session.clone(), Duration::from_secs(5))
            .await
            .unwrap();

        let next_token = mint_refresh_token();
        let next_session = RefreshSession::new("account-1", "session-2", 1_700_000_001, 3).unwrap();
        assert!(store
            .rotate(
                &token,
                &next_token,
                &session,
                next_session.clone(),
                Duration::from_secs(5),
            )
            .await
            .unwrap());
        assert_eq!(store.get(&token).await.unwrap(), None);
        assert_eq!(store.get(&next_token).await.unwrap(), Some(next_session));
        assert!(!store
            .rotate(
                &token,
                &mint_refresh_token(),
                &session,
                sample_session(),
                Duration::from_secs(5),
            )
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn in_memory_rotate_to_same_token_is_rejected_without_consuming() {
        let store = InMemoryRefreshStore::new();
        let token = mint_refresh_token();
        let session = sample_session();
        store
            .set(&token, session.clone(), Duration::from_secs(5))
            .await
            .unwrap();

        let next_session = RefreshSession::new("account-1", "session-2", 1_700_000_001, 3).unwrap();
        assert!(!store
            .rotate(
                &token,
                &token,
                &session,
                next_session,
                Duration::from_secs(5),
            )
            .await
            .unwrap());
        assert_eq!(store.get(&token).await.unwrap(), Some(session));
    }

    #[tokio::test]
    async fn in_memory_rotate_rejects_existing_new_token_without_consuming() {
        let store = InMemoryRefreshStore::new();
        let token = mint_refresh_token();
        let existing_next_token = mint_refresh_token();
        let session = sample_session();
        let existing_next_session =
            RefreshSession::new("account-2", "session-existing", 1_700_000_010, 4).unwrap();
        store
            .set(&token, session.clone(), Duration::from_secs(5))
            .await
            .unwrap();
        store
            .set(
                &existing_next_token,
                existing_next_session.clone(),
                Duration::from_secs(5),
            )
            .await
            .unwrap();

        let replacement_session =
            RefreshSession::new("account-1", "session-2", 1_700_000_001, 3).unwrap();
        assert!(!store
            .rotate(
                &token,
                &existing_next_token,
                &session,
                replacement_session,
                Duration::from_secs(5),
            )
            .await
            .unwrap());
        assert_eq!(store.get(&token).await.unwrap(), Some(session));
        assert_eq!(
            store.get(&existing_next_token).await.unwrap(),
            Some(existing_next_session)
        );
    }

    #[tokio::test]
    async fn in_memory_rotate_mismatch_does_not_consume_old_token() {
        let store = InMemoryRefreshStore::new();
        let token = mint_refresh_token();
        let session = sample_session();
        store
            .set(&token, session.clone(), Duration::from_secs(5))
            .await
            .unwrap();

        let wrong_session =
            RefreshSession::new("account-1", "other-session", 1_700_000_000, 3).unwrap();
        assert!(!store
            .rotate(
                &token,
                &mint_refresh_token(),
                &wrong_session,
                sample_session(),
                Duration::from_secs(5),
            )
            .await
            .unwrap());
        assert_eq!(store.get(&token).await.unwrap(), Some(session));
    }

    #[cfg(feature = "redis")]
    fn redis_url() -> Option<String> {
        std::env::var("AUTH_REFRESH_REDIS_URL")
            .or_else(|_| std::env::var("WORLD_INFRA_REDIS_URL"))
            .ok()
            .filter(|value| !value.trim().is_empty())
    }

    #[cfg(feature = "redis")]
    fn unique_namespace() -> RefreshNamespace {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        RefreshNamespace::new(format!("auth-refresh-core-test-{nanos}"))
            .expect("generated namespace is valid")
    }

    #[cfg(feature = "redis")]
    #[tokio::test]
    async fn redis_store_covers_rotation_expiry_bad_payload_and_namespaces() {
        let Some(url) = redis_url() else {
            return;
        };
        let client = redis::Client::open(url).expect("redis URL should be valid");
        let namespace = unique_namespace();
        let config = v3_config();
        let store = RedisRefreshStore::new(client.clone(), Duration::from_secs(2))
            .with_namespace(namespace.clone())
            .with_config(config.clone());
        assert!(store.ping().await);

        let token = mint_refresh_token();
        let session = sample_session();
        store
            .set(&token, session.clone(), Duration::from_secs(30))
            .await
            .expect("redis set should succeed");
        assert_eq!(
            store.get(&token).await.expect("redis get should succeed"),
            Some(session.clone())
        );

        let next_session = RefreshSession::new("account-1", "session-2", 1_700_000_001, 3)
            .expect("session should be valid");
        assert!(!store
            .rotate(
                &token,
                &token,
                &session,
                next_session.clone(),
                Duration::from_secs(30),
            )
            .await
            .expect("same-token rotate should succeed"));
        assert_eq!(
            store.get(&token).await.expect("redis get should succeed"),
            Some(session.clone())
        );

        let collision_token = mint_refresh_token();
        let collision_session =
            RefreshSession::new("account-2", "session-existing", 1_700_000_010, 4)
                .expect("collision session should be valid");
        store
            .set(
                &collision_token,
                collision_session.clone(),
                Duration::from_secs(30),
            )
            .await
            .expect("collision set should succeed");
        assert!(!store
            .rotate(
                &token,
                &collision_token,
                &session,
                next_session.clone(),
                Duration::from_secs(30),
            )
            .await
            .expect("collision rotate should succeed"));
        assert_eq!(
            store.get(&token).await.expect("redis get should succeed"),
            Some(session.clone())
        );
        assert_eq!(
            store
                .get(&collision_token)
                .await
                .expect("collision get should succeed"),
            Some(collision_session)
        );

        let next_token = mint_refresh_token();
        assert!(store
            .rotate(
                &token,
                &next_token,
                &session,
                next_session.clone(),
                Duration::from_secs(30),
            )
            .await
            .expect("rotate should succeed"));
        assert_eq!(
            store
                .get(&token)
                .await
                .expect("old token get should succeed"),
            None
        );
        assert_eq!(
            store
                .get(&next_token)
                .await
                .expect("new token get should succeed"),
            Some(next_session.clone())
        );
        assert!(!store
            .rotate(
                &token,
                &mint_refresh_token(),
                &session,
                sample_session(),
                Duration::from_secs(30),
            )
            .await
            .expect("replay should succeed"));

        let other = RedisRefreshStore::new(client.clone(), Duration::from_secs(2))
            .with_namespace(unique_namespace())
            .with_config(config.clone());
        assert_eq!(
            other
                .get(&next_token)
                .await
                .expect("other namespace get should succeed"),
            None
        );

        let expiry_token = mint_refresh_token();
        store
            .set(&expiry_token, sample_session(), Duration::from_millis(1))
            .await
            .expect("short set should succeed");
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        assert_eq!(
            store
                .get(&expiry_token)
                .await
                .expect("expired get should succeed"),
            None
        );

        let bad_token = mint_refresh_token();
        let bad_key = store
            .key_for_token(&bad_token)
            .expect("bad token key should build");
        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .expect("direct redis connection should succeed");
        redis::cmd("HSET")
            .arg(&bad_key)
            .arg(config.subject_id_field())
            .arg("account-1")
            .arg(config.session_id_field())
            .arg("session-1")
            .arg(config.issued_at_unix_secs_field())
            .arg("1700000000")
            .query_async::<usize>(&mut conn)
            .await
            .expect("bad payload should write");
        redis::cmd("EXPIRE")
            .arg(&bad_key)
            .arg(30)
            .query_async::<bool>(&mut conn)
            .await
            .expect("bad payload ttl should set");
        assert_eq!(
            store
                .get(&bad_token)
                .await
                .expect("bad payload get should succeed"),
            None
        );
        let exists = redis::cmd("EXISTS")
            .arg(&bad_key)
            .query_async::<usize>(&mut conn)
            .await
            .expect("bad payload should be deleted");
        assert_eq!(exists, 0);

        store
            .delete(&next_token)
            .await
            .expect("delete should succeed");
        assert_eq!(
            store
                .get(&next_token)
                .await
                .expect("deleted token get should succeed"),
            None
        );
    }
}
