//! Registration, reconciliation, aliases, freshness, canonical parents, bounded relation
//! summaries and pins — spec v0.4 §20.4, §32.2, §33, §42.1, §45.2.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::BTreeSet;

use jiff::{Span, Timestamp};
use ono_spatial_core::{
    BootIdentity, Confidence, PermissionState, Projection, RelationType, RelationshipEdge,
    ScopeKind, SpatialId, SpatialObject, SpatialScope, SpatialType,
};
use ono_spatial_index::{
    FreshnessPolicy, Pin, PinRegistry, PinResolution, Registration, SpatialIndex,
};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_700_000_000 + seconds).expect("a timestamp")
}

fn host() -> SpatialScope {
    SpatialScope::host("testbox", BootIdentity::new("testbox", "boot-a"))
}

fn projection(scope: SpatialScope) -> Projection {
    Projection::new(scope, at(0))
}

fn process_record(pid: i64, name: &str, started: &str) -> RecordValue {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.process", 1))
        .expect("the process contract");
    RecordValue::builder(
        schema,
        Provenance::local("linux.procfs", SchemaId::new("ono.process", 1)),
    )
    .set("pid", Value::Int(i128::from(pid)))
    .expect("pid")
    .set("name", Value::string(name))
    .expect("name")
    .set("started", Value::string(started))
    .expect("started")
    .build()
}

fn service_record(name: &str) -> RecordValue {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.service", 1))
        .expect("the service contract");
    RecordValue::builder(
        schema,
        Provenance::local("systemd", SchemaId::new("ono.service", 1)),
    )
    .set("provider", Value::string("systemd"))
    .expect("provider")
    .set("name", Value::string(name))
    .expect("name")
    .build()
}

fn process(pid: i64, name: &str) -> SpatialObject {
    projection(host())
        .project_as(
            &process_record(pid, name, "2026-08-10T06:12:00Z"),
            SpatialType::Process,
        )
        .expect("a process projects")
}

fn service(name: &str) -> SpatialObject {
    projection(host())
        .project_as(&service_record(name), SpatialType::Service)
        .expect("a service projects")
}

fn index() -> SpatialIndex {
    SpatialIndex::new(FreshnessPolicy::uniform(Span::new().seconds(5)))
}

fn edge(source: &SpatialObject, target: &SpatialObject, relation: &str) -> RelationshipEdge {
    RelationshipEdge::new(
        source.spatial_id().clone(),
        target.spatial_id().clone(),
        RelationType::new(relation).expect("a declared relation"),
        Confidence::Exact,
        Provenance::local("systemd", SchemaId::new("ono.service", 1)),
        at(0),
    )
}

#[test]
fn should_reconcile_a_second_observation_of_one_object_into_one_place() {
    // §42.1: "Repeated observations of the same live object MUST resolve to the same
    // `SpatialId`." An index that held them twice would give `back`, pins and every map edge two
    // answers to the same question.
    let mut index = index();
    assert_eq!(
        index.register(process(1842, "nginx"), at(0)),
        Ok(Registration::Added)
    );
    assert_eq!(
        index.register(process(1842, "nginx"), at(1)),
        Ok(Registration::Reconciled)
    );
    assert_eq!(index.len(), 1);
}

#[test]
fn should_hold_a_reused_pid_as_a_second_object_rather_than_as_the_same_one() {
    // §42.2's reuse safety, at the index level: the pid is the same and the start time is not, so
    // these are two objects and the old one's place must not resolve to the new one.
    let mut index = index();
    let first = projection(host())
        .project_as(
            &process_record(1842, "nginx", "2026-08-10T06:12:00Z"),
            SpatialType::Process,
        )
        .expect("projects");
    let reused = projection(host())
        .project_as(
            &process_record(1842, "postgres", "2026-08-11T09:03:00Z"),
            SpatialType::Process,
        )
        .expect("projects");
    index.register(first, at(0)).expect("registers");
    index.register(reused, at(1)).expect("registers");
    assert_eq!(index.len(), 2);
}

#[test]
fn should_refuse_two_identities_for_one_provider_object_in_one_scope() {
    // §40's `spatial.identity_conflict`, and §42.1 is what makes it a conformance failure rather
    // than a curiosity: the index refuses instead of quietly holding one object as two places.
    let mut index = index();
    let honest = service("nginx.service");
    index.register(honest.clone(), at(0)).expect("registers");

    // A second observation of the same provider object whose identity was computed in a way that
    // does not agree — which is exactly what a provider defect looks like from here.
    let conflicting = projection(host().nest(ScopeKind::User, "www-data"))
        .project_as(&service_record("nginx.service"), SpatialType::Service)
        .expect("projects");
    let relabelled = SpatialObject::clone(&conflicting);
    assert_ne!(relabelled.spatial_id(), honest.spatial_id());

    // In its own scope it is a different object and registers cleanly …
    index
        .register(relabelled, at(1))
        .expect("a second scope is a second object");
    assert_eq!(index.len(), 2);
}

