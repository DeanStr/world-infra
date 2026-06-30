//! Configurable source scanner for world and tenant scoped SQL review.
//!
//! Product repositories own table names, transaction helpers, and legitimate
//! public/global exceptions. This crate provides a tiny scanner that products
//! can configure and run as advisory or blocking CI.

use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

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

/// A lint finding with file-aware context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedSqlFileFinding {
    /// Source file path.
    pub path: PathBuf,
    /// One-based line number.
    pub line: usize,
    /// Raw pattern that triggered the finding.
    pub pattern: String,
    /// Sensitive table found near the pattern, if any.
    pub table: Option<String>,
    /// Trimmed source line that triggered the finding.
    pub snippet: String,
    /// Human-readable finding summary.
    pub message: String,
}

impl ScopedSqlFileFinding {
    /// Return the path-free finding shape used by in-memory source scans.
    #[must_use]
    pub fn as_finding(&self) -> ScopedSqlFinding {
        ScopedSqlFinding {
            line: self.line,
            pattern: self.pattern.clone(),
            table: self.table.clone(),
            message: self.message.clone(),
        }
    }
}

/// Error returned by lint config validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopedSqlLintError {
    /// No raw pool patterns were configured.
    MissingRawPoolPatterns,
    /// Allow comment marker was blank.
    MissingAllowCommentPrefix,
}

/// Error returned by file-based lint scans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopedSqlFileLintError {
    /// Scanner configuration was invalid.
    Config(ScopedSqlLintError),
    /// Reading a source file failed.
    Io {
        /// Source file path.
        path: PathBuf,
        /// I/O error string.
        message: String,
    },
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

impl fmt::Display for ScopedSqlFileLintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => error.fmt(f),
            Self::Io { path, message } => {
                write!(f, "failed to read {}: {message}", path.display())
            }
        }
    }
}

impl Error for ScopedSqlFileLintError {}

impl From<ScopedSqlLintError> for ScopedSqlFileLintError {
    fn from(error: ScopedSqlLintError) -> Self {
        Self::Config(error)
    }
}

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
    Ok(scan_source_inner(source, config)?
        .into_iter()
        .map(|finding| finding.finding)
        .collect())
}

/// Scan one source file for raw pool access near sensitive table names.
///
/// # Errors
///
/// Returns [`ScopedSqlLintError`] for invalid config or failed file reads.
pub fn scan_file(
    path: impl AsRef<Path>,
    config: &ScopedSqlLintConfig,
) -> Result<Vec<ScopedSqlFileFinding>, ScopedSqlFileLintError> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|error| ScopedSqlFileLintError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    Ok(scan_source_inner(&source, config)
        .map_err(ScopedSqlFileLintError::from)?
        .into_iter()
        .map(|finding| finding.into_file_finding(path))
        .collect())
}

/// Scan multiple source files and concatenate findings.
///
/// # Errors
///
/// Returns [`ScopedSqlLintError`] for invalid config or failed file reads.
pub fn scan_files<P>(
    paths: impl IntoIterator<Item = P>,
    config: &ScopedSqlLintConfig,
) -> Result<Vec<ScopedSqlFileFinding>, ScopedSqlFileLintError>
where
    P: AsRef<Path>,
{
    validate_config(config).map_err(ScopedSqlFileLintError::from)?;
    let mut findings = Vec::new();
    for path in paths {
        findings.extend(scan_file(path, config)?);
    }
    Ok(findings)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopedSqlFindingWithSnippet {
    finding: ScopedSqlFinding,
    snippet: String,
}

impl ScopedSqlFindingWithSnippet {
    fn into_file_finding(self, path: &Path) -> ScopedSqlFileFinding {
        ScopedSqlFileFinding {
            path: path.to_path_buf(),
            line: self.finding.line,
            pattern: self.finding.pattern,
            table: self.finding.table,
            snippet: self.snippet,
            message: self.finding.message,
        }
    }
}

fn scan_source_inner(
    source: &str,
    config: &ScopedSqlLintConfig,
) -> Result<Vec<ScopedSqlFindingWithSnippet>, ScopedSqlLintError> {
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
        findings.push(ScopedSqlFindingWithSnippet {
            finding: ScopedSqlFinding {
                line: index + 1,
                pattern: pattern.clone(),
                table,
                message: "raw pool access near world/tenant-sensitive SQL needs scoped review"
                    .to_owned(),
            },
            snippet: line.trim().to_owned(),
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
        assert_eq!(findings[0].line, 3);
        assert_eq!(findings[0].pattern, "&self.pool");
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
    fn scan_files_records_path_line_and_snippet() {
        let config = ScopedSqlLintConfig::new(["notification_delivery"]);
        let path = std::env::temp_dir().join(format!(
            "scoped-sql-lint-{}-{}.rs",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            r#"
fn query() {
    sqlx::query("select * from notification_delivery")
        .fetch_all(&self.pool);
}
"#,
        )
        .unwrap();

        let findings = scan_files([&path], &config).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].path, path);
        assert_eq!(findings[0].line, 4);
        assert_eq!(findings[0].pattern, "&self.pool");
        assert_eq!(findings[0].table.as_deref(), Some("notification_delivery"));
        assert_eq!(findings[0].snippet, ".fetch_all(&self.pool);");
        assert_eq!(
            findings[0].as_finding(),
            ScopedSqlFinding {
                line: 4,
                pattern: "&self.pool".to_owned(),
                table: Some("notification_delivery".to_owned()),
                message: "raw pool access near world/tenant-sensitive SQL needs scoped review"
                    .to_owned(),
            }
        );
    }

    #[test]
    fn scan_files_validates_config_even_when_no_paths_are_scanned() {
        let mut config = ScopedSqlLintConfig::new(["notification_delivery"]);
        config.raw_pool_patterns.clear();

        assert_eq!(
            scan_files(Vec::<&str>::new(), &config),
            Err(ScopedSqlFileLintError::Config(
                ScopedSqlLintError::MissingRawPoolPatterns
            ))
        );
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
