//! Durable delivery worker vocabulary and contracts.

use std::{fmt, future::Future, time::Duration};

#[cfg(feature = "world-identity")]
use world_identity_core::WorldRef;

/// Durable delivery status vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeliveryStatus {
    /// Delivery is ready to be claimed.
    Pending,
    /// Delivery is currently leased by a worker.
    Running,
    /// Delivery completed successfully.
    Delivered,
    /// Delivery permanently failed.
    Failed,
    /// Delivery is exhausted after retry policy was spent.
    Exhausted,
}

/// Opaque lease token owned by product persistence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeaseToken(String);

impl LeaseToken {
    /// Construct a lease token.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryError::InvalidLeaseToken`] for a blank token.
    pub fn new(value: impl AsRef<str>) -> Result<Self, DeliveryError> {
        let value = value.as_ref();
        if value.chars().any(char::is_control) {
            return Err(DeliveryError::InvalidLeaseToken);
        }
        let value = value.trim();
        if value.is_empty() {
            return Err(DeliveryError::InvalidLeaseToken);
        }
        Ok(Self(value.to_owned()))
    }

    /// Access the token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Construct a lease token from a UUID.
    #[cfg(feature = "uuid")]
    #[must_use]
    pub fn from_uuid(value: uuid::Uuid) -> Self {
        Self(value.to_string())
    }

    /// Parse this lease token as a UUID.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryError::InvalidLeaseToken`] if the token is not a UUID.
    #[cfg(feature = "uuid")]
    pub fn parse_uuid(&self) -> Result<uuid::Uuid, DeliveryError> {
        self.0.parse().map_err(|_| DeliveryError::InvalidLeaseToken)
    }
}

/// Product-neutral claimed delivery envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedDelivery<Id, Payload> {
    /// Product-owned delivery id.
    pub id: Id,
    /// Lease token required for finalization.
    pub lease_token: LeaseToken,
    /// Zero-based or one-based attempt count as product policy defines it.
    pub attempt: u32,
    /// Product-owned payload.
    pub payload: Payload,
}

/// Optional world-owned metadata for adapters that need it.
#[cfg(feature = "world-identity")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldDelivery<Id, Payload, W, I> {
    /// Shared world reference.
    pub world: WorldRef<W, I>,
    /// Claimed delivery envelope.
    pub delivery: ClaimedDelivery<Id, Payload>,
}

/// Provider attempt outcome vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryAttemptOutcome {
    /// Provider and local persistence both completed successfully.
    Delivered,
    /// Provider failed in a retryable way.
    RetryableFailure {
        /// Optional product/provider retry hint.
        retry_after: Option<Duration>,
    },
    /// Provider failed permanently.
    PermanentFailure,
    /// Provider side effect may have happened, but local persistence did not
    /// safely record completion.
    AmbiguousAfterSideEffect,
}

/// Delivery library error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryError {
    /// Lease token was blank or malformed.
    InvalidLeaseToken,
    /// Backoff configuration was invalid.
    InvalidBackoff,
}

impl fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLeaseToken => f.write_str("lease token is invalid"),
            Self::InvalidBackoff => f.write_str("backoff configuration is invalid"),
        }
    }
}

impl std::error::Error for DeliveryError {}

/// Exponential backoff with cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffPolicy {
    initial: Duration,
    max: Duration,
    multiplier: u32,
}

/// Convert a completed-attempt count into the next one-based attempt number.
#[must_use]
pub const fn attempt_from_completed_count(completed_count: u32) -> u32 {
    completed_count.saturating_add(1)
}

/// Convert a signed completed-attempt database field into the next one-based
/// attempt number.
#[must_use]
pub const fn attempt_from_completed_count_i32(completed_count: i32) -> Option<u32> {
    if completed_count < 0 {
        None
    } else {
        Some((completed_count as u32).saturating_add(1))
    }
}

impl BackoffPolicy {
    /// Construct a backoff policy.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryError::InvalidBackoff`] for zero durations,
    /// multiplier below one, or max below initial.
    pub fn new(initial: Duration, max: Duration, multiplier: u32) -> Result<Self, DeliveryError> {
        if initial.is_zero() || max.is_zero() || multiplier == 0 || max < initial {
            return Err(DeliveryError::InvalidBackoff);
        }
        Ok(Self {
            initial,
            max,
            multiplier,
        })
    }

