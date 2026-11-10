//! Pins, `jump` and `find place` over the kinds of place a KUANG/11 package contributes (#129).
//!
//! Spec v0.4 §20.4: "`pin` marks a place as a persistent user landmark. […] If the target cannot
//! be resolved later, the pin remains but reports unresolved state." A contributed kind of place
//! is learned from a package's handshake (ADR-0584), so in a later session — where the package is
//! installed and lazy (spec §31.68) and nothing has loaded it yet — the kind a pin names is not
//! known. That must cost the one pin, never the store: `unpin`, `find place` and every other pin
//! keep working, and `jump @pin` loads the package that contributes the kind rather than refusing.
//!
//! The same lazy package answers `find place --type`/`--role` and `jump <kind>/<name>` before
//! anything has loaded it: the words a user types are the ones the installed package declares.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(all(feature = "kuang", feature = "spatial"))]
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

/// The targets the package declares on disk, with the role its resource kind carries — read
/// before any of the package's code runs (spec §31.68, ADR-0596).
const TARGETS: &str = r#"
targets:
  - name: echo-place
    schema: dev.example.echo.place/1
    summary: Resources the example package answers for.
    identity_doc: Two observations are the same resource when their `uid` matches.
    roles: [workload]
"#;

/// The example package, declaring on disk what its schemas are called (`contributions.schemas`),
/// as the Kubernetes package does: a kind word is answered by the package that declares it, and
/// only that package is loaded for it (ADR-1021).
fn home_declaring_its_schemas() -> ono_testkit::Scratch {
    let home = echo_plugin_home(ECHO, TARGETS);
    let package = home.path().join("plugins").join(ECHO);
    let manifest = std::fs::read_to_string(package.join("manifest.yaml")).expect("the manifest");
    std::fs::write(
        package.join("manifest.yaml"),
        manifest.replace(
            "  targets: [contributions/targets.yaml]",
            "  targets: [contributions/targets.yaml]\n  schemas: [contributions/schemas.yaml]",
        ),
    )
    .expect("the manifest");
    std::fs::write(
        package.join("contributions/schemas.yaml"),
        "schemas:\n  - id: dev.example.echo.place/1\n    name: EchoPlace\n",
    )
    .expect("the schema document");
    home
}

/// Writes a pin store holding one pin on a kind of place no installed package contributes — the
/// file #129 found after the Kubernetes package had been removed or not yet loaded — beside one
/// pin on a canonical space every session knows.
fn store_with_a_stranger(home: &ono_testkit::Scratch) -> std::path::PathBuf {
    let directory = home.path().join("state/ono");
    std::fs::create_dir_all(&directory).expect("the state directory");
    let path = directory.join("pins.json");
    let compute = ono_spatial_core::SpatialId::of_space("compute").to_string();
    let document = serde_json::json!({
        "version": 1,
        "pins": [
            {
                "name": "alpha",
                "spatial_id": compute,
                "selector": "ono-alpha",
                "object_type": "KubernetesNamespace",
                "scope": "host:testbox",
                "pinned_at": "2026-11-10T00:00:00Z",
            },
            {
                "name": "workshop",
                "spatial_id": compute,
                "selector": "compute",
                "object_type": "Space",
                "scope": "host:testbox",
                "pinned_at": "2026-11-10T00:00:00Z",
            },
        ],
    });
    std::fs::write(&path, document.to_string()).expect("the pin store");
    path
}

fn names_in(path: &std::path::Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).expect("the pin store is still there");
    let document: serde_json::Value = serde_json::from_str(&text).expect("a pin store");
    document["pins"]
        .as_array()
        .expect("a list of pins")
        .iter()
        .map(|pin| pin["name"].as_str().expect("a name").to_owned())
        .collect()
}

#[test]
fn should_remove_a_pin_whose_kind_of_place_nobody_contributes() {
    // #129: `unpin alpha` refused with the very error it was asked to clear, and editing the
    // file by hand was the only way out. A pin is removed by its name, whatever it names.
    let home = echo_plugin_home(ECHO, TARGETS);
    let path = store_with_a_stranger(&home);
    let run = ono_with_plugins(&home, "unpin alpha");
    run.assert_success();
    assert_eq!(
        names_in(&path),
        vec!["workshop".to_owned()],
        "the named pin is gone and the other is untouched, got {:?}",
        run.output()
    );
}

