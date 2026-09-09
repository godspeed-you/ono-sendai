//! The newer-state conflict model (spec §24.2, Appendix C.3, Appendix C.4).
//!
//! Appendix C.3 is one sentence of algorithm — *"The RecoveryPlan computes changes since asset
//! creation within the candidate restore scope"* — and four words of vocabulary. [`analyse`] is
//! that sentence, and the four words are `NewerStateClass`.
//!
//! Three decisions in here are worth knowing before reading the code, because each is a place
//! where a different reading would silently lose data or silently gate the ordinary case.
//!
//! **The change the plan itself made is not newer state.** Appendix C.4's conflict is the *second*
//! edit: the plan wrote `nginx.conf` at 14:03, the user edited it again at 15:12, and it is the
//! 15:12 edit that recovery would discard. The 14:03 write is the thing recovery exists to undo,
//! so counting it as a loss would gate every recovery that ever worked and make §40.1 unreachable
//! for the normal case. [`ConflictRequest::applied_at`] is how the caller says when the original
//! plan finished; without it, every change to a restored object is treated as a conflict, which is
//! §56.3's direction.
//!
//! **Digest evidence outranks a timestamp.** An object rewritten with identical bytes has a newer
//! mtime and nothing to lose, so where both a captured digest and a current one are known, the
//! comparison decides. The timestamp answers only where no digest does.
//!
//! **Absence of evidence is a class of its own.** An object whose current state could not be
//! established is `NewerStateClass::Unknown` rather than assumed unchanged, because
//! `NewerStateClass::requires_acceptance` already answers `true` for it and §56.3 makes an
//! unestablished recovery fact a reason to block.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    DirectoryRestorePolicy, NewerStateClass, NewerStateImpact, NewerStateItem, RestoreMethod,
};
use ono_value::ByteSize;

/// What the world says about one object now (Appendix C.3).
///
/// The three cases are distinct on purpose. An object that is gone is evidence of a change; an
/// object nobody could look at is evidence of nothing, and §56.3 is the difference between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservedState {
    /// The object is there, and this is the digest of what it holds now.
    Present {
        /// When it last changed, where the observer could establish that.
        changed_at: Option<Timestamp>,
        /// The digest of its current content.
        digest: Arc<str>,
    },
    /// The object is not there any more, and that was established.
    Absent {
        /// When it went away, where the observer could establish that.
        changed_at: Option<Timestamp>,
    },
    /// Its current state could not be established, which §56.3 gates on rather than guessing.
    Unestablished {
        /// Why the observer could not answer — a permission denial, a link that was down.
        reason: Arc<str>,
    },
}

impl ObservedState {
    /// The object holds `digest` now, and when it last changed is not known.
    #[must_use]
    pub fn present(digest: impl Into<Arc<str>>) -> Self {
        Self::Present {
            changed_at: None,
            digest: digest.into(),
        }
    }

    /// The object holds `digest` now, and last changed at `at`.
    #[must_use]
    pub fn changed(at: Timestamp, digest: impl Into<Arc<str>>) -> Self {
        Self::Present {
            changed_at: Some(at),
            digest: digest.into(),
        }
    }

    /// The object was removed at `at`.
    #[must_use]
    pub const fn removed(at: Timestamp) -> Self {
        Self::Absent {
            changed_at: Some(at),
        }
    }

    /// The observer could not answer, for `reason` (§56.3).
    #[must_use]
    pub fn unestablished(reason: impl Into<Arc<str>>) -> Self {
        Self::Unestablished {
            reason: reason.into(),
        }
    }

    /// When the object last changed, where that was established.
    #[must_use]
    pub const fn changed_at(&self) -> Option<Timestamp> {
        match self {
            ObservedState::Present { changed_at, .. } | ObservedState::Absent { changed_at } => {
                *changed_at
            }
            ObservedState::Unestablished { .. } => None,
        }
    }

    /// The digest of the current content, where there is content and it was read.
    #[must_use]
    pub fn digest(&self) -> Option<&str> {
        match self {
            ObservedState::Present { digest, .. } => Some(digest),
            ObservedState::Absent { .. } | ObservedState::Unestablished { .. } => None,
        }
    }
}

/// One object of the candidate restore scope, as it stands now (Appendix C.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectObservation {
    object: Arc<str>,
    state: ObservedState,
}

