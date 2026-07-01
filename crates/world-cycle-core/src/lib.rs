//! Product-neutral cycle, lease, and follow-up retry primitives.
//!
//! This crate intentionally does not define product table names, migrations,
//! SQL queries, phase ordering, job payloads, or finalization policy. Products
//! own those boundaries and use these small types to share state-machine and
//! timing vocabulary.

use std::{error::Error, fmt, num::NonZeroU32, str::FromStr, time::Duration};

const MAX_OWNER_ID_LEN: usize = 128;

/// Error returned for invalid cycle/lease policy values.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WorldCycleError {
    /// A field was empty after trimming.
    Empty {
        /// Field name.
        field: &'static str,
    },
    /// A field exceeded its maximum length.
    TooLong {
        /// Field name.
        field: &'static str,
        /// Observed length in bytes.
        len: usize,
        /// Maximum allowed length in bytes.
        max: usize,
    },
    /// A field contained an unsupported character.
    InvalidCharacter {
        /// Field name.
        field: &'static str,
        /// Invalid character.
        ch: char,
    },
    /// Lease TTL must be positive.
    InvalidLeaseTtl,
    /// Backoff policy must have positive durations and multiplier.
    InvalidBackoff,
    /// Status label was unknown.
    UnknownStatus(String),
}

impl fmt::Display for WorldCycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} is empty"),
            Self::TooLong { field, len, max } => {
                write!(f, "{field} length {len} exceeds {max}")
            }
            Self::InvalidCharacter { field, ch } => {
                write!(f, "{field} contains invalid character {ch:?}")
            }
            Self::InvalidLeaseTtl => f.write_str("lease ttl must be positive"),
            Self::InvalidBackoff => f.write_str("backoff policy is invalid"),
            Self::UnknownStatus(status) => write!(f, "unknown cycle status {status:?}"),
        }
    }
}

impl Error for WorldCycleError {}

/// Product-owned worker or lease owner identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkerId(String);

impl WorkerId {
    /// Validate and construct a worker id.
    ///
    /// # Errors
    ///
    /// Returns [`WorldCycleError`] when the id is empty, too long, or contains
    /// unsupported characters.
    pub fn new(value: impl AsRef<str>) -> Result<Self, WorldCycleError> {
        let value = value.as_ref();
        if let Some(ch) = value.chars().find(|ch| ch.is_control()) {
            return Err(WorldCycleError::InvalidCharacter {
                field: "worker_id",
                ch,
            });
        }
        let value = value.trim();
        if value.is_empty() {
            return Err(WorldCycleError::Empty { field: "worker_id" });
        }
        if value.len() > MAX_OWNER_ID_LEN {
            return Err(WorldCycleError::TooLong {
                field: "worker_id",
                len: value.len(),
                max: MAX_OWNER_ID_LEN,
            });
        }
        if let Some(ch) = value
            .chars()
            .find(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/')))
        {
            return Err(WorldCycleError::InvalidCharacter {
                field: "worker_id",
                ch,
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// Access the validated id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for WorkerId {
    type Err = WorldCycleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// Product-neutral cycle phase state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum CyclePhaseStatus {
    /// Phase has not started.
    Pending,
    /// Phase is leased/running.
    Running,
    /// Phase side effects have been applied but the product has not finished
    /// its completion boundary.
    Applied,
    /// Phase completed.
    Complete,
    /// Phase failed and may be reclaimed by product policy.
    Failed,
}

impl CyclePhaseStatus {
    /// Stable lowercase status label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Applied => "applied",
            Self::Complete => "complete",
            Self::Failed => "failed",
        }
    }

    /// Return true when this status is terminal for a phase.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Complete)
    }
}

impl fmt::Display for CyclePhaseStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CyclePhaseStatus {
    type Err = WorldCycleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "applied" => Ok(Self::Applied),
            "complete" | "completed" => Ok(Self::Complete),
            "failed" => Ok(Self::Failed),
            other => Err(WorldCycleError::UnknownStatus(other.to_owned())),
        }
    }
}