#[test]
fn should_keep_find_place_working_when_one_pin_names_a_kind_nobody_contributes() {
    // #129: one pin nobody could resolve broke `find place` for every search. §20.4 keeps such a
    // pin and has it report its own state; it does not get to answer for the store.
    let home = echo_plugin_home(ECHO, TARGETS);
    store_with_a_stranger(&home);
    let run = ono_with_plugins(&home, "find place compute | to json");
    run.assert_success();
    assert!(
        !rows(&run).is_empty(),
        "the search answers as if the stranger were not there, got {:?}",
        run.output()
    );
}

#[test]
fn should_report_a_pin_whose_kind_nobody_contributes_for_itself() {
    // §20.4: the pin "remains but reports unresolved state". `jump @alpha` is the question about
    // that pin, so it is the one place the state is reported — naming the pin and the kind.
    let home = echo_plugin_home(ECHO, TARGETS);
    store_with_a_stranger(&home);
    let run = ono_with_plugins(&home, "jump @alpha");
    assert!(!run.status().is_success(), "the pin cannot be reached");
    assert!(
        run.stderr().contains("`alpha`") && run.stderr().contains("KubernetesNamespace"),
        "the refusal names the pin and the kind nobody contributes, got {:?}",
        run.output()
    );
    let other = ono_with_plugins(&home, "jump @workshop; look | to json");
    other.assert_success();
}

#[test]
fn should_keep_a_pin_whose_kind_nobody_contributes_when_another_pin_is_written() {
    // Writing the store rewrites the file, and a pin this session could not read is still the
    // user's: §20.4 says the pin remains.
    let home = echo_plugin_home(ECHO, TARGETS);
    let path = store_with_a_stranger(&home);
    let run = ono_with_plugins(&home, "jump storage; pin --name shelf");
    run.assert_success();
    let mut names = names_in(&path);
    names.sort();
    assert_eq!(
        names,
        vec![
            "alpha".to_owned(),
            "shelf".to_owned(),
            "workshop".to_owned()
        ],
        "the new pin is added and the unreadable one is kept, got {:?}",
        run.output()
    );
}

#[test]
fn should_jump_to_a_pin_on_a_contributed_place_from_a_later_session() {
    // #129's own reproduction, with the example package standing in for Kubernetes: the place is
    // pinned in one session, and the next one — where the lazy package has not been loaded —
    // reaches it with `jump @ledger`, because the jump loads the package that contributes it.
    let home = home_declaring_its_schemas();
    let first = ono_with_plugins(
        &home,
        "get echo-place | where uid == \"u-3\" | enter; pin --name ledger",
    );
    first.assert_success();
    let later = ono_with_plugins(&home, "jump @ledger; look | to json");
    later.assert_success();
    let here = rows(&later);
    let place = &here[0]["place"];
    assert_eq!(
        place["identity"]["uid"].as_str(),
        Some("u-3"),
        "the pin reaches the resource it marked, by its identity, got {:?}",
        later.output()
    );
}

#[test]
fn should_find_places_by_a_role_an_installed_package_declares_before_it_is_loaded() {
    // #129: `find place --role workload` refused because the package declaring the role was lazy
    // and not loaded yet. The declaration on disk says who answers for the role.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(&home, "find place --role workload | to json");
    run.assert_success();
    let found = rows(&run);
    assert_eq!(
        found.len(),
        3,
        "the three resources of the kind carrying the role, got {:?}",
        run.output()
    );
    assert!(
        found
            .iter()
            .all(|place| place["object_type"].as_str() == Some(PLACE_SCHEMA)),
        "every answer is of the kind that declared the role, got {found:?}"
    );
}

#[test]
fn should_find_places_of_a_kind_an_installed_package_contributes_before_it_is_loaded() {
    // #129: `find place --type KubernetesPod` named core types only. The kind is the package's,
    // and the package is installed; the search loads it rather than calling the word unknown.
    let home = home_declaring_its_schemas();
    let run = ono_with_plugins(&home, "find place ledger --type EchoPlace | to json");
    run.assert_success();
    let found = rows(&run);
    assert_eq!(
        found.len(),
        1,
        "the one resource called `ledger`, got {:?}",
        run.output()
    );
    assert_eq!(found[0]["identity"]["uid"].as_str(), Some("u-3"));
}

