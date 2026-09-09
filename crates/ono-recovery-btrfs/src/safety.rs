//! §56.2's ten facts, as things the implementation proves rather than assumes.
//!
//! §56.2 lists ten facts a Btrfs implementation MUST prove before destructive recovery, and §56.3
//! decides what happens when one of them cannot be proven: *"If any critical recovery fact cannot
//! be established, destructive recovery MUST be blocked rather than guessed."*
//!
//! The checklist is therefore built the fail-closed way round. A [`SafetyChecklist`] starts with
//! every fact outstanding, and each one moves to established only when a query answered it —
//! [`SafetyChecklist::establish`] takes the evidence, which is the text of what `btrfs`, the
//! mount table or the boundary arithmetic actually said. A fact nobody looked at stays
//! outstanding and blocks, so forgetting to check is indistinguishable from checking and failing.
//! That is deliberate: the two are equally uninformed.
//!
//! Evidence is kept rather than reduced to a boolean because §24.4 and Appendix B.10 both show
//! recovery facts to the operator before execution, and "the nested subvolume boundaries were
//! established" is not something a person can act on. "260 `@var/lib-app` is nested inside 258
//! `@var`" is.

use std::sync::Arc;

use ono_value::ErrorValue;

use crate::error::fact_not_established;

/// One of the ten facts §56.2 requires a Btrfs recovery to prove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SafetyFact {
    /// The exact filesystem and subvolume ID (§56.2, §14.1).
    FilesystemAndSubvolumeId,
    /// The target snapshot exists and is valid (§56.2, §11.4).
    SnapshotExistsAndValid,
    /// The nested subvolume boundaries (§56.2, §14.3).
    NestedBoundaries,
    /// Whether a selected restore is possible (§56.2, Appendix C.1).
    SelectiveRestorePossible,
    /// Whether subvolume replacement is required (§56.2, §14.4).
    SubvolumeReplacementRequired,
    /// The default-subvolume and boot impact (§56.2, Appendix D.9).
    DefaultSubvolumeAndBootImpact,
    /// The mount and reboot requirement (§56.2, §14.6).
    MountAndRebootRequirement,
    /// Later files and state that would be discarded (§56.2, Appendix C.3).
    LaterStateDiscarded,
    /// Correct treatment of the read-only snapshot (§56.2, §14.5).
    ReadOnlySnapshotTreatment,
    /// That nothing assumed recursive snapshot coverage (§56.2, §14.3).
    NoRecursiveCoverageAssumption,
}

impl SafetyFact {
    /// All ten, in the order §56.2 lists them.
    pub const ALL: [SafetyFact; 10] = [
        SafetyFact::FilesystemAndSubvolumeId,
        SafetyFact::SnapshotExistsAndValid,
        SafetyFact::NestedBoundaries,
        SafetyFact::SelectiveRestorePossible,
        SafetyFact::SubvolumeReplacementRequired,
        SafetyFact::DefaultSubvolumeAndBootImpact,
        SafetyFact::MountAndRebootRequirement,
        SafetyFact::LaterStateDiscarded,
        SafetyFact::ReadOnlySnapshotTreatment,
        SafetyFact::NoRecursiveCoverageAssumption,
    ];

    /// The stable token a caller can match on.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            SafetyFact::FilesystemAndSubvolumeId => "filesystem-and-subvolume-id",
            SafetyFact::SnapshotExistsAndValid => "snapshot-exists-and-valid",
            SafetyFact::NestedBoundaries => "nested-subvolume-boundaries",
            SafetyFact::SelectiveRestorePossible => "selective-restore-possible",
            SafetyFact::SubvolumeReplacementRequired => "subvolume-replacement-required",
            SafetyFact::DefaultSubvolumeAndBootImpact => "default-subvolume-and-boot-impact",
            SafetyFact::MountAndRebootRequirement => "mount-and-reboot-requirement",
            SafetyFact::LaterStateDiscarded => "later-state-discarded",
            SafetyFact::ReadOnlySnapshotTreatment => "read-only-snapshot-treatment",
            SafetyFact::NoRecursiveCoverageAssumption => "no-recursive-coverage-assumption",
        }
    }

    /// The fact as §56.2 words it, which is what a refusal names.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            SafetyFact::FilesystemAndSubvolumeId => "the exact filesystem and subvolume ID",
            SafetyFact::SnapshotExistsAndValid => "whether the target snapshot exists and is valid",
            SafetyFact::NestedBoundaries => "the nested subvolume boundaries",
            SafetyFact::SelectiveRestorePossible => "whether a selected restore is possible",
            SafetyFact::SubvolumeReplacementRequired => "whether subvolume replacement is required",
            SafetyFact::DefaultSubvolumeAndBootImpact => "the default-subvolume and boot impact",
            SafetyFact::MountAndRebootRequirement => "the mount and reboot requirement",
            SafetyFact::LaterStateDiscarded => "the later files and state that would be discarded",
            SafetyFact::ReadOnlySnapshotTreatment => {
                "the correct treatment of the read-only snapshot"
            }
            SafetyFact::NoRecursiveCoverageAssumption => {
                "that no recursive snapshot coverage was assumed"
            }
        }
    }

    /// The fact a token names.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        SafetyFact::ALL
            .into_iter()
            .find(|fact| fact.token() == token)
    }
}