impl ObjectObservation {
    /// Records that `object` is in `state`.
    #[must_use]
    pub fn new(object: impl Into<Arc<str>>, state: ObservedState) -> Self {
        Self {
            object: object.into(),
            state,
        }
    }

    /// The object.
    #[must_use]
    pub fn object(&self) -> &str {
        &self.object
    }

    /// What the world says about it.
    #[must_use]
    pub const fn state(&self) -> &ObservedState {
        &self.state
    }
}

/// Everything [`analyse`] needs, and nothing it could read for itself (Appendix C.3).
///
/// The observations arrive as values rather than as a filesystem, which is what makes Appendix
/// C.4's worked example a test rather than a staging exercise.
#[derive(Debug, Clone)]
pub struct ConflictRequest<'a> {
    method: RestoreMethod,
    captured_at: Timestamp,
    applied_at: Option<Timestamp>,
    observations: &'a [ObjectObservation],
    captured: Vec<(Arc<str>, Arc<str>)>,
    restore_set: Vec<Arc<str>>,
    directory_policy: DirectoryRestorePolicy,
    destroyed_assets: Vec<Arc<str>>,
    discarded_bytes: Option<ByteSize>,
}

impl<'a> ConflictRequest<'a> {
    /// An analysis of `observations` against an asset captured at `captured_at`, restored by
    /// `method`.
    #[must_use]
    pub const fn new(
        method: RestoreMethod,
        captured_at: Timestamp,
        observations: &'a [ObjectObservation],
    ) -> Self {
        Self {
            method,
            captured_at,
            applied_at: None,
            observations,
            captured: Vec::new(),
            restore_set: Vec::new(),
            directory_policy: DirectoryRestorePolicy::KeepExtraFiles,
            destroyed_assets: Vec::new(),
            discarded_bytes: None,
        }
    }

    /// Records when the plan being recovered from finished (Appendix C.4).
    ///
    /// A change to a restored object at or before this instant is the original plan's own work,
    /// which recovery exists to undo. A change after it is somebody else's, and that is the
    /// conflict Appendix C.4 requires to be shown at target level.
    #[must_use]
    pub const fn applied_at(mut self, at: Timestamp) -> Self {
        self.applied_at = Some(at);
        self
    }

    /// Names an object the recovery would put back (§24.4's "restore").
    #[must_use]
    pub fn restoring(mut self, object: impl Into<Arc<str>>) -> Self {
        self.restore_set.push(object.into());
        self
    }

    /// Records the digest the asset holds for `object` (§18.3's captured state).
    #[must_use]
    pub fn captured(mut self, object: impl Into<Arc<str>>, digest: impl Into<Arc<str>>) -> Self {
        self.captured.push((object.into(), digest.into()));
        self
    }

    /// Sets what a directory restore does with files the asset never held (Appendix C.6).
    #[must_use]
    pub const fn directory_policy(mut self, policy: DirectoryRestorePolicy) -> Self {
        self.directory_policy = policy;
        self
    }

    /// Names a provider-native object the method would destroy — a newer snapshot, a bookmark, a
    /// clone (§13.6, §24.5).
    #[must_use]
    pub fn destroying(mut self, asset: impl Into<Arc<str>>) -> Self {
        self.destroyed_assets.push(asset.into());
        self
    }

    /// Records how much live data the method would discard (§24.5).
    #[must_use]
    pub const fn discarding(mut self, bytes: ByteSize) -> Self {
        self.discarded_bytes = Some(bytes);
        self
    }

    /// The method whose effect on newer state is being analysed.
    #[must_use]
    pub const fn method(&self) -> RestoreMethod {
        self.method
    }

    /// When the asset was captured.
    #[must_use]
    pub const fn captured_instant(&self) -> Timestamp {
        self.captured_at
    }

    /// The objects the recovery would put back.
    #[must_use]
    pub fn restore_set(&self) -> &[Arc<str>] {
        &self.restore_set
    }

    /// The observations the analysis runs over.
    #[must_use]
    pub const fn observations(&self) -> &[ObjectObservation] {
        self.observations
    }

    /// The digest the asset holds for `object`, where the caller supplied one.
    #[must_use]
    pub fn captured_digest(&self, object: &str) -> Option<&str> {
        self.captured
            .iter()
            .find(|(name, _)| name.as_ref() == object)
            .map(|(_, digest)| digest.as_ref())
    }

