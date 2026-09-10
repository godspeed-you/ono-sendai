//! §56.1's destructive-recovery safety checklist, as twelve facts a plan must prove.
//!
//! §56 opens with the reason: *"Because storage rollback can cause severe data loss, the
//! following checklist is mandatory before enabling a first-party destructive recovery path."*
//! §56.1 then lists twelve things the implementation MUST prove for ZFS. This module is that list
//! as a value: [`ZfsFact`] names each one, [`SafetyChecklist`] records for each whether the
//! provider established it and out of what evidence, and [`SafetyChecklist::blocking_error`] is
//! §56.3 — *"If any critical recovery fact cannot be established, destructive recovery MUST be
//! blocked rather than guessed"* — expressed once rather than at twelve call sites.
//!
//! The direction of the default matters more than anything else here. A fact starts *unproven*:
//! `ZfsProvider` has to hand each one its evidence, and a query that failed, returned nothing, or
//! returned something this provider does not recognise leaves the fact where it started. There is
//! no constructor that marks the checklist complete, so a forgotten branch fails closed.

use std::sync::Arc;

use ono_change_core::error::recovery_plan_incomplete;
use ono_value::{ErrorValue, Value};

/// One of the twelve facts §56.1 requires before a destructive ZFS recovery path is enabled.
///
/// The order is §56.1's own, and [`ZfsFact::ALL`] preserves it so a refusal names the first
/// missing fact in the order an operator reading the specification would look for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ZfsFact {
    /// "exact dataset identity" — which dataset, by name, as ZFS itself reports it.
    DatasetIdentity,
    /// "exact snapshot identity" — the snapshot's GUID, not merely a name that looks right.
    SnapshotIdentity,
    /// "target snapshot still exists" — checked now, rather than when the asset was created.
    SnapshotExists,
    /// "whether it is the latest relevant snapshot" — which decides whether history is destroyed.
    LatestRelevantSnapshot,
    /// "all newer snapshots/bookmarks affected by rollback" (Appendix D.5).
    NewerSnapshotsAndBookmarks,
    /// "all clones affected by destructive flags" (§13.6).
    AffectedClones,
    /// "child dataset boundaries" — §13.4's separate boundaries, which a rollback does not reach.
    ChildDatasetBoundaries,
    /// "mount/unmount requirement" (§13.7).
    MountRequirement,
    /// "reboot/offline requirement" (§13.7).
    RebootOrOfflineRequirement,
    /// "expected discarded live data" — Appendix D.5's changed-live-data estimate.
    DiscardedLiveData,
    /// "sufficient privilege" (§11.4, §43.4).
    SufficientPrivilege,
    /// "explicit acceptance for history destruction" (§13.6, §24.5).
    HistoryDestructionAccepted,
}

impl ZfsFact {
    /// The twelve facts, in the order §56.1 lists them.
    pub const ALL: [ZfsFact; 12] = [
        ZfsFact::DatasetIdentity,
        ZfsFact::SnapshotIdentity,
        ZfsFact::SnapshotExists,
        ZfsFact::LatestRelevantSnapshot,
        ZfsFact::NewerSnapshotsAndBookmarks,
        ZfsFact::AffectedClones,
        ZfsFact::ChildDatasetBoundaries,
        ZfsFact::MountRequirement,
        ZfsFact::RebootOrOfflineRequirement,
        ZfsFact::DiscardedLiveData,
        ZfsFact::SufficientPrivilege,
        ZfsFact::HistoryDestructionAccepted,
    ];