#[test]
fn should_jump_to_a_contributed_place_by_its_kind_and_name() {
    // #129: `jump kubernetespod/<name>` answered `spatial.not_found`. Spec v0.4 §11.2's
    // `<type>/<key>` selector reaches a contributed kind like a declared one.
    let home = home_declaring_its_schemas();
    let run = ono_with_plugins(&home, "jump echoplace/ledger; look | to json");
    run.assert_success();
    let here = rows(&run);
    assert_eq!(
        here[0]["place"]["identity"]["uid"].as_str(),
        Some("u-3"),
        "the jump arrives at the resource of that kind and name, got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_jump_to_a_contributed_kind_and_a_name_nothing_answers_to() {
    // The kind is known and nothing of it is called `nowhere`: §40's `spatial.not_found`, which
    // is the right refusal now that the kind itself is no longer the reason.
    let home = home_declaring_its_schemas();
    let run = ono_with_plugins(&home, "jump echoplace/nowhere");
    assert!(!run.status().is_success(), "nothing is called `nowhere`");
    assert!(
        run.stderr().contains("spatial.not_found"),
        "the refusal is `spatial.not_found`, got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_jump_to_a_contributed_kind_and_a_name_two_places_answer_to() {
    // Two resources are called `checkout` (the fixture's point): §29.3 refuses a silent choice
    // between them in a script, so the refusal is the ambiguity and not a miss.
    let home = home_declaring_its_schemas();
    let run = ono_with_plugins(&home, "jump echoplace/checkout");
    assert!(!run.status().is_success(), "two places answer to the name");
    assert!(
        run.stderr().contains("spatial.ambiguous_selector"),
        "the refusal is the ambiguity, got {:?}",
        run.output()
    );
}

/// The installed packages this session loaded, after `script` ran.
fn loaded_after(home: &ono_testkit::Scratch, script: &str) -> Vec<serde_yaml_ng::Value> {
    let run = ono_with_plugins(
        home,
        &format!("{script}; get plugin | where state == \"loaded\" | to json"),
    );
    rows(&run)
}

#[test]
fn should_load_no_package_for_a_jump_to_a_kind_no_package_declares() {
    // K11P: a package is loaded on its first invocation, not on a typo. `jump typo/x` names a
    // kind no installed package declares, and is answered `spatial.not_found` — it used to start
    // every package that declares targets but no `contributions.schemas` document on the way.
    let home = echo_plugin_home(ECHO, TARGETS);
    let loaded = loaded_after(&home, "jump typo/x");
    assert!(
        loaded.is_empty(),
        "a word no declaration names starts nothing, got {loaded:?}"
    );
}

#[test]
fn should_load_no_package_for_a_search_by_a_kind_no_package_declares() {
    let home = echo_plugin_home(ECHO, TARGETS);
    let loaded = loaded_after(&home, "find place ledger --type Typo | count");
    assert!(
        loaded.is_empty(),
        "`find place --type` with a kind nobody declares starts nothing, got {loaded:?}"
    );
}

#[test]
fn should_load_no_package_for_a_pin_whose_kind_no_package_declares() {
    // The pin names `KubernetesNamespace`, which the installed package does not declare.
    let home = echo_plugin_home(ECHO, TARGETS);
    store_with_a_stranger(&home);
    let loaded = loaded_after(&home, "jump @alpha");
    assert!(
        loaded.is_empty(),
        "a pin on a kind nobody declares starts nothing, got {loaded:?}"
    );
}

#[test]
fn should_reach_the_kinds_of_a_package_without_a_schema_document_once_it_is_loaded() {
    // ADR-1021's limitation, stated as what still works: a package that names its schemas only in
    // its handshake is not started for a kind word, and once anything has loaded it, its kinds
    // are known like any other.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; jump echoplace/ledger; look | to json"),
    );
    run.assert_success();
    assert_eq!(
        rows(&run)[0]["place"]["identity"]["uid"].as_str(),
        Some("u-3"),
        "the loaded package's kind is reachable by name, got {:?}",
        run.output()
    );
}
