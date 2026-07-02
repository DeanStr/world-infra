//! Environment parsing primitives.
//!
//! This crate parses values; it does not decide product deployment policy.

use std::{env, error::Error, fmt, str::FromStr, time::Duration};

/// Error returned when an environment variable is missing or malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvError {
    /// A required variable was not present or was blank after trimming.
    Missing {
        /// Environment variable name.
        name: String,
    },
    /// A variable was present but could not be parsed.
    Invalid {
        /// Environment variable name.
        name: String,
        /// Raw value.
        value: String,
        /// Parser error message.
        message: String,
    },
    /// A numeric value was below the configured minimum.
    BelowMinimum {
        /// Environment variable name.
        name: String,
        /// Raw value.
        value: String,
        /// Minimum allowed value.
        minimum: String,
    },
}

impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { name } => write!(f, "{name} is required"),
            Self::Invalid {
                name,
                value,
                message,
            } => write!(f, "{name} has invalid value {value:?}: {message}"),
            Self::BelowMinimum {
                name,
                value,
                minimum,
            } => write!(f, "{name} value {value:?} is below minimum {minimum}"),
        }
    }
}

impl Error for EnvError {}

/// Fallback behavior for malformed optional environment values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackPolicy {
    /// Return the parse/validation error.
    Strict,
    /// Print a short warning to stderr and use the supplied default.
    WarnToStderr,
    /// Use the supplied default without warning.
    SilentDefault,
}

/// Return a trimmed variable value if it exists and is not blank.
pub fn optional_var(name: impl AsRef<str>) -> Option<String> {
    env::var(name.as_ref())
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Return a required trimmed variable value.
///
/// # Errors
///
/// Returns [`EnvError::Missing`] when the variable is absent or blank.
pub fn required_var(name: impl AsRef<str>) -> Result<String, EnvError> {
    let name = name.as_ref();
    optional_var(name).ok_or_else(|| EnvError::Missing {
        name: name.to_owned(),
    })
}

/// Parse a required variable with [`FromStr`].
///
/// # Errors
///
/// Returns [`EnvError`] if the variable is missing or malformed.
pub fn parse_required<T>(name: impl AsRef<str>) -> Result<T, EnvError>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    let name = name.as_ref();
    let value = required_var(name)?;
    value.parse().map_err(|error: T::Err| EnvError::Invalid {
        name: name.to_owned(),
        value,
        message: error.to_string(),
    })
}

/// Parse an optional variable with [`FromStr`].
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] if a present variable is malformed.
pub fn parse_optional<T>(name: impl AsRef<str>) -> Result<Option<T>, EnvError>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    let name = name.as_ref();
    optional_var(name)
        .map(|value| {
            value.parse().map_err(|error: T::Err| EnvError::Invalid {
                name: name.to_owned(),
                value,
                message: error.to_string(),
            })
        })
        .transpose()
}

/// Parse an optional variable or return a default.
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] if a present variable is malformed.
pub fn parse_or<T>(name: impl AsRef<str>, default: T) -> Result<T, EnvError>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    Ok(parse_optional(name)?.unwrap_or(default))
}

/// Parse an optional ordered value and clamp it to a minimum.
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] if a present variable is malformed.
pub fn parse_or_min<T>(name: impl AsRef<str>, default: T, minimum: T) -> Result<T, EnvError>
where
    T: FromStr + Ord + Clone + fmt::Display,
    T::Err: fmt::Display,
{
    let name_ref = name.as_ref();
    let parsed = parse_optional::<T>(name_ref)?.unwrap_or(default);
    if parsed < minimum {
        return Ok(minimum);
    }
    Ok(parsed)
}

/// Parse an optional ordered value with fallback policy and a minimum bound.
///
/// Missing variables use `default`. Malformed or below-minimum present values
/// follow `policy`. The returned fallback is never below `minimum`.
///
/// # Errors
///
/// Returns [`EnvError`] when `policy` is [`FallbackPolicy::Strict`] and a
/// present value is malformed or below minimum.
pub fn parse_or_default_with_min_policy<T>(
    name: impl AsRef<str>,
    default: T,
    minimum: T,
    policy: FallbackPolicy,
) -> Result<T, EnvError>
where
    T: FromStr + Ord + Clone + fmt::Display,
    T::Err: fmt::Display,
{
    let name = name.as_ref();
    let fallback = if default < minimum {
        minimum.clone()
    } else {
        default
    };
    let Some(value) = optional_var(name) else {
        return Ok(fallback);
    };
    let parsed = match value.parse::<T>() {
        Ok(parsed) => parsed,
        Err(error) => {
            return fallback_or_error(
                name,
                &EnvError::Invalid {
                    name: name.to_owned(),
                    value,
                    message: error.to_string(),
                },
                fallback,
                policy,
            );
        }
    };
    if parsed < minimum {
        return fallback_or_error(
            name,
            &EnvError::BelowMinimum {
                name: name.to_owned(),
                value: parsed.to_string(),
                minimum: minimum.to_string(),
            },
            fallback,
            policy,
        );
    }
    Ok(parsed)
}