    /// Calculate delay for an attempt number.
    #[must_use]
    pub fn delay_for_attempt(self, attempt: u32) -> Duration {
        let mut delay = self.initial;
        for _ in 0..attempt.saturating_sub(1) {
            let next = delay.saturating_mul(self.multiplier).min(self.max);
            if next == delay {
                break;
            }
            delay = next;
        }
        delay
    }

    /// Calculate delay for the next attempt after a completed-attempt count.
    #[must_use]
    pub fn delay_for_completed_attempts(self, completed_count: u32) -> Duration {
        self.delay_for_attempt(attempt_from_completed_count(completed_count))
    }

    /// Calculate delay from a signed completed-attempt database field.
    #[must_use]
    pub fn delay_for_completed_attempts_i32(self, completed_count: i32) -> Option<Duration> {
        attempt_from_completed_count_i32(completed_count)
            .map(|attempt| self.delay_for_attempt(attempt))
    }
}

/// Retry schedule wrapper for products that prefer completed-attempt semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrySchedule {
    policy: BackoffPolicy,
}

impl RetrySchedule {
    /// Construct a retry schedule from a backoff policy.
    #[must_use]
    pub const fn new(policy: BackoffPolicy) -> Self {
        Self { policy }
    }

    /// Access the underlying policy.
    #[must_use]
    pub const fn policy(self) -> BackoffPolicy {
        self.policy
    }

    /// Calculate delay for a one-based attempt number.
    #[must_use]
    pub fn delay_for_attempt(self, attempt: u32) -> Duration {
        self.policy.delay_for_attempt(attempt)
    }

    /// Calculate delay for the next attempt after completed attempts.
    #[must_use]
    pub fn delay_for_completed_attempts(self, completed_count: u32) -> Duration {
        self.policy.delay_for_completed_attempts(completed_count)
    }

    /// Calculate delay from a signed completed-attempt database field.
    #[must_use]
    pub fn delay_for_completed_attempts_i32(self, completed_count: i32) -> Option<Duration> {
        self.policy
            .delay_for_completed_attempts_i32(completed_count)
    }
}

/// Calculate delay for the next attempt after completed attempts.
#[must_use]
pub fn delay_for_completed_attempts(policy: BackoffPolicy, completed_count: u32) -> Duration {
    policy.delay_for_completed_attempts(completed_count)
}

/// Calculate delay from a signed completed-attempt database field.
#[must_use]
pub fn delay_for_completed_attempts_i32(
    policy: BackoffPolicy,
    completed_count: i32,
) -> Option<Duration> {
    policy.delay_for_completed_attempts_i32(completed_count)
}

/// Worker run report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeliveryRunReport {
    /// Claimed delivery count.
    pub claimed: u32,
    /// Delivered count.
    pub delivered: u32,
    /// Retryable failure count.
    pub retryable_failed: u32,
    /// Permanent failure count.
    pub permanently_failed: u32,
    /// Ambiguous-after-side-effect count.
    pub ambiguous_after_side_effect: u32,
}

impl DeliveryRunReport {
    /// Record one attempt outcome.
    pub fn record(&mut self, outcome: &DeliveryAttemptOutcome) {
        match outcome {
            DeliveryAttemptOutcome::Delivered => self.delivered += 1,
            DeliveryAttemptOutcome::RetryableFailure { .. } => self.retryable_failed += 1,
            DeliveryAttemptOutcome::PermanentFailure => self.permanently_failed += 1,
            DeliveryAttemptOutcome::AmbiguousAfterSideEffect => {
                self.ambiguous_after_side_effect += 1;
            }
        }
    }
}

/// Product adapter trait for claiming deliveries.
pub trait DeliveryClaimer {
    /// Product-owned delivery id type.
    type Id;
    /// Product-owned payload type.
    type Payload;
    /// Product-owned error type.
    type Error;
    /// Future returned by [`DeliveryClaimer::claim`].
    type ClaimFuture<'a>: Future<Output = Result<Vec<ClaimedDelivery<Self::Id, Self::Payload>>, Self::Error>>
        + Send
    where
        Self: 'a;

    /// Claim up to `limit` delivery envelopes.
    fn claim(&mut self, limit: u32, lease_ttl: Duration) -> Self::ClaimFuture<'_>;
}

/// Product adapter trait for finalizing deliveries.
pub trait DeliveryFinalizer<Id> {
    /// Product-owned error type.
    type Error;
    /// Future returned by [`DeliveryFinalizer::finalize`].
    type FinalizeFuture<'a>: Future<Output = Result<(), Self::Error>> + Send
    where
        Self: 'a,
        Id: 'a;

