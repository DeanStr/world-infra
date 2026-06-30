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
    /// Return whether a recurring deduped notification should reopen.
    #[must_use]
    pub const fn should_reopen_on_recurrence(self) -> bool {
        matches!(
            self,
            Self::Read | Self::Dismissed | Self::Done | Self::Expired
        )
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
        let next = current_delivery_version
            .as_i32()
            .checked_add(1)
            .ok_or(notification_core::NotificationError::InvalidDeliveryVersion)?;
        return Ok(RecurrenceDecision::Reopen {
            next_delivery_version: DeliveryVersion::new(next)?,
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