    /// The fact's stable token, which travels in a refusal's metadata.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ZfsFact::DatasetIdentity => "exact-dataset-identity",
            ZfsFact::SnapshotIdentity => "exact-snapshot-identity",
            ZfsFact::SnapshotExists => "target-snapshot-still-exists",
            ZfsFact::LatestRelevantSnapshot => "whether-it-is-the-latest-relevant-snapshot",
            ZfsFact::NewerSnapshotsAndBookmarks => "newer-snapshots-and-bookmarks",
            ZfsFact::AffectedClones => "clones-affected-by-destructive-flags",
            ZfsFact::ChildDatasetBoundaries => "child-dataset-boundaries",
            ZfsFact::MountRequirement => "mount-unmount-requirement",
            ZfsFact::RebootOrOfflineRequirement => "reboot-offline-requirement",
            ZfsFact::DiscardedLiveData => "expected-discarded-live-data",
            ZfsFact::SufficientPrivilege => "sufficient-privilege",
            ZfsFact::HistoryDestructionAccepted => "explicit-acceptance-for-history-destruction",
        }
    }

    /// The §56.1 bullet, in the specification's own words, for the refusal a person reads.
    #[must_use]
    pub const fn bullet(self) -> &'static str {
        match self {
            ZfsFact::DatasetIdentity => "exact dataset identity",
            ZfsFact::SnapshotIdentity => "exact snapshot identity",
            ZfsFact::SnapshotExists => "target snapshot still exists",
            ZfsFact::LatestRelevantSnapshot => "whether it is the latest relevant snapshot",
            ZfsFact::NewerSnapshotsAndBookmarks => {
                "all newer snapshots/bookmarks affected by rollback"
            }
            ZfsFact::AffectedClones => "all clones affected by destructive flags",
            ZfsFact::ChildDatasetBoundaries => "child dataset boundaries",
            ZfsFact::MountRequirement => "mount/unmount requirement",
            ZfsFact::RebootOrOfflineRequirement => "reboot/offline requirement",
            ZfsFact::DiscardedLiveData => "expected discarded live data",
            ZfsFact::SufficientPrivilege => "sufficient privilege",
            ZfsFact::HistoryDestructionAccepted => "explicit acceptance for history destruction",
        }
    }

    /// Whether this fact gates *any* recovery rather than only a destructive one.
    ///
    /// §56.1 governs destructive paths, and four of its facts govern every path: a snapshot whose
    /// identity, existence or readability could not be established cannot be read from either, so
    /// a selective file restore is refused for the same reason a rollback is.
    #[must_use]
    pub const fn gates_any_recovery(self) -> bool {
        matches!(
            self,
            ZfsFact::DatasetIdentity
                | ZfsFact::SnapshotIdentity
                | ZfsFact::SnapshotExists
                | ZfsFact::SufficientPrivilege
        )
    }
}

/// What one fact's evidence was, or why the provider could not establish it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactEvidence {
    fact: ZfsFact,
    established: bool,
    detail: Arc<str>,
}

impl FactEvidence {
    /// The fact.
    #[must_use]
    pub const fn fact(&self) -> ZfsFact {
        self.fact
    }

    /// Whether the provider established it.
    #[must_use]
    pub const fn is_established(&self) -> bool {
        self.established
    }

    /// The evidence, or the reason there is none.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// The twelve facts of §56.1 and what the provider could prove about each.
///
/// Built by [`SafetyChecklist::unproven`] and narrowed by [`SafetyChecklist::established`]. There
/// is no way to mark a fact proven without handing over the sentence that proves it, which is
/// what keeps §56.3's fail-closed direction a property of the type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyChecklist {
    subject: Arc<str>,
    facts: Vec<FactEvidence>,
}

impl SafetyChecklist {
    /// A checklist for `subject` in which nothing has been proven yet.
    #[must_use]
    pub fn unproven(subject: impl Into<Arc<str>>) -> Self {
        Self {
            subject: subject.into(),
            facts: ZfsFact::ALL
                .into_iter()
                .map(|fact| FactEvidence {
                    fact,
                    established: false,
                    detail: Arc::from("this provider did not establish it"),
                })
                .collect(),
        }
    }

    /// Records that `fact` was established, and out of what.
    #[must_use]
    pub fn established(mut self, fact: ZfsFact, detail: impl Into<Arc<str>>) -> Self {
        if let Some(entry) = self.facts.iter_mut().find(|entry| entry.fact == fact) {
            entry.established = true;
            entry.detail = detail.into();
        }
        self
    }

