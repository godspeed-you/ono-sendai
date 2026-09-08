//! A KUANG/11 package can declare a relation between kinds of place it contributes itself.
//!
//! ADR-0584 made a contributed target a kind of place and said, in its own Consequences, exactly
//! where it stopped:
//!
//! > **`near` finds nothing and `follow` has nothing to follow.** Both are graph questions, and a
//! > contributed place has no edges. A package *can* contribute edges […] but a relation shape is
//! > written `<from>-><to>` in the declared vocabulary of §3.3, so today a package can only assert
//! > edges between *core* types.
//!
//! What these tests hold is the outcome over the real `ono` binary: a shape whose endpoints are
//! the package's own schemas registers a relation, `near` shows the exit it opens, `follow`
//! traverses it, the edge names the package that asserted it — and a shape naming a kind of place
//! nobody contributes is refused when the package is loaded, before any of its code has run.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;
use support::{echo_plugin_home_with, last_json_rows as rows, ono_with_plugins};

const ECHO: &str = "dev.example.echo";
const ZONE_SCHEMA: &str = "dev.example.echo.zone/1";

/// The relation the host registers for the shape below.
///
/// §31.5 reserves `<publisher>.<package>` to its owner, so a contributed relation lives inside
/// the package's own namespace; the two halves of the name are the kinds of place it runs
/// between, as `relations.yaml` writes a declared relation's `source` and `target`.
const RELATION: &str = "dev.example.echo.place_to_zone";

/// The shape the package declares in its manifest: two of its own contributed kinds of place.
const SHAPE: &str = "dev.example.echo.place/1->dev.example.echo.zone/1";

/// The targets the package declares on disk, readable before its code runs (§31.64, §31.68).
///
/// Both endpoints of the shape are here, which is what lets the host settle the shape at load:
/// the schema ids are on disk beside the shape that names them.
const TARGETS: &str = r#"
targets:
  - name: echo-place
    schema: dev.example.echo.place/1
    summary: Resources the example package answers for.
    identity_doc: Two observations are the same resource when their `uid` matches.
  - name: echo-zone
    schema: dev.example.echo.zone/1
    summary: Zones the example package's resources sit in.
    identity_doc: Two observations are the same zone when their `uid` matches.
"#;

/// The script prefix every test shares: the package loaded with the grant §35.5 gates the
/// contribution on, standing on the resource the edge starts at.
fn at_the_place(command: &str) -> String {
    format!(
        "load plugin {ECHO} --grant relation.write; \
         get echo-place | where uid == \"u-3\" | enter; {command}"
    )
}

#[test]
fn should_show_a_contributed_relation_among_the_exits_of_a_contributed_place() {
    // Spec v0.4 §6.2: `near` answers with what surrounds the current place, and §36.1 lets a
    // package contribute the relationship providers that say what that is. Until now the exits
    // of a contributed place were empty however many edges a package asserted, because a shape
    // could only name a core type and no core type is what the package answers with.
    let home = echo_plugin_home_with(ECHO, TARGETS, &[SHAPE]);
    let run = ono_with_plugins(&home, &at_the_place("near | to json"));
    run.assert_success();
    let found = rows(&run);
    let along: Vec<&serde_yaml_ng::Value> = found
        .iter()
        .filter(|row| row["relation"].as_str() == Some(RELATION))
        .collect();
    assert_eq!(
        along.len(),
        1,
        "the exit the package's own shape opened reaches the one zone it asserted an edge to, \
         got {:?}",
        run.output()
    );
    assert_eq!(
        along[0]["object_type"].as_str(),
        Some(ZONE_SCHEMA),
        "the neighbour is a place of the second contributed kind, got {:?}",
        along[0]
    );
    assert_eq!(
        along[0]["identity"]["uid"].as_str(),
        Some("z-1"),
        "the neighbour is the zone the package named, bound by the identity its schema declares, \
         got {:?}",
        along[0]
    );
}

#[test]
fn should_follow_a_contributed_relation_to_a_place_of_another_contributed_kind() {
    // Spec v0.4 §6.4: "`follow` MUST traverse a relationship edge." The relation is one the
    // package declared, both of its ends are kinds of place the package contributed, and the
    // traversal is the ordinary one — the session ends up standing on the far end.
    let home = echo_plugin_home_with(ECHO, TARGETS, &[SHAPE]);
    let run = ono_with_plugins(
        &home,
        &at_the_place(&format!("follow {RELATION}; look | to json")),
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
        Some(ZONE_SCHEMA),
        "following the contributed relation arrives at the contributed kind it leads to, got \
         {place:?}"
    );
    assert_eq!(
        place["identity"]["uid"].as_str(),
        Some("z-1"),
        "the place entered is the one the edge names, not one that shares its display name, got \
         {place:?}"
    );
}