/// Result of trying to claim a cycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CyclePhaseClaim {
    /// The caller owns the phase and may run it.
    Claimed,
    /// The phase has passed the side-effect boundary and needs completion.
    Applied,
    /// The phase is already complete.
    Complete,
    /// The phase is unavailable to this caller.
    Busy,
}

impl CyclePhaseClaim {
    /// Map an existing product status into a failed claim result.
    #[must_use]
    pub const fn from_existing_status(status: Option<CyclePhaseStatus>) -> Self {
        match status {
            Some(CyclePhaseStatus::Applied) => Self::Applied,
            Some(CyclePhaseStatus::Complete) => Self::Complete,
            _ => Self::Busy,
        }
    }
}

/// Product-neutral follow-up retry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum FollowupRetryStatus {
    /// Retry is due or scheduled.
    Pending,
    /// Retry is leased/running.
    Running,
    /// Retry completed.
    Complete,
    /// Retry attempts are exhausted.
    Exhausted,
}

impl FollowupRetryStatus {
    /// Stable lowercase status label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Complete => "complete",
            Self::Exhausted => "exhausted",
        }
    }

    /// Return true when the status is terminal.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Exhausted)
    }

    /// Return true when a newly recorded row in this status should preserve a
    /// retry delay.
    #[must_use]
    pub const fn should_delay_retry(self) -> bool {
        matches!(self, Self::Pending)
    }

    /// Return true when a lease can be carried with this status.
    #[must_use]
    pub const fn allows_lease(self) -> bool {
        !self.is_terminal()
    }
}

impl fmt::Display for FollowupRetryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FollowupRetryStatus {
    type Err = WorldCycleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "complete" | "completed" => Ok(Self::Complete),
            "exhausted" => Ok(Self::Exhausted),
            other => Err(WorldCycleError::UnknownStatus(other.to_owned())),
        }
    }
}

/// Product-neutral runtime status for a world's cycle machinery.
///
/// This is display/control-plane vocabulary, not a storage schema. Products
/// should derive it from their local clock rows, leases, pause controls, and
/// recovery signals while preserving detailed local diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum CycleRuntimeStatus {
    /// World is enabled and waiting for the next eligible cycle boundary.
    Open,
    /// A cycle worker owns active work before finalization.
    Running,
    /// The authoritative finalization boundary is active.
    Finalizing,
    /// Automatic cycle work is intentionally paused.
    Paused,
    /// Product-owned recovery or failed-state signals require attention.
    AttentionRequired,
}

impl CycleRuntimeStatus {
    /// Stable lowercase status label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Running => "running",
            Self::Finalizing => "finalizing",
            Self::Paused => "paused",
            Self::AttentionRequired => "attention_required",
        }
    }
}

impl fmt::Display for CycleRuntimeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CycleRuntimeStatus {
    type Err = WorldCycleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "open" | "ready" | "waiting" => Ok(Self::Open),
            "running" | "simulating" | "processing" => Ok(Self::Running),
            "finalizing" => Ok(Self::Finalizing),
            "paused" => Ok(Self::Paused),
            "attention_required" | "failed" | "stuck" => Ok(Self::AttentionRequired),
            other => Err(WorldCycleError::UnknownStatus(other.to_owned())),
        }
    }
}

/// Product-owned cycle runtime facts used to derive [`CycleRuntimeStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CycleRuntimeSignals {
    /// Product has detected a failed or stuck condition that should outrank
    /// normal lifecycle display.
    pub attention_required: bool,
    /// Product configuration intentionally disables cycle starts.
    pub paused: bool,
    /// Finalization lease or equivalent authoritative boundary is active.
    pub finalizing_active: bool,
    /// Run lease or equivalent pre-finalization work is active.
    pub run_active: bool,
}