    /// Finalize a delivery attempt.
    fn finalize(
        &mut self,
        id: Id,
        lease_token: LeaseToken,
        outcome: DeliveryAttemptOutcome,
    ) -> Self::FinalizeFuture<'_>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_capped() {
        let policy =
            BackoffPolicy::new(Duration::from_secs(2), Duration::from_secs(10), 3).unwrap();
        assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(2));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(6));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_secs(10));
        assert_eq!(
            delay_for_completed_attempts(policy, 2),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn lease_tokens_trim_and_uuid_roundtrip() {
        let token = LeaseToken::new(" lease-1 ").unwrap();
        assert_eq!(token.as_str(), "lease-1");
        assert_eq!(LeaseToken::new(" "), Err(DeliveryError::InvalidLeaseToken));
        assert_eq!(
            LeaseToken::new("lease-1\n"),
            Err(DeliveryError::InvalidLeaseToken)
        );

        #[cfg(feature = "uuid")]
        {
            let uuid = uuid::Uuid::nil();
            let token = LeaseToken::from_uuid(uuid);
            assert_eq!(token.parse_uuid().unwrap(), uuid);
            assert_eq!(
                LeaseToken::new("not-a-uuid").unwrap().parse_uuid(),
                Err(DeliveryError::InvalidLeaseToken)
            );
        }
    }

    #[test]
    fn invalid_backoff_and_error_display_are_stable() {
        assert_eq!(
            BackoffPolicy::new(Duration::ZERO, Duration::from_secs(1), 1),
            Err(DeliveryError::InvalidBackoff)
        );
        assert_eq!(
            BackoffPolicy::new(Duration::from_secs(2), Duration::from_secs(1), 1),
            Err(DeliveryError::InvalidBackoff)
        );
        assert_eq!(
            BackoffPolicy::new(Duration::from_secs(1), Duration::from_secs(2), 0),
            Err(DeliveryError::InvalidBackoff)
        );
        assert_eq!(
            DeliveryError::InvalidLeaseToken.to_string(),
            "lease token is invalid"
        );
        assert_eq!(
            DeliveryError::InvalidBackoff.to_string(),
            "backoff configuration is invalid"
        );
    }

    #[test]
    fn retry_schedule_delegates_completed_attempt_semantics() {
        let policy = BackoffPolicy::new(Duration::from_secs(1), Duration::from_secs(8), 2).unwrap();
        let schedule = RetrySchedule::new(policy);
        assert_eq!(schedule.policy(), policy);
        assert_eq!(schedule.delay_for_attempt(4), Duration::from_secs(8));
        assert_eq!(
            schedule.delay_for_completed_attempts(2),
            Duration::from_secs(4)
        );
        assert_eq!(
            schedule.delay_for_completed_attempts_i32(2),
            Some(Duration::from_secs(4))
        );
        assert_eq!(delay_for_completed_attempts_i32(policy, -1), None);
    }

    #[test]
    fn backoff_short_circuits_when_capped_or_constant() {
        let capped =
            BackoffPolicy::new(Duration::from_secs(2), Duration::from_secs(10), 3).unwrap();
        assert_eq!(capped.delay_for_attempt(u32::MAX), Duration::from_secs(10));

        let constant =
            BackoffPolicy::new(Duration::from_secs(2), Duration::from_secs(10), 1).unwrap();
        assert_eq!(constant.delay_for_attempt(u32::MAX), Duration::from_secs(2));
    }

    #[test]
    fn report_counts_ambiguous_outcomes() {
        let mut report = DeliveryRunReport {
            claimed: 2,
            ..DeliveryRunReport::default()
        };
        report.record(&DeliveryAttemptOutcome::Delivered);
        report.record(&DeliveryAttemptOutcome::AmbiguousAfterSideEffect);
        assert_eq!(report.delivered, 1);
        assert_eq!(report.ambiguous_after_side_effect, 1);
    }

    #[test]
    fn attempt_from_completed_count_is_one_based_and_saturating() {
        assert_eq!(attempt_from_completed_count(0), 1);
        assert_eq!(attempt_from_completed_count(4), 5);
        assert_eq!(attempt_from_completed_count(u32::MAX), u32::MAX);
        assert_eq!(attempt_from_completed_count_i32(4), Some(5));
        assert_eq!(attempt_from_completed_count_i32(-1), None);
    }
}