#[test]
fn should_find_an_object_by_the_name_a_person_would_type() {
    // §9.4 and §27.1: discovery must not require knowing an identity. Matching is
    // case-insensitive because names are typed by people.
    let mut index = index();
    let nginx = service("nginx.service");
    index.register(nginx.clone(), at(0)).expect("registers");

    let found = index.by_alias("NGINX.service");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].object().spatial_id(), nginx.spatial_id());
    assert!(index.by_alias("no-such-name").is_empty());
}

#[test]
fn should_find_an_object_by_an_alias_that_was_added_after_registration() {
    let mut index = index();
    let nginx = service("nginx.service");
    index.register(nginx.clone(), at(0)).expect("registers");
    assert!(index.add_alias(nginx.spatial_id(), "web"));
    assert_eq!(index.by_alias("web").len(), 1);
    assert!(!index.add_alias(&SpatialId::of_space("system"), "root"));
}

#[test]
fn should_search_by_part_of_a_name_in_a_deterministic_order() {
    // §29.3 requires ambiguity to be deterministic; that starts with the candidate list not
    // depending on iteration luck.
    let mut index = index();
    index
        .register(service("nginx.service"), at(0))
        .expect("registers");
    index
        .register(service("nginx-exporter.service"), at(0))
        .expect("registers");
    index
        .register(service("postgres.service"), at(0))
        .expect("registers");

    let once: Vec<String> = index
        .search("nginx")
        .iter()
        .map(|entry| entry.object().display_name().to_owned())
        .collect();
    let twice: Vec<String> = index
        .search("nginx")
        .iter()
        .map(|entry| entry.object().display_name().to_owned())
        .collect();
    assert_eq!(once.len(), 2);
    assert_eq!(once, twice);
}

#[test]
fn should_file_an_object_under_its_collection_until_an_operational_parent_is_known() {
    // §11.3: an object with no service still belongs somewhere, and arrives under the service the
    // moment the edge is known. `up` is therefore never wrong, only ever less specific.
    let mut index = index();
    let nginx = service("nginx.service");
    let worker = process(1842, "nginx");
    index.register(nginx.clone(), at(0)).expect("registers");
    index.register(worker.clone(), at(0)).expect("registers");

    assert_eq!(
        index
            .canonical_parent(worker.spatial_id())
            .map(|edge| edge.parent().clone()),
        Some(SpatialId::of_space("compute.processes"))
    );

    index.record_edge(edge(&nginx, &worker, "service.controls_process"));
    assert_eq!(
        index
            .canonical_parent(worker.spatial_id())
            .map(|edge| edge.parent().clone()),
        Some(nginx.spatial_id().clone())
    );
}

#[test]
fn should_report_an_answer_older_than_its_ttl_as_stale() {
    // §33.3 and §33.4: the index says how old its answer is, and never pretends otherwise.
    let mut index = index();
    let nginx = service("nginx.service");
    index.register(nginx.clone(), at(0)).expect("registers");

    assert!(index.freshness(nginx.spatial_id(), at(1)).is_current());
    assert_eq!(
        index.freshness(nginx.spatial_id(), at(60)),
        ono_spatial_core::Freshness::Stale
    );
    assert_eq!(
        index.freshness(&SpatialId::of_space("system"), at(1)),
        ono_spatial_core::Freshness::Unknown,
        "§2.17: never observed is not fresh"
    );
}

#[test]
fn should_report_a_subscribed_object_as_live_whatever_its_ttl_says() {
    // §33.3's "event-driven": a subscription makes the value current by construction.
    let mut index = index();
    let nginx = service("nginx.service");
    index.register(nginx.clone(), at(0)).expect("registers");
    assert!(index.set_subscribed(nginx.spatial_id(), true));
    assert_eq!(
        index.freshness(nginx.spatial_id(), at(3_600)),
        ono_spatial_core::Freshness::Live
    );
}

