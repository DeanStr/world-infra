//! Provider-neutral billing-risk evidence and redaction primitives.
//!
//! This crate does not implement Stripe clients, product plan policy, price IDs,
//! legal text, refund decisions, dispute response content, admin routes, or
//! provider-specific webhook parsing.

use std::{error::Error, fmt};

use serde_json::Value;

/// Error returned by billing-risk helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BillingRiskError {
    /// A string value was blank.
    Empty {
        /// Field name.
        field: &'static str,
    },
    /// A string value was malformed.
    Invalid {
        /// Field name.
        field: &'static str,
    },
}

impl fmt::Display for BillingRiskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} is empty"),
            Self::Invalid { field } => write!(f, "{field} is invalid"),
        }
    }
}

impl Error for BillingRiskError {}

/// Provider object identifier such as a customer, session, invoice, or dispute.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderObjectId {
    kind: String,
    id: String,
}

impl ProviderObjectId {
    /// Construct a provider object id.
    ///
    /// # Errors
    ///
    /// Returns [`BillingRiskError`] for blank or unsafe values.
    pub fn new(kind: impl AsRef<str>, id: impl AsRef<str>) -> Result<Self, BillingRiskError> {
        let kind = validate_label("kind", kind.as_ref())?;
        let id = validate_provider_id("id", id.as_ref())?;
        Ok(Self { kind, id })
    }

    /// Provider-neutral kind label.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Provider object id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Version label for legal/commercial policy text accepted at checkout.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PolicyVersion(String);

impl PolicyVersion {
    /// Construct a policy version label.
    ///
    /// # Errors
    ///
    /// Returns [`BillingRiskError`] for blank or unsafe labels.
    pub fn new(value: impl AsRef<str>) -> Result<Self, BillingRiskError> {
        Ok(Self(validate_label("policy_version", value.as_ref())?))
    }

    /// Access the version label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_label(field: &'static str, value: &str) -> Result<String, BillingRiskError> {
    if value.chars().any(char::is_control) {
        return Err(BillingRiskError::Invalid { field });
    }
    let value = value.trim();
    if value.is_empty() {
        return Err(BillingRiskError::Empty { field });
    }
    if value.len() > 128
        || value
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':')))
    {
        return Err(BillingRiskError::Invalid { field });
    }
    Ok(value.to_owned())
}

fn validate_provider_id(field: &'static str, value: &str) -> Result<String, BillingRiskError> {
    if value.chars().any(char::is_control) {
        return Err(BillingRiskError::Invalid { field });
    }
    let value = value.trim();
    if value.is_empty() {
        return Err(BillingRiskError::Empty { field });
    }
    if value.len() > 256
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(BillingRiskError::Invalid { field });
    }
    Ok(value.to_owned())
}

/// Provider-neutral billing risk event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BillingRiskEventKind {
    /// Payment failed or entered dunning.
    PaymentFailed,
    /// Refund was created or updated.
    Refund,
    /// Dispute/chargeback was created or updated.
    Dispute,
    /// Chargeback/dispute was created or updated.
    Chargeback,
    /// Provider flagged likely fraud.
    EarlyFraudWarning,
    /// Provider review opened or closed.
    Review,
    /// Product-specific risk event.
    ProductSpecific,
}

/// Product response to a webhook ingest attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WebhookIngestDisposition {
    /// Acknowledge the provider event.
    #[deprecated(note = "use AckApplied, AckDuplicate, or AckIgnored")]
    Ack,
    /// Ask provider to retry because local state was not durably applied.
    Retry,
    /// Ignore the event as duplicate or irrelevant.
    #[deprecated(note = "use AckDuplicate or AckIgnored")]
    Ignore,
    /// Acknowledge because the event was durably applied.
    AckApplied,
    /// Acknowledge because the event was already ingested/applied.
    AckDuplicate,
    /// Acknowledge because the event is intentionally irrelevant locally.
    AckIgnored,
}

impl WebhookIngestDisposition {
    /// Return whether this disposition should acknowledge the provider event.
    ///
    /// Duplicate and intentionally ignored events are still acknowledged so
    /// providers do not retry events that were safely classified locally.
    #[must_use]
    #[allow(deprecated)]
    pub const fn should_ack_provider(self) -> bool {
        matches!(
            self,
            Self::Ack | Self::AckApplied | Self::AckDuplicate | Self::AckIgnored | Self::Ignore
        )
    }

    /// Return whether the provider should retry the webhook later.
    #[must_use]
    pub const fn should_retry_provider(self) -> bool {
        matches!(self, Self::Retry)
    }
}

/// Classify common webhook persistence outcomes.
#[must_use]
pub const fn classify_webhook_persistence(
    duplicate: bool,
    persisted: bool,
    applied: bool,
) -> WebhookIngestDisposition {
    if duplicate {
        WebhookIngestDisposition::AckDuplicate
    } else if persisted && applied {
        WebhookIngestDisposition::AckApplied
    } else {
        WebhookIngestDisposition::Retry
    }
}

/// Support-message external reference used for idempotent ingest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SupportMessageRef(String);

impl SupportMessageRef {
    /// Construct a support-message external reference.
    ///
    /// # Errors
    ///
    /// Returns [`BillingRiskError`] for blank or unsafe references.
    pub fn new(value: impl AsRef<str>) -> Result<Self, BillingRiskError> {
        Ok(Self(validate_provider_id(
            "support_message_ref",
            value.as_ref(),
        )?))
    }