impl CycleRuntimeSignals {
    /// Construct cycle runtime signals.
    #[must_use]
    pub const fn new(
        attention_required: bool,
        paused: bool,
        finalizing_active: bool,
        run_active: bool,
    ) -> Self {
        Self {
            attention_required,
            paused,
            finalizing_active,
            run_active,
        }
    }

    /// Construct cycle runtime signals when the product has no attention or
    /// recovery condition to report.
    #[must_use]
    pub const fn without_attention(
        paused: bool,
        finalizing_active: bool,
        run_active: bool,
    ) -> Self {
        Self::new(false, paused, finalizing_active, run_active)
    }
}

/// Derive a product-neutral runtime status from product-owned signals.
#[must_use]
pub const fn derive_cycle_runtime_status(signals: CycleRuntimeSignals) -> CycleRuntimeStatus {
    if signals.attention_required {
        CycleRuntimeStatus::AttentionRequired
    } else if signals.paused {
        CycleRuntimeStatus::Paused
    } else if signals.finalizing_active {
        CycleRuntimeStatus::Finalizing
    } else if signals.run_active {
        CycleRuntimeStatus::Running
    } else {
        CycleRuntimeStatus::Open
    }
}

/// Product-owned lease state using Unix-second expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseState<'a> {
    /// Current lease owner, if any.
    pub owner_id: Option<&'a str>,
    /// Unix-second expiry, if any.
    pub expires_at: Option<i64>,
}

impl<'a> LeaseState<'a> {
    /// Construct a lease state.
    #[must_use]
    pub const fn new(owner_id: Option<&'a str>, expires_at: Option<i64>) -> Self {
        Self {
            owner_id,
            expires_at,
        }
    }

    /// Return true when the lease has an owner and expires in the future.
    #[must_use]
    pub fn is_active(self, now: i64) -> bool {
        self.owner_id.is_some() && lease_expiry_is_active(self.expires_at, now)
    }

    /// Return true when the lease is active and held by `owner_id`.
    #[must_use]
    pub fn is_active_for(self, owner_id: &str, now: i64) -> bool {
        self.owner_id == Some(owner_id) && lease_expiry_is_active(self.expires_at, now)
    }

    /// Return true when `owner_id` may claim or renew the lease.
    #[must_use]
    pub fn claimable_by(self, owner_id: &str, now: i64) -> bool {
        !self.is_active(now) || self.owner_id == Some(owner_id)
    }
}

/// Lease policy selected by a product adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeasePolicy {
    /// Lease owner.
    pub owner_id: WorkerId,
    /// Lease TTL.
    pub ttl: Duration,
}

impl LeasePolicy {
    /// Construct a lease policy.
    ///
    /// # Errors
    ///
    /// Returns [`WorldCycleError::InvalidLeaseTtl`] for a zero TTL.
    pub fn new(owner_id: WorkerId, ttl: Duration) -> Result<Self, WorldCycleError> {
        if ttl.is_zero() {
            return Err(WorldCycleError::InvalidLeaseTtl);
        }
        Ok(Self { owner_id, ttl })
    }

    /// Return the TTL as whole seconds, preserving sub-second nonzero TTLs as
    /// one second for Unix-second stores.
    #[must_use]
    pub fn ttl_seconds(&self) -> i64 {
        duration_to_positive_i64_seconds(self.ttl)
    }

    /// Calculate the Unix-second expiry from `now`.
    #[must_use]
    pub fn expires_at(&self, now: i64) -> i64 {
        now.saturating_add(self.ttl_seconds())
    }
}

/// Exponential retry backoff for zero-based retry attempt counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryBackoffPolicy {
    initial: Duration,
    max: Duration,
    multiplier: u32,
}

impl RetryBackoffPolicy {
    /// Construct a retry backoff policy.
    ///
    /// # Errors
    ///
    /// Returns [`WorldCycleError::InvalidBackoff`] for zero durations, zero
    /// multiplier, or a max smaller than the initial delay.
    pub fn new(initial: Duration, max: Duration, multiplier: u32) -> Result<Self, WorldCycleError> {
        if initial.is_zero() || max.is_zero() || multiplier == 0 || max < initial {
            return Err(WorldCycleError::InvalidBackoff);
        }
        Ok(Self {
            initial,
            max,
            multiplier,
        })
    }

