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

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "kuang")]
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

/// The same shape, with the word spec §31.26 gives a relation declared beside it.
const NAMED_SHAPE: &str = "dev.example.echo.place/1->dev.example.echo.zone/1; name=sits-in";

#[test]
fn should_follow_a_contributed_relation_by_the_name_its_shape_declares() {
    // Spec §31.26: a relation has a `name` — `opens`, `emits`, `reaches`. #224 (2): without one a
    // user typed `follow dev.example.echo.place_to_zone`. A shape that declares its name is
    // followed by that word, and arrives where the long id arrived.
    let home = echo_plugin_home_with(ECHO, TARGETS, &[NAMED_SHAPE]);
    let run = ono_with_plugins(&home, &at_the_place("follow sits-in; look | to json"));
    run.assert_success();
    let place = &rows(&run)[0]["place"];
    assert_eq!(
        place["identity"]["uid"].as_str(),
        Some("z-1"),
        "the declared word traverses the relation to the zone, got {:?}",
        run.output()
    );
}

#[test]
fn should_show_the_declared_name_of_a_contributed_relation_among_the_exits() {
    // §24.2: the word a place shows is a word a user can type, so the exit carries the name.
    let home = echo_plugin_home_with(ECHO, TARGETS, &[NAMED_SHAPE]);
    let run = ono_with_plugins(&home, &at_the_place("near | to json"));
    run.assert_success();
    let found = rows(&run);
    assert!(
        found
            .iter()
            .any(|row| row["relation"].as_str() == Some("sits-in")),
        "the exit is shown by its declared name, got {:?}",
        run.output()
    );
}

