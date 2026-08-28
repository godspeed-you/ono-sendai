//! Snapshot comparison as spec v0.4 §25.4 allows it: "Where event streams are unavailable, Ono
//! MAY build live changes by comparing successive snapshots. The provenance must identify that
//! the change was inferred from snapshots."
//!
//! Every assertion here is about the *outcome* of a comparison — which changes it names, what it
//! says they were inferred from, and that two identical projections produce nothing at all, which
//! is what §25.2 and §43.6 forbid an implementation from papering over with motion.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use jiff::Timestamp;
use ono_spatial_core::Completeness;
use ono_spatial_core::{
    Confidence, Direction, LandmarkReason, SpatialId, SpatialIdentity, SpatialType,
};
use ono_spatial_events::{ChangeKind, ChangeSource, Freshness, MapSnapshot, compare};
use ono_spatial_query::{EdgeKind, HiddenSummary, MapEdge, MapNode, SpatialMap};
use ono_value::Provenance;

/// The identity of a socket the fixture calls `name` — built through `SpatialIdentity`, because
/// §3.1 makes the id opaque and nothing outside `ono-spatial-core` may spell one by hand.
fn id(name: &str) -> SpatialId {
    SpatialIdentity::observation(SpatialType::Connection, [("inode", name)]).spatial_id()
}

fn node(name: &str, label: &str) -> MapNode {
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

fn edge(name: &str, source: &str, target: &str) -> MapEdge {
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
        observed_at: None,
    }
}

fn map(nodes: Vec<MapNode>, edges: Vec<MapEdge>) -> SpatialMap {
    SpatialMap {
        center: id("listener"),
        focus: None,
        zoom_level: 4,
        nodes,
        edges,
        clusters: Vec::new(),
        landmarks: Vec::new(),
        hidden: HiddenSummary::default(),
        generated_at: Timestamp::UNIX_EPOCH,
        completeness: Completeness::Complete,
    }
}

#[test]
fn should_report_no_change_at_all_when_two_successive_projections_are_the_same() {
    // §25.2 and §43.6: motion must correspond to a real change. Two projections of an unchanged
    // system are the case an implementation is most tempted to animate, so the comparison must
    // answer with nothing rather than with "refreshed".
    let before = MapSnapshot::of(&map(vec![node("listener", "127.0.0.1:8080")], Vec::new()));
    let after = MapSnapshot::of(&map(vec![node("listener", "127.0.0.1:8080")], Vec::new()));

    let changes = compare(&before, &after, Freshness::Polled);

    assert!(
        changes.is_empty(),
        "spec §25.2, §43.6: an unchanged system produces no change, got {:?}",
        changes.changes().collect::<Vec<_>>()
    );
}

#[test]
fn should_name_the_node_that_appeared_when_a_connection_opens_between_two_projections() {
    // §25.1: the live map visualizes node appearance. §3.7 gives the reason vocabulary, and a
    // node that was not there and now is has exactly one word for it: `new_object`.
    let before = MapSnapshot::of(&map(vec![node("listener", "127.0.0.1:8080")], Vec::new()));
    let after = MapSnapshot::of(&map(
        vec![
            node("listener", "127.0.0.1:8080"),
            node("accepted", "127.0.0.1:8080 -> 127.0.0.1:51234"),
        ],
        vec![edge("accepts", "listener", "accepted")],
    ));

    let changes = compare(&before, &after, Freshness::Polled);

    let appeared: Vec<&str> = changes
        .changes()
        .filter(|change| change.kind() == ChangeKind::NodeAppeared)
        .map(|change| change.label())
        .collect();
    assert_eq!(
        appeared,
        vec!["127.0.0.1:8080 -> 127.0.0.1:51234"],
        "spec §25.1: the node that appeared is named, and only that one"
    );
    assert!(
        changes
            .changes()
            .any(|change| change.kind() == ChangeKind::EdgeAppeared),
        "spec §25.1: the edge that came with it appeared too, got {:?}",
        changes.changes().collect::<Vec<_>>()
    );
    assert!(
        changes.reasons().contains(&LandmarkReason::NewObject),
        "spec §3.7: an object that appeared is a `new_object`, got {:?}",
        changes.reasons()
    );
}

#[test]
fn should_name_the_node_that_went_away_when_a_connection_closes_between_two_projections() {
    // §25.1 edge removal and §3.7's `removed_object`. §10.3 lets the node linger as a tombstone;
    // what the comparison itself must state is that it is no longer there.
    let before = MapSnapshot::of(&map(
        vec![
            node("listener", "127.0.0.1:8080"),
            node("accepted", "127.0.0.1:8080 -> 127.0.0.1:51234"),
        ],
        vec![edge("accepts", "listener", "accepted")],
    ));
    let after = MapSnapshot::of(&map(vec![node("listener", "127.0.0.1:8080")], Vec::new()));

    let changes = compare(&before, &after, Freshness::Polled);

    assert!(
        changes
            .changes()
            .any(|change| change.kind() == ChangeKind::NodeRemoved
                && change.label() == "127.0.0.1:8080 -> 127.0.0.1:51234"),
        "spec §25.1: the node that went away is named, got {:?}",
        changes.changes().collect::<Vec<_>>()
    );
    assert!(
        changes
            .changes()
            .any(|change| change.kind() == ChangeKind::EdgeRemoved),
        "spec §25.1: its edge went away with it"
    );
    assert!(
        changes.reasons().contains(&LandmarkReason::RemovedObject),
        "spec §3.7: an object that went away is a `removed_object`, got {:?}",
        changes.reasons()
    );
}

