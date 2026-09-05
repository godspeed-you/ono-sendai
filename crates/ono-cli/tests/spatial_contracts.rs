//! The contracts under the v0.4 Spatial Systems Interface: the error taxonomy, the
//! machine-readable registry, provider conformance, and the two integrations the spatial layer
//! inherits — KUANG/11 packages and the v0.3 external command adapters — plus the session state,
//! the configuration keys and the performance budgets that bound all of it.
//!
//! Narrative: `docs/specs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` — §40 (every
//! spatial refusal is a structured error with a name from the taxonomy), §41 (the registry that
//! keeps renderer, provider, parser and documentation from drifting into different definitions
//! of the world), §42 (provider conformance for spatial objects), §36 (KUANG/11 may extend the
//! world under capabilities and may never create untraceable truth), §37 (typed canonical
//! adapter objects may enter the spatial index after identity reconciliation; raw text never
//! does), §46 (spatial session state), §47 (configuration), §34 (performance budgets), and the
//! settled answers of §53.
//!
//! `crates/ono-cli/tests/provider_conformance.rs` is the model for the drift tests: a declaration no
//! implementation serves is a promise nobody keeps, and a served thing no declaration names is
//! undocumented surface. Both fail here rather than waiting for a user to notice.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ono_testkit::ono;
use ono_testkit::{Scratch, Shell, scratch};
use serde_yaml_ng::Value;

mod support;
use support::{balanced_end, field, list_at, ono_at_home, search};

/// The §11.5 confidence vocabulary.
const CONFIDENCE: [&str; 5] = ["exact", "strong", "inferred", "user_declared", "unknown"];

/// The name the two twins of the ambiguity test answer to.
///
/// Fifteen characters at most: the kernel's `comm`, which `ono.process/1` reports as `name`, is
/// the basename of the exec'd file truncated to that length, and a fixture that wants to be
/// resolved by its name must fit inside it.
const TWIN_NAME: &str = "ono-twin-place";

/// The permission states of §35.2, which "MUST remain distinct".
const PERMISSION_STATES: [&str; 6] = [
    "available",
    "empty",
    "unknown",
    "permission_denied",
    "unsupported",
    "stale",
];

/// The error names §40 lists as required. The list is normative: "Required error codes include".
const SPATIAL_ERRORS: [&str; 14] = [
    "spatial.not_found",
    "spatial.ambiguous_selector",
    "spatial.not_enterable",
    "spatial.no_relation",
    "spatial.no_parent",
    "spatial.history_empty",
    "spatial.destination_gone",
    "spatial.permission_denied",
    "spatial.unsupported",
    "spatial.stale",
    "spatial.remote_unavailable",
    "spatial.scope_violation",
    "spatial.map_too_large",
    "spatial.identity_conflict",
];

/// The configuration keys §47 calls required, with the defaults it spells out.
const SPATIAL_SETTINGS: [(&str, &str); 11] = [
    ("spatial.enabled", "true"),
    ("spatial.startup_horizon", "true"),
    ("spatial.follow_cwd", "storage-only"),
    ("spatial.map.mode", "auto"),
    ("spatial.map.live", "false"),
    ("spatial.map.node_budget", "100"),
    ("spatial.look.change_window", "5m"),
    ("spatial.landmarks.enabled", "true"),
    ("spatial.reduced_motion", "false"),
    ("spatial.remote_search", "explicit"),
    ("spatial.trail.persist", "false"),
];

// --- reading what the shell wrote -------------------------------------------------------------

/// Whether `text` opens like JSON rather than like a rendered table cell. `serde_yaml_ng` parses
/// JSON *and* YAML flow mappings such as the `{name: testbox}` an `ActionResult` table prints,
/// and only the former belongs in a document scan.
fn opens_like_json(text: &str) -> bool {
    let mut characters = text.chars();
    let opening = characters.next();
    let next = characters.find(|character| !character.is_whitespace());
    match (opening, next) {
        (Some('{'), Some('"' | '}')) => true,
        (Some('['), Some(character)) => {
            matches!(character, '"' | '{' | '[' | ']' | '-' | 't' | 'f' | 'n')
                || character.is_ascii_digit()
        }
        _ => false,
    }
}

/// Every JSON document a script wrote to stdout, in order. A v0.4 script mixes documents with
/// plain lines — `load plugin` prints a summary of its own — so the scanner takes the documents
/// and ignores everything between them.
fn documents(run: &ono_testkit::Run) -> Vec<Value> {
    let characters: Vec<char> = run.stdout().chars().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        if matches!(characters[index], '{' | '[')
            && let Some(end) = balanced_end(&characters, index)
            && let text = characters[index..=end].iter().collect::<String>()
            && opens_like_json(&text)
            && let Ok(value) = serde_yaml_ng::from_str::<Value>(&text)
        {
            found.push(value);
            index = end + 1;
            continue;
        }
        index += 1;
    }
    found
}

fn nth_document(run: &ono_testkit::Run, index: usize, what: &str) -> Value {
    documents(run).get(index).cloned().unwrap_or_else(|| {
        panic!(
            "{what} — expected at least {} JSON documents on stdout, got {:?}; stderr: {:?}",
            index + 1,
            run.stdout(),
            run.stderr()
        )
    })
}

fn document(run: &ono_testkit::Run, what: &str) -> Value {
    let found = documents(run);
    assert_eq!(
        found.len(),
        1,
        "{what} — expected exactly one JSON document on stdout, got {:?}; stderr: {:?}",
        run.stdout(),
        run.stderr()
    );
    nth_document(run, 0, what)
}

/// The error a `try { … } catch e { $e | to json }` caught.
fn caught(run: &ono_testkit::Run, what: &str) -> Value {
    let value = document(run, what);
    let rows = value.as_sequence().cloned().unwrap_or_else(|| vec![value]);
    rows.first().cloned().unwrap_or_else(|| {
        panic!(
            "{what} — the block produced no error value; stdout was {:?}, stderr {:?}",
            run.stdout(),
            run.stderr()
        )
    })
}

fn text_at(document: &Value, path: &str, what: &str) -> String {
    field(document, path)
        .as_str()
        .unwrap_or_else(|| panic!("{what} — `{path}` must be a string, got {document:?}"))
        .to_owned()
}

fn rendered(value: &Value) -> String {
    serde_yaml_ng::to_string(value).expect("a value serialises")
}