/// What a recovery has proven, and what it has not (§56.2, §56.3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SafetyChecklist {
    established: Vec<(SafetyFact, Arc<str>)>,
    outstanding: Vec<(SafetyFact, Arc<str>)>,
    examined: Vec<SafetyFact>,
}

impl SafetyChecklist {
    /// A checklist on which nothing has been established yet.
    ///
    /// Every one of the ten starts outstanding with the same reason, so a fact that no query ever
    /// touched blocks exactly as loudly as one whose query failed (§56.3).
    #[must_use]
    pub fn outstanding() -> Self {
        Self {
            established: Vec::new(),
            outstanding: SafetyFact::ALL
                .into_iter()
                .map(|fact| (fact, Arc::from("nothing has established this yet")))
                .collect(),
            examined: Vec::new(),
        }
    }

    /// Records that `fact` was established, and what established it.
    pub fn establish(&mut self, fact: SafetyFact, evidence: impl Into<Arc<str>>) {
        self.examined.retain(|examined| *examined != fact);
        self.outstanding.retain(|(pending, _)| *pending != fact);
        self.established.retain(|(known, _)| *known != fact);
        self.established.push((fact, evidence.into()));
    }

    /// Records that `fact` could not be established, and why (§56.3).
    pub fn block(&mut self, fact: SafetyFact, reason: impl Into<Arc<str>>) {
        if !self.examined.contains(&fact) {
            self.examined.push(fact);
        }
        self.established.retain(|(known, _)| *known != fact);
        self.outstanding.retain(|(pending, _)| *pending != fact);
        self.outstanding.push((fact, reason.into()));
    }

    /// The facts that were established, with their evidence.
    #[must_use]
    pub fn established(&self) -> &[(SafetyFact, Arc<str>)] {
        &self.established
    }

    /// The facts that were not, with the reason each is outstanding.
    #[must_use]
    pub fn missing(&self) -> &[(SafetyFact, Arc<str>)] {
        &self.outstanding
    }

    /// Whether `fact` was established.
    #[must_use]
    pub fn is_established(&self, fact: SafetyFact) -> bool {
        self.established.iter().any(|(known, _)| *known == fact)
    }

    /// The evidence recorded for `fact`, established or not.
    #[must_use]
    pub fn evidence(&self, fact: SafetyFact) -> Option<&str> {
        self.established
            .iter()
            .chain(self.outstanding.iter())
            .find(|(subject, _)| *subject == fact)
            .map(|(_, evidence)| evidence.as_ref())
    }

    /// Whether all ten of §56.2's facts are established.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.outstanding.is_empty()
    }

    /// Whether a query looked at `fact` and could not establish it (§56.3).
    ///
    /// A fact nothing looked at blocks just as hard, and the difference matters only to the
    /// wording of the refusal: naming the fact a query actually failed on is more use to a person
    /// than naming the first one in the list that a short-circuit never reached.
    #[must_use]
    pub fn was_examined(&self, fact: SafetyFact) -> bool {
        self.examined.contains(&fact)
    }

    /// The refusal §56.3 requires when a fact is outstanding.
    ///
    /// One fact is named, because a refusal that lists ten things is one a person reads as
    /// "something went wrong" rather than as "this specific fact is unknown". It is the first, in
    /// §56.2's own order, that a query looked at and could not establish; where a short-circuit
    /// meant no query ran at all, it is the first outstanding one. The rest travel in the message.
    #[must_use]
    pub fn refusal(&self) -> Option<ErrorValue> {
        let pick = |wanted_examined: bool| {
            SafetyFact::ALL.into_iter().find_map(|fact| {
                if self.examined.contains(&fact) != wanted_examined {
                    return None;
                }
                self.outstanding
                    .iter()
                    .find(|(pending, _)| *pending == fact)
                    .map(|(pending, reason)| (*pending, Arc::clone(reason)))
            })
        };
        let first = pick(true).or_else(|| pick(false))?;
        let (fact, reason) = first;
        let others: Vec<&str> = SafetyFact::ALL
            .into_iter()
            .filter(|candidate| *candidate != fact)
            .filter(|candidate| !self.is_established(*candidate))
            .map(SafetyFact::description)
            .collect();
        let detail = if others.is_empty() {
            format!("{reason}. Nothing was changed")
        } else {
            format!(
                "{reason}. Also outstanding: {}. Nothing was changed",
                others.join("; ")
            )
        };
        Some(fact_not_established(fact, &detail))
    }
}
