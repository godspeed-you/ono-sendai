//! A target a KUANG/11 package contributes is a kind of place the spatial commands can reach.
//!
//! ADR-0583 put a loaded package into the `ProviderRegistry` and said plainly where it stopped:
//!
//! > `enter`, `near` and `find` plan over `SpatialType::ALL`, a closed vocabulary in
//! > `ono-spatial-query`; a contributed noun is not in it, so the plan never asks for the target
//! > even though the registry would now answer.
//!
//! ADR-0584 opens that vocabulary. What these tests hold is the outcome, over the real `ono`
//! binary: the objects of a contributed target are found by `find place`, entered by `enter`, and
//! bound to the identity their schema declares rather than to the name a person reads.
//!
//! The example package's `echo-place` target is shaped like the external resources the
//! external-system-provider specification is written for (§11.1, §11.2, §35.4): its schema's
//! `identity` is `uid`, its `name` is a separate field, and two of the three resources it answers
//! with carry the *same* name and different identities. A shell that bound a place to its name
//! could not tell those two apart, so the fixture is the assertion.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;
use support::{echo_plugin_home, last_json_document, ono_with_plugins};

const ECHO: &str = "dev.example.echo";
const PLACE_SCHEMA: &str = "dev.example.echo.place/1";

/// The package's declaration of the targets it answers for, readable before its code runs
/// (spec §31.64, §31.68).
const TARGETS: &str = r#"
targets:
  - name: echo-item
    schema: dev.example.echo.item/1
    summary: Items the example package provides.
    identity_doc: Two observations are the same item when their `seq` matches.
  - name: echo-place
    schema: dev.example.echo.place/1
    summary: Resources the example package answers for.
    identity_doc: Two observations are the same resource when their `uid` matches.
"#;

fn rows(run: &ono_testkit::Run) -> Vec<serde_yaml_ng::Value> {
    let document = last_json_document(run);
    document
        .as_sequence()
        .unwrap_or_else(|| panic!("a sequence of records, got {:?}", run.output()))
        .clone()
}

#[test]
fn should_find_the_objects_of_a_contributed_target_as_places() {
    // Spec v0.4 §6.8 searches "the spatial index **and** provider registries". Until now the
    // search planned over a compile-time list of targets, so a noun a package contributed could
    // never be planned for however well the registry answered for it.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; find place ledger --type EchoPlace | to json"),
    );
    run.assert_success();
    let found = rows(&run);
    assert_eq!(
        found.len(),
        1,
        "one resource is called `ledger`, got {:?}",
        run.output()
    );
    assert_eq!(
        found[0]["object_type"].as_str(),
        Some(PLACE_SCHEMA),
        "the place is the contributed object, carrying the schema the package declared, got {:?}",
        found[0]
    );
    assert_eq!(
        found[0]["name"].as_str(),
        Some("ledger"),
        "the place is called what the resource is called, got {:?}",
        found[0]
    );
}

#[test]
fn should_keep_two_contributed_resources_of_one_name_apart_by_identity() {
    // The external-system-provider specification §11.1 and §11.2: identity is composed from what
    // the provider says makes the resource that resource, and it "MUST survive renaming when the
    // provider allows it". Two resources sharing a name is the same statement read the other way
    // round — a place bound to a name would be one place here, and it is two.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; find place checkout --type EchoPlace | to json"),
    );
    run.assert_success();
    let found = rows(&run);
    assert_eq!(
        found.len(),
        2,
        "two resources are called `checkout` and they are two places, got {:?}",
        run.output()
    );
    let mut identities: Vec<String> = found
        .iter()
        .map(|place| {
            place["identity"]["uid"]
                .as_str()
                .unwrap_or_else(|| panic!("the place binds the schema's identity, got {place:?}"))
                .to_owned()
        })
        .collect();
    identities.sort();
    assert_eq!(
        identities,
        vec!["u-1".to_owned(), "u-2".to_owned()],
        "each place binds the `uid` its record carried, got {found:?}"
    );
    assert_ne!(
        found[0]["spatial_id"], found[1]["spatial_id"],
        "two resources of one name are two identities, got {found:?}"
    );
}

