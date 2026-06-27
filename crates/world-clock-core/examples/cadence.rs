//! Cadence and deterministic seed example.

use std::time::{Duration, SystemTime};

use world_clock_core::{deterministic_cycle_seed, Cadence, CycleNumber};
use world_identity_core::{NoIncarnation, WorldRef};

fn main() {
    let cadence = Cadence::new(Duration::from_secs(60)).expect("non-zero cadence");
    let next = cadence.next_due_after(SystemTime::UNIX_EPOCH);
    assert_eq!(next, SystemTime::UNIX_EPOCH + Duration::from_secs(60));

    let world = WorldRef::new("world-dev-001", NoIncarnation);
    let seed = deterministic_cycle_seed(&world, CycleNumber::new(12), &["fixtures"]);
    assert_ne!(seed, 0);
}