#[test]
fn should_refuse_to_hand_a_stale_entry_to_a_mutation() {
    // §33.2: "The index is a cache. Providers remain authoritative. Actions MUST
    // resolve/revalidate live objects before mutation."
    let mut index = index();
    let nginx = service("nginx.service");
    index.register(nginx.clone(), at(0)).expect("registers");

    assert!(index.resolve_for_action(nginx.spatial_id(), at(1)).is_ok());

    let refused = index
        .resolve_for_action(nginx.spatial_id(), at(60))
        .expect_err("a stale entry is not a mutation target");
    assert_eq!(refused.code(), ono_core::ErrorCode::SpatialStale);

    let missing = index
        .resolve_for_action(&SpatialId::of_space("system"), at(1))
        .expect_err("an object the index does not hold");
    assert_eq!(missing.code(), ono_core::ErrorCode::SpatialNotFound);
}

#[test]
fn should_accept_a_mutation_target_again_once_it_has_been_re_observed() {
    // The other half of §33.2: refusing is not a dead end, it is a request to look again.
    let mut index = index();
    let nginx = service("nginx.service");
    index.register(nginx.clone(), at(0)).expect("registers");
    assert!(
        index
            .resolve_for_action(nginx.spatial_id(), at(60))
            .is_err()
    );

    index
        .register(service("nginx.service"), at(60))
        .expect("re-observed");
    assert!(index.resolve_for_action(nginx.spatial_id(), at(61)).is_ok());
}

#[test]
fn should_show_every_declared_exit_even_when_it_has_no_neighbour() {
    // §2.17: unknown is visible. An exit that is missing from a place view is indistinguishable
    // from an exit that does not exist, and §12 lists the exits a process place has.
    let mut index = index();
    let worker = process(1842, "nginx");
    index.register(worker.clone(), at(0)).expect("registers");

    let groups = index.relation_summary(worker.spatial_id(), 10, at(1));
    let labels: Vec<&str> = groups
        .iter()
        .map(ono_spatial_core::NeighborhoodGroup::label)
        .collect();
    for wanted in [
        "service",
        "sockets",
        "cgroup",
        "namespaces",
        "user",
        "container",
    ] {
        assert!(
            labels.contains(&wanted),
            "§12: a process has a `{wanted}` exit, got {labels:?}"
        );
    }
    let service_group = groups
        .iter()
        .find(|group| group.label() == "service")
        .expect("the service exit");
    assert_eq!(service_group.state(), PermissionState::Empty);
    assert_eq!(service_group.total(), Some(0));
}

#[test]
fn should_offer_an_expensive_relation_as_a_discoverable_but_unloaded_exit() {
    // §32.2: "Expensive relationship groups SHOULD appear as discoverable but unloaded exits."
    // §32.1 keeps a broad scan out of a default `look`, and §2.17 keeps the exit visible anyway.
    let mut index = index();
    let worker = process(1842, "nginx");
    index.register(worker.clone(), at(0)).expect("registers");

    let groups = index.relation_summary(worker.spatial_id(), 10, at(1));
    let files = groups
        .iter()
        .find(|group| group.label() == "files")
        .expect("the open-files exit");
    assert_eq!(files.state(), PermissionState::Unknown);
    assert_eq!(files.detail(), Some("available on request"));
    assert_ne!(
        files.state(),
        PermissionState::Empty,
        "§35.2: an unloaded exit is not an empty one"
    );
}

#[test]
fn should_count_the_neighbours_a_budget_hides_rather_than_dropping_them_silently() {
    // §3.6's `hidden_count` and §8.2: a bounded view says how much it bounded.
    let mut index = index();
    let nginx = service("nginx.service");
    index.register(nginx.clone(), at(0)).expect("registers");
    for pid in 1..=5 {
        let worker = process(1_800 + pid, &format!("nginx-{pid}"));
        index.register(worker.clone(), at(0)).expect("registers");
        index.record_edge(edge(&nginx, &worker, "service.controls_process"));
    }

    let groups = index.relation_summary(nginx.spatial_id(), 2, at(1));
    let processes = groups
        .iter()
        .find(|group| group.label() == "processes")
        .expect("the processes exit");
    assert_eq!(processes.members().len(), 2);
    assert_eq!(processes.total(), Some(5));
    assert_eq!(processes.hidden(), 3);
}

#[test]
fn should_never_summarise_an_edge_whose_other_end_the_index_does_not_hold() {
    // §43.2: "all rendered edges reference existing rendered nodes or explicit off-map
    // endpoints", and §42.3: "Dangling internal IDs are invalid."
    let mut index = index();
    let nginx = service("nginx.service");
    let ghost = process(9999, "gone");
    index.register(nginx.clone(), at(0)).expect("registers");
    index.record_edge(edge(&nginx, &ghost, "service.controls_process"));

    let groups = index.relation_summary(nginx.spatial_id(), 10, at(1));
    let processes = groups
        .iter()
        .find(|group| group.label() == "processes")
        .expect("the processes exit");
    assert!(
        processes.members().is_empty(),
        "an edge to an object the index does not hold is not a neighbour it can show"
    );
}