fn fallback_or_error<T>(
    name: &str,
    error: &EnvError,
    fallback: T,
    policy: FallbackPolicy,
) -> Result<T, EnvError>
where
    T: fmt::Display,
{
    match policy {
        FallbackPolicy::Strict => Err(error.clone()),
        FallbackPolicy::WarnToStderr => {
            eprintln!("{name} is invalid; using default {fallback}");
            Ok(fallback)
        }
        FallbackPolicy::SilentDefault => Ok(fallback),
    }
}

/// Parse a value and reject it if it is below a minimum.
///
/// # Errors
///
/// Returns [`EnvError`] if the variable is missing, malformed, or below minimum.
pub fn parse_required_min<T>(name: impl AsRef<str>, minimum: &T) -> Result<T, EnvError>
where
    T: FromStr + Ord + fmt::Display,
    T::Err: fmt::Display,
{
    let name = name.as_ref();
    let value = parse_required::<T>(name)?;
    if value < *minimum {
        return Err(EnvError::BelowMinimum {
            name: name.to_owned(),
            value: value.to_string(),
            minimum: minimum.to_string(),
        });
    }
    Ok(value)
}

/// Parse strict booleans: `true` or `false`, case-insensitive.
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] for any other value.
pub fn parse_strict_bool(name: impl AsRef<str>, value: impl AsRef<str>) -> Result<bool, EnvError> {
    let name = name.as_ref();
    let value = value.as_ref().trim();
    match value.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(EnvError::Invalid {
            name: name.to_owned(),
            value: value.to_owned(),
            message: "expected true or false".to_owned(),
        }),
    }
}

/// Parse a common boolean literal without allocating an error.
///
/// Accepted true values are `1`, `true`, `yes`, and `on`; accepted false
/// values are `0`, `false`, `no`, and `off`, case-insensitive.
#[must_use]
pub fn parse_bool_literal(value: impl AsRef<str>) -> Option<bool> {
    match value.as_ref().trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Parse lenient booleans: `1/0`, `true/false`, `yes/no`, `on/off`.
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] for any other value.
pub fn parse_lenient_bool(name: impl AsRef<str>, value: impl AsRef<str>) -> Result<bool, EnvError> {
    let name = name.as_ref();
    let raw = value.as_ref();
    parse_bool_literal(raw).ok_or_else(|| EnvError::Invalid {
        name: name.to_owned(),
        value: raw.trim().to_owned(),
        message: "expected one of 1/0, true/false, yes/no, on/off".to_owned(),
    })
}

/// Parse an optional boolean variable using lenient syntax.
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] if a present variable is malformed.
pub fn optional_lenient_bool(name: impl AsRef<str>) -> Result<Option<bool>, EnvError> {
    let name = name.as_ref();
    optional_var(name)
        .map(|value| parse_lenient_bool(name, value))
        .transpose()
}

/// Validate an environment-provided runtime namespace.
///
/// This helper only enforces product-neutral safety: nonblank, no surrounding
/// whitespace, no control/whitespace characters, and a bounded length. Products
/// still own env names, defaulting/fail-open policy, and user-facing error text.
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] if the namespace is unsafe.
pub fn parse_namespace(name: impl AsRef<str>, value: impl AsRef<str>) -> Result<String, EnvError> {
    let name = name.as_ref();
    let raw = value.as_ref();
    let trimmed = raw.trim();
    if raw != trimmed
        || trimmed.is_empty()
        || trimmed.len() > 256
        || trimmed.chars().any(char::is_whitespace)
        || trimmed.chars().any(char::is_control)
    {
        return Err(EnvError::Invalid {
            name: name.to_owned(),
            value: raw.to_owned(),
            message: "expected a nonblank namespace without whitespace or control characters"
                .to_owned(),
        });
    }
    Ok(trimmed.to_owned())
}

/// Parse an optional environment namespace.
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] if a present variable is unsafe.
pub fn optional_namespace(name: impl AsRef<str>) -> Result<Option<String>, EnvError> {
    let name = name.as_ref();
    let Some(value) = env::var(name).ok() else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Ok(None);
    }
    parse_namespace(name, value).map(Some)
}

/// Parse an optional environment namespace or use a validated default.
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] if a present variable or the supplied default
/// namespace is unsafe.
pub fn namespace_or(name: impl AsRef<str>, default: impl AsRef<str>) -> Result<String, EnvError> {
    let name = name.as_ref();
    match optional_namespace(name)? {
        Some(namespace) => Ok(namespace),
        None => parse_namespace(name, default),
    }
}

