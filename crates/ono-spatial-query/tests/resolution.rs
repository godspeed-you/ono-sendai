//! Selector resolution as a caller sees it — spec v0.4 §27.1, §27.2, §27.3, §27.4, §29.3, §40.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use common::{NOW, bridge, index, process, service};
use ono_core::ErrorCode;
use ono_spatial_core::{Freshness, SpatialId, SpatialType, space};
use ono_spatial_index::SpatialIndex;
use ono_spatial_query::{Resolution, ResolutionStep, SelectorContext, place_path, resolve};
use ono_value::Value;

/// An index holding the given records, absorbed through the provider bridge.
fn indexed(records: &[ono_value::RecordValue]) -> SpatialIndex {
    let mut index = index();
    let mut bridge = bridge();
    let absorbed = bridge.absorb(&mut index, records, NOW);
    assert!(
        absorbed.refused().is_empty(),
        "the fixture records must all become places, got {:?}",
        absorbed.refused()
    );
    index
}

fn resolved(index: &SpatialIndex, selector: &str) -> SpatialId {
    match resolve(index, selector, &SelectorContext::anywhere(), NOW) {
        Resolution::Resolved(candidate) => candidate.spatial_id().clone(),
        other => panic!("`{selector}` must resolve to exactly one place, got {other:?}"),
    }
}

#[test]
fn should_answer_one_place_when_a_name_is_held_by_exactly_one_object() {
    // §27.1 step 5: the current-host spatial index answers a name nobody had to know in advance,
    // which is §9's whole point — discovery without prior exact names.
    let index = indexed(&[
        process(1842, "nginx", "running"),
        process(9, "sleep", "sleeping"),
    ]);
    let found = resolved(&index, "nginx");
    assert_eq!(
        index
            .get(&found)
            .expect("the place")
            .object()
            .display_name(),
        "nginx"
    );
}

#[test]
fn should_answer_ambiguous_with_the_disambiguating_context_when_two_places_share_a_name() {
    // §27.2: the picker MUST show disambiguating context — name, type, place path. A script gets
    // the same three columns as a structured refusal instead of a picker (§29.3).
    let index = indexed(&[
        process(11, "sleep", "sleeping"),
        process(12, "sleep", "sleeping"),
    ]);
    let Resolution::Ambiguous(candidates) =
        resolve(&index, "sleep", &SelectorContext::anywhere(), NOW)
    else {
        panic!("two processes called `sleep` are two places");
    };
    assert_eq!(candidates.len(), 2);
    for candidate in &candidates {
        assert_eq!(candidate.name(), "sleep");
        assert_eq!(candidate.object_type(), SpatialType::Process);
        assert_eq!(
            candidate.place_path(),
            "local/compute/processes",
            "§27.2's third column is the place path"
        );
    }
}

#[test]
fn should_refuse_with_the_ambiguous_selector_error_when_a_script_resolves_a_shared_name() {
    // §29.3 with §40: a script never opens a picker; ambiguity is `spatial.ambiguous_selector`,
    // and the refusal carries the candidates so the user can choose.
    let index = indexed(&[
        process(11, "sleep", "sleeping"),
        process(12, "sleep", "sleeping"),
    ]);
    let error = resolve(&index, "sleep", &SelectorContext::anywhere(), NOW)
        .require("sleep")
        .expect_err("two matches cannot resolve to one");
    assert_eq!(error.code(), ErrorCode::SpatialAmbiguousSelector);
    assert!(
        error.message().contains("local/compute/processes"),
        "the refusal names where each candidate is, got {:?}",
        error.message()
    );
}

#[test]
fn should_offer_but_never_take_a_fuzzy_match_when_no_name_matches_exactly() {
    // §27.3: "Fuzzy matching MUST NOT execute destructive operations automatically." A selector
    // that only approximately matches is therefore never resolved — it is offered.
    let index = indexed(&[process(1842, "nginx-worker", "running")]);
    let Resolution::Fuzzy(candidates) = resolve(&index, "nginx", &SelectorContext::anywhere(), NOW)
    else {
        panic!("`nginx` matches `nginx-worker` only approximately");
    };
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].name(), "nginx-worker");

    let error = resolve(&index, "nginx", &SelectorContext::anywhere(), NOW)
        .require("nginx")
        .expect_err("a fuzzy match never acts on its own");
    assert_eq!(error.code(), ErrorCode::SpatialNotFound);
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("nginx-worker")),
        "§40 wants actionable next steps, got {:?}",
        error.help()
    );
}

#[test]
fn should_prefer_an_exact_index_match_over_an_approximate_visible_one() {
    // §27.1 orders a fuzzy visible match above the index, and §27.3 forbids a fuzzy match from
    // acting. Both hold only if an exact answer anywhere outranks an approximate one: otherwise
    // a name the user typed in full would resolve to nothing.
    let index = indexed(&[
        process(1842, "nginx-worker", "running"),
        process(1843, "nginx", "running"),
    ]);
    let found = resolved(&index, "nginx");
    assert_eq!(
        index
            .get(&found)
            .expect("the place")
            .object()
            .display_name(),
        "nginx"
    );
}

#[test]
fn should_resolve_a_canonical_space_by_its_label_or_its_dotted_id() {
    // §27.1 step 3: an exact canonical identifier. The six domains and their collections are
    // reachable by the words a user reads in a place view (§7, §41.1).
    let index = indexed(&[]);
    assert_eq!(
        resolved(&index, "compute"),
        space::space("compute").expect("the domain").spatial_id()
    );
    assert_eq!(
        resolved(&index, "compute.services"),
        space::space("compute.services")
            .expect("the collection")
            .spatial_id()
    );
}

