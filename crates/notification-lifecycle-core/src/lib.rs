//! Product-neutral notification lifecycle state and delivery-version helpers.
//!
//! This crate does not define notification categories, copy, deep links, quiet
//! hours, tier policy, managed-action semantics, or product SQL.

use notification_core::DeliveryVersion;

/// Product-neutral state of a canonical notification item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotificationItemState {
    /// Visible and not completed/dismissed/read.
    Open,
    /// Visible and not read. This is equivalent to [`NotificationItemState::Open`]
    /// for recurrence decisions.
    Unread,
    /// User read the notification.
    Read,
    /// User dismissed the notification.
    Dismissed,
    /// Product marked the action done.
    Done,
    /// Notification expired.
    Expired,
}

impl NotificationItemState {
    /// Derive item state from common product-owned boolean flags.
    ///
    /// When multiple flags are true, terminal/action states take precedence
    /// over read state in this order: expired, done, dismissed, read, unread.
    #[must_use]
    pub const fn from_flags(read: bool, dismissed: bool, done: bool, expired: bool) -> Self {
        if expired {
            Self::Expired
        } else if done {
            Self::Done
        } else if dismissed {
            Self::Dismissed
        } else if read {
            Self::Read
        } else {
            Self::Unread
        }
    }

    /// Return whether a recurring deduped notification should reopen.
    #[must_use]
    pub const fn should_reopen_on_recurrence(self) -> bool {
        matches!(
            self,
            Self::Read | Self::Dismissed | Self::Done | Self::Expired
        )
    }
}

/// Product-neutral effects a product adapter should apply when reopening a
/// deduped notification item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NotificationReopenEffects {
    /// Next delivery version to persist.
    pub next_delivery_version: DeliveryVersion,
    /// Clear any read marker on the existing item.
    pub clear_read: bool,
    /// Clear any dismissal marker on the existing item.
    pub clear_dismissed: bool,
    /// Clear any completed/done marker on the existing item.
    pub clear_done: bool,
    /// Clear any expired marker/expiry state on the existing item.
    pub clear_expired: bool,
    /// Clear product-owned broadcast/fanout state before re-enqueueing.
    ///
    /// Product adapters should treat this as covering `broadcasted_at`,
    /// broadcast claim token/worker fields, claim expiry, and equivalent
    /// product-local state that would otherwise prevent fanout after reopen.
    pub clear_broadcast_state: bool,
}

impl NotificationReopenEffects {
    /// Construct the standard reopen effects for an already-computed next
    /// delivery version.
    #[must_use]
    pub const fn from_next_delivery_version(next_delivery_version: DeliveryVersion) -> Self {
        Self {
            next_delivery_version,
            clear_read: true,
            clear_dismissed: true,
            clear_done: true,
            clear_expired: true,
            clear_broadcast_state: true,
        }
    }

    /// Construct the standard reopen effects for a current delivery version.
    ///
    /// # Errors
    ///
    /// Returns [`notification_core::NotificationError`] if the next delivery
    /// version would overflow.
    pub fn new(
        current_delivery_version: DeliveryVersion,
    ) -> Result<Self, notification_core::NotificationError> {
        Ok(Self::from_next_delivery_version(
            current_delivery_version.next()?,
        ))
    }
}

/// Decision for a recurring notification with a product-owned dedupe key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecurrenceDecision {
    /// Update the current open row without changing delivery version.
    UpdateOpenRow,
    /// Reopen the same row and use the next delivery version.
    Reopen {
        /// Next delivery version to persist.
        next_delivery_version: DeliveryVersion,
    },
    /// Create a new notification row.
    CreateNew,
}

impl RecurrenceDecision {
    /// Return standard reopen effects for a reopen decision.
    #[must_use]
    pub const fn reopen_effects(self) -> Option<NotificationReopenEffects> {
        match self {
            Self::Reopen {
                next_delivery_version,
            } => Some(NotificationReopenEffects::from_next_delivery_version(
                next_delivery_version,
            )),
            Self::UpdateOpenRow | Self::CreateNew => None,
        }
    }
}