    /// Delay for a zero-based attempt index.
    #[must_use]
    pub fn delay_for_attempt_index(self, attempt_index: u64) -> Duration {
        let mut delay = self.initial;
        for _ in 0..attempt_index {
            let next = delay.saturating_mul(self.multiplier).min(self.max);
            if next == delay {
                break;
            }
            delay = next;
        }
        delay
    }
}

/// Follow-up retry policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FollowupRetryPolicy {
    /// Maximum attempts before exhaustion.
    pub max_attempts: NonZeroU32,
    /// Backoff for retry scheduling.
    pub backoff: RetryBackoffPolicy,
}

impl FollowupRetryPolicy {
    /// Construct a follow-up retry policy.
    #[must_use]
    pub const fn new(max_attempts: NonZeroU32, backoff: RetryBackoffPolicy) -> Self {
        Self {
            max_attempts,
            backoff,
        }
    }

    /// Return true when this attempt counter is exhausted.
    #[must_use]
    pub fn attempts_exhausted(self, attempts: u64) -> bool {
        attempts >= u64::from(self.max_attempts.get())
    }

    /// Delay for the product's current zero-based attempt counter.
    #[must_use]
    pub fn delay_for_attempts(self, attempts: u64) -> Duration {
        self.backoff.delay_for_attempt_index(attempts)
    }
}

/// Return true when a Unix-second lease expiry is in the future.
#[must_use]
pub fn lease_expiry_is_active(lease_expires_at: Option<i64>, now: i64) -> bool {
    lease_expires_at.is_some_and(|lease| lease > now)
}

/// Return true when a finalizing lease should block other work.
#[must_use]
pub fn finalizing_lease_is_active(
    finalizing: bool,
    lease_expires_at: Option<i64>,
    now: i64,
) -> bool {
    finalizing && lease_expiry_is_active(lease_expires_at, now)
}

/// Return true when an owned run lease should block other workers.
#[must_use]
pub fn owned_lease_is_active(
    owner_id: Option<&str>,
    lease_expires_at: Option<i64>,
    now: i64,
) -> bool {
    LeaseState::new(owner_id, lease_expires_at).is_active(now)
}

/// Calculate a stale cutoff from Unix-second `now` and a grace duration.
#[must_use]
pub fn stale_cutoff_unix_seconds(now: i64, grace: Duration) -> i64 {
    now.saturating_sub(duration_to_i64_seconds(grace))
}

/// Calculate the oldest cycle to retain when pruning completed phase records.
#[must_use]
pub fn retention_cutoff_cycle(completed_cycle: i64, keep_cycles: NonZeroU32) -> i64 {
    completed_cycle.saturating_sub(i64::from(keep_cycles.get()).saturating_sub(1))
}

fn duration_to_i64_seconds(duration: Duration) -> i64 {
    let seconds = duration.as_secs();
    let rounded = seconds.saturating_add(u64::from(duration.subsec_nanos() > 0));
    i64::try_from(rounded).unwrap_or(i64::MAX)
}

