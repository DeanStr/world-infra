//! Transaction-bound SQLx `SET LOCAL` helpers.
//!
//! This crate does not provide RLS policy or product-specific setting names.
//! Optional transaction helpers only begin a transaction and apply
//! product-provided settings.

use std::{error::Error, fmt};

/// Error returned for tenant-scope helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantScopeError {
    /// Setting name was not on the allowlist or contained unsafe syntax.
    InvalidSettingName,
    /// Setting value contained a NUL byte.
    InvalidSettingValue,
    /// SQLx reported an error.
    Sql(String),
}

impl fmt::Display for TenantScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSettingName => f.write_str("setting name is invalid"),
            Self::InvalidSettingValue => f.write_str("setting value is invalid"),
            Self::Sql(error) => write!(f, "SQL scope helper failed: {error}"),
        }
    }
}

impl Error for TenantScopeError {}

/// Validated PostgreSQL setting name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SettingName(String);

impl SettingName {
    /// Validate a setting name. Use product-owned allowlists around this helper.
    ///
    /// # Errors
    ///
    /// Returns [`TenantScopeError::InvalidSettingName`] for unsafe names.
    pub fn new(value: impl AsRef<str>) -> Result<Self, TenantScopeError> {
        let value = value.as_ref();
        if value.chars().any(char::is_control) {
            return Err(TenantScopeError::InvalidSettingName);
        }
        let value = value.trim();
        if value.is_empty()
            || value.len() > 128
            || value.starts_with('.')
            || value.ends_with('.')
            || value.split('.').any(|part| !is_unquoted_identifier(part))
        {
            return Err(TenantScopeError::InvalidSettingName);
        }
        Ok(Self(value.to_owned()))
    }

    /// Access the setting name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_unquoted_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    !value.is_empty()
        && value.len() <= 63
        && matches!(chars.next(), Some(ch) if ch.is_ascii_lowercase() || ch == '_')
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

/// Serializable `SET LOCAL` value.
pub trait ScopeValue {
    /// Serialize to a string suitable for binding or literal escaping.
    fn to_scope_value(&self) -> Result<String, TenantScopeError>;
}

impl ScopeValue for str {
    fn to_scope_value(&self) -> Result<String, TenantScopeError> {
        if self.contains('\0') {
            return Err(TenantScopeError::InvalidSettingValue);
        }
        Ok(self.to_owned())
    }
}

impl ScopeValue for &str {
    fn to_scope_value(&self) -> Result<String, TenantScopeError> {
        (*self).to_scope_value()
    }
}

impl ScopeValue for String {
    fn to_scope_value(&self) -> Result<String, TenantScopeError> {
        self.as_str().to_scope_value()
    }
}

impl ScopeValue for i32 {
    fn to_scope_value(&self) -> Result<String, TenantScopeError> {
        Ok(self.to_string())
    }
}

impl ScopeValue for u32 {
    fn to_scope_value(&self) -> Result<String, TenantScopeError> {
        Ok(self.to_string())
    }
}

impl ScopeValue for i64 {
    fn to_scope_value(&self) -> Result<String, TenantScopeError> {
        Ok(self.to_string())
    }
}

impl ScopeValue for u64 {
    fn to_scope_value(&self) -> Result<String, TenantScopeError> {
        Ok(self.to_string())
    }
}

#[cfg(feature = "uuid")]
impl ScopeValue for uuid::Uuid {
    fn to_scope_value(&self) -> Result<String, TenantScopeError> {
        Ok(self.to_string())
    }
}

/// Product-provided setting assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAssignment {
    name: SettingName,
    value: String,
}

impl ScopeAssignment {
    /// Construct a setting assignment.
    ///
    /// # Errors
    ///
    /// Returns [`TenantScopeError`] if the value cannot be serialized.
    pub fn new(name: SettingName, value: impl ScopeValue) -> Result<Self, TenantScopeError> {
        Ok(Self {
            name,
            value: value.to_scope_value()?,
        })
    }

    /// Validate a setting name and construct an assignment.
    ///
    /// # Errors
    ///
    /// Returns [`TenantScopeError`] if the setting name or value is invalid.
    pub fn from_parts(
        name: impl AsRef<str>,
        value: impl ScopeValue,
    ) -> Result<Self, TenantScopeError> {
        Self::new(SettingName::new(name)?, value)
    }

    /// Access the setting name.
    #[must_use]
    pub fn name(&self) -> &SettingName {
        &self.name
    }