#[test]
fn should_resolve_the_exact_spatial_id_even_where_no_name_was_ever_typed() {
    // §29.3's third escape from ambiguity: "uses an exact ID". The id is opaque and stable, so a
    // second session that never saw the name still reaches the same place (§3.1, §42.1).
    let index = indexed(&[
        process(11, "sleep", "sleeping"),
        process(12, "sleep", "sleeping"),
    ]);
    let one = index
        .by_alias("11")
        .first()
        .expect("the pid is an alias")
        .object()
        .spatial_id()
        .clone();
    assert_eq!(resolved(&index, one.as_str()), one);
}

#[test]
fn should_narrow_to_one_place_when_the_selector_names_the_space_it_is_in() {
    // A `<space>:<key>` selector is a canonical identifier (§27.1 step 3), and it is what makes
    // one key unambiguous where two places of the geography hold the same name — the bare
    // `nginx` here is `spatial.ambiguous_selector`, and each scoped spelling is not.
    let index = indexed(&[
        process(1842, "nginx", "running"),
        service("nginx", "active"),
    ]);
    assert!(
        matches!(
            resolve(&index, "nginx", &SelectorContext::anywhere(), NOW),
            Resolution::Ambiguous(_)
        ),
        "a process and a service both called `nginx` are two places"
    );
    let service_place = resolved(&index, "compute.services:nginx");
    assert_eq!(
        index
            .get(&service_place)
            .expect("the place")
            .object()
            .object_type(),
        SpatialType::Service
    );
    let process_place = resolved(&index, "compute.processes:nginx");
    assert_eq!(
        index
            .get(&process_place)
            .expect("the place")
            .object()
            .object_type(),
        SpatialType::Process
    );
}

#[test]
fn should_report_the_freshness_and_the_provenance_of_every_candidate() {
    // §27.4: "Search results MUST include freshness/provenance when they may come from cached
    // indexes." Every answer out of the index may.
    let index = indexed(&[process(1842, "nginx", "running")]);
    let Resolution::Resolved(candidate) =
        resolve(&index, "nginx", &SelectorContext::anywhere(), NOW)
    else {
        panic!("one process called `nginx` is one place");
    };
    assert_eq!(candidate.freshness(), Freshness::Fresh);
    assert_eq!(
        candidate
            .provenance()
            .expect("an observed place states where it came from")
            .provider(),
        "test"
    );
    assert_eq!(candidate.step(), ResolutionStep::HostIndex);
}

#[test]
fn should_not_reach_a_linked_host_unless_the_caller_asked_for_it() {
    // §9.3 with §35.4 and §47's `spatial.remote_search = explicit`: discovery does not cross a
    // link because a name resembles something over there.
    let index = indexed(&[process(1842, "nginx", "running")]);
    let context = SelectorContext::anywhere();
    assert!(matches!(
        resolve(&index, "nginx", &context, NOW),
        Resolution::Resolved(_)
    ));
    assert!(
        matches!(
            resolve(&index, "nginx", &context.clone().across_links(true), NOW),
            Resolution::Resolved(_)
        ),
        "asking for links does not change what the local index answers"
    );
}

#[test]
fn should_restrict_the_answer_to_the_requested_type_when_a_type_is_given() {
    // ADR-0124: the spatial type is an option, and it narrows resolution the same way it narrows
    // a search — one verb, one target, the type as a filter.
    let index = indexed(&[
        process(1842, "nginx", "running"),
        service("nginx", "active"),
    ]);
    let found = resolve(
        &index,
        "nginx",
        &SelectorContext::anywhere().of_type(SpatialType::Service),
        NOW,
    );
    let Resolution::Resolved(candidate) = found else {
        panic!("only one `nginx` is a service, got {found:?}");
    };
    assert_eq!(candidate.object_type(), SpatialType::Service);
}

#[test]
fn should_name_the_place_path_of_a_canonical_space_from_the_host_down() {
    // §27.2's third column and §6.8's "path/scope information": the path is the canonical
    // hierarchy, from the host down, and it is what tells two identical names apart.
    let index = indexed(&[]);
    assert_eq!(
        place_path(
            &index,
            &space::space("compute.services")
                .expect("a space")
                .spatial_id()
        ),
        "local/compute/services"
    );
}

#[test]
fn should_answer_a_place_path_rather_than_looping_when_the_hierarchy_holds_a_cycle() {
    // §11.3 makes the canonical parent deterministic, and a path that walks it must terminate
    // whatever the index was told. Two directories filed inside each other is a cycle a
    // non-canonical path spelling can produce, and a walk that follows it forever is a crashed
    // shell rather than a wrong answer.
    let one = common::record(
        "ono.file/1",
        &[
            ("path", Value::string("/srv/one")),
            ("kind", Value::string("dir")),
            ("device", Value::string("0:1")),
            ("inode", Value::Int(11)),
        ],
    );
    let two = common::record(
        "ono.file/1",
        &[
            ("path", Value::string("/srv/two")),
            ("kind", Value::string("dir")),
            ("device", Value::string("0:1")),
            ("inode", Value::Int(12)),
        ],
    );
    let mut index = indexed(&[one, two]);
    let ids: Vec<SpatialId> = index
        .of_type(SpatialType::Directory)
        .into_iter()
        .map(|entry| entry.object().spatial_id().clone())
        .collect();
    assert_eq!(ids.len(), 2, "the fixture is two directories, got {ids:?}");
    assert!(index.set_path_parent(&ids[0], &ids[1]));
    assert!(index.set_path_parent(&ids[1], &ids[0]));

    let path = place_path(&index, &ids[0]);
    assert!(
        path.starts_with("local"),
        "§27.2: the path still starts at the host, got {path}"
    );
}
