//! `find place` and the cost-aware plan behind it — spec v0.4 §6.8, §9.3, §27.4, §29.3, §32.1,
//! §33.3, §34.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::collections::BTreeSet;

use common::{NOW, bridge, index, process, service};
use ono_spatial_core::{Freshness, SpatialType, types_of_target};
use ono_spatial_index::{Pin, PinRegistry, SpatialIndex};
use ono_spatial_query::discovery::{Skipped, root_fields, spatial_targets, targets_for};
use ono_spatial_query::{FindRequest, find_places};

fn indexed(records: &[ono_value::RecordValue]) -> SpatialIndex {
    let mut index = index();
    let mut bridge = bridge();
    let absorbed = bridge.absorb(&mut index, records, NOW);
    assert!(absorbed.refused().is_empty(), "{:?}", absorbed.refused());
    index
}

fn names(places: &[ono_spatial_query::FoundPlace]) -> Vec<&str> {
    places
        .iter()
        .map(ono_spatial_query::FoundPlace::name)
        .collect()
}

#[test]
fn should_answer_every_place_whose_name_contains_the_query() {
    // §6.8 with §9.3: a search over the index reaches objects nobody named in advance.
    let index = indexed(&[
        process(11, "sleep", "sleeping"),
        process(12, "sleep", "sleeping"),
        process(13, "nginx", "running"),
    ]);
    let found = find_places(
        &index,
        &FindRequest::new().matching("sleep"),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(names(&found), vec!["sleep", "sleep"]);
}

#[test]
fn should_carry_the_path_the_scope_the_freshness_and_the_provenance_on_every_result() {
    // §6.8: "Results MUST include enough path/scope information to disambiguate identical names."
    // §27.4: a result that may come from a cache carries its freshness and provenance.
    let index = indexed(&[process(1842, "nginx", "running")]);
    let found = find_places(&index, &FindRequest::new(), &PinRegistry::new(), NOW);
    let place = found.first().expect("one place");
    assert_eq!(place.place_path(), "local/compute/processes");
    assert_eq!(place.scope().to_string(), "host:testbox");
    assert_eq!(place.freshness(), Freshness::Fresh);
    assert_eq!(place.provenance().provider(), "test");
    assert_eq!(place.schema(), "ono.process/1");
    assert_eq!(place.object_type(), SpatialType::Process);
}

#[test]
fn should_keep_only_the_requested_type_when_a_type_filter_is_given() {
    // ADR-0124: `find place --type <type>`; §6.8's `find <type> <query>` with one target word.
    let index = indexed(&[
        process(1842, "nginx", "running"),
        service("nginx", "active"),
    ]);
    let found = find_places(
        &index,
        &FindRequest::new()
            .matching("nginx")
            .of_type(SpatialType::Service),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].object_type(), SpatialType::Service);
}