/// Parse a duration from seconds or a compact suffix such as `250ms`, `5s`,
/// `3m`, or `1h`.
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] if the value is malformed.
pub fn parse_duration(name: impl AsRef<str>, value: impl AsRef<str>) -> Result<Duration, EnvError> {
    let name = name.as_ref();
    let value = value.as_ref().trim();
    let (number, scale) = match value.strip_suffix("ms") {
        Some(number) => (number, 1),
        None => match value.chars().last() {
            Some('s') => (&value[..value.len() - 1], 1_000),
            Some('m') => (&value[..value.len() - 1], 60_000),
            Some('h') => (&value[..value.len() - 1], 3_600_000),
            _ => (value, 1_000),
        },
    };
    let units = number.parse::<u64>().map_err(|error| EnvError::Invalid {
        name: name.to_owned(),
        value: value.to_owned(),
        message: error.to_string(),
    })?;
    Ok(Duration::from_millis(units.saturating_mul(scale)))
}

/// Parse a comma-separated list, trimming whitespace and ignoring blank items.
pub fn parse_csv(value: impl AsRef<str>) -> Vec<String> {
    value
        .as_ref()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Parse a comma-separated header list and lowercase each header name.
pub fn parse_header_list(value: impl AsRef<str>) -> Vec<String> {
    parse_csv(value)
        .into_iter()
        .map(|header| header.to_ascii_lowercase())
        .collect()
}

/// Product-neutral environment classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentKind {
    /// Development or local-test environment.
    Development,
    /// Test environment.
    Test,
    /// Staging-like environment.
    Staging,
    /// Production-like environment.
    Production,
    /// Unknown environment; product policy decides whether this is allowed.
    Unknown,
}

