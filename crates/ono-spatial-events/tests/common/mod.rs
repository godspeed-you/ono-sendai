//! The fixture a comparison test needs: a small map of sockets, built the way the shell builds
//! one.
//!
//! Both suites in this crate compare projections, so both need a node, an edge and a map to make
//! them out of. Written once here rather than twice beside the assertions, because two copies of
//! a fixture drift and then the two suites are testing subtly different systems (AGENTS.md §11).
//!
//! Nothing here decides anything. The instants are the caller's, because §39.2 keeps the clock
//! out of this crate and out of its tests.

#![allow(
    dead_code,
    reason = "each suite uses the part of the fixture its subject needs"
)]

use jiff::Timestamp;
use ono_spatial_core::{
    Completeness, Confidence, Direction, SpatialId, SpatialIdentity, SpatialType,
};
use ono_spatial_query::{EdgeKind, HiddenSummary, MapEdge, MapNode, SpatialMap};
use ono_value::Provenance;

/// The identity of a socket the fixture calls `name` — built through `SpatialIdentity`, because
/// §3.1 makes the id opaque and nothing outside `ono-spatial-core` may spell one by hand.
pub fn id(name: &str) -> SpatialId {
    SpatialIdentity::observation(SpatialType::Connection, [("inode", name)]).spatial_id()
}

/// One established connection, as a map draws it.
pub fn node(name: &str, label: &str) -> MapNode {
    MapNode {
        id: id(name),
        space: None,
        object_type: SpatialType::Connection,
        label: label.to_owned(),
        state: Some("established".to_owned()),
        canonical_parent: None,
        landmark_reasons: Vec::new(),
        depth: 1,
    }
}

/// The edge a listener has to the connection it accepted.
pub fn edge(name: &str, source: &str, target: &str) -> MapEdge {
    MapEdge {
        id: format!("edge:{name}"),
        source: id(source).to_string(),
        source_label: source.to_owned(),
        target: id(target).to_string(),
        target_label: target.to_owned(),
        relation: "socket.accepts_connection".to_owned(),
        kind: EdgeKind::Relationship,
        confidence: Confidence::Strong,
        direction: Direction::Outbound,
        provenance: Provenance::local("linux.sock-diag", ono_value::SchemaId::new("ono.socket", 1)),
        evidence: ono_value::MapValue::new(),
        observed_at: None,
    }
}

/// A map of those nodes and edges, generated at the epoch.
pub fn map(nodes: Vec<MapNode>, edges: Vec<MapEdge>) -> SpatialMap {
    map_at(nodes, edges, Timestamp::UNIX_EPOCH)
}

/// The same map, generated at a stated instant — which is what a comparison dates its changes by
/// (v0.5 §9.2).
pub fn map_at(nodes: Vec<MapNode>, edges: Vec<MapEdge>, generated_at: Timestamp) -> SpatialMap {
    SpatialMap {
        center: id("listener"),
        focus: None,
        zoom_level: 4,
        nodes,
        edges,
        clusters: Vec::new(),
        landmarks: Vec::new(),
        hidden: HiddenSummary::default(),
        generated_at,
        completeness: Completeness::Complete,
    }
}

/// A map of nodes alone, generated at a stated instant.
pub fn nodes_at(nodes: Vec<MapNode>, generated_at: Timestamp) -> SpatialMap {
    map_at(nodes, Vec::new(), generated_at)
}