#[test]
fn should_rank_an_exact_name_before_a_partial_one() {
    // A user who typed the whole name meant it, and `find place x | take 1 | enter` must then
    // reach `x` rather than whatever else contains it (§28.2, §29.3).
    let index = indexed(&[
        process(11, "nginx-worker", "running"),
        process(12, "nginx", "running"),
    ]);
    let found = find_places(
        &index,
        &FindRequest::new().matching("nginx"),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(names(&found), vec!["nginx", "nginx-worker"]);
}

#[test]
fn should_rank_a_pinned_place_first_whatever_else_matches() {
    // §26.4: a pin outranks every heuristic.
    let index = indexed(&[
        process(11, "nginx", "running"),
        process(12, "nginx-worker", "running"),
    ]);
    let worker = index
        .by_alias("nginx-worker")
        .first()
        .expect("the worker is indexed")
        .object()
        .spatial_id()
        .clone();
    let mut pins = PinRegistry::new();
    pins.insert(Pin::new(
        "edge",
        worker,
        "nginx-worker",
        SpatialType::Process,
        "host:testbox",
        NOW,
    ));
    let found = find_places(&index, &FindRequest::new().matching("nginx"), &pins, NOW);
    assert_eq!(names(&found), vec!["nginx-worker", "nginx"]);
    assert!(found[0].is_pinned());
}

#[test]
fn should_bound_the_answer_by_default_and_widen_it_with_all() {
    // §34's search budget: a search answers a bounded, ranked stream. `--all` removes the bound,
    // `--limit` replaces it, and both are the user's word rather than a hidden truncation.
    let processes: Vec<ono_value::RecordValue> = (0..12)
        .map(|step| process(1000 + step, &format!("worker-{step}"), "running"))
        .collect();
    let index = indexed(&processes);
    assert_eq!(
        find_places(
            &index,
            &FindRequest::new().limit(5),
            &PinRegistry::new(),
            NOW
        )
        .len(),
        5
    );
    assert_eq!(
        find_places(
            &index,
            &FindRequest::new().all(true),
            &PinRegistry::new(),
            NOW
        )
        .len(),
        12
    );
}

#[test]
fn should_answer_the_same_order_twice_when_nothing_changed() {
    // §29.3: the same index answers the same search the same way, or `| take 1 | enter` is a
    // coin flip.
    let index = indexed(&[
        process(11, "sleep", "sleeping"),
        process(12, "sleep", "sleeping"),
        process(13, "sleep", "sleeping"),
    ]);
    let once = find_places(
        &index,
        &FindRequest::new().matching("sleep"),
        &PinRegistry::new(),
        NOW,
    );
    let twice = find_places(
        &index,
        &FindRequest::new().matching("sleep"),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(once, twice);
}

#[test]
fn should_keep_only_the_places_under_the_anchor_when_a_search_is_anchored() {
    // §6.8's `find --near <place-selector> <query>`: the anchor bounds the search to its own part
    // of the geography.
    let index = indexed(&[
        process(1842, "nginx", "running"),
        service("nginx", "active"),
    ]);
    let services = ono_spatial_core::space::space("compute.services")
        .expect("the collection")
        .spatial_id();
    let found = find_places(
        &index,
        &FindRequest::new().matching("nginx").near(services),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].object_type(), SpatialType::Service);
}

// --- the cost-aware plan behind a search (§34, §32.1, §33.3) -------------------------------------

fn plan(object_type: Option<SpatialType>, fields: &[&str]) -> ono_spatial_query::TargetPlan {
    let fields: BTreeSet<String> = root_fields(fields.iter().map(|field| (*field).to_owned()));
    targets_for(object_type, &fields, &|_| true, &|target, field| {
        matches!(
            (target, field),
            ("process", "pid" | "ppid" | "state" | "name")
                | ("service", "state" | "name")
                | ("socket", "local" | "state")
                | ("file" | "dir", "path" | "name")
        )
    })
}

#[test]
fn should_ask_only_the_targets_whose_records_could_match_the_predicate() {
    // §34: a search that asked every provider for everything would spend its whole budget before
    // it looked at the first candidate. `--where local.port == 8080` is a question about sockets.
    let narrowed = plan(None, &["local.port"]);
    assert_eq!(narrowed.asked(), ["socket"]);
    assert!(
        narrowed
            .skipped()
            .iter()
            .any(|(target, reason)| *target == "process"
                && matches!(reason, Skipped::MissingField(field) if field == "local")),
        "what was not asked, and why, is part of the answer (§2.17), got {:?}",
        narrowed.skipped()
    );
}

#[test]
fn should_not_walk_the_filesystem_when_no_one_asked_about_files() {
    // §33.3 makes files and directories query-driven and §32.1 makes enumerating them expensive:
    // `find place nginx` must not become a filesystem walk.
    let unasked = plan(None, &["name"]);
    assert!(!unasked.asked().contains(&"file"));
    assert!(!unasked.asked().contains(&"dir"));
    assert!(
        unasked.skipped().iter().any(|(target, reason)| *target == "file"
            && matches!(reason, Skipped::TooExpensive(_))),
        "got {:?}",
        unasked.skipped()
    );
    assert!(
        plan(Some(SpatialType::File), &["name"])
            .asked()
            .contains(&"file"),
        "`--type file` is asking for it"
    );
}

#[test]
fn should_ask_only_the_targets_that_serve_the_requested_type() {
    assert_eq!(plan(Some(SpatialType::Service), &[]).asked(), ["service"]);
}

#[test]
fn should_ask_nothing_when_no_schema_could_carry_the_predicate() {
    // An honest empty answer costs nothing; asking every provider for a field none of them has
    // costs the whole budget.
    assert!(plan(None, &["telepathy"]).is_empty());
}

#[test]
fn should_name_only_targets_the_spatial_geography_places() {
    // The plan's table is a join between the v0.2 target vocabulary and §7's geography; a target
    // that names no spatial type would be asked for objects that can never become places (§42).
    for target in spatial_targets() {
        assert!(
            !types_of_target(target).is_empty(),
            "`{target}` is planned for a spatial search and §7 gives its objects no place"
        );
    }
}