/// Waits until the kernel reports `pid` under `name`, so a fixture process is observable before
/// the shell is asked to resolve it. `spawn` returns once the fork has succeeded; the `exec` that
/// gives the child its name happens a moment later.
fn wait_until_named(pid: u32, name: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .is_ok_and(|comm| comm.trim() == name)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the fixture process {pid} never appeared under the name `{name}`");
}

// --- reading the contracts --------------------------------------------------------------------

fn spec_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts")
}

/// One of the §41 registry documents, at either spelling this repository's conventions allow:
/// `docs/contracts/spatial/<name>` (the layout `providers/`, `commands/`, `schemas/` and `kuang/`
/// already use) or the flat `docs/contracts/<name>` §41 writes literally.
fn registry_path(name: &str) -> PathBuf {
    let nested = spec_dir().join("spatial").join(name);
    if nested.exists() {
        return nested;
    }
    let flat = spec_dir().join(name);
    assert!(
        flat.exists(),
        "§41: the registry is missing — neither `docs/contracts/spatial/{name}` nor \
         `docs/contracts/{name}` exists. v0.4 §41 requires machine-readable contracts sufficient to \
         generate help, completion, tests and SDK bindings; without them the renderer, the \
         providers, the parser and the documentation drift into different definitions of the \
         world (§41 Intent)."
    );
    flat
}

fn registry(name: &str) -> Value {
    let path = registry_path(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("§41: `{}` must be readable: {error}", path.display()));
    serde_yaml_ng::from_str(&text)
        .unwrap_or_else(|error| panic!("§41: `{}` must be valid YAML: {error}", path.display()))
}

/// The entries of a registry document, whether it spells them as a top-level list or under a
/// key named after the file.
fn entries(document: &Value, key: &str) -> Vec<Value> {
    document
        .as_sequence()
        .cloned()
        .or_else(|| document.get(key).and_then(|v| v.as_sequence().cloned()))
        .unwrap_or_else(|| panic!("§41: `{key}` is a list of declarations, got {document:?}"))
}

/// Every provider declaration under `docs/contracts/providers/`.
fn provider_declarations() -> Vec<Value> {
    let dir = spec_dir().join("providers");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("docs/contracts/providers/ must exist: {error}"))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        })
        .collect();
    files.sort();
    files
        .iter()
        .flat_map(|path| {
            let text = std::fs::read_to_string(path).expect("a readable declaration");
            let parsed: Value = serde_yaml_ng::from_str(&text).expect("valid YAML");
            parsed["providers"]
                .as_sequence()
                .unwrap_or_else(|| panic!("{} declares a `providers` list", path.display()))
                .clone()
        })
        .collect()
}

// --- §40 the error model ----------------------------------------------------------------------

#[test]
fn should_register_the_whole_spatial_error_family_in_the_error_taxonomy() {
    // §40 names fourteen required codes. `docs/contracts/errors.yaml` is the closed, additive
    // taxonomy of spec v0.2 §43 (ADR-0006): E00xx parse … E08xx stream, E09xx adapter (v0.3),
    // K11xxx KUANG/11. No family covers spatial refusals today, so v0.4 needs a new one — the
    // next free block is E10xx — and every one of the fourteen names must land in it with a
    // code, a kind from the registry's own `kinds`, a summary and a help line.
    let text =
        std::fs::read_to_string(spec_dir().join("errors.yaml")).expect("the error registry exists");
    let document: Value = serde_yaml_ng::from_str(&text).expect("the error registry is valid YAML");
    let kinds: BTreeSet<String> = document["kinds"]
        .as_sequence()
        .expect("the registry declares its kinds")
        .iter()
        .filter_map(|kind| kind["name"].as_str().map(str::to_owned))
        .collect();
    let errors = document["errors"]
        .as_sequence()
        .expect("the registry declares its errors")
        .clone();

    for required in SPATIAL_ERRORS {
        let entry = errors
            .iter()
            .find(|error| error["name"].as_str() == Some(required))
            .unwrap_or_else(|| {
                panic!(
                    "§40: `{required}` is one of the fourteen required spatial error codes and \
                     docs/contracts/errors.yaml does not define it. The taxonomy is closed and \
                     additive (ADR-0006), so v0.4 adds a new family — E10xx, the next free \
                     block after the E09xx adapter family — rather than reusing a code."
                )
            });
        let code = entry["code"].as_str().unwrap_or_default();
        assert!(
            code.starts_with("Ono-Sendai-E"),
            "spec v0.2 §43: `{required}` carries a stable, user-visible code, got {entry:?}"
        );
        let kind = entry["kind"].as_str().unwrap_or_default();
        assert!(
            kinds.contains(kind),
            "ADR-0006: `{required}` is one of the registry's own kinds {kinds:?}, got {kind:?}"
        );
        for line in ["summary", "help"] {
            assert!(
                entry[line]
                    .as_str()
                    .is_some_and(|value| !value.trim().is_empty()),
                "§40: errors carry actionable next steps, so `{required}` needs a `{line}`, got \
                 {entry:?}"
            );
        }
    }
}

#[test]
fn should_refuse_an_unknown_place_with_a_structured_spatial_error() {
    // §40: "Spatial operations MUST emit structured errors." A name nothing answers to is
    // `spatial.not_found`, not the generic `resolve.target_not_found` v0.2 raises today.
    let run = ono("try { enter no-such-place-anywhere } catch e { $e | to json }");
    let error = caught(&run, "§40: the refusal is a structured error value");
    assert_eq!(
        text_at(&error, "name", "§40: an error carries its dotted name"),
        "spatial.not_found",
        "§40: a spatial selector nothing resolves is `spatial.not_found`, got {error:?}"
    );
    assert!(
        text_at(&error, "code", "§40: an error carries its stable code")
            .starts_with("Ono-Sendai-E"),
        "spec v0.2 §43: the code is the stable identity, got {error:?}"
    );
}

