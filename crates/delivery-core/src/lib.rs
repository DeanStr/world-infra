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
        let value = value.as_ref().trim();
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
}
