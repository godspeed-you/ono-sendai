//! Snapshot comparison (spec v0.4 §25.4).
//!
//! "Where event streams are unavailable, Ono MAY build live changes by comparing successive
//! snapshots. The provenance must identify that the change was inferred from snapshots."
//!
//! A [`MapSnapshot`] is a projection reduced to what a change can be about: which places are
//! drawn, what they are called, what state their provider reported, and which relationships hold
//! between them. Everything that moves without the system moving — when the projection was made,
//! its identity, the order the ranking happened to choose — is deliberately not in it, because a
//! comparison that noticed those would report change on every tick, which is precisely the
//! decorative motion §25.2 forbids.

use std::collections::{BTreeMap, BTreeSet};

use ono_spatial_core::{LandmarkReason, SpatialId};
use ono_spatial_query::SpatialMap;

use crate::change::{ChangeKind, ChangeSet, ChangeSource, Freshness, SpatialChange};

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapSnapshot {
    nodes: BTreeMap<SpatialId, NodeShape>,
    edges: BTreeMap<String, EdgeShape>,
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
        Self { nodes, edges }
    }

    /// Whether the projection drew nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }
}

/// What differs between two projections of the same space (§25.4).
///
/// `freshness` is the caller's, because only the caller knows how the two projections were
/// obtained — §25.3 makes that part of what a live view must expose, and a comparison cannot
/// discover it from the pictures it is comparing.
#[must_use]
pub fn compare(before: &MapSnapshot, after: &MapSnapshot, freshness: Freshness) -> ChangeSet {
    let mut changes = ChangeSet::new(ChangeSource::SnapshotComparison, freshness);

    for (id, shape) in &after.nodes {
        match before.nodes.get(id) {
            None => changes.push(SpatialChange::to_node(
                ChangeKind::NodeAppeared,
                id.clone(),
                &shape.label,
            )),
            Some(previous) if previous != shape => {
                let reasons: BTreeSet<&LandmarkReason> = shape.reasons.iter().collect();
                let before_reasons: BTreeSet<&LandmarkReason> = previous.reasons.iter().collect();
                if previous.label != shape.label || previous.state != shape.state {
                    changes.push(SpatialChange::to_node(
                        ChangeKind::NodeChanged,
                        id.clone(),
                        &shape.label,
                    ));
                }
                if reasons.difference(&before_reasons).next().is_some() {
                    changes.push(SpatialChange::to_node(
                        ChangeKind::LandmarkAppeared,
                        id.clone(),
                        &shape.label,
                    ));
                }
                if before_reasons.difference(&reasons).next().is_some() {
                    changes.push(SpatialChange::to_node(
                        ChangeKind::LandmarkRemoved,
                        id.clone(),
                        &shape.label,
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
            ));
        }
    }
    changes
}