    /// Whether the recovery would put `object` back itself.
    #[must_use]
    pub fn restores(&self, object: &str) -> bool {
        self.restore_set.iter().any(|name| name.as_ref() == object)
    }

    /// Whether `object` sits under a directory the recovery would restore (Appendix C.6).
    fn under_restored_directory(&self, object: &str) -> bool {
        self.restore_set.iter().any(|directory| {
            object.len() > directory.len()
                && object.starts_with(directory.as_ref())
                && object[directory.len()..].starts_with('/')
        })
    }
}

/// Classifies everything that changed since the asset, inside the restore scope (Appendix C.3).
///
/// An object that did not change since the asset was captured produces no item at all: it is not
/// newer state, and a row saying "this would survive" about a file nobody touched is noise in the
/// one view §24.3 needs an operator to read closely.
#[must_use]
pub fn analyse(request: &ConflictRequest<'_>) -> NewerStateImpact {
    let items = request
        .observations()
        .iter()
        .filter_map(|observation| classify(request, observation))
        .collect();
    let mut impact = NewerStateImpact::analysed(items);
    for asset in &request.destroyed_assets {
        impact = impact.destroying(Arc::clone(asset));
    }
    if let Some(bytes) = request.discarded_bytes {
        impact = impact.discarding(bytes);
    }
    impact
}

/// What one observation means for the recovery, or `None` when it is not newer state.
fn classify(request: &ConflictRequest<'_>, observation: &ObjectObservation) -> Option<NewerStateItem> {
    let object = observation.object();
    if let ObservedState::Unestablished { reason } = observation.state() {
        return Some(NewerStateItem::new(
            object,
            NewerStateClass::Unknown,
            format!(
                "the current state of {object} could not be established, so what recovery would \
                 do to it is unknown: {reason}"
            ),
        ));
    }
    let changed_at = observation.state().changed_at();
    let newer = match (
        request.captured_digest(object),
        observation.state().digest(),
    ) {
        (Some(captured), Some(current)) => captured != current,
        (Some(_), None) => true,
        (None, _) => match changed_at {
            Some(at) => at > request.captured_instant(),
            None => {
                return Some(NewerStateItem::new(
                    object,
                    NewerStateClass::Unknown,
                    format!(
                        "whether {object} changed since the recovery point could not be \
                         established: the asset records no digest for it and its change time is \
                         not known"
                    ),
                ));
            }
        },
    };
    if !newer {
        return None;
    }
    let item = if request.restores(object) {
        restored_object_item(request, object, changed_at)?
    } else if request.method().discards_newer_state() {
        NewerStateItem::new(
            object,
            NewerStateClass::DiscardedByMethod,
            format!(
                "{} rewinds the whole domain, so this{} goes with it",
                request.method(),
                at_phrase(changed_at)
            ),
        )
    } else if request.directory_policy == DirectoryRestorePolicy::ExactTree
        && request.under_restored_directory(object)
    {
        NewerStateItem::new(
            object,
            NewerStateClass::DiscardedByMethod,
            "Appendix C.6: the objective asked for exact-tree equivalence, so a file the asset \
             never held is deleted"
                .to_owned(),
        )
    } else {
        NewerStateItem::new(
            object,
            NewerStateClass::PreservedByMethod,
            format!(
                "outside the restore set, so {} leaves it alone",
                request.method()
            ),
        )
    };
    Some(match changed_at {
        Some(at) => item.changed_at(at),
        None => item,
    })
}

/// What a change to an object the recovery restores means (Appendix C.4).
///
/// Returns `None` when the change is the original plan's own, which is the state recovery is
/// undoing rather than state it would take away.
fn restored_object_item(
    request: &ConflictRequest<'_>,
    object: &str,
    changed_at: Option<Timestamp>,
) -> Option<NewerStateItem> {
    if let (Some(applied), Some(at)) = (request.applied_at, changed_at)
        && at <= applied
    {
        return None;
    }
    Some(NewerStateItem::new(
        object,
        NewerStateClass::Conflicting,
        format!(
            "the recovery target and the newer content are the same object: recovery would \
             discard the edit{}",
            at_phrase(changed_at)
        ),
    ))
}

/// ` made at <instant>`, or nothing at all when the instant is not known.
fn at_phrase(changed_at: Option<Timestamp>) -> String {
    changed_at.map_or_else(String::new, |at| format!(" made at {at}"))
}
