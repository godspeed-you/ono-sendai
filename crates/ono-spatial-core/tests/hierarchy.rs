//! The canonical geography, the canonical parent and scope boundaries — spec v0.4 §4, §7, §11.1,
//! §11.3, §3.2, §2.18, and the unit coverage §43.1 requires ("canonical parent selection",
//! "scope boundary detection").

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use jiff::Timestamp;
use ono_spatial_core::{
    BootIdentity, Confidence, HierarchyKind, RelationType, RelationshipEdge, ScopeKind, SpatialId,
    SpatialIdentity, SpatialScope, SpatialType, canonical_parent, hierarchy, parent_of_space,
    path_to_space, space,
};
use ono_value::{Provenance, SchemaId};

fn edge(source: &SpatialId, target: &SpatialId, relation: &str) -> RelationshipEdge {
    RelationshipEdge::new(
        source.clone(),
        target.clone(),
        RelationType::new(relation).expect("a declared relation"),
        Confidence::Exact,
        Provenance::local("test", SchemaId::new("ono.process", 1)),
        Timestamp::UNIX_EPOCH,
    )
}

fn object(kind: SpatialType, name: &str) -> SpatialId {
    SpatialIdentity::stable(kind, [("name", name)]).spatial_id()
}

#[test]
fn should_expose_the_six_canonical_domains_under_one_root() {
    // §4 and §53: "Six canonical domains: Compute, Network, Storage, Containers, Identity,
    // Devices", under a root that is never a flat list of everything (§7.1).
    let domains: Vec<&str> = space::domains().map(|space| space.id).collect();
    assert_eq!(
        domains,
        vec![
            "compute",
            "network",
            "storage",
            "containers",
            "identity",
            "devices"
        ]
    );
    for domain in space::domains() {
        assert_eq!(domain.parent, Some("system"));
    }
    assert_eq!(space::root().parent, None, "§7.1: the root has no parent");
}

#[test]
fn should_reach_every_space_from_the_root_by_canonical_parents_alone() {
    // §11.1: hierarchy "provides a stable path". A space that cannot be reached from the root by
    // parents alone would be a place `up` could leave but never return to.
    for candidate in space::spaces() {
        let path = path_to_space(candidate.id);
        assert_eq!(
            path.first().map(|space| space.id),
            Some("system"),
            "§11.1: `{}` must be reachable from the root, walked {:?}",
            candidate.id,
            path.iter().map(|space| space.id).collect::<Vec<_>>()
        );
    }
}

#[test]
fn should_offer_the_collections_the_specification_names_for_every_domain() {
    // §7.2, §7.3, §7.4, §7.6 name the collections each domain MUST provide access to.
    let children =
        |domain: &str| -> Vec<&str> { space::children(domain).map(|space| space.label).collect() };
    for wanted in ["processes", "services", "jobs", "cgroups"] {
        assert!(children("compute").contains(&wanted), "§7.2: {wanted}");
    }
    for wanted in [
        "interfaces",
        "addresses",
        "routes",
        "neighbors",
        "listeners",
        "connections",
        "namespaces",
    ] {
        assert!(children("network").contains(&wanted), "§7.3: {wanted}");
    }
    for wanted in ["filesystems", "mounts", "directories"] {
        assert!(children("storage").contains(&wanted), "§7.4: {wanted}");
    }
    for wanted in ["users", "groups", "sessions"] {
        assert!(children("identity").contains(&wanted), "§7.6: {wanted}");
    }
}

#[test]
fn should_give_the_root_no_canonical_parent_so_up_refuses_rather_than_looping() {
    // §6.6/§40: `up` from the root is `spatial.no_parent`. A root that were its own parent would
    // make `up` a silent no-op, which §2.2 forbids: location is explicit.
    assert_eq!(parent_of_space("system"), None);
    let compute = parent_of_space("compute").expect("a domain has a parent");
    assert_eq!(compute.parent(), &SpatialId::of_space("system"));
    assert_eq!(compute.kind(), HierarchyKind::Grouping);
}

#[test]
fn should_choose_the_service_as_a_processs_canonical_parent_when_one_controls_it() {
    // §11.1's own path: SYSTEM -> COMPUTE -> SERVICES -> nginx.service, with the process under
    // the service. §13: the service is the place that survives the process.
    let process = object(SpatialType::Process, "1842");
    let service = object(SpatialType::Service, "nginx.service");
    let container = object(SpatialType::Container, "payments-api");
    let edges = vec![
        edge(&container, &process, "container.contains_process"),
        edge(&service, &process, "service.controls_process"),
    ];
    let parent = canonical_parent(&process, SpatialType::Process, &edges).expect("a parent");
    assert_eq!(parent.parent(), &service);
}

