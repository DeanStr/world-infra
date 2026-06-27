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

/// Parse lenient booleans: `1/0`, `true/false`, `yes/no`, `on/off`.
///
/// # Errors
///
/// Returns [`EnvError::Invalid`] for any other value.
pub fn parse_lenient_bool(name: impl AsRef<str>, value: impl AsRef<str>) -> Result<bool, EnvError> {
    let name = name.as_ref();
    let value = value.as_ref().trim();
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(EnvError::Invalid {
            name: name.to_owned(),
            value: value.to_owned(),
            message: "expected one of 1/0, true/false, yes/no, on/off".to_owned(),
        }),
    }
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
}