fn duration_to_positive_i64_seconds(duration: Duration) -> i64 {
    duration_to_i64_seconds(duration).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_claim_maps_existing_statuses() {
        assert_eq!(
            CyclePhaseClaim::from_existing_status(Some(CyclePhaseStatus::Applied)),
            CyclePhaseClaim::Applied
        );
        assert_eq!(
            CyclePhaseClaim::from_existing_status(Some(CyclePhaseStatus::Complete)),
            CyclePhaseClaim::Complete
        );
        assert_eq!(
            CyclePhaseClaim::from_existing_status(Some(CyclePhaseStatus::Running)),
            CyclePhaseClaim::Busy
        );
        assert_eq!(
            CyclePhaseClaim::from_existing_status(None),
            CyclePhaseClaim::Busy
        );
    }

    #[test]
    fn leases_track_active_owner_and_claimability() {
        let active = LeaseState::new(Some("worker-a"), Some(120));
        assert!(active.is_active(100));
        assert!(active.is_active_for("worker-a", 100));
        assert!(active.claimable_by("worker-a", 100));
        assert!(!active.claimable_by("worker-b", 100));

        let expired = LeaseState::new(Some("worker-a"), Some(90));
        assert!(!expired.is_active(100));
        assert!(expired.claimable_by("worker-b", 100));
    }

    #[test]
    fn lease_policy_preserves_sub_second_ttl() {
        let policy = LeasePolicy::new(
            WorkerId::new("worker-a").unwrap(),
            Duration::from_millis(500),
        )
        .unwrap();
        assert_eq!(policy.ttl_seconds(), 1);
        assert_eq!(policy.expires_at(41), 42);
    }

    #[test]
    fn lease_policy_rounds_fractional_ttl_up_to_unix_seconds() {
        let policy = LeasePolicy::new(
            WorkerId::new("worker-a").unwrap(),
            Duration::from_millis(1500),
        )
        .unwrap();
        assert_eq!(policy.ttl_seconds(), 2);
        assert_eq!(policy.expires_at(40), 42);
    }

    #[test]
    fn followup_status_controls_delay_and_leases() {
        assert!(FollowupRetryStatus::Pending.should_delay_retry());
        assert!(FollowupRetryStatus::Running.allows_lease());
        assert!(!FollowupRetryStatus::Complete.allows_lease());
        assert!(FollowupRetryStatus::Exhausted.is_terminal());
        assert_eq!(FollowupRetryStatus::Pending.as_str(), "pending");
        assert_eq!(FollowupRetryStatus::Running.to_string(), "running");
        assert_eq!(
            FollowupRetryStatus::from_str("completed").unwrap(),
            FollowupRetryStatus::Complete
        );
        assert_eq!(
            FollowupRetryStatus::from_str("unknown"),
            Err(WorldCycleError::UnknownStatus("unknown".to_owned()))
        );
    }

    #[test]
    fn cycle_runtime_status_uses_stable_precedence() {
        assert_eq!(
            derive_cycle_runtime_status(CycleRuntimeSignals::new(true, true, true, true)),
            CycleRuntimeStatus::AttentionRequired
        );
        assert_eq!(
            derive_cycle_runtime_status(CycleRuntimeSignals::without_attention(true, true, true)),
            CycleRuntimeStatus::Paused
        );
        assert_eq!(
            derive_cycle_runtime_status(CycleRuntimeSignals::without_attention(false, true, true)),
            CycleRuntimeStatus::Finalizing
        );
        assert_eq!(
            derive_cycle_runtime_status(CycleRuntimeSignals::without_attention(false, false, true)),
            CycleRuntimeStatus::Running
        );
        assert_eq!(
            derive_cycle_runtime_status(CycleRuntimeSignals::without_attention(
                false, false, false
            )),
            CycleRuntimeStatus::Open
        );
        assert_eq!(
            CycleRuntimeStatus::from_str("failed").unwrap(),
            CycleRuntimeStatus::AttentionRequired
        );
        assert_eq!(CycleRuntimeStatus::Open.as_str(), "open");
        assert_eq!(CycleRuntimeStatus::Finalizing.to_string(), "finalizing");
        assert_eq!(
            CycleRuntimeStatus::from_str("simulating").unwrap(),
            CycleRuntimeStatus::Running
        );
    }

    #[test]
    fn retry_backoff_is_zero_based_and_capped() {
        let backoff =
            RetryBackoffPolicy::new(Duration::from_secs(30), Duration::from_secs(3600), 2).unwrap();
        assert_eq!(backoff.delay_for_attempt_index(0), Duration::from_secs(30));
        assert_eq!(backoff.delay_for_attempt_index(1), Duration::from_secs(60));
        assert_eq!(
            backoff.delay_for_attempt_index(u64::MAX),
            Duration::from_secs(3600)
        );
        assert_eq!(
            RetryBackoffPolicy::new(Duration::ZERO, Duration::from_secs(1), 1),
            Err(WorldCycleError::InvalidBackoff)
        );
        assert_eq!(
            RetryBackoffPolicy::new(Duration::from_secs(2), Duration::from_secs(1), 1),
            Err(WorldCycleError::InvalidBackoff)
        );
    }

    #[test]
    fn followup_policy_tracks_exhaustion() {
        let policy = FollowupRetryPolicy::new(
            NonZeroU32::new(5).unwrap(),
            RetryBackoffPolicy::new(Duration::from_secs(1), Duration::from_secs(8), 2).unwrap(),
        );
        assert!(!policy.attempts_exhausted(4));
        assert!(policy.attempts_exhausted(5));
        assert_eq!(policy.delay_for_attempts(3), Duration::from_secs(8));
    }

    #[test]
    fn retention_cutoff_keeps_requested_window() {
        assert_eq!(
            retention_cutoff_cycle(100, NonZeroU32::new(24).unwrap()),
            77
        );
        assert_eq!(
            retention_cutoff_cycle(i64::MIN, NonZeroU32::new(24).unwrap()),
            i64::MIN
        );
    }

    #[test]
    fn worker_ids_statuses_and_errors_are_stable() {
        let worker = WorkerId::new(" worker/1 ").unwrap();
        assert_eq!(worker.as_str(), "worker/1");
        assert_eq!(worker.to_string(), "worker/1");
        assert_eq!(
            WorkerId::new(" "),
            Err(WorldCycleError::Empty { field: "worker_id" })
        );
        assert!(matches!(
            WorkerId::new("x".repeat(129)),
            Err(WorldCycleError::TooLong {
                field: "worker_id",
                len: 129,
                max: 128
            })
        ));
        assert_eq!(
            WorkerId::new("bad worker"),
            Err(WorldCycleError::InvalidCharacter {
                field: "worker_id",
                ch: ' '
            })
        );
        assert_eq!(
            WorkerId::new("worker/1\n"),
            Err(WorldCycleError::InvalidCharacter {
                field: "worker_id",
                ch: '\n'
            })
        );

        assert_eq!(CyclePhaseStatus::Pending.as_str(), "pending");
        assert_eq!(CyclePhaseStatus::Complete.to_string(), "complete");
        assert!(CyclePhaseStatus::Complete.is_terminal());
        assert!(!CyclePhaseStatus::Failed.is_terminal());
        assert_eq!(
            CyclePhaseStatus::from_str("completed").unwrap(),
            CyclePhaseStatus::Complete
        );
        assert_eq!(
            WorldCycleError::InvalidLeaseTtl.to_string(),
            "lease ttl must be positive"
        );
        assert_eq!(
            WorldCycleError::UnknownStatus("weird".to_owned()).to_string(),
            "unknown cycle status \"weird\""
        );
    }

    #[test]
    fn lease_helpers_cover_empty_and_finalizing_states() {
        assert!(!lease_expiry_is_active(None, 100));
        assert!(lease_expiry_is_active(Some(101), 100));
        assert!(finalizing_lease_is_active(true, Some(101), 100));
        assert!(!finalizing_lease_is_active(false, Some(101), 100));
        assert!(owned_lease_is_active(Some("worker-a"), Some(101), 100));
        assert!(!owned_lease_is_active(None, Some(101), 100));
        assert_eq!(stale_cutoff_unix_seconds(100, Duration::from_secs(15)), 85);
        assert_eq!(
            stale_cutoff_unix_seconds(i64::MIN, Duration::from_secs(15)),
            i64::MIN
        );
        assert_eq!(
            LeasePolicy::new(WorkerId::new("worker-a").unwrap(), Duration::ZERO),
            Err(WorldCycleError::InvalidLeaseTtl)
        );
    }
}