#[test]
fn should_still_follow_a_named_contributed_relation_by_its_id() {
    // The id is the relation's identity, and a script written before the name was declared keeps
    // working: the name is a second word for the relation, never a rename of it.
    let home = echo_plugin_home_with(ECHO, TARGETS, &[NAMED_SHAPE]);
    let run = ono_with_plugins(
        &home,
        &at_the_place(&format!("follow {RELATION}; look | to json")),
    );
    run.assert_success();
    assert_eq!(
        rows(&run)[0]["place"]["identity"]["uid"].as_str(),
        Some("z-1"),
        "the id still traverses the relation, got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_relation_name_that_is_not_a_word() {
    // A name is typed after `follow`, so it is one kebab-case word; anything else is a
    // declaration nobody can use, refused at load with the shape named (§31.22's registration
    // checks).
    let shape = "dev.example.echo.place/1->dev.example.echo.zone/1; name=Sits In";
    let home = echo_plugin_home_with(ECHO, TARGETS, &[shape]);
    let run = ono_with_plugins(&home, &format!("load plugin {ECHO}"));
    assert!(
        !run.status().is_success(),
        "a name that is not a word does not load, got {:?}",
        run.output()
    );
    assert!(
        run.output().contains("package.invalid")
            && run.output().contains("the relation name `Sits In`"),
        "the refusal is `package.invalid` and names what was declared, got {:?}",
        run.output()
    );
}

/// The named shape, declared `expensive` (v0.4 §32.1).
const EXPENSIVE_SHAPE: &str =
    "dev.example.echo.place/1->dev.example.echo.zone/1; name=sits-in; cost=expensive";

#[test]
fn should_leave_an_expensive_contributed_relation_on_request_in_a_default_look() {
    // v0.4 §32.1: "Default `look` and `map` MUST avoid expensive relationships unless cached or
    // already available." #224 (3): a contributed relation was always `normal`, so a package
    // whose edges cost a cluster-wide scan had no way to say so. Declared `expensive`, the exit is
    // shown and left for the caller to ask for.
    let home = echo_plugin_home_with(ECHO, TARGETS, &[EXPENSIVE_SHAPE]);
    let run = ono_with_plugins(&home, &at_the_place("look | to json"));
    run.assert_success();
    let exit = &rows(&run)[0]["exits"]["sits-in"];
    assert_eq!(
        exit["detail"].as_str(),
        Some("available on request"),
        "the expensive exit is not read by a default look, got {exit:?}"
    );
}

#[test]
fn should_read_an_expensive_contributed_relation_when_it_is_asked_for() {
    // The same exit is one word away: naming it is asking for it (§6.4, §32.1).
    let home = echo_plugin_home_with(ECHO, TARGETS, &[EXPENSIVE_SHAPE]);
    let run = ono_with_plugins(&home, &at_the_place("follow sits-in; look | to json"));
    run.assert_success();
    assert_eq!(
        rows(&run)[0]["place"]["identity"]["uid"].as_str(),
        Some("z-1"),
        "following the expensive relation asks for it, got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_relation_cost_that_is_not_a_cost_class() {
    let shape = "dev.example.echo.place/1->dev.example.echo.zone/1; cost=pricey";
    let home = echo_plugin_home_with(ECHO, TARGETS, &[shape]);
    let run = ono_with_plugins(&home, &format!("load plugin {ECHO}"));
    assert!(
        !run.status().is_success()
            && run
                .output()
                .contains("the relation cost `pricey` is not a cost class"),
        "a cost outside §32.1's classes does not load, got {:?}",
        run.output()
    );
}

/// A shape from the package's own kind to a kind another package contributes — one that is not
/// installed here (#224 (1)).
const FOREIGN_SHAPE: &str = "dev.example.echo.place/1->dev.example.racks.rack/1; name=racked-in";

#[test]
fn should_load_a_package_whose_shape_names_another_packages_kind_of_place() {
    // #224 (1): `check_shapes` refused an endpoint that was neither a declared type nor the
    // package's own schema, so no package could relate its places to another package's. The
    // other package's schemas are its own to declare, and it may not be installed: the shape is
    // accepted at load (ADR-1008).
    let home = echo_plugin_home_with(ECHO, TARGETS, &[SHAPE, FOREIGN_SHAPE]);
    let run = ono_with_plugins(
        &home,
        &at_the_place(&format!("follow {RELATION}; look | to json")),
    );
    run.assert_success();
    assert_eq!(
        rows(&run)[0]["place"]["identity"]["uid"].as_str(),
        Some("z-1"),
        "the package loads and its other relations work, got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_following_a_relation_whose_far_kind_no_loaded_package_contributes() {
    // The refusal moves to traversal, and says what it waits for — external-system-provider
    // §14.5 keeps "target known but provider not installed" distinguishable from "no such
    // relation".
    let home = echo_plugin_home_with(ECHO, TARGETS, &[FOREIGN_SHAPE]);
    let run = ono_with_plugins(&home, &at_the_place("follow racked-in"));
    assert!(!run.status().is_success(), "there is nowhere to go");
    let said = run.stderr();
    assert!(
        said.contains("spatial.unsupported")
            && said.contains("`dev.example.racks.rack/1`")
            && said.contains("no loaded package contributes"),
        "the refusal names the kind of place the relation waits for, got {:?}",
        run.output()
    );
}

#[test]
fn should_still_refuse_a_shape_naming_a_schema_of_its_own_package_it_does_not_declare() {
    // The package's own namespace is the package's to declare, on disk, before it runs: a schema
    // there that no target declares is a mistake the load can see (ADR-0585).
    let shape = "dev.example.echo.place/1->dev.example.echo.nowhere/1";
    let home = echo_plugin_home_with(ECHO, TARGETS, &[shape]);
    let run = ono_with_plugins(&home, &format!("load plugin {ECHO}"));
    assert!(
        !run.status().is_success() && run.output().contains("dev.example.echo.nowhere/1"),
        "an undeclared schema of the package's own is refused at load, got {:?}",
        run.output()
    );
}

/// Two relations from the same kind of place: a `normal` one to another resource, and the
/// `expensive` one to the zone (#224 (3), v0.4 §32.1).
const MIXED_SHAPES: [&str; 2] = [
    "dev.example.echo.place/1->dev.example.echo.place/1; name=beside",
    EXPENSIVE_SHAPE,
];

/// The relation ids the package computed, in the order it computed them.
///
/// The fixture writes each relation it answers for into its private working directory (spec
/// §31.10's per-instance state), which is what lets a suite tell a relation the host *asked the
/// package for* from one it only left out of what it showed.
fn computed(home: &ono_testkit::Scratch) -> Vec<String> {
    std::fs::read_to_string(
        home.path()
            .join("state/ono/kuang")
            .join(ECHO)
            .join("work/asserted.log"),
    )
    .unwrap_or_default()
    .lines()
    .map(str::to_owned)
    .collect()
}

#[test]
fn should_not_ask_the_package_for_an_expensive_relation_when_a_default_look_reads_a_normal_one() {
    // v0.4 §32.1: "Default `look` and `map` MUST avoid expensive relationships unless cached or
    // already available", and `contributions.v1.yaml` promises that a relation declared beyond
    // `normal` is asked for only when the caller asks for it. With a `normal` relation beside the
    // expensive one, the package was asked for every edge it had, and the expensive one was paid
    // for on every default look while the exit still said "available on request".
    let home = echo_plugin_home_with(ECHO, TARGETS, &MIXED_SHAPES);
    let run = ono_with_plugins(&home, &at_the_place("look | to json"));
    run.assert_success();
    assert_eq!(
        rows(&run)[0]["exits"]["sits-in"]["detail"].as_str(),
        Some("available on request"),
        "the expensive exit is left on request, got {:?}",
        run.output()
    );
    let asked = computed(&home);
    assert!(
        asked
            .iter()
            .any(|id| id == "dev.example.echo.place_to_place"),
        "the normal relation is read by the default look, got {asked:?}"
    );
    assert!(
        !asked
            .iter()
            .any(|id| id == "dev.example.echo.place_to_zone"),
        "the package is not asked for the expensive relation nobody asked for, got {asked:?}"
    );
}

#[test]
fn should_read_the_expensive_relation_of_a_mixed_package_when_it_is_followed() {
    // The same expensive exit is one word away, beside a normal one: `follow sits-in` asks for
    // it, and the package answers it (§6.4, §32.1).
    let home = echo_plugin_home_with(ECHO, TARGETS, &MIXED_SHAPES);
    let run = ono_with_plugins(&home, &at_the_place("follow sits-in; look | to json"));
    run.assert_success();
    assert_eq!(
        rows(&run)[0]["place"]["identity"]["uid"].as_str(),
        Some("z-1"),
        "following the expensive relation arrives at the zone, got {:?}",
        run.output()
    );
    assert!(
        computed(&home)
            .iter()
            .any(|id| id == "dev.example.echo.place_to_zone"),
        "the package was asked for the relation that was followed, got {:?}",
        computed(&home)
    );
}

#[test]
fn should_not_ask_the_package_for_an_expensive_relation_when_a_one_hop_map_is_drawn() {
    // §32.1 names `map` beside `look`: a map of the resource's immediate neighbourhood draws the
    // normal relation and does not pay for the expensive one. (A map two hops deep reads its
    // centre as completely as `look --all` does, for every relation alike.)
    let home = echo_plugin_home_with(ECHO, TARGETS, &MIXED_SHAPES);
    let run = ono_with_plugins(&home, &at_the_place("map --depth 1 | count"));
    run.assert_success();
    let asked = computed(&home);
    assert!(
        asked
            .iter()
            .any(|id| id == "dev.example.echo.place_to_place"),
        "the map reads the normal relation, got {asked:?}"
    );
    assert!(
        !asked
            .iter()
            .any(|id| id == "dev.example.echo.place_to_zone"),
        "a one-hop map does not ask for the expensive relation, got {asked:?}"
    );
}

#[test]
fn should_forget_a_waiting_relation_once_its_package_is_unloaded() {
    // ADR-1008's waiting shape outlived the package that declared it: after `unload plugin` the
    // relation still said what it waited for, and the moment another package contributed the
    // kind, the unloaded package's relation would have been registered. An unloaded package
    // contributes nothing (lifecycle.v1 `unload`), so the word means nothing here any more —
    // `spatial.unsupported` is the answer for a relation that exists and waits (ADR-1008).
    let home = echo_plugin_home_with(ECHO, TARGETS, &[SHAPE, FOREIGN_SHAPE]);
    let run = ono_with_plugins(
        &home,
        &at_the_place(&format!("unload plugin {ECHO} | count; follow racked-in")),
    );
    assert!(!run.status().is_success(), "there is nowhere to go");
    let said = run.stderr();
    assert!(
        !said.contains("spatial.unsupported") && !said.contains("dev.example.racks.rack/1"),
        "the unloaded package's waiting relation is gone: the word is refused as one nothing \
         declares, not as a relation still waiting for its kind, got {:?}",
        run.output()
    );
}

#[test]
fn should_follow_the_cost_a_reloaded_package_declares_now() {
    // A reload replaces the package (spec §31.72), and with it what its manifest declares: the
    // first declaration of a relation was kept for the rest of the session, so a package that
    // changed `cost=` on reload went on being treated by the cost it had before (ADR-1006).
    let home = echo_plugin_home_with(ECHO, TARGETS, &[EXPENSIVE_SHAPE]);
    let manifest = home.path().join("plugins").join(ECHO).join("manifest.yaml");
    let run = ono_with_plugins(
        &home,
        &format!(
            "load plugin {ECHO} --grant relation.write; \
             sed -i 's/; cost=expensive//' {}; \
             load plugin {ECHO} --grant relation.write; \
             get echo-place | where uid == \"u-3\" | enter; look | to json",
            manifest.display()
        ),
    );
    run.assert_success();
    let exit = &rows(&run)[0]["exits"]["sits-in"];
    assert_eq!(
        (exit["detail"].as_str(), exit["count"].as_i64()),
        (None, Some(1)),
        "the reloaded package declares the relation `normal`, so a default look reads it, got \
         {exit:?}"
    );
}