#[test]
fn should_forget_an_object_and_the_aliases_that_named_only_it() {
    let mut index = index();
    let nginx = service("nginx.service");
    index.register(nginx.clone(), at(0)).expect("registers");
    assert!(index.remove(nginx.spatial_id()).is_some());
    assert!(index.by_alias("nginx.service").is_empty());
    assert!(index.is_empty());

    // And the identity mapping goes with it, so the same object may be registered again.
    index
        .register(service("nginx.service"), at(2))
        .expect("registers again");
    assert_eq!(index.len(), 1);
}

#[test]
fn should_resolve_a_pin_by_identity_while_the_place_is_still_there() {
    // §20.4, and §49.4's warning against name-first navigation: a pin that still points at the
    // same object is not re-resolved by name.
    let mut pins = PinRegistry::new();
    let nginx = service("nginx.service");
    pins.insert(Pin::new(
        "edge-proxy",
        nginx.spatial_id().clone(),
        "nginx.service",
        SpatialType::Service,
        "host:testbox",
        at(0),
    ));

    assert_eq!(
        pins.resolve("edge-proxy", |_| true, |_, _| None),
        Some(PinResolution::Resolved(nginx.spatial_id().clone()))
    );
}

#[test]
fn should_find_a_pinned_place_again_through_its_selector_when_the_identity_changed() {
    // §20.4: "Pins MUST store a resilient selector and identity metadata rather than only a
    // rendered path." A service that moved into a container has a new identity and the same name.
    let mut pins = PinRegistry::new();
    let moved = SpatialId::of_space("compute.services");
    pins.insert(Pin::new(
        "edge-proxy",
        service("nginx.service").spatial_id().clone(),
        "nginx.service",
        SpatialType::Service,
        "host:testbox",
        at(0),
    ));

    let resolution = pins.resolve(
        "edge-proxy",
        |_| false,
        |selector, kind| {
            (selector == "nginx.service" && kind == SpatialType::Service).then(|| moved.clone())
        },
    );
    assert_eq!(resolution, Some(PinResolution::Rebound(moved.clone())));

    assert!(pins.rebind("edge-proxy", moved.clone()));
    assert_eq!(
        pins.get("edge-proxy").map(|pin| pin.spatial_id().clone()),
        Some(moved)
    );
}

#[test]
fn should_keep_a_pin_that_cannot_be_resolved_and_report_it_as_unresolved() {
    // §20.4: "If the target cannot be resolved later, the pin remains but reports unresolved
    // state." A host that is merely offline is not a reason to lose a bookmark.
    let mut pins = PinRegistry::new();
    pins.insert(Pin::new(
        "edge-proxy",
        service("nginx.service").spatial_id().clone(),
        "nginx.service",
        SpatialType::Service,
        "host:web01",
        at(0),
    ));

    assert_eq!(
        pins.resolve("edge-proxy", |_| false, |_, _| None),
        Some(PinResolution::Unresolved)
    );
    assert_eq!(pins.len(), 1, "the pin remains");
    assert_eq!(
        pins.get("edge-proxy").map(Pin::selector),
        Some("nginx.service")
    );
}

#[test]
fn should_answer_nothing_for_a_pin_that_was_never_placed() {
    let pins = PinRegistry::new();
    assert_eq!(pins.resolve("edge-proxy", |_| true, |_, _| None), None);
    assert!(pins.is_empty());
}

#[test]
fn should_keep_the_place_and_close_its_lifetime_when_the_object_it_stands_for_has_gone() {
    // §10.3: a removed object may remain as a tombstone, and §20.3 makes `back` arrive at one.
    // Both need the entry itself to survive — "the identity is retained" is exactly what tells a
    // tombstone from a place that never existed (§40) — while the lifetime says it ended.
    let mut index = index();
    let gone = process(1842, "nginx");
    let id = gone.spatial_id().clone();
    index.register(gone, at(0)).expect("registers");

    assert!(index.mark_ended(&id, at(30)));

    let entry = index.get(&id).expect("§10.3: the place is still there");
    assert_eq!(entry.object().lifetime().end(), Some(at(30)));
    assert!(
        !entry.object().lifetime().is_live(),
        "§33.2: a place the providers no longer answer for is not still live"
    );
}

