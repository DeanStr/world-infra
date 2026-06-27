//! Delivery backoff and report example.

use std::time::Duration;

use delivery_core::{BackoffPolicy, DeliveryAttemptOutcome, DeliveryRunReport};

fn main() {
    let backoff = BackoffPolicy::new(Duration::from_secs(5), Duration::from_secs(60), 2)
        .expect("valid backoff");
    assert_eq!(backoff.delay_for_attempt(3), Duration::from_secs(20));

    let mut report = DeliveryRunReport::default();
    report.record(&DeliveryAttemptOutcome::AmbiguousAfterSideEffect);
    assert_eq!(report.ambiguous_after_side_effect, 1);
}
