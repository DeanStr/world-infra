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

/// Look up an OpenAPI operation by exact path key and method name.
///
/// This intentionally does not use dot-path lookup because OpenAPI path keys
/// contain slashes and braces, such as `/worlds/{worldId}`.
///
/// # Errors
///
/// Returns [`ContractTestError::MissingPath`] when the path or method is absent.
/// Returns [`ContractTestError::ExpectedObject`] when `paths`, the path item, or
/// the method operation is not an object.
pub fn operation_at(
    document: &Value,
    path: impl AsRef<str>,
    method: impl AsRef<str>,
) -> Result<&Value, ContractTestError> {
    let path = path.as_ref();
    let method = method.as_ref().to_ascii_lowercase();
    let paths = document
        .get("paths")
        .ok_or_else(|| ContractTestError::MissingPath {
            path: "paths".to_owned(),
        })?
        .as_object()
        .ok_or_else(|| ContractTestError::ExpectedObject {
            path: "paths".to_owned(),
        })?;
    let path_item = paths
        .get(path)
        .ok_or_else(|| ContractTestError::MissingPath {
            path: format!("paths.{path}"),
        })?
        .as_object()
        .ok_or_else(|| ContractTestError::ExpectedObject {
            path: format!("paths.{path}"),
        })?;
    let operation = path_item
        .get(&method)
        .ok_or_else(|| ContractTestError::MissingPath {
            path: format!("paths.{path}.{method}"),
        })?;
    if operation.is_object() {
        Ok(operation)
    } else {
        Err(ContractTestError::ExpectedObject {
            path: format!("paths.{path}.{method}"),
        })
    }
}

/// Return sorted response codes from an OpenAPI operation.
///
/// # Errors
///
/// Returns [`ContractTestError::MissingPath`] when `responses` is absent.
/// Returns [`ContractTestError::ExpectedObject`] when the operation or
/// `responses` is not an object.
pub fn response_codes(operation: &Value) -> Result<Vec<&str>, ContractTestError> {
    let operation = operation
        .as_object()
        .ok_or_else(|| ContractTestError::ExpectedObject {
            path: "operation".to_owned(),
        })?;
    let responses = operation
        .get("responses")
        .ok_or_else(|| ContractTestError::MissingPath {
            path: "responses".to_owned(),
        })?
        .as_object()
        .ok_or_else(|| ContractTestError::ExpectedObject {
            path: "responses".to_owned(),
        })?;
    let mut codes = responses.keys().map(String::as_str).collect::<Vec<_>>();
    codes.sort_unstable();
    Ok(codes)
}

/// Assert an OpenAPI operation declares a response code.
///
/// # Errors
///
/// Returns [`ContractTestError::MissingPath`] when the code is absent.
pub fn assert_response_code(
    operation: &Value,
    status: impl AsRef<str>,
) -> Result<(), ContractTestError> {
    let status = status.as_ref();
    if response_codes(operation)?.contains(&status) {
        return Ok(());
    }
    Err(ContractTestError::MissingPath {
        path: format!("responses.{status}"),
    })
}

/// Assert an OpenAPI operation requires a named security scheme.
///
/// OpenAPI operation `security` is an array of alternative requirement
/// objects. This helper passes only when every alternative requirement object
/// contains `scheme`; if any alternative omits it, the scheme is optional rather
/// than required. It does not interpret product-specific auth policy, scopes,
/// tokens, or inherited path/global security.
///
/// # Errors
///
/// Returns [`ContractTestError::MissingPath`] when `security` or the scheme is
/// absent. Returns [`ContractTestError::ExpectedObject`] when the operation or a
/// security requirement item is not an object. Returns
/// [`ContractTestError::JsonMismatch`] when `security` is present but not an
/// array.
pub fn assert_security_scheme_required(
    operation: &Value,
    scheme: impl AsRef<str>,
) -> Result<(), ContractTestError> {
    let scheme = scheme.as_ref();
    let requirements = security_requirements(operation, scheme)?;
    if requirements.is_empty() {
        return Err(ContractTestError::MissingPath {
            path: format!("security.{scheme}"),
        });
    }

    for requirement in requirements {
        if !requirement.contains_key(scheme) {
            return Err(ContractTestError::MissingPath {
                path: format!("security.{scheme}"),
            });
        }
    }

    Ok(())
}

/// Assert an OpenAPI operation declares a named security scheme in at least one
/// alternative requirement.
///
/// OpenAPI operation `security` is an array of alternative requirement objects.
/// This helper is useful when a product wants to prove a scheme is supported or
/// advertised, but not necessarily required for every alternative. Use
/// [`assert_security_scheme_required`] when the scheme must be present in every
/// alternative.
///
/// # Errors
///
/// Returns [`ContractTestError::MissingPath`] when `security` or the scheme is
/// absent. Returns [`ContractTestError::ExpectedObject`] when the operation or a
/// security requirement item is not an object. Returns
/// [`ContractTestError::JsonMismatch`] when `security` is present but not an
/// array.
pub fn assert_security_scheme_declared(
    operation: &Value,
    scheme: impl AsRef<str>,
) -> Result<(), ContractTestError> {
    let scheme = scheme.as_ref();
    let requirements = security_requirements(operation, scheme)?;

    for requirement in requirements {
        if requirement.contains_key(scheme) {
            return Ok(());
        }
    }

    Err(ContractTestError::MissingPath {
        path: format!("security.{scheme}"),
    })
}