    /// Access the reference.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Default sensitive provider-snapshot keys to redact.
pub const DEFAULT_SENSITIVE_KEYS: &[&str] = &[
    "authorization",
    "client_secret",
    "cvc",
    "number",
    "password",
    "secret",
    "token",
];

/// Redact sensitive JSON object keys recursively.
#[must_use]
pub fn redact_json_snapshot(value: &Value, sensitive_keys: &[&str]) -> Value {
    match value {
        Value::Object(map) => {
            let mut redacted = serde_json::Map::new();
            for (key, value) in map {
                if sensitive_keys
                    .iter()
                    .any(|sensitive| key.eq_ignore_ascii_case(sensitive))
                {
                    redacted.insert(key.clone(), Value::String("[redacted]".to_owned()));
                } else {
                    redacted.insert(key.clone(), redact_json_snapshot(value, sensitive_keys));
                }
            }
            Value::Object(redacted)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_json_snapshot(item, sensitive_keys))
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// Checkout-time evidence skeleton shared by products.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutEvidence<ProviderId> {
    /// Product-owned account id.
    pub account_id: String,
    /// Selected product plan/tier label.
    pub plan: String,
    /// Provider customer/session/subscription/invoice ids, as available.
    pub provider_ids: Vec<ProviderId>,
    /// Terms version accepted at checkout.
    pub terms_version: PolicyVersion,
    /// Refund policy version shown at checkout.
    pub refund_policy_version: PolicyVersion,
    /// Cancellation policy version shown at checkout.
    pub cancellation_policy_version: PolicyVersion,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_classification_retries_when_apply_fails() {
        assert_eq!(
            classify_webhook_persistence(false, true, false),
            WebhookIngestDisposition::Retry
        );
        assert_eq!(
            classify_webhook_persistence(false, true, true),
            WebhookIngestDisposition::AckApplied
        );
        assert_eq!(
            classify_webhook_persistence(true, false, false),
            WebhookIngestDisposition::AckDuplicate
        );
        #[allow(deprecated)]
        {
            assert!(WebhookIngestDisposition::Ack.should_ack_provider());
            assert!(WebhookIngestDisposition::Ignore.should_ack_provider());
        }
        assert!(WebhookIngestDisposition::AckIgnored.should_ack_provider());
        assert!(!WebhookIngestDisposition::AckIgnored.should_retry_provider());
        assert!(WebhookIngestDisposition::Retry.should_retry_provider());
        assert!(!WebhookIngestDisposition::Retry.should_ack_provider());
    }

    #[test]
    fn redacts_nested_sensitive_json_keys() {
        let value = serde_json::json!({
            "id": "evt_1",
            "data": { "client_secret": "secret", "nested": [{ "token": "abc" }] }
        });
        let redacted = redact_json_snapshot(&value, DEFAULT_SENSITIVE_KEYS);
        assert_eq!(redacted["data"]["client_secret"], "[redacted]");
        assert_eq!(redacted["data"]["nested"][0]["token"], "[redacted]");
    }

    #[test]
    fn provider_ids_policy_versions_and_support_refs_validate_boundaries() {
        let id = ProviderObjectId::new(" stripe:customer ", " cus_123 ").unwrap();
        assert_eq!(id.kind(), "stripe:customer");
        assert_eq!(id.id(), "cus_123");
        assert_eq!(
            ProviderObjectId::new("", "cus_123"),
            Err(BillingRiskError::Empty { field: "kind" })
        );
        assert_eq!(
            ProviderObjectId::new("stripe customer", "cus_123"),
            Err(BillingRiskError::Invalid { field: "kind" })
        );
        assert_eq!(
            ProviderObjectId::new("customer", "bad id"),
            Err(BillingRiskError::Invalid { field: "id" })
        );
        assert_eq!(
            ProviderObjectId::new("customer", "cus_123\n"),
            Err(BillingRiskError::Invalid { field: "id" })
        );

        let terms = PolicyVersion::new("terms:v1.2").unwrap();
        assert_eq!(terms.as_str(), "terms:v1.2");
        assert_eq!(
            PolicyVersion::new("terms v1"),
            Err(BillingRiskError::Invalid {
                field: "policy_version"
            })
        );
        assert_eq!(
            PolicyVersion::new("terms:v1\n"),
            Err(BillingRiskError::Invalid {
                field: "policy_version"
            })
        );

        let support_ref = SupportMessageRef::new(" msg_123 ").unwrap();
        assert_eq!(support_ref.as_str(), "msg_123");
        assert_eq!(
            SupportMessageRef::new(""),
            Err(BillingRiskError::Empty {
                field: "support_message_ref"
            })
        );
    }

    #[test]
    fn billing_error_display_and_redaction_preserve_safe_values() {
        assert_eq!(
            BillingRiskError::Empty { field: "kind" }.to_string(),
            "kind is empty"
        );
        assert_eq!(
            BillingRiskError::Invalid { field: "id" }.to_string(),
            "id is invalid"
        );
        let value = serde_json::json!({
            "safe": "visible",
            "items": ["plain", {"Authorization": "Bearer secret"}]
        });
        let redacted = redact_json_snapshot(&value, DEFAULT_SENSITIVE_KEYS);
        assert_eq!(redacted["safe"], "visible");
        assert_eq!(redacted["items"][0], "plain");
        assert_eq!(redacted["items"][1]["Authorization"], "[redacted]");
    }
}
