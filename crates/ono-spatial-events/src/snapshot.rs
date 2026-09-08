//! Snapshot comparison (spec v0.4 §25.4).
//!
//! "Where event streams are unavailable, Ono MAY build live changes by comparing successive
//! snapshots. The provenance must identify that the change was inferred from snapshots."
//!
//! A [`MapSnapshot`] is a projection reduced to what a change can be about: which places are
//! drawn, what they are called, what state their provider reported, and which relationships hold
//! between them. Everything that moves without the system moving — its identity, the order the
//! ranking happened to choose — is deliberately not in it, because a comparison that noticed
//! those would report change on every tick, which is precisely the decorative motion §25.2
//! forbids.
//!
//! A snapshot does carry **when it was observed**, taken from the projection it reduces, and that
//! instant takes no part in what counts as a difference: two identical projections made an hour
//! apart still compare to nothing at all. What the instant is for is v0.5 §9.2. A difference
//! between two observations happened somewhere in the interval separating them, and the change
//! records that interval rather than a point inside it — so a consumer building a temporal event
//! from it never has to invent a timestamp, which v0.5 §3.3 forbids.

use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;
use ono_spatial_core::{LandmarkReason, Neighborhood, SpatialId};
use ono_spatial_index::SpatialIndex;
use ono_spatial_query::SpatialMap;

use crate::change::{
    ChangeKind, ChangeSet, ChangeSource, Freshness, ObservationWindow, ObservedAt, SpatialChange,
};

/// What a node looks like, as far as a change is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeShape {
    label: String,
    state: Option<String>,
    reasons: Vec<LandmarkReason>,
}

/// What an edge looks like, as far as a change is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EdgeShape {
    label: String,
    ends: Vec<SpatialId>,
}

/// One projection, reduced to what can differ (§25.4).
#[derive(Debug, Clone, Default, Eq)]
pub struct MapSnapshot {
    nodes: BTreeMap<SpatialId, NodeShape>,
    edges: BTreeMap<String, EdgeShape>,
    /// When the projection this reduces was made. It is what the observation was, rather than
    /// something about it that can differ, so [`PartialEq`] ignores it.
    observed_at: Timestamp,
}

/// Two snapshots are equal when they saw the same space, whatever the clock said while they
/// were taken.
///
/// §25.2 makes change a property of the system: "Motion and visual updates MUST correspond to
/// actual topology or metric changes." Time passing is not such a change, so the observation
/// instant is excluded from equality exactly as it is excluded from [`compare`].
impl PartialEq for MapSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes && self.edges == other.edges
    }
}

impl MapSnapshot {
    /// The comparable shape of a map.
    #[must_use]
    pub fn of(map: &SpatialMap) -> Self {
        let mut nodes = BTreeMap::new();
        for node in &map.nodes {
            nodes.insert(
                node.id.clone(),
                NodeShape {
                    label: node.label.clone(),
                    state: node.state.clone(),
                    reasons: node.landmark_reasons.clone(),
                },
            );
        }
        let mut edges = BTreeMap::new();
        for edge in &map.edges {
            let ends = [edge.source.as_str(), edge.target.as_str()]
                .into_iter()
                .filter_map(SpatialId::parse)
                .collect();
            edges.insert(
                edge.id.clone(),
                EdgeShape {
                    label: format!(
                        "{} {} {}",
                        edge.source_label, edge.relation, edge.target_label
                    ),
                    ends,
                },
            );
        }
        Self {
            nodes,
            edges,
            observed_at: map.generated_at,
        }
    }

    /// Whether the projection drew nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// When the projection this reduces was made (v0.5 §3.3).
    ///
    /// The instant is the projection's own — [`SpatialMap::generated_at`] — so nothing here reads
    /// a clock and a comparison is reproducible from its inputs alone (v0.5 §39.2).
    #[must_use]
    pub fn observed_at(&self) -> Timestamp {
        self.observed_at
    }
}

/// What differs between two projections of the same space (§25.4, v0.5 §9.2).
///
/// `freshness` is the caller's, because only the caller knows how the two projections were
/// obtained — §25.3 makes that part of what a live view must expose, and a comparison cannot
/// discover it from the pictures it is comparing.
///
/// Every change this reports is dated `between` the two projections' own instants. That is all a
/// comparison knows: the difference arose after the first observation and by the second one, and
/// v0.5 §9.2 forbids claiming any moment inside that interval.
#[must_use]
pub fn compare(before: &MapSnapshot, after: &MapSnapshot, freshness: Freshness) -> ChangeSet {
    let window = ObservationWindow::new(before.observed_at, after.observed_at);
    let observed = ObservedAt::Between {
        from: window.since(),
        until: window.until(),
    };
    let mut changes = ChangeSet::new(ChangeSource::SnapshotComparison, freshness, window);

    for (id, shape) in &after.nodes {
        match before.nodes.get(id) {
            None => changes.push(SpatialChange::to_node(
                ChangeKind::NodeAppeared,
                id.clone(),
                &shape.label,
                observed,
            )),
            Some(previous) if previous != shape => {
                let reasons: BTreeSet<&LandmarkReason> = shape.reasons.iter().collect();
                let before_reasons: BTreeSet<&LandmarkReason> = previous.reasons.iter().collect();
                if previous.label != shape.label || previous.state != shape.state {
                    changes.push(SpatialChange::to_node(
                        ChangeKind::NodeChanged,
                        id.clone(),
                        &shape.label,
                        observed,
                    ));
                }
                if reasons.difference(&before_reasons).next().is_some() {
                    changes.push(SpatialChange::to_node(
                        ChangeKind::LandmarkAppeared,
                        id.clone(),
                        &shape.label,
                        observed,
                    ));
                }
                if before_reasons.difference(&reasons).next().is_some() {
                    changes.push(SpatialChange::to_node(
                        ChangeKind::LandmarkRemoved,
                        id.clone(),
                        &shape.label,
                        observed,
                    ));
                }
            }
            Some(_) => {}
        }
    }
    for (id, shape) in &before.nodes {
        if !after.nodes.contains_key(id) {
            changes.push(SpatialChange::to_node(
                ChangeKind::NodeRemoved,
                id.clone(),
                &shape.label,
                observed,
            ));
        }
    }
    for (id, shape) in &after.edges {
        if !before.edges.contains_key(id) {
            changes.push(SpatialChange::to_edge(
                ChangeKind::EdgeAppeared,
                id,
                &shape.label,
                shape.ends.clone(),
                observed,
            ));
        }
    }
    for (id, shape) in &before.edges {
        if !after.edges.contains_key(id) {
            changes.push(SpatialChange::to_edge(
                ChangeKind::EdgeRemoved,
                id,
                &shape.label,
                shape.ends.clone(),
                observed,
            ));
        }
    }
    changes
}