/// Decide what to do when an event recurs.
///
/// # Errors
///
/// Returns [`notification_core::NotificationError`] if the next delivery
/// version would overflow the positive i32 version type.
pub fn recurrence_decision(
    has_dedupe_key: bool,
    current_state: Option<NotificationItemState>,
    current_delivery_version: DeliveryVersion,
) -> Result<RecurrenceDecision, notification_core::NotificationError> {
    let Some(state) = current_state else {
        return Ok(RecurrenceDecision::CreateNew);
    };
    if !has_dedupe_key {
        return Ok(RecurrenceDecision::CreateNew);
    }
    if state.should_reopen_on_recurrence() {
        return Ok(RecurrenceDecision::Reopen {
            next_delivery_version: current_delivery_version.next()?,
        });
    }
    Ok(RecurrenceDecision::UpdateOpenRow)
}

/// Relationship between a delivery row version and current notification item version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeliveryVersionStatus {
    /// Delivery row matches the current item version.
    Current,
    /// Delivery row is older than the current item version and should not send.
    Stale,
    /// Delivery row is newer than the item version, which indicates product drift.
    Future,
}

/// Compare delivery row and item versions.
#[must_use]
pub fn classify_delivery_version(
    row_version: DeliveryVersion,
    item_version: DeliveryVersion,
) -> DeliveryVersionStatus {
    match row_version.as_i32().cmp(&item_version.as_i32()) {
        std::cmp::Ordering::Less => DeliveryVersionStatus::Stale,
        std::cmp::Ordering::Equal => DeliveryVersionStatus::Current,
        std::cmp::Ordering::Greater => DeliveryVersionStatus::Future,
    }
}

/// Broadcast claim outcome vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BroadcastClaimOutcome {
    /// No rows were ready to claim.
    Noop,
    /// Rows were claimed and fanout should be attempted.
    Claimed {
        /// Claimed row count.
        count: u32,
    },
    /// Fanout completed and rows may be marked broadcasted.
    Broadcasted {
        /// Broadcasted row count.
        count: u32,
    },
    /// Fanout failed and rows should remain retryable or be released locally.
    RetryableFailure,
}

impl BroadcastClaimOutcome {
    /// Return whether broadcast work has reached a final local outcome.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Noop | Self::Broadcasted { .. } | Self::RetryableFailure
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recurring_read_notification_reopens_with_next_version() {
        let decision = recurrence_decision(
            true,
            Some(NotificationItemState::Read),
            DeliveryVersion::new(2).unwrap(),
        )
        .unwrap();
        assert_eq!(
            decision,
            RecurrenceDecision::Reopen {
                next_delivery_version: DeliveryVersion::new(3).unwrap()
            }
        );
        assert_eq!(
            decision.reopen_effects(),
            Some(NotificationReopenEffects {
                next_delivery_version: DeliveryVersion::new(3).unwrap(),
                clear_read: true,
                clear_dismissed: true,
                clear_done: true,
                clear_expired: true,
                clear_broadcast_state: true,
            })
        );
    }

    #[test]
    fn recurring_expired_notification_reopens_and_clears_expired_state() {
        let decision = recurrence_decision(
            true,
            Some(NotificationItemState::Expired),
            DeliveryVersion::new(4).unwrap(),
        )
        .unwrap();
        assert_eq!(
            decision,
            RecurrenceDecision::Reopen {
                next_delivery_version: DeliveryVersion::new(5).unwrap()
            }
        );
        assert_eq!(
            decision.reopen_effects(),
            Some(NotificationReopenEffects {
                next_delivery_version: DeliveryVersion::new(5).unwrap(),
                clear_read: true,
                clear_dismissed: true,
                clear_done: true,
                clear_expired: true,
                clear_broadcast_state: true,
            })
        );
    }

    #[test]
    fn item_state_can_be_derived_from_product_flags() {
        assert_eq!(
            NotificationItemState::from_flags(false, false, false, false),
            NotificationItemState::Unread
        );
        assert_eq!(
            NotificationItemState::from_flags(true, false, false, false),
            NotificationItemState::Read
        );
        assert_eq!(
            NotificationItemState::from_flags(true, true, false, false),
            NotificationItemState::Dismissed
        );
        assert_eq!(
            NotificationItemState::from_flags(true, true, true, true),
            NotificationItemState::Expired
        );
    }

    #[test]
    fn stale_delivery_versions_do_not_send() {
        assert_eq!(
            classify_delivery_version(
                DeliveryVersion::new(1).unwrap(),
                DeliveryVersion::new(2).unwrap()
            ),
            DeliveryVersionStatus::Stale
        );
    }
}
