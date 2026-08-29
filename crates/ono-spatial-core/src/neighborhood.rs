//! Neighborhoods, and the states a group can be in (spec v0.4 §3.6, §35.2, §33.4).
//!
//! §3.6: "A neighborhood is a bounded, ranked projection of objects and relationships around the
//! current place. It is not simply 'all adjacent nodes'." The ranking is the query layer's job
//! (§45.3); the shape, the bound and the honesty about what is missing are here, because §2.17
//! and §35.2 make them part of the data model rather than of the rendering: "files — permission
//! denied for 14 process FDs" is a different fact from "files — 0", and the difference must
//! survive being written to JSON.

use jiff::Timestamp;

use crate::{Landmark, RelationType, SpatialId};

/// What a neighborhood group could be told about its objects (§35.2).
///
/// "These states MUST remain distinct."
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PermissionState {
    /// The objects were read and are here.
    Available,
    /// The objects were read and there are none.
    Empty,
    /// Something is there, but this user cannot be told what — a count without contents, an
    /// object behind a boundary.
    Unknown,
    /// This user may not read the objects. Not empty (§35.2's own example).
    PermissionDenied,
    /// No installed provider answers for them (§4).
    Unsupported,
    /// The last answer is older than the caller was willing to accept (§33.3).
    Stale,
}

impl PermissionState {
    /// Every state, as §35.2 lists them.
    pub const ALL: &'static [PermissionState] = &[
        PermissionState::Available,
        PermissionState::Empty,
        PermissionState::Unknown,
        PermissionState::PermissionDenied,
        PermissionState::Unsupported,
        PermissionState::Stale,
    ];

    /// The name §35.2 and `docs/spec/spatial/spatial.yaml` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PermissionState::Available => "available",
            PermissionState::Empty => "empty",
            PermissionState::Unknown => "unknown",
            PermissionState::PermissionDenied => "permission_denied",
            PermissionState::Unsupported => "unsupported",
            PermissionState::Stale => "stale",
        }
    }

    /// The state with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|state| state.as_str() == name)
    }

    /// The state a provider's refusal reaches a place view as (§35.2, §42.4).
    ///
    /// §42.4: "Denied information must produce `permission_denied` or `unknown`, never false
    /// empty collections." The mapping is total, so there is no refusal that can arrive as
    /// absence: anything the taxonomy calls a permission failure is `permission_denied`, a
    /// provider that cannot answer here at all is `unsupported`, and everything else is
    /// `unknown` — which is still visible, and still not empty (§2.17).
    #[must_use]
    pub fn of_refusal(error: &ono_value::ErrorValue) -> Self {
        use ono_core::{ErrorCode, ErrorKind};
        match error.code() {
            ErrorCode::ProviderUnavailable | ErrorCode::ProviderUnsupported => {
                PermissionState::Unsupported
            }
            code if code.kind() == ErrorKind::Permission => PermissionState::PermissionDenied,
            _ => PermissionState::Unknown,
        }
    }

    /// Whether the group's contents are what the user sees.
    ///
    /// Every other state means something is missing, and §2.17 requires the missing thing to be
    /// visible rather than rendered as absence.
    #[must_use]
    pub fn is_complete(self) -> bool {
        matches!(self, PermissionState::Available | PermissionState::Empty)
    }
}

/// How complete a bounded projection is (§3.6's `completeness: Completeness`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Completeness {
    /// Everything adjacent to the centre is present.
    Complete,
    /// The view budget hid some neighbours; the hidden count says how many (§3.6, §8.2).
    Bounded,
    /// A source could not be read — denied, unsupported or stale (§35.2).
    Partial,
    /// Completeness could not be established.
    Unknown,
}

impl Completeness {
    /// Every value.
    pub const ALL: &'static [Completeness] = &[
        Completeness::Complete,
        Completeness::Bounded,
        Completeness::Partial,
        Completeness::Unknown,
    ];

    /// The name `docs/spec/spatial/spatial.yaml` spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Completeness::Complete => "complete",
            Completeness::Bounded => "bounded",
            Completeness::Partial => "partial",
            Completeness::Unknown => "unknown",
        }
    }

    /// The value with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|value| value.as_str() == name)
    }
}