fn security_requirements<'a>(
    operation: &'a Value,
    scheme: &str,
) -> Result<Vec<&'a Map<String, Value>>, ContractTestError> {
    let operation = operation
        .as_object()
        .ok_or_else(|| ContractTestError::ExpectedObject {
            path: "operation".to_owned(),
        })?;
    let security = operation
        .get("security")
        .ok_or_else(|| ContractTestError::MissingPath {
            path: "security".to_owned(),
        })?;
    let requirements = security
        .as_array()
        .ok_or_else(|| ContractTestError::JsonMismatch {
            expected: format!("security array containing scheme {scheme:?}"),
            actual: security.to_string(),
        })?;

    let mut parsed = Vec::with_capacity(requirements.len());
    for requirement in requirements {
        let requirement =
            requirement
                .as_object()
                .ok_or_else(|| ContractTestError::ExpectedObject {
                    path: "security[]".to_owned(),
                })?;
        parsed.push(requirement);
    }

    Ok(parsed)
}

/// Assert an OpenAPI operation description contains expected text.
///
/// # Errors
///
/// Returns [`ContractTestError::JsonMismatch`] when the description is missing
/// or does not contain `text`.
pub fn assert_description_contains(
    operation: &Value,
    text: impl AsRef<str>,
) -> Result<(), ContractTestError> {
    let text = text.as_ref();
    let actual = operation
        .as_object()
        .ok_or_else(|| ContractTestError::ExpectedObject {
            path: "operation".to_owned(),
        })?
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if actual.contains(text) {
        return Ok(());
    }
    Err(ContractTestError::JsonMismatch {
        expected: format!("description containing {text:?}"),
        actual: actual.to_owned(),
    })
}

