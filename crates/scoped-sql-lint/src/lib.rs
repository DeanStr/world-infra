//! Configurable source scanner for world and tenant scoped SQL review.
//!
//! Product repositories own table names, transaction helpers, and legitimate
//! public/global exceptions. This crate provides a tiny scanner that products
//! can configure and run as advisory or blocking CI.

use std::{error::Error, fmt};

/// Scanner configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedSqlLintConfig {
    /// Product-sensitive table names to search near raw pool access.
    pub sensitive_tables: Vec<String>,
    /// Raw pool/access patterns that should be reviewed when near sensitive SQL.
    pub raw_pool_patterns: Vec<String>,
    /// Comment marker that suppresses a finding for the current or next line.
    pub allow_comment_prefix: String,
    /// Number of lines around a raw access line to search for sensitive tables.
    pub context_radius: usize,
}

impl ScopedSqlLintConfig {
    /// Construct a config from product-owned table names.
    #[must_use]
    pub fn new(tables: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            sensitive_tables: tables.into_iter().map(Into::into).collect(),
            raw_pool_patterns: vec![
                "&self.pool".to_owned(),
                "sqlx_pool()".to_owned(),
                "require_sqlx_pool".to_owned(),
                ".pool()".to_owned(),
            ],
            allow_comment_prefix: "scoped-sqlx-lint: allow".to_owned(),
            context_radius: 8,
        }
    }
}

/// A single lint finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedSqlFinding {
    /// One-based line number.
    pub line: usize,
    /// Raw pattern that triggered the finding.
    pub pattern: String,
    /// Sensitive table found near the pattern, if any.
    pub table: Option<String>,
    /// Human-readable finding summary.
    pub message: String,
}

/// Error returned by lint config validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopedSqlLintError {
    /// No raw pool patterns were configured.
    MissingRawPoolPatterns,
    /// Allow comment marker was blank.
    MissingAllowCommentPrefix,
}

impl fmt::Display for ScopedSqlLintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRawPoolPatterns => {
                f.write_str("at least one raw pool pattern is required")
            }
            Self::MissingAllowCommentPrefix => f.write_str("allow comment prefix is required"),
        }
    }
}

impl Error for ScopedSqlLintError {}

/// Validate configuration.
///
/// # Errors
///
/// Returns [`ScopedSqlLintError`] for missing scanner basics.
pub fn validate_config(config: &ScopedSqlLintConfig) -> Result<(), ScopedSqlLintError> {
    if config.raw_pool_patterns.is_empty() {
        return Err(ScopedSqlLintError::MissingRawPoolPatterns);
    }
    if config.allow_comment_prefix.trim().is_empty() {
        return Err(ScopedSqlLintError::MissingAllowCommentPrefix);
    }
    Ok(())
}

/// Scan source text for raw pool access near sensitive table names.
///
/// # Errors
///
/// Returns [`ScopedSqlLintError`] for invalid config.
pub fn scan_source(
    source: &str,
    config: &ScopedSqlLintConfig,
) -> Result<Vec<ScopedSqlFinding>, ScopedSqlLintError> {
    validate_config(config)?;
    let lines = source.lines().collect::<Vec<_>>();
    let mut findings = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(pattern) = config
            .raw_pool_patterns
            .iter()
            .find(|pattern| line.contains(pattern.as_str()))
        else {
            continue;
        };
        if is_allowed(&lines, index, &config.allow_comment_prefix) {
            continue;
        }
        let table = nearby_sensitive_table(&lines, index, config);
        if table.is_none() && !config.sensitive_tables.is_empty() {
            continue;
        }
        findings.push(ScopedSqlFinding {
            line: index + 1,
            pattern: pattern.clone(),
            table,
            message: "raw pool access near world/tenant-sensitive SQL needs scoped review"
                .to_owned(),
        });
    }
    Ok(findings)
}

fn is_allowed(lines: &[&str], index: usize, allow_prefix: &str) -> bool {
    lines[index].contains(allow_prefix)
        || index
            .checked_sub(1)
            .and_then(|previous| lines.get(previous))
            .is_some_and(|line| line.contains(allow_prefix))
}

fn nearby_sensitive_table(
    lines: &[&str],
    index: usize,
    config: &ScopedSqlLintConfig,
) -> Option<String> {
    let start = index.saturating_sub(config.context_radius);
    let end = (index + config.context_radius + 1).min(lines.len());
    let window = lines[start..end].join("\n").to_ascii_lowercase();
    config
        .sensitive_tables
        .iter()
        .find(|table| contains_identifier(&window, table))
        .cloned()
}

fn contains_identifier(window: &str, table: &str) -> bool {
    let table = table.to_ascii_lowercase();
    if table.is_empty() {
        return false;
    }
    window.match_indices(&table).any(|(index, _)| {
        let before = window[..index].chars().next_back();
        let after = window[index + table.len()..].chars().next();
        !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char)
    })
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_raw_pool_near_sensitive_table() {
        let config = ScopedSqlLintConfig::new(["world_cycle_jobs"]);
        let source = r#"
            sqlx::query("select * from world_cycle_jobs")
                .fetch_all(&self.pool)
                .await?;
        "#;
        let findings = scan_source(source, &config).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].table.as_deref(), Some("world_cycle_jobs"));
    }

    #[test]
    fn table_names_are_matched_on_identifier_boundaries() {
        let config = ScopedSqlLintConfig::new(["world", "notification_delivery"]);
        let source = r#"
            sqlx::query("select * from notification_delivery where world_id = $1")
                .fetch_all(&self.pool)
                .await?;
        "#;
        let findings = scan_source(source, &config).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].table.as_deref(), Some("notification_delivery"));
    }

    #[test]
    fn allow_comment_suppresses_next_line() {
        let config = ScopedSqlLintConfig::new(["world_cycle_jobs"]);
        let source = r#"
            sqlx::query("select * from world_cycle_jobs")
                // scoped-sqlx-lint: allow public audit query
                .fetch_all(&self.pool)
                .await?;
        "#;
        assert!(scan_source(source, &config).unwrap().is_empty());
    }
}