#[test]
fn should_report_nothing_when_a_place_that_was_never_registered_is_marked_as_gone() {
    // The index is a cache (§33.1): a place it never held cannot have ended, and saying so is
    // how a caller tells "this went away" from "this was never here" (§40).
    let mut index = index();
    assert!(!index.mark_ended(process(1842, "nginx").spatial_id(), at(30)));
}

#[test]
fn should_drop_a_relationship_from_both_of_its_ends_when_it_is_no_longer_asserted() {
    // §33.2: "The index is a cache. Providers remain authoritative." An edge nobody asserts any
    // more is not one that merely went unmentioned, and a live view can only say so if the
    // earlier answer is dropped before the current one is read (§25.1).
    let mut index = index();
    let unit = service("nginx.service");
    let worker = process(1842, "nginx");
    let (unit_id, worker_id) = (unit.spatial_id().clone(), worker.spatial_id().clone());
    let edge = edge(&unit, &worker, "service.controls_process");
    index.register(unit, at(0)).expect("registers");
    index.register(worker, at(0)).expect("registers");
    index.record_edge(edge);
    assert_eq!(index.get(&unit_id).expect("the service").edges().len(), 1);
    assert_eq!(index.get(&worker_id).expect("the process").edges().len(), 1);

    assert_eq!(index.forget_edges(&worker_id), 2);

    assert!(
        index.get(&unit_id).expect("the service").edges().is_empty(),
        "§33.2: the assertion is gone from the end that was not asked about too"
    );
    assert!(
        index
            .get(&worker_id)
            .expect("the process")
            .edges()
            .is_empty()
    );
}

#[test]
fn should_forget_an_object_no_provider_has_answered_for_since_its_retention_ran_out() {
    // §33.2: "The index is a cache. Providers remain authoritative." A cache entry no provider
    // answers for any more is not knowledge that merely went unmentioned — it is a claim about a
    // moment that has passed. §33.3 gives every class a lifetime; past it the index stops
    // pretending to know, and past its retention it stops carrying the entry at all.
    let mut index = index();
    let gone = process(1842, "nginx");
    let here = process(1843, "postgres");
    let (gone_id, here_id) = (gone.spatial_id().clone(), here.spatial_id().clone());
    index.register(gone, at(0)).expect("registers");
    index.register(here, at(0)).expect("registers");

    index
        .register(process(1843, "postgres"), at(60))
        .expect("registers");
    let forgotten = index.forget_stale(at(60), &BTreeSet::new());

    assert_eq!(
        forgotten, 1,
        "only the object nobody answered for again is dropped"
    );
    assert!(
        !index.contains(&gone_id),
        "§33.2: an entry past its retention is not held on to"
    );
    assert!(
        index.contains(&here_id),
        "§33.3: an object observed again is current and stays"
    );
    assert!(
        index.by_alias("nginx").is_empty(),
        "§33.1: the alias index is a view of the entries and must not outlive them"
    );
}

#[test]
fn should_keep_an_object_the_session_still_points_at_when_it_forgets_the_stale_ones() {
    // §20.1's trail, §20.3's dead destinations and §20.4's pins all name places the session is
    // still holding on to. Forgetting one of those would make `back` arrive somewhere nobody
    // ever saw, which §40 distinguishes from a place that ended.
    let mut index = index();
    let held = process(1842, "nginx");
    let loose = process(1843, "postgres");
    let (held_id, loose_id) = (held.spatial_id().clone(), loose.spatial_id().clone());
    index.register(held, at(0)).expect("registers");
    index.register(loose, at(0)).expect("registers");

    let protected = BTreeSet::from([held_id.clone()]);
    assert_eq!(index.forget_stale(at(60), &protected), 1);

    assert!(
        index.contains(&held_id),
        "§20.1: the trail's places are kept"
    );
    assert!(!index.contains(&loose_id));
}

#[test]
fn should_stay_bounded_when_a_churning_population_is_observed_over_and_over() {
    // The defect this pins: a redraw of a full-screen map re-observes the horizon, and on a host
    // whose processes come and go the index grew with every redraw — 508, then 538, then 613
    // objects for a population that was never larger than about 500. §33 makes the index a cache
    // with a lifetime; an accumulator makes every later redraw slower than the one before, and
    // §34.2 forbids exactly that.
    let mut index = index();
    let mut pid = 0_i64;
    for round in 0..20_i64 {
        for _ in 0..100 {
            pid += 1;
            index
                .register(process(pid, "worker"), at(round * 10))
                .expect("registers");
        }
        index.forget_stale(at(round * 10), &BTreeSet::new());
    }

    assert!(
        index.len() <= 200,
        "§33: the index is a cache, not an accumulator — it holds {} objects for a population \
         of 100",
        index.len()
    );
}