#[test]
fn should_enter_a_place_of_a_contributed_target() {
    // Spec v0.4 §6.3 and §28.2: an object a pipeline produced is enterable, and the place it
    // arrives at is the object. `look` then reports where the session stands.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!(
            "load plugin {ECHO}; get echo-place | where uid == \"u-2\" | enter; look | to json"
        ),
    );
    run.assert_success();
    let here = rows(&run);
    assert_eq!(
        here.len(),
        1,
        "`look` reports one place, got {:?}",
        run.output()
    );
    let place = &here[0]["place"];
    assert_eq!(
        place["object_type"].as_str(),
        Some(PLACE_SCHEMA),
        "the session stands in the contributed object, got {place:?}"
    );
    assert_eq!(
        place["identity"]["uid"].as_str(),
        Some("u-2"),
        "entering binds the identity of the resource entered, not the name it shares with \
         another, got {place:?}"
    );
}

#[test]
fn should_bind_the_lifetime_identity_of_a_contributed_place_and_not_its_name() {
    // Spec v0.4 §3.1: "identity MUST NOT be the display name", and §10.1 fixes what a host may
    // claim about how long an identity lasts. A package states the fields; the host states the
    // tier, and the weakest tier it can prove is the one that holds for as long as the resource
    // does (§10.1 Tier B).
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; find place ledger --type EchoPlace | to json"),
    );
    run.assert_success();
    let found = rows(&run);
    let place = &found[0];
    assert_eq!(
        place["identity"]["uid"].as_str(),
        Some("u-3"),
        "the identity is composed from the schema's `identity` field, got {place:?}"
    );
    assert!(
        place["identity"]["name"].is_null(),
        "the name a person reads is not part of what makes the place that place (spec v0.4 \
         §3.1), got {place:?}"
    );
    assert_eq!(
        place["identity_tier"].as_str(),
        Some("lifetime"),
        "a host cannot prove a package's identity outlives the resource, so it claims no more \
         than the resource's own lifetime (spec v0.4 §10.1), got {place:?}"
    );
    assert_eq!(
        place["canonical_ref"]["schema"].as_str(),
        Some(PLACE_SCHEMA),
        "the place keeps the provider's own reference to the object, got {place:?}"
    );
}

#[test]
fn should_not_enumerate_a_contributed_target_that_nothing_asked_for() {
    // Spec v0.4 §32.1 and §34, and the external-system-provider specification §12.6: enumerating
    // an external system may mean a network round trip per resource, and a package has no way to
    // declare that its target is cheap. So a contributed target is `expensive`, and a search
    // reaches it only when it was asked to — exactly as `dir` and `file` are reached (§33.3).
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; find place ledger | to json"),
    );
    run.assert_success();
    assert!(
        rows(&run).is_empty(),
        "an untyped search must not fan out to every loaded package, got {:?}",
        run.output()
    );
}

#[test]
fn should_finish_a_search_over_a_contributed_target_that_never_ends() {
    // `echo-tick` answers until it is cancelled. A package declares no boundedness for a target
    // it contributes, so the host does not take an unbounded read on trust: the search asks for
    // as many objects as it can answer with and stops there, which is what `Query::max` is for.
    // The assertion is that the command returns at all.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; find place --type EchoItem --limit 2 | to json"),
    );
    run.assert_success();
    assert!(
        !rows(&run).is_empty(),
        "an endless contributed target still answers with the places it did emit, got {:?}",
        run.output()
    );
}

#[test]
fn should_say_that_no_domain_holds_a_contributed_place_when_up_has_nowhere_to_go() {
    // Spec v0.4 §7 declares six domains and §36.4 lets a package declare an aggregate space of
    // its own — with an id, a label, a parent domain and a membership query. A contributed
    // *target* declares none of that, so a place of that kind sits in no collection and `up` has
    // nowhere to go. §40 wants the refusal to say which question failed, and "you are at the top
    // of this host" would be an answer about the host rather than about the place.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-place | where uid == \"u-3\" | enter; up"),
    );
    assert!(
        run.stderr()
            .contains("no canonical domain holds this kind of place"),
        "`up` from a contributed place must refuse with the reason it actually has, got {:?}",
        run.output()
    );
}
