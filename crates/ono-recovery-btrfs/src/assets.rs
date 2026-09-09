//! The multi-subvolume set, and what it is allowed to claim (Appendix D.7).
//!
//! When a plan spans several subvolumes the protection engine ends up holding several snapshots,
//! and Appendix D.7 is unusually direct about what may then be said of them:
//!
//! > The set MUST record that its member snapshots were created sequentially unless a higher-level
//! > mechanism can prove a common atomic point. Ono MUST not invent cross-subvolume atomicity.
//!
//! Each individual `btrfs subvolume snapshot` is atomic for its own subvolume, so a member is
//! [`ConsistencyClass::FilesystemConsistent`] on its own. Two of them taken one after the other
//! are not a filesystem-consistent picture of both: a write that landed in `@var` between the
//! two snapshots is in one and not the other, which is exactly what
//! [`ConsistencyClass::CrashConsistent`] describes. [`RecoveryAssetSet::composed_consistency`]
//! computes that rather than storing it, so no caller can set the stronger word.
//!
//! [`ProtectionShortfall`] is Appendix F.1's other half. If the fourth snapshot succeeds and the
//! fifth fails, no plan target is mutated and the four already created are *retained* until
//! somebody decides about cleanup — so the failure carries them rather than dropping them on the
//! floor, and the refusal it raises is the original prepare failure rather than a cleanup one.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{ConsistencyClass, RecoveryAsset};
use ono_value::ErrorValue;

/// Whether anything proved the members share one point in time (Appendix D.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetAtomicity {
    /// The members were created one after another, and nothing proves a common point.
    ///
    /// This is the only value this provider ever produces for itself. Btrfs has no cross-subvolume
    /// snapshot transaction, and Appendix D.7 forbids inventing one.
    Sequential,
    /// A higher-level mechanism proved a common atomic point, and named itself.
    ///
    /// Nothing in v0.6's Btrfs provider can set this; it exists so that a mechanism which really
    /// can — a storage array's consistency group, an application quiesce spanning both subvolumes
    /// (§39.3) — has somewhere truthful to say so.
    ProvenCommonPoint {
        /// What proved it, in a sentence naming the mechanism.
        proof: Arc<str>,
    },
}

impl SetAtomicity {
    /// Whether the members were created sequentially (Appendix D.7).
    #[must_use]
    pub const fn is_sequential(&self) -> bool {
        matches!(self, SetAtomicity::Sequential)
    }

    /// What proved a common point, where anything did.
    #[must_use]
    pub fn proof(&self) -> Option<&str> {
        match self {
            SetAtomicity::Sequential => None,
            SetAtomicity::ProvenCommonPoint { proof } => Some(proof),
        }
    }
}

/// The snapshots one plan's protection created across several subvolumes (Appendix D.7).
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryAssetSet {
    members: Vec<RecoveryAsset>,
    atomicity: SetAtomicity,
}

impl RecoveryAssetSet {
    /// A set whose members were created one after another (Appendix D.7).
    #[must_use]
    pub const fn sequential(members: Vec<RecoveryAsset>) -> Self {
        Self {
            members,
            atomicity: SetAtomicity::Sequential,
        }
    }

