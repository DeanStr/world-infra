//! Product-neutral world clock primitives.

use std::{
    error::Error,
    fmt,
    time::{Duration, SystemTime},
};

use world_identity_core::WorldRef;

/// A one-based or zero-based product cycle number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CycleNumber(u64);

impl CycleNumber {
    /// Construct a cycle number.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Return the next cycle, saturating at `u64::MAX`.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl fmt::Display for CycleNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Product-neutral world day value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldDay(u64);

impl WorldDay {
    /// Construct a world day.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for WorldDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Error returned for invalid cadence configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockError {
    /// Cadence duration was zero.
    ZeroCadence,
}

impl fmt::Display for ClockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCadence => f.write_str("cadence duration must be non-zero"),
        }
    }
}

impl Error for ClockError {}

/// Cadence between product-owned cycle ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    duration: Duration,
}

impl Cadence {
    /// Construct a non-zero cadence.
    ///
    /// # Errors
    ///
    /// Returns [`ClockError::ZeroCadence`] for zero duration.
    pub fn new(duration: Duration) -> Result<Self, ClockError> {
        if duration.is_zero() {
            return Err(ClockError::ZeroCadence);
        }
        Ok(Self { duration })
    }

    /// Access the cadence duration.
    #[must_use]
    pub const fn duration(self) -> Duration {
        self.duration
    }

    /// Calculate the next due time from a last-run timestamp.
    #[must_use]
    pub fn next_due_after(self, last_due: SystemTime) -> SystemTime {
        system_time_saturating_add(last_due, self.duration)
    }

    /// Return true when `now` is on or after the next due time.
    #[must_use]
    pub fn is_due(self, last_due: SystemTime, now: SystemTime) -> bool {
        now >= self.next_due_after(last_due)
    }
}

/// Calculate a cutoff time before a due timestamp.
#[must_use]
pub fn cutoff_before(due_at: SystemTime, cutoff: Duration) -> SystemTime {
    due_at.checked_sub(cutoff).unwrap_or(SystemTime::UNIX_EPOCH)
}

fn system_time_saturating_add(time: SystemTime, duration: Duration) -> SystemTime {
    let mut duration = duration;
    loop {
        if let Some(next) = time.checked_add(duration) {
            return next;
        }
        duration /= 2;
        if duration.is_zero() {
            return time;
        }
    }
}

/// Deterministic seed helper based on FNV-1a over product-provided labels.
#[must_use]
pub fn deterministic_cycle_seed<W, I>(
    world: &WorldRef<W, I>,
    cycle: CycleNumber,
    labels: &[&str],
) -> u64
where
    W: fmt::Display,
    I: fmt::Display,
{
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for part in [
        world.world_id.to_string(),
        world.incarnation.to_string(),
        cycle.to_string(),
    ] {
        fnv1a_update(&mut hash, part.as_bytes());
        fnv1a_update(&mut hash, b"\0");
    }
    for label in labels {
        fnv1a_update(&mut hash, label.as_bytes());
        fnv1a_update(&mut hash, b"\0");
    }
    hash
}

fn fnv1a_update(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_identity_core::NoIncarnation;

    #[test]
    fn cadence_calculates_due_time() {
        let cadence = Cadence::new(Duration::from_secs(60)).unwrap();
        let last = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        assert_eq!(cadence.duration(), Duration::from_secs(60));
        assert_eq!(
            cadence.next_due_after(last),
            SystemTime::UNIX_EPOCH + Duration::from_secs(160)
        );
        assert!(!cadence.is_due(last, SystemTime::UNIX_EPOCH + Duration::from_secs(159)));
        assert!(cadence.is_due(last, SystemTime::UNIX_EPOCH + Duration::from_secs(160)));
    }

    #[test]
    fn cadence_due_time_saturates_extreme_duration() {
        let cadence = Cadence::new(Duration::MAX).unwrap();
        let due = cadence.next_due_after(SystemTime::UNIX_EPOCH);
        assert!(due > SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn cycle_numbers_days_and_clock_errors_are_stable() {
        assert_eq!(CycleNumber::new(7).get(), 7);
        assert_eq!(CycleNumber::new(7).next().get(), 8);
        assert_eq!(CycleNumber::new(u64::MAX).next().get(), u64::MAX);
        assert_eq!(CycleNumber::new(7).to_string(), "7");
        assert_eq!(WorldDay::new(42).get(), 42);
        assert_eq!(WorldDay::new(42).to_string(), "42");
        assert_eq!(Cadence::new(Duration::ZERO), Err(ClockError::ZeroCadence));
        assert_eq!(
            ClockError::ZeroCadence.to_string(),
            "cadence duration must be non-zero"
        );
        assert_eq!(
            cutoff_before(
                SystemTime::UNIX_EPOCH + Duration::from_secs(5),
                Duration::from_secs(4)
            ),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1)
        );
    }

    #[test]
    fn deterministic_seed_is_stable() {
        let world = WorldRef::new("world-dev-001", NoIncarnation);
        assert_eq!(
            deterministic_cycle_seed(&world, CycleNumber::new(12), &["fixtures"]),
            deterministic_cycle_seed(&world, CycleNumber::new(12), &["fixtures"])
        );
        assert_ne!(
            deterministic_cycle_seed(&world, CycleNumber::new(12), &["fixtures"]),
            deterministic_cycle_seed(&world, CycleNumber::new(13), &["fixtures"])
        );
    }
}