#[test]
fn should_name_the_contributing_package_in_the_evidence_of_a_contributed_edge() {
    // Spec §31.25 keeps evidence inspectable and §31.64 records origin on every registry entry;
    // v0.4 §36.2 forbids "uninspectable phantom edges" and §53 settles that a plugin "cannot
    // create untraceable truth". So an edge a package asserted is distinguishable from one the
    // host derived: the provenance names the package, the evidence names the package, and the
    // confidence is never raised to `exact` for something the host did not observe.
    let home = echo_plugin_home_with(ECHO, TARGETS, &[SHAPE]);
    let run = ono_with_plugins(&home, &at_the_place("near | to json"));
    run.assert_success();
    let found = rows(&run);
    let edge = found
        .iter()
        .find(|row| row["relation"].as_str() == Some(RELATION))
        .unwrap_or_else(|| panic!("the contributed exit, got {:?}", run.output()));
    assert_eq!(
        edge["provider"].as_str(),
        Some(ECHO),
        "§11.4: a displayed relationship says who asserted it, and this one was asserted by the \
         package, got {edge:?}"
    );
    assert_eq!(
        edge["provenance"]["provider"].as_str(),
        Some(ECHO),
        "§31.57: a value's provenance names the contributing package, got {edge:?}"
    );
    assert_eq!(
        edge["evidence"]["origin"].as_str(),
        Some(ECHO),
        "§31.25, §36.2: the evidence beside the edge stays inspectable and names the package, so \
         a contributed edge is never mistaken for one the host observed, got {edge:?}"
    );
    assert_eq!(
        edge["provider_relation"].as_str(),
        Some("sits-in"),
        "§11.4: the contributor's own word for the relation travels with the edge, got {edge:?}"
    );
    assert_ne!(
        edge["confidence"].as_str(),
        Some("exact"),
        "§22.2, §36.2: the host did not observe this edge, so it is never presented as exact, \
         got {edge:?}"
    );
}

#[test]
fn should_contribute_no_relation_between_contributed_places_when_the_permission_is_denied() {
    // §35.5: "the spatial host MUST filter plugin nodes/edges according to capability scope
    // **before** merging them into maps". A bounded `relation.write` is included without a
    // question (K11P §7.1), so the gate is the user's decision: with the permission denied, a
    // shape between two contributed kinds is a contribution like any other and the same gate
    // holds for it — the exit does not exist at all.
    let home = echo_plugin_home_with(ECHO, TARGETS, &[SHAPE]);
    let run = ono_with_plugins(
        &home,
        &format!(
            "set permission {ECHO} relation-write --decision deny | count; \
             load plugin {ECHO}; get echo-place | where uid == \"u-3\" | enter; near | to json"
        ),
    );
    assert!(
        !run.output().contains(RELATION),
        "with `relation.write` denied the package contributes no relation, so no exit of the \
         place bears its name, got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_relation_shape_whose_endpoint_nobody_contributes() {
    // The ordering problem ADR-0584 named: shapes are read from the manifest and contributed
    // types are learned from the handshake, so a shape naming a type that is not yet known
    // cannot be checked where it is read. It is settled by reading both halves from disk — the
    // shape from the manifest, the schemas from the `contributions.targets` documents §31.68
    // already reads without running anything — so a wrong declaration is refused at load, with
    // the endpoint named, rather than at the moment somebody types `follow`.
    let home = echo_plugin_home_with(ECHO, TARGETS, &["Pod->dev.example.echo.zone/1"]);
    let run = ono_with_plugins(&home, &format!("load plugin {ECHO}"));
    assert!(
        !run.status().is_success(),
        "a package whose declaration names a kind of place nobody contributes does not load, got \
         {:?}",
        run.output()
    );
    let said = run.output();
    assert!(
        said.contains("Pod"),
        "the refusal names the endpoint that could not be resolved, got {said:?}"
    );
    assert!(
        said.contains("Pod->dev.example.echo.zone/1"),
        "the refusal names the shape it came from, got {said:?}"
    );
}