/// One place's neighborhood, reduced to what a change can be about (§24.3, §25.4).
///
/// The map snapshot above compares a *projection*; this compares a *place*. `look --changes` has
/// no map: its horizon is the groups §12–§18 give the place, and what can differ in them is which
/// neighbours are behind each exit, what those neighbours are called and what state their
/// provider reports — plus the state of the exit itself, because §35.2 makes `files —
/// permission denied` a different fact from `files — 3`.
#[derive(Debug, Clone, Default, Eq)]
pub struct PlaceSnapshot {
    members: BTreeMap<SpatialId, NodeShape>,
    groups: BTreeMap<String, String>,
    /// When the neighborhood this reduces was observed. [`PartialEq`] ignores it, for the reason
    /// [`MapSnapshot`] gives.
    observed_at: Timestamp,
}

/// Two observations of a place are equal when they saw the same place, for the reason
/// [`MapSnapshot`]'s own [`PartialEq`] gives.
impl PartialEq for PlaceSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.members == other.members && self.groups == other.groups
    }
}

impl PlaceSnapshot {
    /// The comparable shape of a neighborhood, named through the index that holds it.
    #[must_use]
    pub fn of(index: &SpatialIndex, neighborhood: &Neighborhood) -> Self {
        let mut members = BTreeMap::new();
        let mut groups = BTreeMap::new();
        for group in neighborhood.groups() {
            groups.insert(group.label().to_owned(), group.state().as_str().to_owned());
            for member in group.members() {
                let Some(entry) = index.get(member) else {
                    continue;
                };
                members.insert(
                    member.clone(),
                    NodeShape {
                        label: entry.object().display_name().to_owned(),
                        state: None,
                        reasons: entry
                            .landmarks()
                            .iter()
                            .map(ono_spatial_core::Landmark::reason)
                            .collect(),
                    },
                );
            }
        }
        Self {
            members,
            groups,
            observed_at: neighborhood.generated_at(),
        }
    }

    /// Whether the place had no neighbours at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty() && self.groups.is_empty()
    }

    /// When the neighborhood this reduces was observed (v0.5 §3.3).
    ///
    /// It is the neighborhood's own instant, which is what lets a baseline stored between two
    /// `look --changes` calls still say which interval the comparison spans (v0.5 §9.2).
    #[must_use]
    pub fn observed_at(&self) -> Timestamp {
        self.observed_at
    }
}

/// What differs between two observations of one place (§24.3, §25.4, v0.5 §9.2).
///
/// The changes are dated `between` the two observations' own instants, for the reason
/// [`compare`] gives.
#[must_use]
pub fn compare_places(
    before: &PlaceSnapshot,
    after: &PlaceSnapshot,
    freshness: Freshness,
) -> ChangeSet {
    let window = ObservationWindow::new(before.observed_at, after.observed_at);
    let observed = ObservedAt::Between {
        from: window.since(),
        until: window.until(),
    };
    let mut changes = ChangeSet::new(ChangeSource::SnapshotComparison, freshness, window);
    for (id, shape) in &after.members {
        match before.members.get(id) {
            None => changes.push(SpatialChange::to_node(
                ChangeKind::NodeAppeared,
                id.clone(),
                &shape.label,
                observed,
            )),
            Some(previous) if previous.label != shape.label => changes.push(
                SpatialChange::to_node(ChangeKind::NodeChanged, id.clone(), &shape.label, observed),
            ),
            Some(previous) => {
                let now: BTreeSet<&LandmarkReason> = shape.reasons.iter().collect();
                let then: BTreeSet<&LandmarkReason> = previous.reasons.iter().collect();
                if now.difference(&then).next().is_some() {
                    changes.push(SpatialChange::to_node(
                        ChangeKind::LandmarkAppeared,
                        id.clone(),
                        &shape.label,
                        observed,
                    ));
                }
                if then.difference(&now).next().is_some() {
                    changes.push(SpatialChange::to_node(
                        ChangeKind::LandmarkRemoved,
                        id.clone(),
                        &shape.label,
                        observed,
                    ));
                }
            }
        }
    }
    for (id, shape) in &before.members {
        if !after.members.contains_key(id) {
            changes.push(SpatialChange::to_node(
                ChangeKind::NodeRemoved,
                id.clone(),
                &shape.label,
                observed,
            ));
        }
    }
    changes
}
