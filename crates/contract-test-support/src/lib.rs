//! Product-neutral helpers for API, event, and schema contract tests.
//!
//! This crate deliberately does not know product route names, payload meanings,
//! generated client types, or whether a product contract is open or closed. It
//! provides small assertion and JSON-normalization helpers for product tests.

use std::{collections::BTreeSet, error::Error, fmt};

use serde_json::{Map, Value};

/// Error returned by contract-test helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractTestError {
    /// JSON parsing failed.
    Json(String),
    /// JSON values differed after canonical normalization.
    JsonMismatch {
        /// Expected canonical JSON.
        expected: String,
        /// Actual canonical JSON.
        actual: String,
    },
    /// A required path was absent.
    MissingPath {
        /// Dot-separated path.
        path: String,
    },
    /// A path was present when it should not be.
    UnexpectedPath {
        /// Dot-separated path.
        path: String,
    },
    /// An object had an unexpected key.
    UnexpectedKey {
        /// Dot-separated object path.
        path: String,
        /// Unexpected key.
        key: String,
    },
    /// A helper expected a JSON object.
    ExpectedObject {
        /// Dot-separated path.
        path: String,
    },
}

impl fmt::Display for ContractTestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "JSON parse or serialize failed: {error}"),
            Self::JsonMismatch { expected, actual } => {
                write!(
                    f,
                    "canonical JSON mismatch; expected {expected}, got {actual}"
                )
            }
            Self::MissingPath { path } => write!(f, "required contract path {path} is missing"),
            Self::UnexpectedPath { path } => write!(f, "contract path {path} was unexpected"),
            Self::UnexpectedKey { path, key } => {
                write!(f, "contract object {path} contained unexpected key {key}")
            }
            Self::ExpectedObject { path } => write!(f, "contract path {path} is not an object"),
        }
    }
}

impl Error for ContractTestError {}

/// Parse JSON into a [`serde_json::Value`].
///
/// # Errors
///
/// Returns [`ContractTestError::Json`] when parsing fails.
pub fn parse_json(input: impl AsRef<str>) -> Result<Value, ContractTestError> {
    serde_json::from_str(input.as_ref()).map_err(|error| ContractTestError::Json(error.to_string()))
}

/// Return a canonical JSON value with object keys recursively sorted.
#[must_use]
pub fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let mut sorted = Map::new();
            for key in keys {
                sorted.insert(key.clone(), canonical_value(&map[key]));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        _ => value.clone(),
    }
}

/// Serialize JSON in canonical, whitespace-free form.
///
/// # Errors
///
/// Returns [`ContractTestError::Json`] when serialization fails.
pub fn canonical_json(value: &Value) -> Result<String, ContractTestError> {
    serde_json::to_string(&canonical_value(value))
        .map_err(|error| ContractTestError::Json(error.to_string()))
}

/// Parse and canonicalize a JSON string.
///
/// # Errors
///
/// Returns [`ContractTestError`] when parsing or serialization fails.
pub fn canonical_json_str(input: impl AsRef<str>) -> Result<String, ContractTestError> {
    canonical_json(&parse_json(input)?)
}

/// Assert two JSON values match after canonical normalization.
///
/// # Errors
///
/// Returns [`ContractTestError::JsonMismatch`] for a mismatch.
pub fn assert_json_contract(expected: &Value, actual: &Value) -> Result<(), ContractTestError> {
    let expected = canonical_json(expected)?;
    let actual = canonical_json(actual)?;
    if expected == actual {
        return Ok(());
    }
    Err(ContractTestError::JsonMismatch { expected, actual })
}

/// Lookup a dot-separated object path.
#[must_use]
pub fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }
    let mut current = value;
    for part in path.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

/// Assert a dot-separated path exists.
///
/// # Errors
///
/// Returns [`ContractTestError::MissingPath`] when the path is absent.
pub fn assert_required_path(value: &Value, path: impl AsRef<str>) -> Result<(), ContractTestError> {
    let path = path.as_ref();
    get_path(value, path)
        .map(|_| ())
        .ok_or_else(|| ContractTestError::MissingPath {
            path: path.to_owned(),
        })
}

/// Assert a dot-separated path is absent.
///
/// # Errors
///
/// Returns [`ContractTestError::UnexpectedPath`] when the path exists.
pub fn assert_absent_path(value: &Value, path: impl AsRef<str>) -> Result<(), ContractTestError> {
    let path = path.as_ref();
    if get_path(value, path).is_none() {
        return Ok(());
    }
    Err(ContractTestError::UnexpectedPath {
        path: path.to_owned(),
    })
}

/// Assert that an object has no keys beyond the allowlist.
///
/// # Errors
///
/// Returns [`ContractTestError`] when the path is not an object or contains an
/// unexpected key.
pub fn assert_object_keys_allowed(
    value: &Value,
    path: impl AsRef<str>,
    allowed: &[&str],
) -> Result<(), ContractTestError> {
    let path = path.as_ref();
    let object = get_path(value, path)
        .ok_or_else(|| ContractTestError::MissingPath {
            path: path.to_owned(),
        })?
        .as_object()
        .ok_or_else(|| ContractTestError::ExpectedObject {
            path: path.to_owned(),
        })?;
    let allowed = allowed.iter().copied().collect::<BTreeSet<_>>();
    for key in object.keys() {
        if !allowed.contains(key.as_str()) {
            return Err(ContractTestError::UnexpectedKey {
                path: path.to_owned(),
                key: key.to_owned(),
            });
        }
    }
    Ok(())
}

/// Return a copy of a JSON object with a top-level field removed.
///
/// This is useful in product tests that prove required-field removal is
/// rejected by a product-owned validator.
#[must_use]
pub fn without_top_level_field(value: &Value, field: &str) -> Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove(field);
    }
    value
}

/// Return a copy of a JSON object with a top-level unknown field inserted.
///
/// This is useful in product tests that prove closed schemas reject unknown
/// properties.
#[must_use]
pub fn with_unknown_top_level_field(value: &Value, field: &str) -> Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.insert(field.to_owned(), Value::String("unexpected".to_owned()));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_sorts_nested_object_keys() {
        let value = parse_json(r#"{"b":2,"a":{"z":1,"m":2}}"#).unwrap();
        assert_eq!(
            canonical_json(&value).unwrap(),
            r#"{"a":{"m":2,"z":1},"b":2}"#
        );
    }

    #[test]
    fn path_and_closed_object_assertions_work() {
        let value = parse_json(r#"{"messageType":"cycleCompleted","world":{"id":1}}"#).unwrap();
        assert_required_path(&value, "world.id").unwrap();
        assert_absent_path(&value, "world.instanceId").unwrap();
        assert_object_keys_allowed(&value, "", &["messageType", "world"]).unwrap();
        assert!(assert_object_keys_allowed(&value, "", &["messageType"]).is_err());
    }
}
