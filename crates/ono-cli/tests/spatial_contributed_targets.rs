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
use support::{echo_plugin_home, last_json_rows as rows, ono_with_plugins};

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
    // `echo-tick` answers until it is cancelled, and declares as much (ADR-0588). A search still
    // has to finish, so it asks for as many objects as it can answer with and stops there, which
    // is what `Query::limit` is for. A search that named no limit would be refused rather than
    // hang.
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
fn should_find_places_by_the_semantic_role_their_kind_declares() {
    // External-system-provider §15.5 and §25: a package registers its native kinds under small
    // semantic roles, and place search reaches them by role. `echo-place` declares `workload`
    // in its handshake, so `find place --role workload` asks that target — expensive or not,
    // because the role asked for it by name — and every place it answers carries the role beside
    // its native type, which stays exactly what it was (ADR-0596).
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; find place --role workload | to json"),
    );
    run.assert_success();
    let found = rows(&run);
    assert_eq!(
        found.len(),
        3,
        "the three resources of the one kind carrying `workload`, got {:?}",
        run.output()
    );
    for place in &found {
        assert_eq!(
            place["roles"][0].as_str(),
            Some("workload"),
            "the role is on the place record, got {place:?}"
        );
        assert!(
            place["roles"][1].is_null(),
            "one role and no other, got {place:?}"
        );
        assert_eq!(
            place["object_type"].as_str(),
            Some(PLACE_SCHEMA),
            "the native type is preserved beside the role, got {place:?}"
        );
    }
}

#[test]
fn should_refuse_a_role_no_loaded_package_declares() {
    // A role nobody answers for is a question the shell cannot answer, not a search that finds
    // nothing — the rule `--type` already follows (ADR-0596).
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; find place --role storage | to json"),
    );
    assert!(
        run.stderr()
            .contains("no loaded package answers for the role `storage`")
            && run.stderr().contains("workload"),
        "the refusal names the roles that exist, got {:?}",
        run.output()
    );
}

#[test]
fn should_exclude_a_kind_that_carries_no_role_from_a_role_search() {
    // The zone kind declares no role, so a role search never answers with a zone, even one the
    // session already holds (ADR-0596).
    let home = support::echo_plugin_home_with(ECHO, TARGETS_WITH_ZONE, &[PLACE_TO_ZONE]);
    let run = ono_with_plugins(
        &home,
        &format!(
            "load plugin {ECHO}; find place --type EchoZone | count; find place --role workload \
             | where object_type == \"dev.example.echo.zone/1\" | count"
        ),
    );
    run.assert_success();
    let counts: Vec<&str> = run
        .stdout()
        .lines()
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(
        counts.last().copied(),
        Some("0"),
        "no zone answers a `workload` search, got {:?}",
        run.output()
    );
}

/// The targets, with the zone kind the place kind declares as its parent.
const TARGETS_WITH_ZONE: &str = r#"
targets:
  - name: echo-place
    schema: dev.example.echo.place/1
    summary: Resources the example package answers for.
    identity_doc: Two observations are the same resource when their `uid` matches.
    roles: [workload]
    parent: dev.example.echo.zone/1
  - name: echo-zone
    schema: dev.example.echo.zone/1
    summary: Zones the example package's resources sit in.
    identity_doc: Two observations are the same zone when their `uid` matches.
"#;

/// The shape that carries the containment edge from a place to its zone.
const PLACE_TO_ZONE: &str = "dev.example.echo.place/1->dev.example.echo.zone/1";

#[test]
fn should_go_up_from_a_contributed_place_to_the_parent_its_package_declared() {
    // Spec v0.4 §11.3 and §36.4: a package declares the kind of place above each kind it
    // contributes, and `up` is that hierarchy — spatial containment along the relation the
    // package contributes for the pair, never an ownership shortcut (ADR-0597). The place `u-3`
    // sits in zone `z-1`, and that is where `up` lands.
    let home = support::echo_plugin_home_with(ECHO, TARGETS_WITH_ZONE, &[PLACE_TO_ZONE]);
    let run = ono_with_plugins(
        &home,
        &format!(
            "load plugin {ECHO} --grant relation.write; get echo-place | where uid == \"u-3\" \
             | enter; up; look --json"
        ),
    );
    run.assert_success();
    // `look --json` is the place view, one JSON object; the place itself is its `place` member.
    let line = run
        .stdout()
        .lines()
        .rfind(|line| line.starts_with('{'))
        .unwrap_or_else(|| panic!("a place view on stdout, got {:?}", run.output()));
    let view = support::json(line);
    let place = &view["place"];
    assert_eq!(
        place["object_type"].as_str(),
        Some("dev.example.echo.zone/1"),
        "`up` lands on the zone the package declared as the parent, got {:?}",
        run.output()
    );
    assert_eq!(
        place["identity"]["uid"].as_str(),
        Some("z-1"),
        "the zone the edge names, got {place:?}"
    );
}

#[test]
fn should_refuse_up_from_the_top_of_what_a_package_contributes() {
    // The zone declares no parent: it is the top of what the package contributes, and `up` says
    // so rather than filing it under a domain of this host (§2.17; ADR-0597).
    let home = support::echo_plugin_home_with(ECHO, TARGETS_WITH_ZONE, &[PLACE_TO_ZONE]);
    let run = ono_with_plugins(
        &home,
        &format!(
            "load plugin {ECHO} --grant relation.write; get echo-zone | where uid == \"z-1\" \
             | enter; up"
        ),
    );
    assert!(
        run.stderr()
            .contains("is the top of what its package contributes"),
        "`up` from the top of a contributed hierarchy names the reason, got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_declared_parent_whose_shape_the_manifest_does_not_carry() {
    // A parent needs an edge to be reached along, and the edge needs a shape. A target that
    // declares a parent in a package whose manifest declares no such shape is refused at load,
    // before the runtime is spawned — the same moment a shape naming nobody is refused
    // (ADR-0585, ADR-0597).
    let home = echo_plugin_home(ECHO, TARGETS_WITH_ZONE);
    let run = ono_with_plugins(&home, &format!("load plugin {ECHO}"));
    assert!(
        run.stderr().contains("no relation shape") && run.stderr().contains(PLACE_TO_ZONE),
        "the package is refused naming the missing shape, got {:?}",
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
    let home = support::echo_plugin_home_with(ECHO, TARGETS_WITH_ZONE, &[PLACE_TO_ZONE]);
    // With `relation.write` denied the package contributes no edge, so the parent it declared
    // cannot be reached — and the refusal says which grant is missing rather than that the
    // host's hierarchy ends here (ADR-0597). Denied rather than merely not granted, because a
    // bounded relation contribution is included without a question (K11P §7.1).
    let run = ono_with_plugins(
        &home,
        &format!(
            "set permission {ECHO} relation-write --decision deny | count; \
             load plugin {ECHO}; get echo-place | where uid == \"u-3\" | enter; up"
        ),
    );
    assert!(
        run.stderr()
            .contains("no canonical domain holds this kind of place")
            && run.stderr().contains("relation.write"),
        "`up` from a contributed place must refuse with the reason it actually has, got {:?}",
        run.output()
    );
}