#[test]
fn should_say_the_change_was_inferred_from_snapshots_when_no_event_stream_answered() {
    // §25.4: "The provenance must identify that the change was inferred from snapshots." A change
    // whose source is indistinguishable from a provider event is exactly the fabricated liveness
    // §2.12 and §2.17 forbid.
    let before = MapSnapshot::of(&map(vec![node("listener", "127.0.0.1:8080")], Vec::new()));
    let after = MapSnapshot::of(&map(
        vec![
            node("listener", "127.0.0.1:8080"),
            node("accepted", "127.0.0.1:8080 -> 127.0.0.1:51234"),
        ],
        Vec::new(),
    ));

    let changes = compare(&before, &after, Freshness::Polled);

    assert_eq!(
        changes.source(),
        ChangeSource::SnapshotComparison,
        "spec §25.4: a change found by comparing two projections says so"
    );
    assert_eq!(
        changes.source().as_str(),
        "snapshot_comparison",
        "spec §25.4: the provenance is a word a reader of the structured output can act on"
    );
}

#[test]
fn should_report_the_state_that_moved_when_a_node_changes_without_appearing_or_leaving() {
    // §25.1 lists state transitions among what a live map reflects. The node is the same node —
    // its identity did not move — so this is neither an appearance nor a removal, and §3.7 calls
    // it `recently_changed`.
    let mut listening = node("listener", "127.0.0.1:8080");
    listening.state = Some("listen".to_owned());
    let mut closing = node("listener", "127.0.0.1:8080");
    closing.state = Some("close_wait".to_owned());

    let changes = compare(
        &MapSnapshot::of(&map(vec![listening], Vec::new())),
        &MapSnapshot::of(&map(vec![closing], Vec::new())),
        Freshness::Polled,
    );

    let moved: Vec<ChangeKind> = changes
        .changes()
        .map(ono_spatial_events::SpatialChange::kind)
        .collect();
    assert_eq!(
        moved,
        vec![ChangeKind::NodeChanged],
        "spec §25.1: a state transition is a change of the node, not a new node"
    );
    assert!(
        changes.reasons().contains(&LandmarkReason::RecentlyChanged),
        "spec §3.7: a node whose state moved is `recently_changed`, got {:?}",
        changes.reasons()
    );
}

#[test]
fn should_name_the_places_whose_landmarks_must_be_recomputed_when_the_topology_moved() {
    // §26: landmarks are relevance over real state, so every place a change touched has to be
    // re-judged. The comparison is what knows which those are.
    let before = MapSnapshot::of(&map(vec![node("listener", "127.0.0.1:8080")], Vec::new()));
    let after = MapSnapshot::of(&map(
        vec![
            node("listener", "127.0.0.1:8080"),
            node("accepted", "127.0.0.1:8080 -> 127.0.0.1:51234"),
        ],
        vec![edge("accepts", "listener", "accepted")],
    ));

    let affected = compare(&before, &after, Freshness::Polled).affected();

    assert!(
        affected.contains(&id("accepted")),
        "spec §26: the place that appeared is re-judged, got {affected:?}"
    );
    assert!(
        affected.contains(&id("listener")),
        "spec §26: so is the place at the other end of the edge that appeared, got {affected:?}"
    );
}

#[test]
fn should_carry_the_freshness_of_the_source_the_changes_were_found_through() {
    // §25.3: "Live views MUST expose whether updates are event-driven, polled, cached, stale or
    // partial." The comparison does not decide it — the source does — but it carries it, because
    // the view that renders the change is the one that has to say it.
    let before = MapSnapshot::of(&map(vec![node("listener", "127.0.0.1:8080")], Vec::new()));
    let after = MapSnapshot::of(&map(
        vec![
            node("listener", "127.0.0.1:8080"),
            node("accepted", "127.0.0.1:8080 -> 127.0.0.1:51234"),
        ],
        Vec::new(),
    ));

    assert_eq!(
        compare(&before, &after, Freshness::EventDriven).freshness(),
        Freshness::EventDriven,
        "spec §25.3: an event-driven view says so"
    );
    assert_eq!(
        compare(&before, &after, Freshness::Polled)
            .freshness()
            .as_str(),
        "polled",
        "spec §25.3: a polled view says so, in the vocabulary the section fixes"
    );
}