    /// Records that `fact` could not be established, and why.
    #[must_use]
    pub fn missing(mut self, fact: ZfsFact, detail: impl Into<Arc<str>>) -> Self {
        if let Some(entry) = self.facts.iter_mut().find(|entry| entry.fact == fact) {
            entry.established = false;
            entry.detail = detail.into();
        }
        self
    }

    /// Records `fact` as established when `evidence` is `Some`, and missing otherwise.
    #[must_use]
    pub fn establishing(
        self,
        fact: ZfsFact,
        evidence: Option<String>,
        absence: &'static str,
    ) -> Self {
        match evidence {
            Some(detail) => self.established(fact, detail),
            None => self.missing(fact, absence),
        }
    }

    /// What the checklist is about — the snapshot reference a recovery would use.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Every fact and its evidence, in §56.1's order.
    #[must_use]
    pub fn facts(&self) -> &[FactEvidence] {
        &self.facts
    }

    /// The evidence recorded for one fact.
    #[must_use]
    pub fn evidence(&self, fact: ZfsFact) -> Option<&FactEvidence> {
        self.facts.iter().find(|entry| entry.fact == fact)
    }

    /// Whether `fact` was established.
    #[must_use]
    pub fn is_established(&self, fact: ZfsFact) -> bool {
        self.evidence(fact)
            .is_some_and(FactEvidence::is_established)
    }

    /// The facts that were not established, in §56.1's order.
    #[must_use]
    pub fn unestablished(&self) -> Vec<ZfsFact> {
        self.facts
            .iter()
            .filter(|entry| !entry.established)
            .map(|entry| entry.fact)
            .collect()
    }

    /// Whether all twelve were established, which is what §56.1 requires of a destructive path.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.facts.iter().all(|entry| entry.established)
    }

    /// The refusal §56.3 requires when a fact needed for `destructive` recovery is missing.
    ///
    /// The headline names the first missing fact in §56.1's order and the `facts` metadata names
    /// every one of them, so an operator sees the whole shortfall rather than fixing one query at
    /// a time. A non-destructive recovery is held only to the four facts every path needs.
    #[must_use]
    pub fn blocking_error(&self, destructive: bool) -> Option<ErrorValue> {
        self.blocking_error_deferring(destructive, &[])
    }

    /// §56.3's refusal, with `deferred` facts left to a check that comes later.
    ///
    /// The one fact a plan is built without is the operator's acceptance: §24.5 gives it at
    /// apply, after the plan it covers was shown. A recovery plan therefore defers
    /// [`ZfsFact::HistoryDestructionAccepted`] — provided what it covers was enumerated — and the
    /// act that destroys history is held to all twelve.
    #[must_use]
    pub fn blocking_error_deferring(
        &self,
        destructive: bool,
        deferred: &[ZfsFact],
    ) -> Option<ErrorValue> {
        let missing: Vec<ZfsFact> = self
            .unestablished()
            .into_iter()
            .filter(|fact| destructive || fact.gates_any_recovery())
            .filter(|fact| !deferred.contains(fact))
            .collect();
        let first = missing.first().copied()?;
        let detail = self
            .evidence(first)
            .map_or_else(String::new, |entry| entry.detail.to_string());
        Some(
            recovery_plan_incomplete(
                first.bullet(),
                &format!("The subject is {}. {detail}", self.subject),
            )
            .with_metadata(
                "facts",
                Value::list(missing.iter().map(|fact| Value::string(fact.as_str()))),
            )
            .with_metadata("subject", Value::string(&self.subject)),
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_start_with_every_fact_unproven() {
        let checklist = SafetyChecklist::unproven("tank/data@ono-b91c");
        assert_eq!(
            checklist.unestablished().len(),
            12,
            "§56.1: twelve facts, none of them true because nobody looked"
        );
        assert!(!checklist.is_complete());
    }

    #[test]
    fn should_block_a_destructive_path_while_any_fact_is_missing() {
        let checklist = SafetyChecklist::unproven("tank/data@ono-b91c")
            .established(ZfsFact::DatasetIdentity, "tank/data");
        let error = checklist
            .blocking_error(true)
            .expect("§56.3: a missing fact blocks destructive recovery");
        assert_eq!(error.code().name(), "recovery.plan_incomplete");
    }
}