#[test]
fn should_choose_the_same_canonical_parent_whatever_order_the_edges_arrive_in() {
    // §11.3: "The canonical parent MUST be deterministic for a given view profile." The order
    // edges happened to be discovered in is not part of the view profile.
    let process = object(SpatialType::Process, "1842");
    let service = object(SpatialType::Service, "nginx.service");
    let container = object(SpatialType::Container, "payments-api");
    let forward = vec![
        edge(&service, &process, "service.controls_process"),
        edge(&container, &process, "container.contains_process"),
    ];
    let reversed: Vec<_> = forward.iter().rev().cloned().collect();
    assert_eq!(
        canonical_parent(&process, SpatialType::Process, &forward),
        canonical_parent(&process, SpatialType::Process, &reversed)
    );
}

#[test]
fn should_fall_back_to_the_collection_when_no_operational_parent_exists() {
    // §11.3: an object with no service and no container still belongs somewhere in the
    // geography, so `up` arrives at `compute.processes` rather than nowhere.
    let process = object(SpatialType::Process, "1842");
    let parent = canonical_parent(&process, SpatialType::Process, &[]).expect("a parent");
    assert_eq!(parent.parent(), &SpatialId::of_space("compute.processes"));
    assert_eq!(parent.kind(), HierarchyKind::Grouping);
}

#[test]
fn should_never_reach_a_canonical_parent_through_a_relation_that_is_not_a_parent_rule() {
    // §43.2's property, stated as an outcome: "up never traverses arbitrary graph edges". Adding
    // any relation that is not on the type's ordered parent list leaves `up` where it was.
    let process = object(SpatialType::Process, "1842");
    let baseline = canonical_parent(&process, SpatialType::Process, &[]);
    let rules: Vec<&str> = hierarchy::parent_rules(SpatialType::Process)
        .iter()
        .map(|rule| rule.relation)
        .collect();

    for relation in ono_spatial_core::relations() {
        if rules.contains(&relation.id) {
            continue;
        }
        let Some(other_type) = relation.target_from(SpatialType::Process) else {
            continue;
        };
        let neighbour = object(other_type, "neighbour");
        let edges = vec![edge(&neighbour, &process, relation.id)];
        assert_eq!(
            canonical_parent(&process, SpatialType::Process, &edges),
            baseline,
            "§2.6/§43.2: `{}` is a relationship, not a hierarchy edge; `up` must not follow it",
            relation.id
        );
    }
}

#[test]
fn should_report_no_boundary_between_a_scope_and_itself() {
    let host = SpatialScope::host("testbox", BootIdentity::new("testbox", "boot"));
    assert_eq!(host.boundary_to(&host), None);
}

#[test]
fn should_report_the_boundary_when_a_movement_enters_a_container() {
    // §2.18: "Crossing a host, namespace, container or mount boundary MUST be apparent."
    let host = SpatialScope::host("testbox", BootIdentity::new("testbox", "boot"));
    let container = host.nest(ScopeKind::Container, "payments-api");
    let boundary = host.boundary_to(&container).expect("a crossing");
    assert_eq!(boundary.kind(), ScopeKind::Container);
    assert!(boundary.is_entering());
    assert!(!boundary.is_remote());
    assert!(host.contains(&container));
    assert!(!container.contains(&host));
}

#[test]
fn should_report_the_outermost_boundary_when_a_movement_crosses_several() {
    // §2.18 again: moving from a container here to a container on another host has crossed a
    // host boundary, and calling it a container boundary would understate it.
    let here = SpatialScope::host("testbox", BootIdentity::new("testbox", "boot"))
        .nest(ScopeKind::Container, "payments-api");
    let there = SpatialScope::remote_host("web01", BootIdentity::new("web01", "boot"))
        .nest(ScopeKind::Container, "edge-proxy");
    let boundary = here.boundary_to(&there).expect("a crossing");
    assert_eq!(boundary.kind(), ScopeKind::RemoteHost);
    assert!(
        boundary.is_remote(),
        "§35.4: leaving the host must be visible as leaving the host"
    );
}

#[test]
fn should_report_a_boundary_when_a_movement_leaves_a_nested_scope() {
    let host = SpatialScope::host("testbox", BootIdentity::new("testbox", "boot"));
    let namespace = host.nest(ScopeKind::Namespace, "net:[4026533331]");
    let boundary = namespace.boundary_to(&host).expect("a crossing");
    assert_eq!(boundary.kind(), ScopeKind::Namespace);
    assert!(!boundary.is_entering(), "the movement left the namespace");
}

#[test]
fn should_render_a_nested_scope_chain_outermost_first() {
    // §3.2's own example order.
    let scope = SpatialScope::host("web01", BootIdentity::new("web01", "boot"))
        .nest(ScopeKind::Container, "payments-api")
        .nest(ScopeKind::Namespace, "net:[4026533331]");
    let rendered: Vec<String> = scope.chain().iter().map(ToString::to_string).collect();
    assert_eq!(
        rendered,
        vec![
            "host:web01",
            "container:payments-api",
            "namespace:net:[4026533331]"
        ]
    );
}