/// How current an answer is (§33.4).
///
/// §33.4 requires `inspect` to reveal source freshness and fixes no vocabulary for it; ADR-0129
/// does. The distinction that matters is the one between "old" and "never known": a value with
/// no observation time is not a fresh value (§2.17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Freshness {
    /// A provider subscription is delivering changes; the value is current by construction.
    Live,
    /// Observed within the object class's TTL (§33.3).
    Fresh,
    /// Observed, but older than the TTL. Shown with its age; a mutation revalidates first (§33.2).
    Stale,
    /// Never observed, or the provider stated no observation time.
    Unknown,
}

impl Freshness {
    /// Every state.
    pub const ALL: &'static [Freshness] = &[
        Freshness::Live,
        Freshness::Fresh,
        Freshness::Stale,
        Freshness::Unknown,
    ];

    /// The name `docs/spec/spatial/spatial.yaml` spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Freshness::Live => "live",
            Freshness::Fresh => "fresh",
            Freshness::Stale => "stale",
            Freshness::Unknown => "unknown",
        }
    }

    /// The state with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|value| value.as_str() == name)
    }

    /// Whether an operation that refuses stale data may proceed (§40's `spatial.stale`).
    #[must_use]
    pub fn is_current(self) -> bool {
        matches!(self, Freshness::Live | Freshness::Fresh)
    }
}

/// One exit of a place: the objects a relation leads to, and what is known about them (§3.6).
#[derive(Debug, Clone, PartialEq)]
pub struct NeighborhoodGroup {
    label: String,
    relation: Option<RelationType>,
    members: Vec<SpatialId>,
    total: Option<usize>,
    state: PermissionState,
    freshness: Freshness,
    detail: Option<String>,
}

impl NeighborhoodGroup {
    /// A group of `members`, read successfully.
    #[must_use]
    pub fn available(label: impl Into<String>, members: Vec<SpatialId>) -> Self {
        let state = if members.is_empty() {
            PermissionState::Empty
        } else {
            PermissionState::Available
        };
        Self {
            label: label.into(),
            relation: None,
            total: Some(members.len()),
            members,
            state,
            freshness: Freshness::Fresh,
            detail: None,
        }
    }

    /// A group in a stated §35.2 state, leading to `members` and standing for `total` places.
    ///
    /// The three-way split of §24.2 and §35.2 needs all three parts at once, and neither
    /// [`available`] nor [`withheld`] can express it: an exit is *there* — `enter services` is a
    /// move whether or not systemd answers — while what lies behind it may be unreadable, and
    /// what lies behind it is what a count would be about. So the members say where the exit
    /// goes, the state says what could be learned about its contents, and the total says how many
    /// there are where that could be learned at all. A `total` of `None` is not zero (§2.17).
    ///
    /// [`available`]: NeighborhoodGroup::available
    /// [`withheld`]: NeighborhoodGroup::withheld
    #[must_use]
    pub fn reported(
        label: impl Into<String>,
        state: PermissionState,
        members: Vec<SpatialId>,
        total: Option<usize>,
    ) -> Self {
        Self {
            label: label.into(),
            relation: None,
            members,
            total,
            state,
            freshness: Freshness::Fresh,
            detail: None,
        }
    }

    /// Records what the provider said in place of a count (§35.2).
    #[must_use]
    pub fn explained(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// A group that could not be read, and why (§35.2).
    ///
    /// `detail` is what §35.2's own example puts in the place of a count: "permission denied for
    /// 14 process FDs". A refused group with nothing to say is still not an empty one.
    #[must_use]
    pub fn withheld(
        label: impl Into<String>,
        state: PermissionState,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            relation: None,
            members: Vec::new(),
            total: None,
            state,
            freshness: Freshness::Unknown,
            detail: Some(detail.into()),
        }
    }

