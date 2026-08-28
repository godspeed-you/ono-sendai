//! What a change *is*, how fresh the view that saw it is, and what it was seen through
//! (spec v0.4 §25.1, §25.3, §25.4, §3.7, §26).

use std::collections::BTreeSet;

use ono_spatial_core::{LandmarkReason, SpatialId};

/// How current the data behind a live view is (§25.3).
///
/// The five words are the section's own, and the order below is the order of decreasing
/// liveness, which is what [`Freshness::weaker`] uses: a view is only as live as its least
/// live source, because a picture assembled from one subscription and one poll updates at the
/// pace of the poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Freshness {
    /// A provider announced the change (§25.3's `event-driven`).
    EventDriven,
    /// The runtime compared successive snapshots at an interval (§25.3, v0.2 §18.2).
    Polled,
    /// What was read before, with nothing watching it now.
    Cached,
    /// Older than the source's own freshness policy allows.
    Stale,
    /// Some of the view could not be read at all (§35.2).
    Partial,
}

impl Freshness {
    /// The word §25.3 fixes, normalised to the identifier spelling the structured output uses.
    ///
    /// The spec writes `event-driven` in prose; every other freshness word in this workspace is a
    /// single identifier, and a field value that needs quoting in one place and not in another is
    /// a contract a script has to special-case.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Freshness::EventDriven => "event_driven",
            Freshness::Polled => "polled",
            Freshness::Cached => "cached",
            Freshness::Stale => "stale",
            Freshness::Partial => "partial",
        }
    }

    /// The less live of two sources — what a view fed by both may honestly claim (§25.3).
    #[must_use]
    pub fn weaker(self, other: Freshness) -> Freshness {
        self.max(other)
    }
}

/// What a change was observed through (§25.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSource {
    /// A provider event stream announced it (§25.1).
    ProviderEvents,
    /// It was inferred by comparing two successive projections (§25.4).
    SnapshotComparison,
}

impl ChangeSource {
    /// The word the structured output carries.
    ///
    /// §25.4: "The provenance must identify that the change was inferred from snapshots." This is
    /// that identification, and it is a value rather than a prose note so a script can branch on
    /// it (§29.4).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeSource::ProviderEvents => "provider_events",
            ChangeSource::SnapshotComparison => "snapshot_comparison",
        }
    }
}

/// What kind of difference was seen (§25.1's list of what a live map may visualize).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    /// A place that was not drawn before is drawn now.
    NodeAppeared,
    /// A place that was drawn before is gone.
    NodeRemoved,
    /// The same place, in a different state or under a different name.
    NodeChanged,
    /// A relationship that was not asserted before is asserted now.
    EdgeAppeared,
    /// A relationship that was asserted before is not asserted any more.
    EdgeRemoved,
    /// A place began deserving attention (§3.7).
    LandmarkAppeared,
    /// A place stopped deserving it.
    LandmarkRemoved,
}

impl ChangeKind {
    /// The word for it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::NodeAppeared => "node_appeared",
            ChangeKind::NodeRemoved => "node_removed",
            ChangeKind::NodeChanged => "node_changed",
            ChangeKind::EdgeAppeared => "edge_appeared",
            ChangeKind::EdgeRemoved => "edge_removed",
            ChangeKind::LandmarkAppeared => "landmark_appeared",
            ChangeKind::LandmarkRemoved => "landmark_removed",
        }
    }

    /// The §3.7 reason this kind of change is, where it is one.
    ///
    /// §3.7's reason vocabulary is closed and a core rule may not invent a word for it, so the
    /// three change reasons it does declare — `new_object`, `removed_object`, `recently_changed`
    /// — are exactly the three kinds that map onto one. An edge appearing is a change of the two
    /// places it joins, and those are reported through them.
    #[must_use]
    pub fn reason(self) -> Option<LandmarkReason> {
        match self {
            ChangeKind::NodeAppeared => Some(LandmarkReason::NewObject),
            ChangeKind::NodeRemoved => Some(LandmarkReason::RemovedObject),
            ChangeKind::NodeChanged => Some(LandmarkReason::RecentlyChanged),
            _ => None,
        }
    }
}

/// One difference between two observations of the same space (§25.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpatialChange {
    kind: ChangeKind,
    subject: String,
    label: String,
    places: Vec<SpatialId>,
}

impl SpatialChange {
    /// A change to the node at `place`.
    #[must_use]
    pub fn to_node(kind: ChangeKind, place: SpatialId, label: impl Into<String>) -> Self {
        Self {
            kind,
            subject: place.to_string(),
            label: label.into(),
            places: vec![place],
        }
    }

    /// A change to the edge `id`, between the places it joins.
    #[must_use]
    pub fn to_edge(
        kind: ChangeKind,
        id: impl Into<String>,
        label: impl Into<String>,
        ends: Vec<SpatialId>,
    ) -> Self {
        Self {
            kind,
            subject: id.into(),
            label: label.into(),
            places: ends,
        }
    }

    /// What kind of difference this is.
    #[must_use]
    pub fn kind(&self) -> ChangeKind {
        self.kind
    }

    /// The node or edge identity it happened to.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// What a person calls it.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The places this change touches — one for a node, both ends for an edge (§26).
    pub fn places(&self) -> impl Iterator<Item = &SpatialId> {
        self.places.iter()
    }
}

/// Everything that differed between two observations, and how it was seen (§25.3, §25.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    changes: Vec<SpatialChange>,
    source: ChangeSource,
    freshness: Freshness,
}

impl ChangeSet {
    /// An empty set, which is what "nothing changed" looks like (§25.2, §43.6).
    #[must_use]
    pub fn new(source: ChangeSource, freshness: Freshness) -> Self {
        Self {
            changes: Vec::new(),
            source,
            freshness,
        }
    }

    /// Records one difference.
    pub fn push(&mut self, change: SpatialChange) {
        self.changes.push(change);
    }

    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// How many differences there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// The differences, in the order they were found.
    pub fn changes(&self) -> impl Iterator<Item = &SpatialChange> {
        self.changes.iter()
    }

    /// What the changes were seen through (§25.4).
    #[must_use]
    pub fn source(&self) -> ChangeSource {
        self.source
    }

    /// How live the view that saw them is (§25.3).
    #[must_use]
    pub fn freshness(&self) -> Freshness {
        self.freshness
    }

    /// The places whose landmarks have to be recomputed (§26).
    ///
    /// §26.1 makes a landmark relevance over real state, and §2.11 makes highlighting driven by
    /// "real state, change, importance or user pinning". A change is therefore the trigger for
    /// re-judging exactly the places it touched — both ends of an edge included, because an edge
    /// appearing is what a connection spike is made of (§3.7).
    #[must_use]
    pub fn affected(&self) -> BTreeSet<SpatialId> {
        self.changes
            .iter()
            .flat_map(|change| change.places.iter().cloned())
            .collect()
    }

    /// The §3.7 reasons these changes amount to.
    #[must_use]
    pub fn reasons(&self) -> BTreeSet<LandmarkReason> {
        self.changes
            .iter()
            .filter_map(|change| change.kind.reason())
            .collect()
    }
}