/// Assert an OpenAPI response description contains expected text.
///
/// # Errors
///
/// Returns [`ContractTestError::MissingPath`] when the response code is absent.
/// Returns [`ContractTestError::JsonMismatch`] when the response description is
/// missing or does not contain `text`.
pub fn assert_response_description_contains(
    operation: &Value,
    status: impl AsRef<str>,
    text: impl AsRef<str>,
) -> Result<(), ContractTestError> {
    let status = status.as_ref();
    let text = text.as_ref();
    let response = operation
        .as_object()
        .ok_or_else(|| ContractTestError::ExpectedObject {
            path: "operation".to_owned(),
        })?
        .get("responses")
        .ok_or_else(|| ContractTestError::MissingPath {
            path: "responses".to_owned(),
        })?
        .as_object()
        .ok_or_else(|| ContractTestError::ExpectedObject {
            path: "responses".to_owned(),
        })?
        .get(status)
        .ok_or_else(|| ContractTestError::MissingPath {
            path: format!("responses.{status}"),
        })?
        .as_object()
        .ok_or_else(|| ContractTestError::ExpectedObject {
            path: format!("responses.{status}"),
        })?;
    let actual = response
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if actual.contains(text) {
        return Ok(());
    }
    Err(ContractTestError::JsonMismatch {
        expected: format!("responses.{status}.description containing {text:?}"),
        actual: actual.to_owned(),
    })
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

    #[test]
    fn json_contract_mismatches_and_parse_errors_are_descriptive() {
        assert!(matches!(
            parse_json("{bad"),
            Err(ContractTestError::Json(_))
        ));
        let expected = parse_json(r#"{"a":1}"#).unwrap();
        let actual = parse_json(r#"{"a":2}"#).unwrap();
        let error = assert_json_contract(&expected, &actual).unwrap_err();
        assert_eq!(
            error,
            ContractTestError::JsonMismatch {
                expected: r#"{"a":1}"#.to_owned(),
                actual: r#"{"a":2}"#.to_owned(),
            }
        );
        assert_eq!(
            error.to_string(),
            r#"canonical JSON mismatch; expected {"a":1}, got {"a":2}"#
        );
    }

    #[test]
    fn path_helpers_cover_absent_non_object_and_mutation_cases() {
        let value = parse_json(r#"{"messageType":"cycleCompleted","world":{"id":1}}"#).unwrap();
        assert_eq!(get_path(&value, ""), Some(&value));
        assert_eq!(
            assert_required_path(&value, "world.instanceId"),
            Err(ContractTestError::MissingPath {
                path: "world.instanceId".to_owned()
            })
        );
        assert_eq!(
            assert_absent_path(&value, "world.id"),
            Err(ContractTestError::UnexpectedPath {
                path: "world.id".to_owned()
            })
        );
        assert_eq!(
            assert_object_keys_allowed(&value, "messageType", &[]),
            Err(ContractTestError::ExpectedObject {
                path: "messageType".to_owned()
            })
        );
        assert_eq!(assert_object_keys_allowed(&value, "world", &["id"]), Ok(()));
        assert_eq!(
            without_top_level_field(&value, "messageType")["messageType"],
            Value::Null
        );
        assert_eq!(
            with_unknown_top_level_field(&value, "extra")["extra"],
            Value::String("unexpected".to_owned())
        );
    }

    #[test]
    fn openapi_operation_helpers_use_exact_path_keys() {
        let document = parse_json(
            r#"{
                "paths": {
                    "/worlds/{worldId}/clock": {
                        "get": {
                            "description": "Bearer auth is required.",
                            "security": [{ "bearerAuth": [] }],
                            "responses": {
                                "200": { "description": "Clock payload." },
                                "401": { "description": "Unauthorized." },
                                "403": { "description": "Forbidden." }
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let operation = operation_at(&document, "/worlds/{worldId}/clock", "GET").unwrap();

        assert_eq!(
            response_codes(operation).unwrap(),
            vec!["200", "401", "403"]
        );
        assert_response_code(operation, "401").unwrap();
        assert_security_scheme_required(operation, "bearerAuth").unwrap();
        assert_description_contains(operation, "Bearer auth").unwrap();
        assert_response_description_contains(operation, "403", "Forbidden").unwrap();
        assert_eq!(
            assert_response_code(operation, "404"),
            Err(ContractTestError::MissingPath {
                path: "responses.404".to_owned()
            })
        );
        assert!(assert_description_contains(operation, "session").is_err());
        assert!(operation_at(&document, "/missing", "get").is_err());
        assert_eq!(
            response_codes(&Value::String("not-an-operation".to_owned())),
            Err(ContractTestError::ExpectedObject {
                path: "operation".to_owned()
            })
        );
        assert_eq!(
            assert_response_code(&parse_json(r#"{"responses":[]}"#).unwrap(), "200"),
            Err(ContractTestError::ExpectedObject {
                path: "responses".to_owned()
            })
        );
    }

    #[test]
    fn openapi_security_scheme_helper_requires_scheme_in_every_alternative() {
        let operation = parse_json(
            r#"{
                "security": [
                    { "bearerAuth": [], "apiKeyAuth": [] },
                    { "bearerAuth": [] }
                ],
                "responses": {
                    "200": { "description": "OK" }
                }
            }"#,
        )
        .unwrap();
        assert_security_scheme_required(&operation, "bearerAuth").unwrap();
        assert_security_scheme_declared(&operation, "apiKeyAuth").unwrap();
        assert_eq!(
            assert_security_scheme_required(&operation, "apiKeyAuth"),
            Err(ContractTestError::MissingPath {
                path: "security.apiKeyAuth".to_owned()
            })
        );
        assert_eq!(
            assert_security_scheme_required(&operation, "cookieAuth"),
            Err(ContractTestError::MissingPath {
                path: "security.cookieAuth".to_owned()
            })
        );
        assert_eq!(
            assert_security_scheme_required(
                &parse_json(r#"{"responses":{}}"#).unwrap(),
                "bearerAuth"
            ),
            Err(ContractTestError::MissingPath {
                path: "security".to_owned()
            })
        );
        assert_eq!(
            assert_security_scheme_required(
                &parse_json(r#"{"security":{"bearerAuth":[]},"responses":{}}"#).unwrap(),
                "bearerAuth",
            ),
            Err(ContractTestError::JsonMismatch {
                expected: "security array containing scheme \"bearerAuth\"".to_owned(),
                actual: r#"{"bearerAuth":[]}"#.to_owned(),
            })
        );
        assert_eq!(
            assert_security_scheme_required(
                &parse_json(r#"{"security":["bearerAuth"],"responses":{}}"#).unwrap(),
                "bearerAuth",
            ),
            Err(ContractTestError::ExpectedObject {
                path: "security[]".to_owned()
            })
        );
    }

    #[test]
    fn openapi_security_scheme_helper_rejects_optional_alternatives() {
        let anonymous_or_bearer = parse_json(
            r#"{
                "security": [
                    {},
                    { "bearerAuth": [] }
                ],
                "responses": {
                    "200": { "description": "OK" }
                }
            }"#,
        )
        .unwrap();
        assert_security_scheme_declared(&anonymous_or_bearer, "bearerAuth").unwrap();
        assert_eq!(
            assert_security_scheme_required(&anonymous_or_bearer, "bearerAuth"),
            Err(ContractTestError::MissingPath {
                path: "security.bearerAuth".to_owned()
            })
        );

        let api_key_or_bearer = parse_json(
            r#"{
                "security": [
                    { "apiKeyAuth": [] },
                    { "bearerAuth": [] }
                ],
                "responses": {
                    "200": { "description": "OK" }
                }
            }"#,
        )
        .unwrap();
        assert_security_scheme_declared(&api_key_or_bearer, "bearerAuth").unwrap();
        assert_eq!(
            assert_security_scheme_required(&api_key_or_bearer, "bearerAuth"),
            Err(ContractTestError::MissingPath {
                path: "security.bearerAuth".to_owned()
            })
        );
    }
}