    /// Records which relation the group's members are reached by.
    #[must_use]
    pub fn along(mut self, relation: RelationType) -> Self {
        self.relation = Some(relation);
        self
    }

    /// Records how many members there are in total, when only some are listed (§32.2).
    #[must_use]
    pub fn of_total(mut self, total: usize) -> Self {
        self.total = Some(total);
        self
    }

    /// Records how current the answer is (§33.4).
    #[must_use]
    pub fn observed(mut self, freshness: Freshness) -> Self {
        self.freshness = freshness;
        self
    }

    /// The label a place view shows — the relation's label, or the collection's.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The relation the members are reached by, where they are reached by one.
    #[must_use]
    pub fn relation(&self) -> Option<&RelationType> {
        self.relation.as_ref()
    }

    /// The members that are listed.
    #[must_use]
    pub fn members(&self) -> &[SpatialId] {
        &self.members
    }

    /// How many members there are, where that is known. `None` is not zero (§2.17).
    #[must_use]
    pub fn total(&self) -> Option<usize> {
        self.total
    }

    /// What the user was told about the group (§35.2).
    #[must_use]
    pub fn state(&self) -> PermissionState {
        self.state
    }

    /// How current the answer is (§33.4).
    #[must_use]
    pub fn freshness(&self) -> Freshness {
        self.freshness
    }

    /// The explanation shown in place of a count for a group that could not be read.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// How many members are listed but not shown, when only some were kept.
    #[must_use]
    pub fn hidden(&self) -> usize {
        self.total
            .unwrap_or(self.members.len())
            .saturating_sub(self.members.len())
    }
}

/// A bounded, ranked projection around a place (§3.6).
#[derive(Debug, Clone, PartialEq)]
pub struct Neighborhood {
    center: SpatialId,
    groups: Vec<NeighborhoodGroup>,
    landmarks: Vec<Landmark>,
    generated_at: Timestamp,
    completeness: Completeness,
}

impl Neighborhood {
    /// A neighborhood around `center`.
    ///
    /// The completeness is derived rather than asserted: a projection with a group that could not
    /// be read is `Partial`, one that hid neighbours for the budget is `Bounded`, and only one
    /// that is neither is `Complete`. That is §2.9 and §2.17 in one place, so a caller cannot
    /// claim completeness it does not have.
    #[must_use]
    pub fn new(center: SpatialId, groups: Vec<NeighborhoodGroup>, generated_at: Timestamp) -> Self {
        let completeness = if groups.iter().any(|group| !group.state().is_complete()) {
            Completeness::Partial
        } else if groups.iter().any(|group| group.hidden() > 0) {
            Completeness::Bounded
        } else {
            Completeness::Complete
        };
        Self {
            center,
            groups,
            landmarks: Vec::new(),
            generated_at,
            completeness,
        }
    }

    /// Adds the landmarks of the projection (§3.6, §26).
    #[must_use]
    pub fn with_landmarks(mut self, landmarks: Vec<Landmark>) -> Self {
        self.landmarks = landmarks;
        self
    }

    /// The place the projection is around.
    #[must_use]
    pub fn center(&self) -> &SpatialId {
        &self.center
    }

    /// The exits, in the order the query layer ranked them.
    #[must_use]
    pub fn groups(&self) -> &[NeighborhoodGroup] {
        &self.groups
    }

    /// What deserves attention here (§26).
    #[must_use]
    pub fn landmarks(&self) -> &[Landmark] {
        &self.landmarks
    }

    /// How many neighbours the budget hid (§3.6's `hidden_count`).
    #[must_use]
    pub fn hidden_count(&self) -> usize {
        self.groups.iter().map(NeighborhoodGroup::hidden).sum()
    }

    /// When the projection was made.
    #[must_use]
    pub fn generated_at(&self) -> Timestamp {
        self.generated_at
    }

    /// How complete it is (§3.6).
    #[must_use]
    pub fn completeness(&self) -> Completeness {
        self.completeness
    }
}
