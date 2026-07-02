//! Product-neutral operator dead-letter and recovery report vocabulary.
//!
//! This crate names stuck-work states and operator outcomes. It does not decide
//! product policy, execute SQL, expose admin routes, or choose whether a
//! particular item should be retried, completed, failed, or manually reconciled.

/// Reason a work item needs operator or recovery attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeadLetterReason {
    /// Retry policy was exhausted.
    ExhaustedRetries,
    /// A worker lease expired or became stale.
    StaleLease,
    /// A provider side effect may have happened but local persistence is unclear.
    AmbiguousAfterSideEffect,
    /// A required external or internal dependency was unavailable.
    MissingDependency,
    /// A provider rejected the request permanently.
    ProviderRejected,
    /// Product invariants did not hold.
    InvariantViolation,
    /// Product-specific reason not modeled by this crate.
    ProductSpecific,
}

/// Operator action requested or applied for a dead-letter item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OperatorAction {
    /// Retry through the normal product pipeline.
    Retry,
    /// Mark work complete because the product can prove the side effect happened.
    MarkComplete,
    /// Mark work failed and stop retrying.
    MarkFailed,
    /// Escalate for manual reconciliation before further automated mutation.
    RequireManualReconciliation,
    /// Leave unchanged.
    Noop,
}

impl OperatorAction {
    /// Return whether the action requires a human decision before automated retry.
    #[must_use]
    pub const fn requires_manual_reconciliation(self) -> bool {
        matches!(self, Self::RequireManualReconciliation)
    }
}

/// Product-neutral dead-letter item envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadLetterItem<Id> {
    /// Product-owned item identifier.
    pub id: Id,
    /// Reason the item is in the dead-letter/recovery surface.
    pub reason: DeadLetterReason,
    /// Number of attempts already recorded by product persistence.
    pub attempts: u32,
    /// Optional operator-facing detail. Products must redact secrets before
    /// constructing this value.
    pub detail: Option<String>,
}

impl<Id> DeadLetterItem<Id> {
    /// Return whether this item should be visible in an operator attention
    /// surface.
    #[must_use]
    pub const fn needs_attention(&self) -> bool {
        true
    }
}

/// Result of attempting one operator action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorActionResult<Id> {
    /// Product-owned item identifier.
    pub id: Id,
    /// Action attempted.
    pub action: OperatorAction,
    /// Whether product persistence changed.
    pub changed: bool,
    /// Operator warning, if any.
    pub warning: Option<String>,
}

/// Summary of an operator audit/repair pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OperatorRunReport {
    /// Items inspected.
    pub inspected: usize,
    /// Items eligible for action.
    pub eligible: usize,
    /// Items changed.
    pub changed: usize,
    /// Whether the pass intentionally avoided mutations.
    pub dry_run: bool,
    /// Operator-visible warnings.
    pub warnings: Vec<String>,
}

impl OperatorRunReport {
    /// Construct a dry-run report.
    #[must_use]
    pub fn dry_run() -> Self {
        Self {
            dry_run: true,
            ..Self::default()
        }
    }

    /// Construct an apply-mode report.
    #[must_use]
    pub fn apply() -> Self {
        Self::default()
    }

    /// Record a per-item result.
    pub fn record<Id>(&mut self, result: &OperatorActionResult<Id>) {
        self.inspected = self.inspected.saturating_add(1);
        if !matches!(result.action, OperatorAction::Noop) {
            self.eligible = self.eligible.saturating_add(1);
        }
        if result.changed {
            self.changed = self.changed.saturating_add(1);
        }
        if let Some(warning) = &result.warning {
            self.warnings.push(warning.clone());
        }
    }

    /// Return whether the report has warnings or eligible unchanged work.
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        !self.warnings.is_empty() || (self.eligible > 0 && self.changed < self.eligible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_tracks_operator_results() {
        let mut report = OperatorRunReport::dry_run();
        assert!(report.dry_run);
        report.record(&OperatorActionResult {
            id: 1,
            action: OperatorAction::RequireManualReconciliation,
            changed: false,
            warning: Some("ambiguous provider state".to_owned()),
        });
        assert_eq!(report.inspected, 1);
        assert_eq!(report.eligible, 1);
        assert!(report.needs_attention());
    }

    #[test]
    fn actions_items_and_apply_reports_expose_attention_semantics() {
        assert!(OperatorAction::RequireManualReconciliation.requires_manual_reconciliation());
        assert!(!OperatorAction::Retry.requires_manual_reconciliation());
        let item = DeadLetterItem {
            id: "delivery-1",
            reason: DeadLetterReason::AmbiguousAfterSideEffect,
            attempts: 3,
            detail: Some("provider accepted before timeout".to_owned()),
        };
        assert!(item.needs_attention());

        let mut report = OperatorRunReport::apply();
        assert!(!report.dry_run);
        report.record(&OperatorActionResult {
            id: "delivery-1",
            action: OperatorAction::Retry,
            changed: true,
            warning: None,
        });
        report.record(&OperatorActionResult {
            id: "delivery-2",
            action: OperatorAction::Noop,
            changed: false,
            warning: None,
        });
        assert_eq!(report.inspected, 2);
        assert_eq!(report.eligible, 1);
        assert_eq!(report.changed, 1);
        assert!(!report.needs_attention());
    }

    #[test]
    fn report_counters_saturate_at_maximum() {
        let mut report = OperatorRunReport {
            inspected: usize::MAX,
            eligible: usize::MAX,
            changed: usize::MAX,
            ..OperatorRunReport::default()
        };
        report.record(&OperatorActionResult {
            id: "delivery-1",
            action: OperatorAction::Retry,
            changed: true,
            warning: None,
        });

        assert_eq!(report.inspected, usize::MAX);
        assert_eq!(report.eligible, usize::MAX);
        assert_eq!(report.changed, usize::MAX);
    }
}