    /// Access the serialized setting value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Escape a setting value as a PostgreSQL string literal.
///
/// Prefer [`set_local`] for execution; it uses parameterized `set_config`.
/// This helper remains for reviewed statement display and product tests.
///
/// # Errors
///
/// Returns [`TenantScopeError::InvalidSettingValue`] for NUL bytes.
pub fn quote_setting_value(value: impl ScopeValue) -> Result<String, TenantScopeError> {
    let value = value.to_scope_value()?;
    let escaped = value.replace('\\', "\\\\").replace('\'', "''");
    Ok(format!("E'{escaped}'"))
}

/// Build a reviewed `SET LOCAL` statement.
///
/// # Errors
///
/// Returns [`TenantScopeError`] for invalid values.
pub fn set_local_statement(
    name: &SettingName,
    value: impl ScopeValue,
) -> Result<String, TenantScopeError> {
    Ok(format!(
        "SET LOCAL {} = {}",
        name.as_str(),
        quote_setting_value(value)?
    ))
}

/// Set a local PostgreSQL setting inside an existing transaction.
///
/// # Errors
///
/// Returns [`TenantScopeError::Sql`] when SQLx execution fails.
#[cfg(feature = "sqlx-postgres")]
pub async fn set_local<'c>(
    tx: &mut sqlx::Transaction<'c, sqlx::Postgres>,
    name: &SettingName,
    value: impl ScopeValue,
) -> Result<(), TenantScopeError> {
    let value = value.to_scope_value()?;
    sqlx::query("SELECT set_config($1, $2, true)")
        .bind(name.as_str())
        .bind(value)
        .execute(&mut **tx)
        .await
        .map_err(|error| TenantScopeError::Sql(error.to_string()))?;
    Ok(())
}

/// Set a prebuilt assignment inside an existing transaction.
///
/// # Errors
///
/// Returns [`TenantScopeError::Sql`] when SQLx execution fails.
#[cfg(feature = "sqlx-postgres")]
pub async fn set_local_assignment<'c>(
    tx: &mut sqlx::Transaction<'c, sqlx::Postgres>,
    assignment: &ScopeAssignment,
) -> Result<(), TenantScopeError> {
    set_local(tx, assignment.name(), assignment.value()).await
}

/// Set many local PostgreSQL settings inside an existing transaction.
///
/// # Errors
///
/// Returns [`TenantScopeError::Sql`] when SQLx execution fails.
#[cfg(feature = "sqlx-postgres")]
pub async fn set_local_many<'c>(
    tx: &mut sqlx::Transaction<'c, sqlx::Postgres>,
    assignments: &[ScopeAssignment],
) -> Result<(), TenantScopeError> {
    for assignment in assignments {
        set_local_assignment(tx, assignment).await?;
    }
    Ok(())
}

/// Set an `i64` local setting inside an existing transaction.
///
/// # Errors
///
/// Returns [`TenantScopeError`] when SQLx execution fails.
#[cfg(feature = "sqlx-postgres")]
pub async fn set_local_i64<'c>(
    tx: &mut sqlx::Transaction<'c, sqlx::Postgres>,
    name: &SettingName,
    value: i64,
) -> Result<(), TenantScopeError> {
    set_local(tx, name, value).await
}

/// Set an `i32` local setting inside an existing transaction.
///
/// # Errors
///
/// Returns [`TenantScopeError`] when SQLx execution fails.
#[cfg(feature = "sqlx-postgres")]
pub async fn set_local_i32<'c>(
    tx: &mut sqlx::Transaction<'c, sqlx::Postgres>,
    name: &SettingName,
    value: i32,
) -> Result<(), TenantScopeError> {
    set_local(tx, name, value).await
}

/// Set a `u64` local setting inside an existing transaction.
///
/// # Errors
///
/// Returns [`TenantScopeError`] when SQLx execution fails.
#[cfg(feature = "sqlx-postgres")]
pub async fn set_local_u64<'c>(
    tx: &mut sqlx::Transaction<'c, sqlx::Postgres>,
    name: &SettingName,
    value: u64,
) -> Result<(), TenantScopeError> {
    set_local(tx, name, value).await
}

/// Set a `u32` local setting inside an existing transaction.
///
/// # Errors
///
/// Returns [`TenantScopeError`] when SQLx execution fails.
#[cfg(feature = "sqlx-postgres")]
pub async fn set_local_u32<'c>(
    tx: &mut sqlx::Transaction<'c, sqlx::Postgres>,
    name: &SettingName,
    value: u32,
) -> Result<(), TenantScopeError> {
    set_local(tx, name, value).await
}

/// Set a string local setting inside an existing transaction.
///
/// # Errors
///
/// Returns [`TenantScopeError`] when SQLx execution fails.
#[cfg(feature = "sqlx-postgres")]
pub async fn set_local_str<'c>(
    tx: &mut sqlx::Transaction<'c, sqlx::Postgres>,
    name: &SettingName,
    value: &str,
) -> Result<(), TenantScopeError> {
    set_local(tx, name, value).await
}

/// Set a UUID local setting inside an existing transaction.
///
/// # Errors
///
/// Returns [`TenantScopeError`] when SQLx execution fails.
#[cfg(all(feature = "sqlx-postgres", feature = "uuid"))]
pub async fn set_local_uuid<'c>(
    tx: &mut sqlx::Transaction<'c, sqlx::Postgres>,
    name: &SettingName,
    value: uuid::Uuid,
) -> Result<(), TenantScopeError> {
    set_local(tx, name, value).await
}