    /// A set some other mechanism proved a common atomic point for (Appendix D.7).
    #[must_use]
    pub fn with_proven_common_point(
        members: Vec<RecoveryAsset>,
        proof: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            members,
            atomicity: SetAtomicity::ProvenCommonPoint {
                proof: proof.into(),
            },
        }
    }

    /// The member snapshots, in the order they were created.
    #[must_use]
    pub fn members(&self) -> &[RecoveryAsset] {
        &self.members
    }

    /// How the members relate in time (Appendix D.7).
    #[must_use]
    pub const fn atomicity(&self) -> &SetAtomicity {
        &self.atomicity
    }

    /// Whether the set records sequential creation, which is what Appendix D.7 requires of it.
    #[must_use]
    pub const fn is_sequential(&self) -> bool {
        self.atomicity.is_sequential()
    }

    /// How many subvolumes the set spans (§14.3).
    #[must_use]
    pub fn span(&self) -> usize {
        self.members.len()
    }

    /// Each member's own creation instant, as the filesystem recorded it (Appendix D.7).
    #[must_use]
    pub fn creation_instants(&self) -> Vec<Timestamp> {
        self.members
            .iter()
            .map(RecoveryAsset::created_at)
            .collect()
    }

    /// The consistency the whole set may claim (§11.3, Appendix D.7).
    ///
    /// Two rules compose it, and neither can be overridden. A set is never stronger than its
    /// weakest member, and a set of more than one member created sequentially is never stronger
    /// than crash-consistent — because between the first snapshot and the second, the system went
    /// on writing.
    #[must_use]
    pub fn composed_consistency(&self) -> ConsistencyClass {
        let weakest = self
            .members
            .iter()
            .map(RecoveryAsset::consistency)
            .reduce(ConsistencyClass::weakest_of)
            .unwrap_or(ConsistencyClass::Unknown);
        if self.members.len() > 1 && self.atomicity.is_sequential() {
            weakest.weakest_of(ConsistencyClass::CrashConsistent)
        } else {
            weakest
        }
    }

    /// The sentence describing what the set is, for the plan view (§24.4, Appendix D.7).
    #[must_use]
    pub fn detail(&self) -> String {
        match &self.atomicity {
            SetAtomicity::Sequential if self.members.len() > 1 => format!(
                "{} subvolume snapshots created one after another; no mechanism proves a common \
                 point in time across them, so the set is {} taken as a whole",
                self.members.len(),
                self.composed_consistency().as_str()
            ),
            SetAtomicity::Sequential => format!(
                "one subvolume snapshot, {}",
                self.composed_consistency().as_str()
            ),
            SetAtomicity::ProvenCommonPoint { proof } => format!(
                "{} subvolume snapshots sharing a common point proved by {proof}",
                self.members.len()
            ),
        }
    }
}

/// A protection run that created some of its snapshots and then failed (Appendix F.1).
#[derive(Debug, Clone, PartialEq)]
pub struct ProtectionShortfall {
    created: Vec<RecoveryAsset>,
    failed_scope: Arc<str>,
    error: ErrorValue,
}

impl ProtectionShortfall {
    /// Records that `created` exist, and that protecting `failed_scope` failed with `error`.
    #[must_use]
    pub fn new(
        created: Vec<RecoveryAsset>,
        failed_scope: impl Into<Arc<str>>,
        error: ErrorValue,
    ) -> Self {
        Self {
            created,
            failed_scope: failed_scope.into(),
            error,
        }
    }

    /// The snapshots that were created before the failure, and still exist (Appendix F.1).
    ///
    /// They are retained rather than removed. Appendix F.1 permits automatic cleanup only when
    /// three conditions hold — created solely for this prepare, cleanup itself safe and
    /// provider-declared, no quiesce or recovery dependency requiring retention — and a provider
    /// cannot know the third. So the decision belongs to the caller, and the assets travel to it.
    #[must_use]
    pub fn retained(&self) -> &[RecoveryAsset] {
        &self.created
    }

    /// The subvolume whose snapshot failed.
    #[must_use]
    pub fn failed_scope(&self) -> &str {
        &self.failed_scope
    }

    /// The original prepare failure (Appendix F.1).
    ///
    /// Appendix F.1's last line — *"Cleanup failure must not obscure the original prepare
    /// failure"* — is why this is the error the caller sees, whatever a later cleanup does.
    #[must_use]
    pub const fn error(&self) -> &ErrorValue {
        &self.error
    }

    /// The failure as a plain error, for a caller that has already dealt with the retained assets.
    #[must_use]
    pub fn into_error(self) -> ErrorValue {
        self.error
    }
}
