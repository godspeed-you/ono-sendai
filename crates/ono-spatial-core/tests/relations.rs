//! The relation vocabulary and the edges built from it — spec v0.4 §2.5, §2.6, §3.4, §3.5, §11.5,
//! §32.1, §41.2, and the unit coverage §43.1 requires ("relation inverse handling").

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use jiff::Timestamp;
use ono_spatial_core::{
    Confidence, ConfidenceClaim, CostClass, Direction, HierarchicalEdge, HierarchyKind,
    RelationType, RelationshipEdge, SpatialId, SpatialIdentity, SpatialType, ValidityWindow,
    relation, relations,
};
use ono_value::{Provenance, SchemaId, Value};

fn object(kind: SpatialType, name: &str) -> SpatialId {
    SpatialIdentity::stable(kind, [("name", name)]).spatial_id()
}

fn edge(relation: &str) -> RelationshipEdge {
    let spec = relation::spec(relation).expect("a declared relation");
    RelationshipEdge::new(
        object(spec.source, "source"),
        object(spec.target, "target"),
        RelationType::new(relation).expect("a declared relation"),
        Confidence::Exact,
        Provenance::local("test", SchemaId::new("ono.process", 1)),
        Timestamp::UNIX_EPOCH,
    )
}

#[test]
fn should_refuse_to_build_a_relation_the_registry_does_not_declare() {
    // §2.5: "Every edge is explainable." The explanation starts with the declaration, so a name
    // nothing declares cannot become an edge at all — it is `spatial.no_relation` (§40).
    assert_eq!(RelationType::new("process.eats_socket"), None);
    assert!(RelationType::new("process.owns_socket").is_some());
}

#[test]
fn should_read_one_edge_from_both_ends_with_the_labels_the_registry_declares() {
    // §41.2's `canonical_label` and `inverse_label`: `follow socket` from a process and
    // `follow owner` from that socket are the two ends of one edge, not two edges.
    let edge = edge("process.owns_socket");
    assert_eq!(edge.label_from(edge.source()), Some("socket"));
    assert_eq!(edge.label_from(edge.target()), Some("owner"));
    assert_eq!(edge.other_end(edge.source()), Some(edge.target()));
    assert_eq!(
        edge.label_from(&object(SpatialType::User, "root")),
        None,
        "an edge has no label for an object it does not touch"
    );
}

#[test]
fn should_keep_an_inverted_edge_the_same_assertion() {
    // Inverting reads the same relationship from the other end; it does not create a second,
    // weaker or fresher claim. §22.2's rule that nothing is promoted applies to the whole edge.
    let edge = edge("process.owns_socket");
    let inverted = edge.inverted();
    assert_eq!(inverted.source(), edge.target());
    assert_eq!(inverted.target(), edge.source());
    assert_eq!(inverted.direction(), Direction::Inbound);
    assert_eq!(inverted.confidence(), edge.confidence());
    assert_eq!(inverted.observed_at(), edge.observed_at());
    assert_eq!(inverted.relation(), edge.relation());
    assert_eq!(inverted.inverted().direction(), edge.direction());
}

#[test]
fn should_leave_a_bidirectional_edges_direction_alone_when_inverted() {
    // §14.4: a connection has two ends and neither is the origin, so there is no direction to
    // flip.
    let edge = edge("socket.connected_to");
    assert_eq!(edge.direction(), Direction::Bidirectional);
    assert_eq!(edge.inverted().direction(), Direction::Bidirectional);
}

#[test]
fn should_give_two_assertions_of_one_relationship_the_same_edge_id() {
    // §11.4's `inspect relation @edge-17` needs an edge to have a name that survives being seen
    // twice.
    assert_eq!(
        edge("process.owns_socket").edge_id(),
        edge("process.owns_socket").edge_id()
    );
    assert_ne!(
        edge("process.owns_socket").edge_id(),
        edge("process.opened_file").edge_id()
    );
}

#[test]
fn should_give_an_inference_a_different_edge_id_than_an_observation() {
    // An inference is not the same assertion as an observation, so it may not be absorbed into
    // one (spec v0.2 §22.2, carried into v0.4 by §11.5).
    let observed = edge("process.connects_to");
    let inferred = RelationshipEdge::new(
        observed.source().clone(),
        observed.target().clone(),
        *observed.relation(),
        Confidence::Inferred,
        observed.provenance().clone(),
        observed.observed_at(),
    );
    assert_ne!(observed.edge_id(), inferred.edge_id());
}

#[test]
fn should_hold_every_field_the_specification_lists_for_a_relationship_edge() {
    // §3.5's minimum fields, each readable.
    let edge = edge("process.owns_socket")
        .valid(ValidityWindow::since(Timestamp::UNIX_EPOCH))
        .with_attribute("fd", Value::Int(7));
    assert!(!edge.edge_id().as_str().is_empty());
    assert_eq!(edge.relation().as_str(), "process.owns_socket");
    assert_eq!(edge.direction(), Direction::Outbound);
    assert_eq!(edge.confidence(), Confidence::Exact);
    assert_eq!(edge.provenance().provider(), "test");
    assert_eq!(edge.observed_at(), Timestamp::UNIX_EPOCH);
    assert_eq!(
        edge.validity().and_then(|window| window.from()),
        Some(Timestamp::UNIX_EPOCH)
    );
    assert_eq!(edge.attributes().get("fd"), Some(&Value::Int(7)));
}

#[test]
fn should_not_claim_a_window_closed_when_nobody_saw_it_close() {
    // §2.17: an unknown end is not an end. A connection observed with no close time is not a
    // connection known to have finished.
    let open = ValidityWindow::since(Timestamp::UNIX_EPOCH);
    assert!(!open.has_closed_by(Timestamp::from_second(1_700_000_000).expect("a timestamp")));
}