#[test]
fn should_refuse_an_ambiguous_selector_in_a_script_rather_than_open_a_picker() {
    // §29.3: "Scripts MUST never open interactive pickers. Ambiguity is an error unless the
    // script explicitly selects first/unique or uses an exact ID." §27.2 names the error.
    // Two processes with the same name make the selector genuinely ambiguous; the name is
    // invented by this test, so nothing on the machine can collide with it.
    let dir = scratch();
    let twin = dir.path().join(TWIN_NAME);
    // Not a copy of `/bin/sleep`: on many Linux hosts it is a multi-call `coreutils` binary that
    // dispatches on its own argv[0], so a copy under another name answers `unknown program` and
    // exits — nothing then carries the name and nothing is ambiguous. A symlink to `/bin/sh` does
    // carry it: the kernel takes a process's `comm` — the `name` of `ono.process/1` — from the
    // basename of the path handed to `execve`, and a shell does not care what it was called.
    // A symlink also leaves no writable descriptor behind, which a copy does: a concurrent test's
    // `spawn` inherits that descriptor across `fork` and the twin's `exec` then fails `ETXTBSY`.
    std::os::unix::fs::symlink("/bin/sh", &twin).expect("a shell to reach under the twin name");
    // Each twin blocks reading the pipe this test holds open, so both stay alive for the whole
    // `enter` and neither execs anything else that would take the name away again.
    let mut children: Vec<std::process::Child> = (0..2)
        .map(|_| {
            std::process::Command::new(&twin)
                .args(["-c", "read line"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("the twin starts")
        })
        .collect();
    for child in &children {
        wait_until_named(child.id(), TWIN_NAME);
    }

    let run = ono(&format!(
        "try {{ enter {TWIN_NAME} }} catch e {{ $e | to json }}"
    ));
    let error = caught(&run, "§29.3: ambiguity in a script is an error value");
    assert_eq!(
        text_at(&error, "name", "§40: an error carries its dotted name"),
        "spatial.ambiguous_selector",
        "§27.2/§29.3: two processes answer to this name, so the script gets a structured \
         ambiguity error and never a picker, got {error:?}"
    );
    let disambiguation = rendered(&error);
    assert!(
        disambiguation.contains(TWIN_NAME),
        "§27.2: the refusal shows the candidates it could not choose between, got {error:?}"
    );

    for child in &mut children {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[test]
fn should_refuse_to_go_back_or_up_from_the_root_with_a_named_spatial_error() {
    // §40 lists `spatial.history_empty` and `spatial.no_parent` as separate codes, because §6.6
    // and §53 make `back` and `up` deliberately different: one follows history, the other
    // follows canonical hierarchy. At the fresh root neither has anywhere to go, and the two
    // refusals must not collapse into one message.
    let back = ono("try { back } catch e { $e | to json }");
    assert_eq!(
        text_at(
            &caught(&back, "§40: the refusal is a structured error value"),
            "name",
            "§40: an error carries its dotted name"
        ),
        "spatial.history_empty",
        "§40: `back` with an empty trail is `spatial.history_empty`, got {:?}",
        back.stdout()
    );

    let up = ono("try { up } catch e { $e | to json }");
    assert_eq!(
        text_at(
            &caught(&up, "§40: the refusal is a structured error value"),
            "name",
            "§40: an error carries its dotted name"
        ),
        "spatial.no_parent",
        "§40/§53: `up` at the root SYSTEM place is `spatial.no_parent` — a different failure \
         from an empty history, got {:?}",
        up.stdout()
    );
}

// --- §41 the machine-readable spatial registry -------------------------------------------------

#[test]
fn should_ship_the_machine_readable_spatial_registry() {
    // §41: "v0.4 requires machine-readable contracts sufficient to generate help, completion,
    // tests and SDK bindings", and names five documents. Reading chosen: `spatial-errors.yaml`
    // may be satisfied by the spatial family inside `docs/contracts/errors.yaml` — the taxonomy is
    // one closed registry (ADR-0006) and splitting it would create exactly the drift §41 exists
    // to prevent — so it is checked by the error-family test above and by its own file only if
    // one is shipped. The other four have no home today and must exist.
    for name in [
        "spatial.yaml",
        "spaces.yaml",
        "relations.yaml",
        "landmarks.yaml",
    ] {
        let document = registry(name);
        assert!(
            !matches!(document, Value::Null),
            "§41: `{name}` must not be empty — `cargo xtask spec-check` fails on empty \
             contracts, and an empty registry generates nothing"
        );
    }
}

#[test]
fn should_declare_every_canonical_space_with_the_fields_the_registry_requires() {
    // §41.1 gives the required shape verbatim: id, label, parent, object_type, enterable,
    // commands, summary_fields. §53 fixes the root geography at six canonical domains.
    let spaces = entries(&registry("spaces.yaml"), "spaces");
    assert!(
        !spaces.is_empty(),
        "§41.1: at least the canonical domains are declared"
    );

    let ids: BTreeSet<String> = spaces
        .iter()
        .filter_map(|space| space["id"].as_str().map(str::to_owned))
        .collect();
    for domain in [
        "compute",
        "network",
        "storage",
        "containers",
        "identity",
        "devices",
    ] {
        assert!(
            ids.iter()
                .any(|id| id == domain || id.ends_with(&format!(".{domain}"))),
            "§53/§7: the root geography is six canonical domains and `{domain}` is one of them; \
             the registry declares {ids:?}"
        );
    }

    for space in &spaces {
        let id = space["id"].as_str().unwrap_or_default().to_owned();
        assert!(
            !id.is_empty(),
            "§41.1: every space has an `id`, got {space:?}"
        );
        for required in ["label", "object_type", "commands", "summary_fields"] {
            assert!(
                space.get(required).is_some_and(|value| !value.is_null()),
                "§41.1: `{id}` must declare `{required}`, got {space:?}"
            );
        }
        assert!(
            space.get("parent").is_some(),
            "§41.1: `{id}` must declare `parent` — the root's is null, but the field is not \
             optional, got {space:?}"
        );
        assert!(
            space["enterable"].as_bool().is_some(),
            "§41.1: `enterable` is a boolean, and it is what tells completion and `enter` \
             whether `{id}` is a destination, got {space:?}"
        );
        let commands: BTreeSet<String> = space["commands"]
            .as_sequence()
            .map(|list| {
                list.iter()
                    .filter_map(|c| c.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            commands.contains("look"),
            "§6.1: `look` describes the current place, so every place supports it; `{id}` \
             declares {commands:?}"
        );
        if let Some(parent) = space["parent"].as_str() {
            assert!(
                ids.contains(parent),
                "§41.1/§3.4: `{id}` names `{parent}` as its canonical parent and no space with \
                 that id is declared; a dangling parent makes `up` non-deterministic (§54)"
            );
        }
    }
}

#[test]
fn should_declare_every_relation_with_its_direction_labels_and_confidence() {
    // §41.2 gives the required shape verbatim: id, source, target, direction, canonical_label,
    // inverse_label, confidence. The inverse label is what makes `follow owner` the readable
    // other end of `follow socket` (§6.4), and the confidence is what §11.5 pins.
    let relations = entries(&registry("relations.yaml"), "relations");
    assert!(
        !relations.is_empty(),
        "§41.2: at least the built-in relations are declared"
    );

    let mut ids = BTreeSet::new();
    for relation in &relations {
        let id = relation["id"].as_str().unwrap_or_default().to_owned();
        assert!(
            !id.is_empty(),
            "§41.2: every relation has an `id`, got {relation:?}"
        );
        assert!(
            ids.insert(id.clone()),
            "§41.2: relation ids are unique; `{id}` is declared twice"
        );
        for required in [
            "source",
            "target",
            "direction",
            "canonical_label",
            "inverse_label",
        ] {
            assert!(
                relation.get(required).is_some_and(|value| !value.is_null()),
                "§41.2: `{id}` must declare `{required}`, got {relation:?}"
            );
        }
        let direction = relation["direction"].as_str().unwrap_or_default();
        assert!(
            ["outbound", "inbound", "bidirectional"].contains(&direction),
            "§41.2/§22: `direction` is what a MapEdge renders, so it comes from a fixed \
             vocabulary; `{id}` declares {direction:?}"
        );
        let confidence = relation["confidence"].as_str().unwrap_or_default();
        assert!(
            CONFIDENCE.contains(&confidence) || confidence == "exact_or_provider_declared",
            "§41.2/§11.5: `confidence` is one of {CONFIDENCE:?} or the \
             `exact_or_provider_declared` of §41.2's own example; `{id}` declares {confidence:?}"
        );
    }
}

#[test]
fn should_serve_exactly_the_canonical_spaces_the_registry_declares() {
    // Drift in both directions, the way `providers.rs` checks the provider registry: a space
    // the shell serves that no file declares is undocumented surface, and a space a file
    // declares that the shell does not serve is a promise nobody keeps. §41.3 makes the
    // registry the source of help, completion and map legends, so the two cannot disagree.
    let declared: BTreeSet<String> = entries(&registry("spaces.yaml"), "spaces")
        .iter()
        .filter(|space| {
            space["parent"]
                .as_str()
                .is_none_or(|parent| parent == "system")
        })
        .filter_map(|space| space["id"].as_str().map(str::to_owned))
        .collect();

    let run = ono("map --json --depth 1");
    let map = document(
        &run,
        "§22: `map --json` returns a renderer-independent SpatialMap",
    );
    let served: BTreeSet<String> = list_at(&map, "nodes", "§22: a SpatialMap carries `nodes`")
        .iter()
        .filter_map(|node| {
            search(node, "space")
                .or_else(|| search(node, "id"))
                .and_then(|value| value.as_str().map(str::to_owned))
        })
        .collect();

    assert_eq!(
        declared, served,
        "§41: docs/contracts spaces registry and the root map must agree exactly — a served space no \
         file declares is undocumented surface, a declared space nothing serves is a promise \
         nobody keeps"
    );
}

#[test]
fn should_serve_every_relation_it_declares_and_declare_every_relation_it_serves() {
    // The relation half of the same drift check. `near --all` is an ordinary object stream
    // (§29.4), so the relations the shell actually emits are readable; every one must be
    // declared. In the other direction, a declared relation must be a name `follow` accepts —
    // an undeclared name is `spatial.no_relation` (§40), and a *declared* one that raises the
    // same error is a registry entry nothing implements.
    let declared = entries(&registry("relations.yaml"), "relations");
    let labels: BTreeSet<String> = declared
        .iter()
        .filter_map(|relation| relation["canonical_label"].as_str().map(str::to_owned))
        .collect();

    let run = ono("enter process/1; near --all | to json");
    let neighbours = document(&run, "§29.4: `near` is an ordinary object stream");
    let served: BTreeSet<String> = neighbours
        .as_sequence()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|neighbour| {
            search(neighbour, "relation").and_then(|value| value.as_str().map(str::to_owned))
        })
        .collect();
    assert!(
        !served.is_empty(),
        "§12: a process place has exits, got {:?}",
        run.stdout()
    );
    let undeclared: Vec<&String> = served.difference(&labels).collect();
    assert!(
        undeclared.is_empty(),
        "§41.2/§41.3: the relation names the shell emits generate completion, help and map \
         legends from the registry, so every one must be declared there; {undeclared:?} are not \
         among {labels:?}"
    );

    for label in &labels {
        let probe = ono(&format!(
            "enter process/1; try {{ follow {label} }} catch e {{ $e | to json }}"
        ));
        let failed = documents(&probe)
            .first()
            .and_then(|value| search(value, "name"))
            .and_then(|value| value.as_str().map(str::to_owned));
        assert_ne!(
            failed.as_deref(),
            Some("spatial.no_relation"),
            "§41.2: `{label}` is declared in the relation registry, so `follow {label}` must be \
             a name the shell knows; a declared relation nothing implements is a promise nobody \
             keeps. (`spatial.not_found` for a process that has no such neighbour is fine — the \
             name was understood.)"
        );
    }
}

// --- §42 provider conformance for spatial objects ----------------------------------------------

#[test]
fn should_declare_the_spatial_claims_on_every_provider_that_feeds_the_spatial_index() {
    // §42: "A provider that exposes objects to spatial navigation MUST pass additional
    // conformance tests beyond ordinary schema validity", and lists eight required claims. The
    // claims are contract, so they live beside the provider's other declarations in
    // `docs/contracts/providers/*.yaml`, which `providers.rs` already compares against the built
    // registry. A provider that serves none of the spatial object types declares no `spatial`
    // block and is not checked here.
    let spatial_types: BTreeSet<&str> = [
        "process",
        "service",
        "socket",
        "connection",
        "interface",
        "route",
        "filesystem",
        "mount",
        "device",
        "container",
        "user",
        "group",
        "file",
        "dir",
    ]
    .into_iter()
    .collect();

    let mut checked = 0usize;
    for provider in provider_declarations() {
        let id = provider["id"].as_str().unwrap_or_default().to_owned();
        let targets: BTreeSet<String> = provider["targets"]
            .as_sequence()
            .map(|list| {
                list.iter()
                    .filter_map(|t| t.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if !targets
            .iter()
            .any(|target| spatial_types.contains(target.as_str()))
        {
            continue;
        }
        checked += 1;
        let claims = provider.get("spatial").cloned().unwrap_or_else(|| {
            panic!(
                "§42: `{id}` serves {targets:?}, which are spatial object types, so it must \
                 declare its spatial claims — identity strategy, canonical parent strategy, \
                 supported relationships, freshness strategy, event support, permission \
                 behaviour, cost class and landmark-relevant metrics/states"
            )
        });
        for required in [
            "identity_strategy",
            "canonical_parent",
            "relationships",
            "freshness",
            "events",
            "permissions",
            "cost_class",
            "landmark_metrics",
        ] {
            assert!(
                claims.get(required).is_some_and(|value| !value.is_null()),
                "§42: `{id}` must declare `{required}` among its spatial claims, got {claims:?}"
            );
        }
        let tier = claims["identity_strategy"].as_str().unwrap_or_default();
        assert!(
            ["stable", "lifetime", "observation"].contains(&tier),
            "§10.1: the identity tier is A (stable conceptual), B (lifetime) or C (observation); \
             `{id}` claims {tier:?}"
        );
        let cost = claims["cost_class"].as_str().unwrap_or_default();
        assert!(
            !cost.is_empty(),
            "§32.1: a relationship provider declares its cost class so lazy expansion can \
             decide what to fetch; `{id}` declares none"
        );
    }
    assert!(
        checked > 0,
        "§42: at least one shipped provider serves a spatial object type; the declarations \
         under docs/contracts/providers/ named none"
    );
}

#[test]
fn should_resolve_repeated_observations_of_one_object_to_the_same_spatial_id() {
    // §42.1: "Repeated observations of the same live object MUST resolve to the same
    // `SpatialId` within the provider's advertised identity tier." pid 1 outlives this test, so
    // two observations of it — in one session and across two sessions — are observations of one
    // conceptual object. §3.1 also keeps the display name out of identity, so the id must not
    // simply be the rendered name.
    let same_session = ono("enter process/1; look --json; home; enter process/1; look --json");
    let first = text_at(
        &nth_document(&same_session, 0, "§6.3: the process place, first visit"),
        "place.id",
        "§3.1: a place carries an opaque `SpatialId`",
    );
    let second = text_at(
        &nth_document(&same_session, 1, "§6.3: the process place, second visit"),
        "place.id",
        "§3.1: a place carries an opaque `SpatialId`",
    );
    assert_eq!(
        first, second,
        "§42.1: two observations of pid 1 in one session are the same spatial object"
    );

    let other_session = ono("enter process/1; look --json");
    let across = text_at(
        &document(&other_session, "§6.3: the process place in a fresh session"),
        "place.id",
        "§3.1: a place carries an opaque `SpatialId`",
    );
    assert_eq!(
        first, across,
        "§10.1/§42.1: a process identity is a Tier B lifetime identity — pid plus start time — \
         and neither end of that pair changed between the two runs, so the id must not have \
         either. An id that is only stable inside one process is not an identity."
    );
    assert_ne!(
        first, "systemd",
        "§3.1: `SpatialId` MUST be opaque to users; the display name is not identity"
    );
}

#[test]
fn should_report_denied_information_as_denied_rather_than_as_an_empty_collection() {
    // §42.4: "Denied information must produce `permission_denied` or `unknown`, never false
    // empty collections." §35.2 keeps the six states distinct and gives the example verbatim:
    // "files permission denied for 14 process FDs" is preferable to "files 0". pid 1 is
    // root-owned, so an unprivileged user cannot read its file descriptors — the container the
    // acceptance suite runs in makes that certain, and so does any ordinary developer machine.
    let run = ono("enter process/1; look --json");
    let view = document(&run, "§6.1: `look --json` returns a PlaceView");
    let files = field(&view, "files");
    let state = search(&files, "state")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| {
            panic!(
                "§35.2: every neighbourhood group carries one of the six states {PERMISSION_STATES:?}; \
                 the `files` group of pid 1 carried none: {view:?}"
            )
        });
    assert!(
        PERMISSION_STATES.contains(&state.as_str()),
        "§35.2: the state is one of {PERMISSION_STATES:?}, got {state:?}"
    );
    assert!(
        ["permission_denied", "unknown"].contains(&state.as_str()),
        "§42.4/§35.3/§53: an unprivileged user cannot read root-owned file descriptors, so the \
         honest answer is `permission_denied` or `unknown` — never `empty`, and never a count \
         of zero. The state was {state:?} in {view:?}"
    );
}

// --- §36 KUANG/11 spatial extensions ------------------------------------------------------------

/// A scratch plugin directory holding the example package with a declared relation contribution.
///
/// The manifest is the one `plugins.rs` installs, plus `contributions.relations` (spec v0.2
/// §31.7's `from->to` shape) and an optional `relation.write`. Only the relation lines are new,
/// so a failure here is about the spatial contribution and not about package loading.
fn relation_plugin_home() -> Scratch {
    let dir = scratch();
    let manifest = r#"
format: kuang-package/1
package:
  id: dev.example.echo
  name: echo
  version: 0.1.0
  description: Emits what it is asked to emit, and knows one relation.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: native-process
  entry: runtime/echo
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
contributions:
  relations: ["process->process"]
capabilities:
  optional:
    - relation.write
network:
  outbound: none
"#;
    dir.write("dev.example.echo/manifest.yaml", manifest);
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    let entry = dir.path().join("dev.example.echo/runtime/echo");
    std::fs::create_dir_all(entry.parent().expect("a parent")).expect("the runtime directory");
    std::fs::copy(&binary, &entry).expect("the example plugin binary is built");
    dir
}

fn ono_with_plugins(home: &Scratch, script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .env("ONO_PLUGIN_PATH", home.path().display().to_string())
        .run()
}

#[test]
fn should_keep_a_package_relation_out_of_the_map_until_its_capability_is_granted() {
    // §35.5: "KUANG/11 plugins cannot use the map as a side channel to expose information
    // outside granted capabilities. The spatial host MUST filter plugin nodes/edges according to
    // capability scope before merging them into maps." §36.2 forbids exposing data outside
    // capabilities at all. The package below declares a relation contribution and asks for
    // `relation.write` optionally, so without the grant it loads degraded — and a degraded
    // package's edges must be absent from the map, not merely unlabelled.
    let home = relation_plugin_home();
    let denied = ono_with_plugins(
        &home,
        "load plugin dev.example.echo; map --json --relations dev.example.echo",
    );
    let map = document(
        &denied,
        "§22: `map --json` returns a SpatialMap even when a package contributed nothing",
    );
    let leaked: Vec<Value> = list_at(&map, "edges", "§22: a SpatialMap carries `edges`")
        .into_iter()
        .filter(|edge| rendered(edge).contains("dev.example.echo"))
        .collect();
    assert!(
        leaked.is_empty(),
        "§35.5/§36.2: `relation.write` was not granted, so the package's edges are filtered out \
         before the merge; the map carried {leaked:?}"
    );
    assert!(
        denied.stdout().contains("degraded") || denied.stderr().contains("relation.write"),
        "§31.17/§35.5: the package is degraded and says which capability it lacks, rather than \
         failing silently; got stdout {:?} and stderr {:?}",
        denied.stdout(),
        denied.stderr()
    );
}

#[test]
fn should_carry_the_contributing_package_as_the_origin_of_every_plugin_edge() {
    // §36: "Ono core retains control of identity, security and rendering contracts", and §36.2
    // forbids "uninspectable phantom edges" and edges that "appear exact without provenance".
    // §53 puts it as a settled decision: plugins "cannot create untraceable truth". So a
    // contributed edge carries the package that contributed it and a confidence from §11.5, and
    // `inspect relation` (§11.4) can answer for it.
    let home = relation_plugin_home();
    let granted = ono_with_plugins(
        &home,
        "load plugin dev.example.echo --grant relation.write; \
         map --json --relations dev.example.echo",
    );
    let map = document(&granted, "§22: `map --json` returns a SpatialMap");
    let edges: Vec<Value> = list_at(&map, "edges", "§22: a SpatialMap carries `edges`")
        .into_iter()
        .filter(|edge| rendered(edge).contains("dev.example.echo"))
        .collect();
    assert!(
        !edges.is_empty(),
        "§36.1: with `relation.write` granted the package's relationship provider contributes \
         its edges, got {:?}",
        granted.stdout()
    );
    for edge in &edges {
        let origin = search(edge, "origin")
            .or_else(|| search(edge, "provider"))
            .map(|value| rendered(&value))
            .unwrap_or_default();
        assert!(
            origin.contains("dev.example.echo"),
            "§31.64/§36.2: every registry entry records origin, so a contributed edge names the \
             package it came from; got {edge:?}"
        );
        let confidence = text_at(edge, "confidence", "§22: a MapEdge carries `confidence`");
        assert!(
            CONFIDENCE.contains(&confidence.as_str()),
            "§11.5/§36.2: a plugin edge carries a confidence from {CONFIDENCE:?} and never \
             appears exact without provenance, got {confidence:?} in {edge:?}"
        );
    }
}

// --- §37 integration with the v0.3 external command adapters ------------------------------------

#[test]
fn should_reconcile_an_adapted_object_with_its_native_twin_into_one_place() {
    // §37.1: "Objects from adapters MUST be reconciled with canonical provider identities before
    // appearing as duplicate map nodes." §53 says the same as a settled decision. `lo` is served
    // both by the native netlink provider (`get interface`) and by the v0.3 `ip` adapter, and
    // observing it through both must not put two `lo` nodes in the network space.
    let run = ono("get interface | where name == \"lo\" | count | to text; \
         ip link | where name == \"lo\" | count | to text; \
         enter network; near --type interface --all | where name == \"lo\" | count | to text");
    run.assert_success();
    let counts: Vec<&str> = run
        .stdout()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(
        counts.last().copied(),
        Some("1"),
        "§37.1: the loopback interface was observed through the native provider and through the \
         `ip` adapter, and the network space still holds exactly one `lo` place — two nodes \
         would be the duplicate §37.1 forbids. The three counts were {counts:?}"
    );

    let provenance = ono("ip link | where name == \"lo\" | count | to text; \
         enter network; near --type interface --all | where name == \"lo\" | inspect | to json");
    let inspected = rendered(&document(
        &provenance,
        "spec v0.2 §11.4: `inspect` returns a record",
    ));
    assert!(
        inspected.contains("linux.netlink") && inspected.contains("adapter:"),
        "§37.1/spec v0.3 §1.47: reconciliation keeps both sources on the record, so a user can \
         see that the adapter and the canonical provider agreed about this object; got \
         {inspected}"
    );
}

#[test]
fn should_show_a_place_only_an_adapter_observed_when_standing_in_the_collection_that_holds_it() {
    // §37.1's second sentence — "If identity cannot be safely reconciled, both objects may appear
    // with provenance" — and ADR-0193's reading of it: "Where no provider answers, the adapted
    // record is all there is and it appears with its own provenance." No canonical provider
    // answers for `network.addresses`, so the addresses `ip addr` reported are all the shell has;
    // standing in the space that holds them and being told nothing is known there is the claim
    // §2.17 forbids.
    let empty = ono("home; enter network; enter addresses; look --json");
    empty.assert_success();
    assert!(
        rendered(&document(
            &empty,
            "§29.1: `look --json` answers off a terminal"
        ))
        .contains("unsupported"),
        "the premise of this test: nothing has observed an address yet, so the space says so"
    );

    let run = ono("ip addr | count | to text; \
         home; enter network; enter addresses; near --all | select display_name | to text");
    run.assert_success();
    assert!(
        run.stdout().contains("127.0.0.1"),
        "§37.1: the loopback address the `ip` adapter observed is a place in ADDRESSES, so \
         standing there shows it rather than a refusal; got {:?}",
        run.stdout()
    );
}

#[test]
fn should_describe_a_search_result_and_a_place_view_with_the_same_record() {
    // ADR-0140: `find place` streams `ono.spatial-place/1`, and "the same schema is the `place`
    // of a `PlaceView`, so `look --json` and `find place` describe a place the same way". They
    // did not: `look` carried the state and the §24.1 summary the provider reported and `find`
    // left both null — which under §2.17 is the shell saying it does not know something it does.
    let found = ono("find place --type process --where pid == 1 | select name state | to json");
    found.assert_success();
    let entered = ono("enter process 1; look --json");
    entered.assert_success();

    let view = rendered(&document(
        &entered,
        "§29.1: `look --json` answers off a terminal",
    ));
    assert!(
        view.contains("state:"),
        "the premise of this test: the place view reports the state of pid 1, got {view}"
    );
    assert!(
        !found.stdout().contains("\"state\":null"),
        "ADR-0140/§2.17: a search result describes the place the way the place view does, and \
         null is the word for something nobody knows; got {:?}",
        found.stdout()
    );
}

#[test]
fn should_find_a_place_by_its_properties_when_the_index_holds_it_and_no_provider_serves_it() {
    // §6.8: `find` "MUST search the spatial index **and** provider registries". The predicate was
    // evaluated against what the providers answered and against nothing else, so a place the
    // session is holding that no canonical provider serves — an adapted observation, §37.1 and
    // ADR-0193 — was findable by name and invisible to a property. `find place --type address`
    // answers twenty-three, `--where family == "inet"` answered none of them.
    let run = ono("ip addr | count | to text; \
         find place --type address | count | to text; \
         find place --type address --where family == \"inet\" | count | to text");
    run.assert_success();
    let counts: Vec<&str> = run
        .stdout()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let by_property = counts
        .last()
        .and_then(|count| count.parse::<u32>().ok())
        .unwrap_or_default();
    assert!(
        by_property > 0,
        "§6.8: the addresses the `ip` adapter observed are in the index, so a predicate over \
         their own fields finds them; the three counts were {counts:?}"
    );
}

#[test]
fn should_not_call_a_collection_unsupported_while_it_holds_a_place_the_shell_observed() {
    // §35.2 keeps `unsupported` distinct from `available`, and §2.17 forbids a claim the shell
    // cannot support: a group that can name its members is not a group nobody could answer for.
    let run = ono("ip addr | count | to text; home; enter network; enter addresses; look --json");
    run.assert_success();
    let view = rendered(&document(
        &run,
        "§29.1: `look --json` answers off a terminal",
    ));
    assert!(
        !view.contains("unsupported"),
        "§35.2/§2.17: a collection holding observed places is `available`, not `unsupported`; \
         got {view}"
    );
}

#[test]
fn should_never_let_raw_command_output_become_a_place() {
    // §37.2: "Raw external command output MUST NOT become spatial nodes through generic table
    // heuristics. Only canonical typed adapter output or explicit plugin schemas may enter the
    // spatial index." So bytes are not enterable, and the refusal is the named `spatial.not_enterable`
    // of §40 rather than a generic parse or resolution failure.
    let run = ono("try { raw ip link | enter } catch e { $e | to json }");
    let error = caught(&run, "§40: the refusal is a structured error value");
    assert_eq!(
        text_at(&error, "name", "§40: an error carries its dotted name"),
        "spatial.not_enterable",
        "§37.2: a byte stream has no spatial identity, so entering it is `spatial.not_enterable` \
         and never a table parsed into places, got {error:?}"
    );

    // And nothing about the attempt may have entered the index: the network space is unchanged.
    let after = ono("raw ip link | to text; \
         enter network; near --type interface --all | where name == \"lo\" | count | to text");
    assert_eq!(
        after.stdout().lines().last().unwrap_or(""),
        "1",
        "§37.2: running the program raw leaves the spatial index alone, got {:?}",
        after.stdout()
    );
}

// --- §46 spatial session state ------------------------------------------------------------------

#[test]
fn should_start_every_session_at_the_local_system_root() {
    // §46.1: "The current place MAY persist across shell restarts if configured, but the default
    // v0.4 behavior is: start at local SYSTEM root." §47 makes the same point from the other
    // side with `spatial.trail.persist = false`. So a session that navigated away leaves nothing
    // behind for the next one.
    let home = scratch();
    let navigated = ono_at_home(&home, "enter compute; look --json");
    let away = text_at(
        &document(&navigated, "§6.1: the view after entering COMPUTE"),
        "place.id",
        "§3.1: a place carries an opaque `SpatialId`",
    );

    let fresh = ono_at_home(&home, "look --json");
    let start = document(&fresh, "§6.1: the view a fresh session opens on");
    assert_ne!(
        text_at(
            &start,
            "place.id",
            "§3.1: a place carries an opaque `SpatialId`"
        ),
        away,
        "§46.1: the default is to start at the local SYSTEM root, so the previous session's \
         place is not restored"
    );
    let kind = rendered(&field(&start, "place"));
    assert!(
        kind.to_lowercase().contains("system"),
        "§46.1/§7.1: a fresh session opens on the root SYSTEM place, got {start:?}"
    );

    let trail = ono_at_home(&home, "trail --json");
    let steps = document(&trail, "§29.1: `trail --json` must work non-interactively");
    assert!(
        steps.as_sequence().is_some_and(|items| items.is_empty())
            || list_at(&steps, "steps", "§20.1: the trail is a list of steps").is_empty(),
        "§46.1: \"Trail persistence across sessions is disabled by default for privacy and \
         stale-identity reasons\", so a fresh session's trail is empty, got {steps:?}"
    );
}

#[test]
fn should_keep_a_scripts_navigation_out_of_the_callers_place() {
    // §29.2: "A script MUST NOT silently change the caller's interactive spatial context." The
    // script below navigates as far as it likes; the caller must come back to where it was.
    let dir = scratch();
    dir.write("wander.ono", "enter compute\nenter services\nlook --json\n");
    let script = dir.path().join("wander.ono");
    let binary = ono_testkit::ono_binary();

    let run = ono(&format!(
        "enter storage; {} {}; look --json",
        binary.display(),
        script.display()
    ));
    let inner = nth_document(&run, 0, "§29.2: the script's own view of where it went");
    let outer = nth_document(&run, 1, "§29.2: the caller's view afterwards");
    assert_ne!(
        text_at(
            &inner,
            "place.id",
            "§3.1: a place carries an opaque `SpatialId`"
        ),
        text_at(
            &outer,
            "place.id",
            "§3.1: a place carries an opaque `SpatialId`"
        ),
        "§29.2: the script's current place is script-local; the caller stayed in STORAGE"
    );
    assert!(
        rendered(&field(&outer, "place"))
            .to_lowercase()
            .contains("storage"),
        "§29.2: the caller is where it was before it ran the script, got {outer:?}"
    );
}

#[test]
fn should_keep_the_trail_session_local_while_a_pin_survives_the_session() {
    // §46.1: "Pins MAY persist. Trail persistence across sessions is disabled by default", and
    // §53 settles it: "Default trail persistence? Session-only; pins may persist." §20.4: a pin
    // "MUST store a resilient selector and identity metadata rather than only a rendered path",
    // and an unresolvable pin "remains but reports unresolved state".
    let home = scratch();
    let pinned = ono_at_home(&home, "enter compute; pin --name workshop");
    pinned.assert_success();

    let later = ono_at_home(&home, "jump @workshop; look --json");
    let view = document(&later, "§20.4: `jump @<bookmark>` resolves a persisted pin");
    assert!(
        rendered(&field(&view, "place"))
            .to_lowercase()
            .contains("compute"),
        "§20.4/§46.1: a pin persists across sessions and resolves to the place it marked, got \
         {view:?}"
    );

    let trail = ono_at_home(&home, "trail --json");
    let steps = document(&trail, "§29.1: `trail --json` must work non-interactively");
    assert!(
        steps.as_sequence().is_some_and(|items| items.is_empty())
            || list_at(&steps, "steps", "§20.1: the trail is a list of steps").is_empty(),
        "§46.1: the pin survived the session and the trail did not, got {steps:?}"
    );
}

// --- §47 configuration ---------------------------------------------------------------------------

#[test]
fn should_expose_every_spatial_setting_as_a_typed_setting_with_its_default() {
    // §47 calls eleven keys required and gives each one's default. "The exact configuration file
    // syntax follows Ono's base configuration system", which is the typed catalogue of ADR-0094:
    // every setting is declared with a type and a built-in default, and `get config` reports it
    // with its layer. With no file and no ONO_* variable, every layer is `default`.
    let home = scratch();
    let run = ono_at_home(&home, "get config spatial. | to json");
    run.assert_success();
    let settings = document(
        &run,
        "spec v0.2 §30: `get config` streams config-setting.v1 records",
    )
    .as_sequence()
    .cloned()
    .unwrap_or_default();
    let present: BTreeSet<String> = settings
        .iter()
        .filter_map(|setting| setting["key"].as_str().map(str::to_owned))
        .collect();

    for (key, default) in SPATIAL_SETTINGS {
        let setting = settings
            .iter()
            .find(|setting| setting["key"].as_str() == Some(key))
            .unwrap_or_else(|| {
                panic!(
                    "§47: `{key}` is a required configuration key and `get config spatial.` does \
                     not report it. The catalogue currently reports {present:?}."
                )
            });
        assert_eq!(
            setting["layer"].as_str(),
            Some("default"),
            "ADR-0010: with no configuration file and no ONO_* variable, `{key}` reads its \
             built-in default, got {setting:?}"
        );
        assert!(
            setting["type"]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "config-setting.v1: `{key}` declares its type, got {setting:?}"
        );
        let value = rendered(&setting["value"]);
        assert_eq!(
            value.trim().trim_matches('\''),
            default,
            "§47: the default of `{key}` is spelled out in the spec, got {setting:?}"
        );
    }
}

#[test]
fn should_keep_the_typed_shell_working_when_the_spatial_layer_is_disabled() {
    // §47: "Disabling `spatial.enabled` MUST leave the typed shell and ordinary commands
    // functional." The pipeline still answers; the spatial verb refuses in a way a script can
    // branch on (§40) rather than disappearing or panicking.
    let home = scratch();
    let pipeline = ono_at_home(
        &home,
        "set config spatial.enabled = false\nget process | where pid == 1 | count | to text",
    );
    pipeline.assert_success();
    assert_eq!(
        pipeline.stdout().lines().last().unwrap_or(""),
        "1",
        "§47: the typed shell is untouched by the spatial switch, got {:?}",
        pipeline.stdout()
    );

    let refused = ono_at_home(
        &home,
        "set config spatial.enabled = false\ntry { look --json } catch e { $e | to json }",
    );
    let error = caught(&refused, "§40: the refusal is a structured error value");
    assert_eq!(
        text_at(&error, "name", "§40: an error carries its dotted name"),
        "spatial.unsupported",
        "§40/§47: with the layer switched off a spatial verb answers `spatial.unsupported` — a \
         named refusal a script can branch on, not a missing command, got {error:?}"
    );
}

// --- §34 performance budgets ---------------------------------------------------------------------

#[test]
fn should_bound_the_default_map_to_its_node_budget() {
    // §34.2: "unbounded graph rendering is prohibited", with a default visible-node budget of
    // about 30 for the text map and 100 for the interactive one; §47 makes the latter the
    // configurable `spatial.map.node_budget = 100`. §6.9 and §22 add that what was left out is
    // disclosed rather than dropped, which is what `hidden: HiddenSummary` is for (§54: "Are
    // hidden objects/counts disclosed?").
    let run = ono("map --json --all");
    let map = document(
        &run,
        "§22: `map --json` returns a renderer-independent SpatialMap",
    );
    let nodes = list_at(&map, "nodes", "§22: a SpatialMap carries `nodes`");
    assert!(
        nodes.len() <= 100,
        "§34.2/§47: even `--all` stays inside `spatial.map.node_budget` (100 by default); the \
         map carried {} nodes",
        nodes.len()
    );
    assert!(
        field(&map, "hidden").get("count").is_some() || !field(&map, "hidden").is_null(),
        "§22/§54: whatever the budget left out is disclosed in the map's `hidden` summary, got \
         {map:?}"
    );
}

#[test]
fn should_answer_repeated_looks_far_inside_the_look_budget() {
    // §34 budgets a cached `look` at under 50 ms and interactive startup at under 150 ms.
    // A wall-clock assertion that tight is flaky on shared hardware, so the *marginal* cost is
    // what is measured: the same session answers one `look` and then twenty more, and the
    // difference divided by twenty is the cost of a warm `look`. Ten times the budget still
    // catches a catastrophic regression — a provider sweep where an index lookup belongs —
    // while machine noise does not fail the gate.
    let once = Instant::now();
    ono("look --json").assert_success();
    let baseline = once.elapsed();

    let script = std::iter::repeat_n("look --json", 21)
        .collect::<Vec<_>>()
        .join("; ");
    let many = Instant::now();
    ono(&script).assert_success();
    let repeated = many.elapsed();

    let marginal = repeated.saturating_sub(baseline) / 20;
    assert!(
        marginal < Duration::from_millis(500),
        "§34: a warm `look` is budgeted at under 50 ms; twenty extra looks cost {marginal:?} \
         each (one look took {baseline:?}, twenty-one took {repeated:?}), which is an order of \
         magnitude outside the budget and means the view is being recomputed rather than read \
         from the spatial index (§33.1)"
    );
}
