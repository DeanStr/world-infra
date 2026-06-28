//! Transaction-bound SQLx `SET LOCAL` helpers.
//!
//! This crate does not provide pool wrappers, transaction factories, RLS policy,
//! or product-specific setting names.

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
        let value = value.as_ref().trim();
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
    matches!(chars.next(), Some(ch) if ch.is_ascii_lowercase() || ch == '_')
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malicious_setting_names() {
        assert!(SettingName::new("app.world_id").is_ok());
        assert!(SettingName::new("app.world_id_2").is_ok());
        assert!(SettingName::new("App.world_id").is_err());
        assert!(SettingName::new("app.123").is_err());
        assert!(SettingName::new("1.app").is_err());
        assert!(SettingName::new("app.world_id;drop table worlds").is_err());
        assert!(SettingName::new(".app").is_err());
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
}