/// Classify common environment labels without enforcing product policy.
pub fn classify_environment(name: impl AsRef<str>) -> EnvironmentKind {
    match name.as_ref().trim().to_ascii_lowercase().as_str() {
        "dev" | "development" | "local" => EnvironmentKind::Development,
        "test" | "ci" => EnvironmentKind::Test,
        "stage" | "staging" | "preprod" | "pre-production" => EnvironmentKind::Staging,
        "prod" | "production" => EnvironmentKind::Production,
        _ => EnvironmentKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bool_modes() {
        assert!(parse_strict_bool("FLAG", "TRUE").unwrap());
        assert!(parse_strict_bool("FLAG", "1").is_err());
        assert!(parse_lenient_bool("FLAG", "yes").unwrap());
        assert!(!parse_lenient_bool("FLAG", "off").unwrap());
        assert_eq!(parse_bool_literal(" ON "), Some(true));
        assert_eq!(parse_bool_literal("maybe"), None);
    }

    #[test]
    fn parses_duration_suffixes() {
        assert_eq!(
            parse_duration("T", "250ms").unwrap(),
            Duration::from_millis(250)
        );
        assert_eq!(parse_duration("T", "2").unwrap(), Duration::from_secs(2));
        assert_eq!(parse_duration("T", "3m").unwrap(), Duration::from_secs(180));
    }

    #[test]
    fn csv_ignores_blank_segments() {
        assert_eq!(parse_csv(" a, ,b, "), vec!["a", "b"]);
        assert_eq!(
            parse_header_list(" X-Real-IP,Forwarded "),
            vec!["x-real-ip", "forwarded"]
        );
    }

    #[test]
    fn fallback_policy_controls_invalid_optional_values() {
        let key = "WORLD_ENV_FALLBACK_POLICY_TEST";
        env::set_var(key, "bad");
        assert!(
            parse_or_default_with_min_policy::<u32>(key, 10, 1, FallbackPolicy::Strict).is_err()
        );
        assert_eq!(
            parse_or_default_with_min_policy::<u32>(key, 10, 1, FallbackPolicy::SilentDefault)
                .unwrap(),
            10
        );
        env::set_var(key, "0");
        assert_eq!(
            parse_or_default_with_min_policy::<u32>(key, 10, 1, FallbackPolicy::SilentDefault)
                .unwrap(),
            10
        );
        env::remove_var(key);
    }

    #[test]
    fn required_and_optional_parsers_trim_and_report_errors() {
        let key = "WORLD_ENV_REQUIRED_TEST";
        let previous = env::var(key).ok();
        env::remove_var(key);
        assert_eq!(
            required_var(key),
            Err(EnvError::Missing {
                name: key.to_owned()
            })
        );

        env::set_var(key, " 42 ");
        assert_eq!(required_var(key).unwrap(), "42");
        assert_eq!(parse_required::<u32>(key).unwrap(), 42);
        assert_eq!(parse_optional::<u32>(key).unwrap(), Some(42));
        assert_eq!(parse_or::<u32>("WORLD_ENV_ABSENT_TEST", 7).unwrap(), 7);

        env::set_var(key, " nope ");
        assert!(matches!(
            parse_required::<u32>(key),
            Err(EnvError::Invalid { name, value, .. }) if name == key && value == "nope"
        ));
        restore_env(key, previous);
    }

    #[test]
    fn min_policy_clamps_missing_defaults_and_warn_policy_falls_back() {
        let key = "WORLD_ENV_MIN_POLICY_TEST";
        let previous = env::var(key).ok();
        env::remove_var(key);
        assert_eq!(parse_or_min::<u32>(key, 0, 5).unwrap(), 5);
        assert_eq!(
            parse_or_default_with_min_policy::<u32>(key, 0, 5, FallbackPolicy::Strict).unwrap(),
            5
        );

        env::set_var(key, "1");
        assert_eq!(
            parse_or_default_with_min_policy::<u32>(key, 10, 5, FallbackPolicy::WarnToStderr)
                .unwrap(),
            10
        );
        assert_eq!(
            parse_required_min::<u32>(key, &5),
            Err(EnvError::BelowMinimum {
                name: key.to_owned(),
                value: "1".to_owned(),
                minimum: "5".to_owned(),
            })
        );
        restore_env(key, previous);
    }

    #[test]
    fn lenient_bool_env_and_duration_errors_are_precise() {
        let key = "WORLD_ENV_BOOL_TEST";
        let previous = env::var(key).ok();
        env::remove_var(key);
        assert_eq!(optional_lenient_bool(key).unwrap(), None);
        env::set_var(key, "on");
        assert_eq!(optional_lenient_bool(key).unwrap(), Some(true));
        env::set_var(key, "maybe");
        assert!(matches!(
            optional_lenient_bool(key),
            Err(EnvError::Invalid { name, value, .. }) if name == key && value == "maybe"
        ));
        assert!(parse_duration("DURATION", "abc").is_err());
        assert_eq!(
            parse_duration("DURATION", "1h").unwrap(),
            Duration::from_secs(3600)
        );
        restore_env(key, previous);
    }

    #[test]
    fn namespace_parsing_preserves_product_owned_names_and_errors() {
        assert_eq!(
            parse_namespace("APP_NAMESPACE", "airline:api/v1").unwrap(),
            "airline:api/v1"
        );
        assert!(matches!(
            parse_namespace("APP_NAMESPACE", " airline "),
            Err(EnvError::Invalid { name, value, .. }) if name == "APP_NAMESPACE" && value == " airline "
        ));
        assert!(matches!(
            parse_namespace("APP_NAMESPACE", "bad namespace"),
            Err(EnvError::Invalid { name, value, .. }) if name == "APP_NAMESPACE" && value == "bad namespace"
        ));

        let key = "WORLD_ENV_NAMESPACE_TEST";
        let previous = env::var(key).ok();
        env::remove_var(key);
        assert_eq!(optional_namespace(key).unwrap(), None);
        assert_eq!(namespace_or(key, "chairman").unwrap(), "chairman");
        env::set_var(key, "   ");
        assert_eq!(optional_namespace(key).unwrap(), None);
        assert_eq!(namespace_or(key, "chairman").unwrap(), "chairman");
        env::set_var(key, " chairman ");
        assert!(matches!(
            optional_namespace(key),
            Err(EnvError::Invalid { name, value, .. }) if name == key && value == " chairman "
        ));
        env::set_var(key, "chairman:api");
        assert_eq!(
            optional_namespace(key).unwrap(),
            Some("chairman:api".to_owned())
        );
        restore_env(key, previous);
    }

    #[test]
    fn display_and_environment_classification_are_stable() {
        assert_eq!(
            EnvError::Missing {
                name: "DATABASE_URL".to_owned()
            }
            .to_string(),
            "DATABASE_URL is required"
        );
        assert_eq!(
            EnvError::Invalid {
                name: "PORT".to_owned(),
                value: "abc".to_owned(),
                message: "invalid digit".to_owned(),
            }
            .to_string(),
            "PORT has invalid value \"abc\": invalid digit"
        );
        assert_eq!(classify_environment("local"), EnvironmentKind::Development);
        assert_eq!(classify_environment("ci"), EnvironmentKind::Test);
        assert_eq!(
            classify_environment("pre-production"),
            EnvironmentKind::Staging
        );
        assert_eq!(classify_environment("prod"), EnvironmentKind::Production);
        assert_eq!(classify_environment("weird"), EnvironmentKind::Unknown);
    }

    fn restore_env(name: &str, value: Option<String>) {
        if let Some(value) = value {
            env::set_var(name, value);
        } else {
            env::remove_var(name);
        }
    }
}