/// Begin a transaction and apply product-provided local settings.
///
/// Product adapters own authorization and the setting names passed here.
///
/// # Errors
///
/// Returns [`TenantScopeError::Sql`] when beginning the transaction or applying
/// scope fails.
#[cfg(feature = "sqlx-postgres")]
pub async fn begin_scoped_tx<'p>(
    pool: &'p sqlx::Pool<sqlx::Postgres>,
    assignments: &[ScopeAssignment],
) -> Result<sqlx::Transaction<'p, sqlx::Postgres>, TenantScopeError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| TenantScopeError::Sql(error.to_string()))?;
    set_local_many(&mut tx, assignments).await?;
    Ok(tx)
}

/// Begin a read-only repeatable-read transaction and apply local settings.
///
/// # Errors
///
/// Returns [`TenantScopeError::Sql`] when beginning or configuring the
/// transaction fails.
#[cfg(feature = "sqlx-postgres")]
pub async fn begin_readonly_repeatable_scoped_tx<'p>(
    pool: &'p sqlx::Pool<sqlx::Postgres>,
    assignments: &[ScopeAssignment],
) -> Result<sqlx::Transaction<'p, sqlx::Postgres>, TenantScopeError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| TenantScopeError::Sql(error.to_string()))?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|error| TenantScopeError::Sql(error.to_string()))?;
    set_local_many(&mut tx, assignments).await?;
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malicious_setting_names() {
        assert!(SettingName::new("app.world_id").is_ok());
        assert_eq!(
            SettingName::new(" app.world_id ").unwrap().as_str(),
            "app.world_id"
        );
        assert!(SettingName::new("app.world_id_2").is_ok());
        assert!(SettingName::new("App.world_id").is_err());
        assert!(SettingName::new("app.123").is_err());
        assert!(SettingName::new("1.app").is_err());
        assert!(SettingName::new("app.world_id;drop table worlds").is_err());
        assert!(SettingName::new("app.world_id\n").is_err());
        assert!(SettingName::new(".app").is_err());
        assert!(SettingName::new("app.").is_err());
        assert!(SettingName::new("a".repeat(129)).is_err());
        assert!(SettingName::new(format!("app.{}", "a".repeat(64))).is_err());
    }

    #[test]
    fn escapes_setting_values() {
        let name = SettingName::new("app.world_id").unwrap();
        assert_eq!(
            set_local_statement(&name, "world-'quoted'").unwrap(),
            "SET LOCAL app.world_id = E'world-''quoted'''"
        );
        assert_eq!(
            set_local_statement(&name, r"world\';select pg_sleep(10);--").unwrap(),
            r"SET LOCAL app.world_id = E'world\\'';select pg_sleep(10);--'"
        );
        assert!(quote_setting_value("bad\0value").is_err());
    }

    #[test]
    fn scope_assignments_validate_name_and_value() {
        let assignment = ScopeAssignment::from_parts("app.world_id", 7_i64).unwrap();
        assert_eq!(assignment.name().as_str(), "app.world_id");
        assert_eq!(assignment.value(), "7");
        let assignment = ScopeAssignment::from_parts("app.world_id", 7_i32).unwrap();
        assert_eq!(assignment.value(), "7");
        let assignment = ScopeAssignment::from_parts("app.airline_count", 3_u32).unwrap();
        assert_eq!(assignment.value(), "3");
        assert!(ScopeAssignment::from_parts("app.world_id;drop", 7_i64).is_err());
        assert!(ScopeAssignment::from_parts("app.world_id", "bad\0value").is_err());
    }

    #[test]
    fn scope_value_variants_and_uuid_serialize_for_set_local() {
        assert_eq!("value".to_scope_value().unwrap(), "value");
        assert_eq!("value".to_owned().to_scope_value().unwrap(), "value");
        assert_eq!(7_u64.to_scope_value().unwrap(), "7");

        #[cfg(feature = "uuid")]
        {
            let uuid = uuid::Uuid::nil();
            assert_eq!(uuid.to_scope_value().unwrap(), uuid.to_string());
            let assignment = ScopeAssignment::from_parts("app.world_id", uuid).unwrap();
            assert_eq!(assignment.value(), "00000000-0000-0000-0000-000000000000");
        }
    }

    #[test]
    fn tenant_scope_error_display_is_stable() {
        assert_eq!(
            TenantScopeError::InvalidSettingName.to_string(),
            "setting name is invalid"
        );
        assert_eq!(
            TenantScopeError::InvalidSettingValue.to_string(),
            "setting value is invalid"
        );
        assert_eq!(
            TenantScopeError::Sql("connection closed".to_owned()).to_string(),
            "SQL scope helper failed: connection closed"
        );
    }
}