#[test]
fn should_refuse_an_edge_whose_confidence_the_registry_does_not_admit() {
    // §41.3 makes the registry the source of the map legend a user reads; a relation declared
    // `exact` that carries an inferred edge would make that legend a lie.
    let declared_exact = edge("process.owns_socket");
    assert!(declared_exact.honours_declaration());

    let overstated = RelationshipEdge::new(
        declared_exact.source().clone(),
        declared_exact.target().clone(),
        *declared_exact.relation(),
        Confidence::Inferred,
        declared_exact.provenance().clone(),
        declared_exact.observed_at(),
    );
    assert!(!overstated.honours_declaration());

    let provider_declared = RelationshipEdge::new(
        object(SpatialType::Process, "1842"),
        object(SpatialType::Endpoint, "postgres:5432"),
        RelationType::new("process.connects_to").expect("a declared relation"),
        Confidence::Inferred,
        declared_exact.provenance().clone(),
        declared_exact.observed_at(),
    );
    assert!(
        provider_declared.honours_declaration(),
        "§41.2's `exact_or_provider_declared` leaves the claim to the provider"
    );
}

#[test]
fn should_never_raise_a_weaker_claim_to_exact_when_bridged_to_the_trace_graph() {
    // Spec v0.2 §22.2 forbids presenting a derivation as something a provider observed. The
    // bridge to the two-valued graph vocabulary may lose precision; it may not gain certainty.
    assert_eq!(Confidence::Exact.to_graph(), ono_render::Confidence::Exact);
    for weaker in [
        Confidence::Strong,
        Confidence::Inferred,
        Confidence::UserDeclared,
        Confidence::Unknown,
    ] {
        assert_eq!(
            weaker.to_graph(),
            ono_render::Confidence::Inferred,
            "`{weaker}` is not an observation"
        );
    }
    assert_eq!(
        Confidence::from_graph(ono_render::Confidence::Inferred),
        Confidence::Inferred,
        "an inferred graph edge stays inferred rather than becoming `strong`"
    );
}

#[test]
fn should_resolve_a_label_by_the_type_the_user_is_standing_on() {
    // ADR-0128: labels are unique per source type, not globally. `follow process` means the
    // obvious thing from a service, a user and a container alike.
    for (from, expected) in [
        (SpatialType::Service, "service.controls_process"),
        (SpatialType::User, "process.run_by_user"),
        (SpatialType::Container, "container.contains_process"),
    ] {
        let found = relation::resolve_label(from, "process");
        assert_eq!(
            found.iter().map(|spec| spec.id).collect::<Vec<_>>(),
            vec![expected],
            "`follow process` from {from}"
        );
    }
    assert!(
        relation::resolve_label(SpatialType::Process, "no-such-exit").is_empty(),
        "§40: an unknown label is `spatial.no_relation`"
    );
}

#[test]
fn should_offer_a_process_the_exits_the_specification_names() {
    // §12's minimum groups: parent, children, service, user, cgroup, namespaces, files,
    // sockets/connections, container.
    let exits: Vec<&str> = relation::exits_from(SpatialType::Process)
        .map(|(label, _)| label)
        .collect();
    for wanted in [
        "parent",
        "children",
        "service",
        "user",
        "cgroup",
        "namespaces",
        "files",
        "sockets",
        "container",
    ] {
        assert!(
            exits.contains(&wanted),
            "§12: a process has a `{wanted}` exit, got {exits:?}"
        );
    }
}

#[test]
fn should_declare_a_cost_class_that_keeps_a_broad_scan_out_of_a_default_look() {
    // §32.1: "Default `look` and `map` MUST avoid expensive relationships unless cached or
    // already available", and §32.2 makes those discoverable but unloaded exits.
    let files = relation::spec("process.opened_file").expect("a declared relation");
    assert_eq!(files.cost_class, CostClass::Expensive);
    assert!(!files.cost_class.is_eager());

    let sockets = relation::spec("process.owns_socket").expect("a declared relation");
    assert!(sockets.cost_class.is_eager());
}

#[test]
fn should_keep_hierarchy_and_relationship_edges_apart() {
    // §2.6: "Hierarchy and graph are separate concepts. Parent/child spatial grouping MUST NOT
    // be confused with arbitrary relationships." They are different types with no conversion,
    // and a hierarchical edge carries nothing that could be read as a dependency (§3.4).
    let hierarchical = HierarchicalEdge::new(
        SpatialId::of_space("compute"),
        SpatialId::of_space("compute.services"),
        HierarchyKind::Grouping,
    );
    assert_eq!(hierarchical.parent(), &SpatialId::of_space("compute"));
    assert_eq!(hierarchical.kind(), HierarchyKind::Grouping);
}

#[test]
fn should_declare_every_relation_with_a_readable_end_at_both_sides() {
    // §41.2 and §42.3: an edge whose ends cannot be named is an edge nobody can follow back.
    for spec in relations() {
        assert!(!spec.canonical_label.is_empty(), "{}", spec.id);
        assert!(!spec.inverse_label.is_empty(), "{}", spec.id);
        assert!(
            spec.labels_from(spec.source)
                .any(|label| label == spec.canonical_label),
            "{}",
            spec.id
        );
        assert!(
            spec.labels_from(spec.target)
                .any(|label| label == spec.inverse_label),
            "{}",
            spec.id
        );
        assert!(
            matches!(
                spec.confidence,
                ConfidenceClaim::Fixed(_) | ConfidenceClaim::ProviderDeclared
            ),
            "{}",
            spec.id
        );
    }
}
