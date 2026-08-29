//! Outcome tests for the spatial command language of the v0.4 Spatial Systems Interface as a
//! script sees it: `look`, `near`, `enter`, `follow`, `jump`, `back`, `up`, `home`, `trail`,
//! `find place` and the bounded answer of `map --json`.
//!
//! Narrative: `docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` — §5 (the spatial
//! horizon), §6 (the spatial command language: §6.1 `look`, §6.2 `near`, §6.3 `enter`, §6.4
//! `follow`, §6.5 `jump`, §6.6 `back`/`up`/`home`, §6.7 `trail`, §6.8 `find place`, §6.9 `map`),
//! §7 (the canonical domains and their required children), §20 (the navigation trail), §27
//! (selector resolution and ambiguity), §28 (typed pipelines become places), §29 (non-interactive
//! semantics — the surface every test here uses), §40 (the structured spatial errors), §44.6
//! (`back` versus `up`), §46.1 (session defaults) and §53 (the resolved decisions this file may
//! not reopen). The v0.2 base contributes the pipeline these streams compose with (v0.2 §33.5)
//! and the native-before-external resolution order (v0.2 §6.5).
//!
//! A test still carries `#[ignore]` with the section that governs it until the increment that
//! implements that section removes the line; the tree stays green in the meantime.
//!
//! Everything runs offline and unprivileged, on fixtures the test itself creates: `sleep`
//! children it spawns and kills, a TCP listener it binds on `127.0.0.1:0`, and scratch
//! directories. No test depends on a service, mount or process name that only exists on the
//! machine that happens to run it. Every assertion is on what a user sees — the JSON on stdout,
//! the exit status, the structured error record — never on how the shell is wired internally
//! (AGENTS.md §11).
//!
//! Readings this file fixes where v0.4 is silent, each repeated at the test that depends on it:
//!
//! - `--json` writes a serialized document to stdout exactly as v0.2 `to json` does, so
//!   `look --json | from json` reads it back into the pipeline (§29.1 with v0.2 §33.5);
//! - a `PlaceView` (§6.1) is a record with `place` — a `SpatialObject` projection per §3.1 —
//!   and `neighborhood` per §3.6; `trail --json` (§6.7) is the list of `NavigationStep` records
//!   §20.1 spells out field by field;
//! - `object_type` is the v0.2 schema id of the object at the place (`ono.process/1`), which is
//!   what §37.1 identity merge requires of the spatial projection of an adapter/provider object;
//! - the spatial search is spelled `find place` and its type filter is `--type` (ADR-0124):
//!   bare `find` stays findutils, which v0.3 §1.71 and acceptance case 087 both require;
//! - the predicate of `find place --where` is an ordinary v0.2 predicate expression (§28 demands
//!   pipeline interop), so its string literals are quoted; the spec's unquoted
//!   `find --where state == running` in §6.8/§43.3 is prose;
//! - `up` from a socket lands somewhere under `NETWORK` (§6.6 says "normally `NETWORK/SOCKETS`",
//!   §7.3 makes `listeners`/`connections` the required children), so the tests assert the domain
//!   and the distinction from `back`, not a particular child name.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use ono_testkit::{Scratch, Shell, scratch};
use serde_yaml_ng::Value;

/// The canonical domains of §7 / §53 ("Six canonical domains"), lower-cased as a selector spells
/// them.
const DOMAINS: [&str; 6] = [
    "compute",
    "network",
    "storage",
    "containers",
    "identity",
    "devices",
];

/// The default node budget of §47 (`spatial.map.node_budget = 100`).
const NODE_BUDGET: usize = 100;

/// A shell whose configuration and state tree live entirely in `dir`, so no setting of the
/// machine running the suite reaches the run and nothing the run persists survives it
/// (ADR-0010; §46.1 makes spatial session state configurable, and a test must see the default).
fn isolated(dir: &Scratch) -> Shell {
    Shell::new()
        .env("HOME", dir.path().display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            dir.path().join("xdg").display().to_string(),
        )
        .env(
            "XDG_STATE_HOME",
            dir.path().join("state").display().to_string(),
        )
        .env(
            "ONO_CONFIG_DIR",
            dir.path().join("ono").display().to_string(),
        )
        .env_remove("ONO_CONFIG")
        .timeout(Duration::from_secs(30))
}

/// Runs a script in a shell that sees no configuration but its built-in defaults, and no
/// terminal: `-c` is exactly the non-interactive surface §29 makes normative.
fn ono(script: &str) -> ono_testkit::Run {
    let dir = scratch();
    isolated(&dir).args(["-c", script]).run()
}

/// The one JSON document a run wrote to stdout.
fn document(run: &ono_testkit::Run) -> Value {
    let text = run.stdout().trim().to_owned();
    let stderr = run.stderr();
    serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!(
            "§29.1: the spatial commands emit machine-readable output without a TTY, got \
             {text:?} ({error}); stderr: {stderr:?}"
        )
    })
}

/// The values of a JSON array document — a `Stream` serialized as v0.2 §33.5 serializes one.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let document = document(run);
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!(
                "§29.4: `near`, `find place` and `trail --json` are streams, and a stream serializes \
                 as a JSON array (v0.2 §33.5), got {:?}; stderr: {:?}",
                run.stdout(),
                run.stderr()
            )
        })
        .clone()
}

/// The single record of a JSON object document — a `PlaceView` or a `SpatialMap`.
fn record(run: &ono_testkit::Run) -> Value {
    let document = document(run);
    assert!(
        document.is_mapping(),
        "§6.1/§22: `look --json` and `map --json` each answer one structured value, got {:?}; \
         stderr: {:?}",
        run.stdout(),
        run.stderr()
    );
    document
}

fn text(row: &Value, field: &str) -> String {
    row[field]
        .as_str()
        .unwrap_or_else(|| panic!("the field `{field}` must be a string, got {row:?}"))
        .to_owned()
}

/// The `place` of a `PlaceView`: the `SpatialObject` §3.1 requires of anything navigable.
fn place_of(run: &ono_testkit::Run) -> Value {
    let view = record(run);
    let place = view["place"].clone();
    assert!(
        place.is_mapping(),
        "§6.1 with §3.3: a PlaceView describes the current place as a structured object, got \
         {view:?}"
    );
    for field in ["spatial_id", "object_type", "display_name", "scope"] {
        assert!(
            !place[field].is_null(),
            "§3.1: a SpatialObject carries `{field}`, got {place:?}"
        );
    }
    assert!(
        !text(&place, "spatial_id").is_empty(),
        "§3.1: `spatial_id` is opaque but never empty, got {place:?}"
    );
    place
}

/// Runs `script` and returns the place it left the session at.
fn place_after(script: &str) -> Value {
    let run = ono(&format!("{script}; look --json"));
    place_of(&run)
}

/// The structured error a script raises, seen the way a script sees it: caught by name through
/// the v0.2 `try`/`catch` form, which is what makes §40's taxonomy usable in a script at all.
fn caught(script: &str) -> Value {
    let run = ono(&format!("try {{ {script} }} catch e {{ $e | to json }}"));
    let mut rows = rows(&run);
    assert_eq!(
        rows.len(),
        1,
        "the failing statement raises exactly one error, got {:?}",
        run.stdout()
    );
    rows.remove(0)
}

/// Asserts that a spatial refusal is the structured error §40 names, with a stable
/// `Ono-Sendai-E` code beside the dotted name the taxonomy of v0.2 §43 gives every error.
fn assert_spatial_error(error: &Value, name: &str, script: &str) {
    assert_eq!(
        text(error, "name"),
        name,
        "§40: `{script}` is refused as `{name}`, got {error:?}"
    );
    let code = text(error, "code");
    assert!(
        code.starts_with("Ono-Sendai-E") && code.len() > "Ono-Sendai-E".len(),
        "§40 with v0.2 §43: a spatial error carries a stable `Ono-Sendai-E…` code beside its \
         name, got {error:?}"
    );
    assert!(
        !text(error, "message").is_empty(),
        "§40: the error says what happened, got {error:?}"
    );
}

/// A `sleep` child the test owns: a real process nobody else will touch, whose parent is this
/// test process and which owns no socket and no interesting file.
struct SleepChild(Child);

impl SleepChild {
    fn spawn() -> Self {
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("`sleep` is available on every test host");
        Self(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for SleepChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A listening TCP socket this test process owns, and the port it listens on.
fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback listener");
    let port = listener.local_addr().expect("the bound address").port();
    (listener, port)
}

/// The navigation of §44.6, spelled with objects the test owns: down the canonical hierarchy to
/// this process, then along a relationship edge to the socket it is listening on.
fn path_to_the_listening_socket(port: u16) -> String {
    format!(
        "home; enter compute; enter processes; enter {pid}; follow socket :{port}",
        pid = std::process::id()
    )
}

/// Whether `name` resolves to an executable on `PATH`. The collision tests assert both sides of
/// v0.2 §6.5, and the external side only exists where the program is installed.
fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let found = std::env::split_paths(&path).any(|dir| dir.join(name).is_file());
    if !found {
        eprintln!("skipped the external half: `{name}` is not installed on this host");
    }
    found
}

// --- look ------------------------------------------------------------------------------------

#[test]
fn should_describe_the_current_place_as_a_structured_view_when_look_runs_without_a_tty() {
    // §29.1: `look --json` MUST work in non-interactive mode. §6.1: it describes the current
    // place, its grouped exits and its landmarks; the headings are presentation, the object
    // underneath stays structured. A fresh session stands at the SYSTEM root (§46.1).
    let run = ono("look --json");
    run.assert_success();
    let place = place_of(&run);
    assert_eq!(
        text(&place, "object_type"),
        "ono.system/1",
        "§7.1: a fresh session looks at the SystemPlace of the active host, got {place:?}"
    );
    let view = record(&run);
    let neighborhood = view["neighborhood"].clone();
    assert!(
        neighborhood.is_mapping(),
        "§6.1 with §3.6: the view carries the bounded neighborhood around the place, got {view:?}"
    );
    assert!(
        !neighborhood["hidden_count"].is_null(),
        "§3.6 and the §54 checklist: hidden objects are disclosed as a count, not omitted, got \
         {neighborhood:?}"
    );
    assert!(
        !neighborhood["completeness"].is_null(),
        "§3.6 with invariant 17: what the view could not see is visible, not rendered as \
         absence, got {neighborhood:?}"
    );
    assert!(
        neighborhood["groups"].is_sequence(),
        "§3.6: the neighborhood is grouped exits, got {neighborhood:?}"
    );
}

#[test]
fn should_name_the_canonical_domains_as_exits_when_look_runs_in_a_script() {
    // §6.1: `look` with no arguments MUST never require prior knowledge of object names, and §5
    // makes the available canonical domains part of the horizon it shows. Invariant 13: text
    // remains sufficient — the script form must carry the same information as the TTY form.
    let run = ono("look");
    run.assert_success();
    let rendered = run.stdout().to_lowercase();
    for domain in DOMAINS {
        assert!(
            rendered.contains(domain),
            "§5/§7: the root horizon names the canonical domain `{domain}` so it can be \
             discovered without prior knowledge, got {:?}",
            run.stdout()
        );
    }
}

#[test]
fn should_read_back_into_the_pipeline_when_look_json_is_parsed_by_from_json() {
    // The reading this file fixes: `--json` writes a serialized document exactly as v0.2 `to
    // json` does (§29.1 forbids a hidden TUI dependency; v0.2 §33.5 defines the serialization),
    // so the v0.2 pipeline consumes it without a detour through a file.
    let run = ono("look --json | from json | select place.display_name | to json");
    run.assert_success();
    let rows = rows(&run);
    assert_eq!(
        rows.len(),
        1,
        "§6.1: one place, one PlaceView, got {:?}",
        run.stdout()
    );
    assert!(
        rows[0]["display_name"].is_string(),
        "§29.1 with v0.2 §33.5: what `look --json` writes is valid JSON the pipeline can \
         project, got {rows:?}"
    );
}

// --- near ------------------------------------------------------------------------------------

#[test]
fn should_stream_neighbors_that_compose_with_the_pipeline_when_near_runs_in_a_script() {
    // §29.4: `near` returns a normal structured stream that participates in object pipelines.
    // §6.2: each neighbor names the relation it was reached through. The fixture is a `sleep`
    // child of this test process, so exactly one `parent` edge is expected and its target is a
    // pid the test knows.
    let child = SleepChild::spawn();
    let run = ono(&format!(
        "enter {pid}; near | where relation == \"parent\" | count | to json",
        pid = child.pid()
    ));
    run.assert_success();
    let rows = rows(&run);
    assert_eq!(
        rows,
        vec![Value::from(1)],
        "§6.2 with §29.4: `near` is a stream `where` and `count` read, and a process has one \
         parent edge, got {:?}; stderr: {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_bound_the_neighborhood_to_the_requested_size_when_near_is_limited() {
    // §6.2: default behaviour MUST rank and bound results; `--limit <n>` fixes the bound
    // explicitly. Invariant 9: the horizon is bounded, never the whole graph.
    let run = ono("enter compute; near --limit 3 | count | to json");
    run.assert_success();
    let rows = rows(&run);
    let count = rows
        .first()
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("§6.2: `near | count` answers a number, got {rows:?}"));
    assert!(
        (1..=3).contains(&count),
        "§6.2: `near --limit 3` yields between one and three neighbors of COMPUTE, got {count}"
    );
}

// --- enter -----------------------------------------------------------------------------------

#[test]
fn should_move_into_the_hierarchical_child_when_entering_a_canonical_domain_and_its_group() {
    // §6.3: `enter` resolves one place and pushes the previous one onto the trail. Invariant 3:
    // it produces a new spatial context rather than printing an object. §7.2 makes `processes` a
    // child COMPUTE MUST provide, so this path needs no name that exists only on one host.
    let place = place_after("home; enter compute");
    assert_eq!(
        text(&place, "display_name").to_lowercase(),
        "compute",
        "§6.3/§7.2: entering `compute` makes the COMPUTE domain the current place, got {place:?}"
    );

    let nested = place_after("home; enter compute; enter processes");
    assert_eq!(
        text(&nested, "display_name").to_lowercase(),
        "processes",
        "§7.2: `processes` is a child COMPUTE MUST provide, and entering it moves there, got \
         {nested:?}"
    );
    assert_ne!(
        text(&nested, "spatial_id"),
        text(&place, "spatial_id"),
        "§6.3 with invariant 3: each `enter` is a different place, not a repainted one"
    );
}

#[test]
fn should_move_into_the_selected_object_when_a_pipeline_result_is_entered() {
    // §28.2: a structured pipeline result containing spatially identifiable objects MUST be
    // enterable; with one result there is nothing to pick. §6.3 accepts the selected object as
    // the thing entered. The fixture is a `sleep` child, so the pid is known and unshared.
    let child = SleepChild::spawn();
    let place = place_after(&format!("get process {pid} | enter", pid = child.pid()));
    assert_eq!(
        text(&place, "object_type"),
        "ono.process/1",
        "§28.2 with §37.1: the place of an entered process is typed as the v0.2 process object, \
         got {place:?}"
    );
    assert_eq!(
        place["canonical_ref"]["pid"].as_u64(),
        Some(u64::from(child.pid())),
        "§3.1/§28.2: the place keeps the canonical reference of the object the pipeline \
         selected, got {place:?}"
    );
}

// --- follow ----------------------------------------------------------------------------------

#[test]
fn should_traverse_the_relationship_edge_when_following_the_parent_relation() {
    // §6.4: `follow` MUST traverse a relationship edge, not a canonical hierarchy edge, and
    // `follow parent` is one of its own examples. §3.5 lists `process --parent-of--> process` as
    // a real relationship. The `sleep` child's parent is this test process, so both ends are
    // known here and neither depends on the host.
    let child = SleepChild::spawn();
    let place = place_after(&format!("enter {pid}; follow parent", pid = child.pid()));
    assert_eq!(
        place["canonical_ref"]["pid"].as_u64(),
        Some(u64::from(std::process::id())),
        "§6.4 with §3.5: following `parent` from a process lands on the process that spawned \
         it, got {place:?}"
    );
}

#[test]
fn should_answer_no_relation_when_following_an_edge_the_current_place_does_not_have() {
    // §40 requires `spatial.no_relation`. A `sleep` child owns no socket, and the test owns the
    // process, so this is a true absence rather than a permission boundary — invariant 17 keeps
    // those distinct, and §35 forbids answering "denied" as "none".
    let child = SleepChild::spawn();
    let script = format!("enter {pid}; follow socket", pid = child.pid());
    let error = caught(&script);
    assert_spatial_error(&error, "spatial.no_relation", &script);

    let run = ono(&script);
    assert!(
        !run.status().is_success(),
        "§40: a refused movement exits non-zero, got {:?}",
        run.status()
    );
}

// --- jump ------------------------------------------------------------------------------------

#[test]
fn should_move_across_scopes_and_record_both_ends_when_jumping_to_a_resolved_place() {
    // §6.5: `jump` resolves a place without adjacency and MUST visibly record source and
    // destination in the trail. The jump here crosses from a COMPUTE place to a STORAGE place —
    // a directory the test created, so the destination exists on every host and nowhere else.
    let dir = scratch();
    let target = dir.path().display().to_string();
    let child = SleepChild::spawn();
    let script = format!(
        "enter {pid}; jump storage:{target}",
        pid = child.pid(),
        target = target
    );

    let place = place_after(&script);
    assert_eq!(
        text(&place, "display_name"),
        target,
        "§6.5: the jump lands on the resolved storage place, got {place:?}"
    );

    let run = ono(&format!("{script}; trail --json"));
    run.assert_success();
    let steps = rows(&run);
    let jump = steps
        .iter()
        .find(|step| step["movement"].as_str() == Some("jump"))
        .unwrap_or_else(|| panic!("§20.1: the trail records the movement kind, got {steps:?}"));
    assert!(
        !jump["from"].is_null() && !jump["to"].is_null(),
        "§6.5: a jump records where it came from and where it went, got {jump:?}"
    );
    assert_ne!(
        text(jump, "from"),
        text(jump, "to"),
        "§6.5: teleportation between two different places, got {jump:?}"
    );
}

// --- back, up, home --------------------------------------------------------------------------

#[test]
fn should_return_to_the_process_when_back_follows_the_navigation_history() {
    // §44.6, first half: after descending the hierarchy to a process and following its socket,
    // `back` MUST return to the process. Invariant 4: every movement is reversible along the
    // actual trail. The socket is one this test bound on the loopback interface, so the path is
    // the same on every host.
    let (_listener, port) = listener();
    let place = place_after(&format!("{}; back", path_to_the_listening_socket(port)));
    assert_eq!(
        text(&place, "object_type"),
        "ono.process/1",
        "§6.6/§44.6: `back` follows history, and the previous place was the process, got \
         {place:?}"
    );
    assert_eq!(
        place["canonical_ref"]["pid"].as_u64(),
        Some(u64::from(std::process::id())),
        "§44.6: `back` returns to the very process the socket was followed from, got {place:?}"
    );
}

#[test]
fn should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy() {
    // §44.6, second half, over the identical path: `up` MUST go to the socket's canonical
    // hierarchy parent — under NETWORK — and therefore NOT to the process `back` returns to.
    // §6.6 says "normally NETWORK/SOCKETS" while §7.3 makes `listeners`/`connections` the
    // required children, so this asserts the domain and the distinction, not a child name.
    let (_listener, port) = listener();
    let path = path_to_the_listening_socket(port);
    let up = place_after(&format!("{path}; up"));
    assert_ne!(
        text(&up, "object_type"),
        "ono.process/1",
        "§6.6/§44.6: `up` follows canonical hierarchy, not history — it does not land on the \
         process, got {up:?}"
    );
    // ADR-0151: the field that names where a place sits in the canonical hierarchy is
    // `place_path` (`local/network/listeners`), because ADR-0140 keeps `scope` for the §3.2
    // boundary — `host:web01`. The assertion is the one this test always made, over the field
    // that carries the answer.
    let where_it_landed = format!(
        "{} {} {}",
        text(&up, "display_name").to_lowercase(),
        text(&up, "place_path").to_lowercase(),
        serde_yaml_ng::to_string(&up["scope"])
            .unwrap_or_default()
            .to_lowercase()
    );
    assert!(
        where_it_landed.contains("network"),
        "§7.3/§44.6: the canonical parent of a socket lies under NETWORK, got {up:?}"
    );

    let back = place_after(&format!("{path}; back"));
    assert_ne!(
        text(&up, "spatial_id"),
        text(&back, "spatial_id"),
        "§53 and §44.6: `back` follows history and `up` follows hierarchy — from the same place \
         they are deliberately different destinations; `back` gave {back:?}, `up` gave {up:?}"
    );
}

#[test]
fn should_return_to_the_system_root_when_home_runs_after_deep_navigation() {
    // §6.6: `home` returns to the root SYSTEM place of the current host. §7.1: that root is a
    // SystemPlace naming the host and its domains, never a flat list of every known object.
    let (_listener, port) = listener();
    let place = place_after(&format!("{}; home", path_to_the_listening_socket(port)));
    assert_eq!(
        text(&place, "object_type"),
        "ono.system/1",
        "§6.6/§7.1: `home` lands on the SystemPlace of the active host, got {place:?}"
    );

    let fresh = place_after("look --json > /dev/null; home");
    assert_eq!(
        text(&place, "spatial_id"),
        text(&fresh, "spatial_id"),
        "§7.1: the root of a host is one stable place, however it was reached, got {place:?} \
         and {fresh:?}"
    );
}

#[test]
fn should_answer_history_empty_when_back_runs_with_no_previous_place() {
    // §40 requires `spatial.history_empty`. A fresh non-interactive session has moved nowhere,
    // so there is nothing to return to and the refusal must be structured rather than silent.
    let error = caught("back");
    assert_spatial_error(&error, "spatial.history_empty", "back");
    assert!(
        !ono("back").status().is_success(),
        "§40: a refused movement exits non-zero"
    );
}

#[test]
fn should_answer_no_parent_when_up_runs_at_the_system_root() {
    // §40 requires `spatial.no_parent`, and §7.1 makes the SYSTEM root the top of the canonical
    // hierarchy for a host: above it there is nothing to move to.
    let error = caught("home; up");
    assert_spatial_error(&error, "spatial.no_parent", "home; up");
}

// --- trail -----------------------------------------------------------------------------------

#[test]
fn should_record_every_movement_with_its_kind_and_relation_when_the_trail_is_read_as_json() {
    // §29.1: `trail --json` MUST work without a TTY. §20.1 spells the NavigationStep field by
    // field, and §6.4 requires the traversed relation to be recorded. The path is the §44.6 one:
    // three `enter`s down the hierarchy, then one `follow` along a relationship edge.
    let (_listener, port) = listener();
    let run = ono(&format!(
        "{}; trail --json",
        path_to_the_listening_socket(port)
    ));
    run.assert_success();
    let steps = rows(&run);
    assert!(
        steps.len() >= 4,
        "§6.7: the trail keeps every movement of the session, and this path made four, got \
         {steps:?}"
    );
    for step in &steps {
        for field in ["timestamp", "from", "to", "movement"] {
            assert!(
                !step[field].is_null(),
                "§20.1: a NavigationStep carries `{field}`, got {step:?}"
            );
        }
        assert!(
            matches!(
                step["movement"].as_str(),
                Some("enter" | "follow" | "jump" | "back" | "up" | "home")
            ),
            "§20.1: `movement` is one of enter|follow|jump|back|up|home, got {step:?}"
        );
    }
    let movements: Vec<String> = steps
        .iter()
        .map(|step| text(step, "movement"))
        .collect::<Vec<_>>();
    let tail = movements
        .split_at(movements.len().saturating_sub(4))
        .1
        .to_vec();
    assert_eq!(
        tail,
        vec!["enter", "enter", "enter", "follow"],
        "§6.7/§20.1: the trail preserves the movements in the order they happened, got \
         {steps:?}"
    );
    let followed = steps
        .iter()
        .find(|step| step["movement"].as_str() == Some("follow"))
        .expect("the path ends in a follow");
    assert_eq!(
        text(followed, "relation"),
        "socket",
        "§6.4: the relation traversed MUST be recorded in the trail, got {followed:?}"
    );
}

// --- find ------------------------------------------------------------------------------------

#[test]
fn should_stream_places_with_scope_and_provenance_when_find_searches_with_a_predicate() {
    // §29.4: `find place` is a normal structured stream. §6.8: results MUST carry enough
    // path/scope information to disambiguate identical names, and MUST come from the index and the
    // provider registries rather than from grepping rendered text. §27.4: a result that may come
    // from a cache carries its freshness/provenance. The predicate is spelled as an ordinary
    // v0.2 expression — the reading recorded at the top of this file.
    let _child = SleepChild::spawn();
    let run = ono("find place --where state == \"running\" | take 5 | to json");
    run.assert_success();
    let rows = rows(&run);
    assert!(
        !rows.is_empty(),
        "§6.8: a system with running processes answers a `state == running` search, got {:?}; \
         stderr: {:?}",
        run.stdout(),
        run.stderr()
    );
    for row in &rows {
        assert!(
            !row["spatial_id"].is_null(),
            "§3.1: every found place is identified, got {row:?}"
        );
        assert!(
            !row["scope"].is_null(),
            "§6.8: results carry the scope that disambiguates identical names, got {row:?}"
        );
        assert!(
            !row["provenance"].is_null(),
            "§27.4/§3.1: a search result says where the fact came from and how fresh it is, got \
             {row:?}"
        );
    }
}

#[test]
fn should_refuse_a_predicate_over_a_field_no_kind_of_place_declares() {
    // v0.2 §11.3 and §15.4, with v0.4 §2.17 and §29.3. `get process | where cpy > 20` refuses
    // before anything is enumerated, and names the nearest declared field. `find place` searches
    // across kinds instead of down one stream, but the reasoning is the same: a word no kind of
    // place declares can only be a typo, and answering it with an empty stream makes a typo in a
    // script indistinguishable from an empty system (ADR-0210).
    let run = ono("find place --where telepathy == 1 | count | to json");
    assert!(
        !run.status().is_success(),
        "v0.2 §11.3: a predicate over a field nothing declares is refused, got exit {:?} and \
         stdout {:?}",
        run.status(),
        run.stdout()
    );
    let seen = format!("{}{}", run.stdout(), run.stderr());
    assert!(
        seen.contains("Ono-Sendai-E0202") && seen.contains("telepathy"),
        "v0.2 §11.3: the refusal is `type.unknown_field` naming the field, got {seen:?}"
    );

    let near = ono("find place --where memroy > 1 | count | to json");
    let seen = format!("{}{}", near.stdout(), near.stderr());
    assert!(
        seen.contains("memory"),
        "v0.2 §15.4: the refusal offers the nearest declared field, got {seen:?}"
    );
}

#[test]
fn should_still_search_across_kinds_when_only_some_of_them_declare_the_field() {
    // The other half of the same rule, and the reason it is not simply "refuse whenever a
    // candidate lacks the field": a cross-type search is what `find place` is for, and a mount
    // having no `pid` is not an error. Only a field *no* candidate declares is a typo.
    let child = SleepChild::spawn();
    let run = ono(&format!(
        "find place --where pid == {} | count | to json",
        child.pid()
    ));
    run.assert_success();
    let counted = rows(&run)
        .first()
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            panic!(
                "§29.4: `find place | count` answers a number, got {:?}",
                run.stdout()
            )
        });
    assert!(
        counted >= 1,
        "§6.8: a predicate over a field only processes declare still searches the processes, got \
         {counted}; stderr {:?}",
        run.stderr()
    );

    // The same rule one level down, and the case the container caught: a provider *target* may
    // serve more than one schema. `filesystem` serves `ono.filesystem/1` and `ono.mount/1`, and
    // only the second declares a `filesystem` field, so the record — not the target — is the
    // granularity at which "this one cannot be asked" is decided. Every Linux has a tmpfs.
    let mounts = ono("find place --where filesystem == \"tmpfs\" | count | to json");
    mounts.assert_success();
    let counted = rows(&mounts)
        .first()
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            panic!(
                "§29.4: `find place | count` answers a number, got {:?}",
                mounts.stdout()
            )
        });
    assert!(
        counted >= 1,
        "§6.8: a target whose providers serve several schemas still answers from the one that \
         declares the field, got {counted}; stderr {:?}",
        mounts.stderr()
    );
}

#[test]
fn should_surface_an_evaluation_error_rather_than_answering_an_empty_search() {
    // §2.17 and §29.3. `memory > 1` compares a bytesize with an int, which the v0.2 pipeline
    // reports as `Ono-Sendai-E0203` on every row. A search that swallowed it would answer `0`
    // for a question it never managed to ask, which is uncertainty rendered as absence
    // (ADR-0210).
    let run = ono("find place --type process --where memory > 1 | count | to json");
    assert!(
        !run.status().is_success(),
        "§2.17: a predicate that cannot be evaluated is an error, not an empty answer, got exit \
         {:?} and stdout {:?}",
        run.status(),
        run.stdout()
    );
    let seen = format!("{}{}", run.stdout(), run.stderr());
    assert!(
        seen.contains("Ono-Sendai-E0203"),
        "§2.17: the search reports the comparison the pipeline reports, got {seen:?}"
    );
}

#[test]
fn should_compose_with_the_v02_pipeline_when_a_find_result_is_filtered_and_counted() {
    // §29.4 with §28: `find place` participates in object pipelines rather than forming a
    // parallel silo. Two `sleep` children the test owns give a lower bound nothing can lower.
    //
    // Both halves read *one* search. The machine's own `sleep` processes come and go — this
    // suite's other tests spawn some of them — so two separate searches would be comparing two
    // moments of the machine rather than two ends of one stream, which AGENTS.md §11 forbids a
    // test from depending on. The search is bound once and read twice, and the count is
    // redirected so that the one document on stdout is still the stream.
    let one = SleepChild::spawn();
    let two = SleepChild::spawn();
    let dir = scratch();
    let tally = dir.path().join("count.json");
    let run = isolated(&dir)
        .args([
            "-c",
            &format!(
                "let found = find place sleep; $found | count | to json > {tally}; \
                 $found | to json",
                tally = tally.display()
            ),
        ])
        .run();
    run.assert_success();
    let found = rows(&run);
    let processes = found
        .iter()
        .filter(|row| row["object_type"].as_str() == Some("ono.process/1"))
        .count();
    assert!(
        processes >= 2,
        "§6.8/§29.4: `find place` answers spatial objects the pipeline reads, and the two \
         `sleep` children ({}, {}) are among them — File records from the v0.3 findutils adapter \
         mean the spatial command is missing, got {found:?}",
        one.pid(),
        two.pid()
    );

    let written = std::fs::read_to_string(&tally).expect("`count` wrote its answer");
    let count = serde_yaml_ng::from_str::<Value>(written.trim())
        .ok()
        .and_then(|document| document.as_sequence()?.first()?.as_u64())
        .unwrap_or_else(|| panic!("§29.4: `find place | count` answers a number, got {written:?}"));
    assert_eq!(
        usize::try_from(count).unwrap_or(usize::MAX),
        found.len(),
        "§29.4: the same stream feeds `to json` and `count`, got {count} against {} rows",
        found.len()
    );
}

// --- ambiguity in scripts ----------------------------------------------------------------------

#[test]
fn should_answer_ambiguous_selector_when_a_script_selector_matches_several_places() {
    // §29.3: scripts MUST never open interactive pickers; ambiguity is an error. §27.2 names it
    // `spatial.ambiguous_selector`, and §40 requires it to be structured. Two `sleep` children
    // guarantee at least two matches whatever else the host is running.
    let _one = SleepChild::spawn();
    let _two = SleepChild::spawn();
    let error = caught("enter sleep");
    assert_spatial_error(&error, "spatial.ambiguous_selector", "enter sleep");

    let run = ono("enter sleep");
    assert!(
        !run.status().is_success(),
        "§29.3: an ambiguous selector fails the script, got {:?}",
        run.status()
    );
    assert!(
        run.stdout().trim().is_empty(),
        "§29.3: a script gets an error, never a picker — nothing is offered on stdout, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_resolve_the_ambiguity_when_the_script_explicitly_selects_the_first_match() {
    // §29.3: ambiguity is an error "unless the script explicitly selects first/unique or uses an
    // exact ID". The explicit first selection is spelled with the v0.2 pipeline §28.3 already
    // uses to turn a result into a place: `… | take 1 | enter`.
    let one = SleepChild::spawn();
    let two = SleepChild::spawn();
    let place = place_after("find place sleep | take 1 | enter");
    let pid = place["canonical_ref"]["pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("§28.2: the entered place keeps its canonical ref, {place:?}"));
    assert!(
        pid == u64::from(one.pid()) || pid == u64::from(two.pid()) || pid > 0,
        "§29.3: selecting the first match resolves the ambiguity and enters one real process, \
         got {place:?}"
    );
    assert_eq!(
        text(&place, "object_type"),
        "ono.process/1",
        "§29.3/§28.2: the resolved place is the selected object, got {place:?}"
    );
}

#[test]
fn should_resolve_the_ambiguity_when_the_script_names_the_exact_spatial_id() {
    // §29.3: an exact ID resolves what a name cannot. §3.1: the SpatialId is opaque to users but
    // stable while the object is the same object — so an id read in one run still names the same
    // place in the next one, which §27.1 step 5 (the current-host index) is what makes possible.
    let child = SleepChild::spawn();
    let _second = SleepChild::spawn();
    let lookup = ono(&format!(
        "get process {pid} | enter; look --json | from json | select place.spatial_id | to json",
        pid = child.pid()
    ));
    lookup.assert_success();
    let rows = rows(&lookup);
    let id = rows
        .first()
        .and_then(|row| row["spatial_id"].as_str())
        .unwrap_or_else(|| panic!("§3.1: the place has a spatial id, got {rows:?}"))
        .to_owned();

    let place = place_after(&format!("enter \"{id}\""));
    assert_eq!(
        text(&place, "spatial_id"),
        id,
        "§29.3/§27.1: an exact spatial id resolves to exactly that place, in a session that \
         never saw the ambiguous name, got {place:?}"
    );
    assert_eq!(
        place["canonical_ref"]["pid"].as_u64(),
        Some(u64::from(child.pid())),
        "§3.1: the id identifies the same conceptual object across sessions, got {place:?}"
    );
}

// --- not found -------------------------------------------------------------------------------

#[test]
fn should_answer_not_found_when_a_navigation_argument_names_nothing() {
    // §40 requires `spatial.not_found`. The name below cannot exist as a place, a relation or a
    // bookmark on any host, so every navigation verb must refuse it the same structured way
    // rather than moving somewhere approximate — §27.3 forbids fuzzy matching from acting alone.
    for script in [
        "enter ono-spatial-nothing-of-this-name",
        "follow ono-spatial-nothing-of-this-name",
        "jump ono-spatial-nothing-of-this-name",
    ] {
        let error = caught(script);
        assert_spatial_error(&error, "spatial.not_found", script);
        let run = ono(script);
        assert!(
            !run.status().is_success(),
            "§40: `{script}` names nothing and must fail the script, got {:?}",
            run.status()
        );
        assert!(
            run.stdout().trim().is_empty(),
            "§40: a refusal writes a structured error, not a place, got {:?}",
            run.stdout()
        );
    }
}

// --- map, bounded ------------------------------------------------------------------------------

#[test]
fn should_answer_a_bounded_graph_when_map_json_runs_without_a_tty() {
    // §29.4: `map --json` returns a bounded graph value; §6.9 forbids it depending on terminal
    // rendering; §47 sets the default bound at `spatial.map.node_budget = 100`; invariant 9
    // forbids dumping the complete system graph. The shape of that graph is the §22 data
    // contract and is asserted elsewhere — what this test pins is that it is bounded and that a
    // script gets it at all.
    let run = ono("map --json");
    run.assert_success();
    let map = record(&run);
    let nodes = map["nodes"]
        .as_sequence()
        .unwrap_or_else(|| panic!("§22: a SpatialMap has nodes, got {map:?}"))
        .len();
    assert!(
        nodes > 0,
        "§6.9: the default map shows the current place and its horizon, got {map:?}"
    );
    assert!(
        nodes <= NODE_BUDGET,
        "§47/§6.9 with invariant 9: the default map stays within the node budget of \
         {NODE_BUDGET}, got {nodes} nodes"
    );
    assert!(
        !map["hidden"].is_null(),
        "§22 with the §54 checklist: what the bound left out is disclosed, not silently \
         dropped, got {map:?}"
    );
}

// --- scripts do not leak their place -----------------------------------------------------------

#[test]
fn should_leave_the_callers_place_untouched_when_a_called_script_navigates() {
    // §29.2: a script MAY navigate, but its current place is script-local — it MUST NOT silently
    // change the caller's spatial context. The caller stands in COMPUTE, the called script walks
    // off to STORAGE, and the caller is still in COMPUTE afterwards.
    let dir = scratch();
    let script = dir.write("navigate.ono", "home\nenter storage\nlook --json\n");
    let binary = ono_testkit::ono_binary();
    let run = isolated(&dir)
        .args([
            "-c",
            &format!(
                "enter compute; {binary} {script} > /dev/null; look --json",
                binary = binary.display(),
                script = script.display()
            ),
        ])
        .run();
    run.assert_success();
    let place = place_of(&run);
    assert_eq!(
        text(&place, "display_name").to_lowercase(),
        "compute",
        "§29.2: the called script's navigation is script-local; the caller is still where it \
         was, got {place:?}"
    );
}

#[test]
fn should_start_at_the_system_root_with_an_empty_trail_when_a_new_session_begins() {
    // §46.1: the default v0.4 behaviour is to start at the local SYSTEM root, and trail
    // persistence across sessions is off. So one run's navigation is invisible to the next even
    // when both share a home directory — which is also what makes every other test in this file
    // independent of the order the suite runs them in.
    let dir = scratch();
    let first = isolated(&dir)
        .args(["-c", "home; enter compute; enter processes; look --json"])
        .run();
    first.assert_success();
    assert_eq!(
        text(&place_of(&first), "display_name").to_lowercase(),
        "processes",
        "§7.2: the first session really did move"
    );

    let second = isolated(&dir).args(["-c", "look --json"]).run();
    second.assert_success();
    assert_eq!(
        text(&place_of(&second), "object_type"),
        "ono.system/1",
        "§46.1: a new session starts at the local SYSTEM root, whatever the previous one did"
    );

    let trail = isolated(&dir).args(["-c", "trail --json"]).run();
    trail.assert_success();
    assert!(
        rows(&trail).is_empty(),
        "§46.1: trail persistence across sessions is disabled by default, got {:?}",
        trail.stdout()
    );
}

// --- name collisions with Unix programs ----------------------------------------------------------

#[test]
fn should_run_the_native_spatial_find_and_keep_the_external_find_reachable_when_both_exist() {
    // ADR-0124 §Decision 1: the spatial search keeps its target word, so `find place …` is the
    // spatial command and the bare word `find` still reaches findutils. v0.2 §6.5 resolves
    // native-command-before-`PATH`-executable by verb *and target*, exactly as it already does
    // for `find file`, and `explain` shows which step matched. Invariant 15 keeps the other side
    // true: Unix remains underneath, so the forced `exec:` namespace still runs the program.
    let run = ono("find place --where state == \"running\" | count | to json");
    run.assert_success();
    let rows = rows(&run);
    assert!(
        rows.first().and_then(Value::as_u64).is_some(),
        "§6.8 with v0.2 §6.5: `find place` answers a stream the pipeline counts — an external \
         `find` complaining about `--where` means the native command is missing, got {:?}; \
         stderr: {:?}",
        run.stdout(),
        run.stderr()
    );

    let explained = ono("explain find place nginx");
    explained.assert_success();
    assert!(
        explained.stdout().contains("ono.place.find"),
        "ADR-0124: which §6.5 step matched is inspectable rather than folklore, got {:?}",
        explained.stdout()
    );

    if on_path("find") {
        let dir = scratch();
        let bare = ono(&format!(
            "find {root} -maxdepth 0",
            root = dir.path().display()
        ));
        bare.assert_success();
        assert_eq!(
            bare.stdout().trim(),
            dir.path().display().to_string(),
            "ADR-0124 §Decision 1 with v0.3 §1.71: the bare word `find` is still findutils, so \
             every script and every finger that already knows it keeps working, got {:?}",
            bare.stdout()
        );

        let external = ono(&format!(
            "exec:find {root} -maxdepth 0",
            root = dir.path().display()
        ));
        external.assert_success();
        assert_eq!(
            external.stdout().trim(),
            dir.path().display().to_string(),
            "v0.2 §6.5 with invariant 15: `exec:` reaches the external program explicitly \
             whatever the registry holds, got {:?}",
            external.stdout()
        );
    }
}

#[test]
fn should_run_the_native_spatial_look_and_keep_the_external_look_reachable_when_both_exist() {
    // The same rule for `look`, which collides with util-linux's `look(1)` on many hosts: the
    // spatial spelling MUST be the spatial command (§6.1, §29.1), and `exec:look` MUST still be
    // the program (v0.2 §6.5, invariant 15).
    let run = ono("look --json");
    run.assert_success();
    let place = place_of(&run);
    assert!(
        !text(&place, "display_name").is_empty(),
        "§6.1 with v0.2 §6.5: `look --json` is the spatial command even where `look(1)` is \
         installed — a usage message from util-linux means the native command is missing, got \
         {:?}; stderr: {:?}",
        run.stdout(),
        run.stderr()
    );

    if on_path("look") {
        let external = ono("exec:look --version");
        external.assert_success();
        assert!(
            external.stdout().to_lowercase().contains("look"),
            "v0.2 §6.5 with invariant 15: `exec:look` still runs the program, got {:?}",
            external.stdout()
        );
    }
}

#[test]
fn should_keep_running_external_commands_when_spatial_navigation_has_happened() {
    // Invariant 15 and §44.10: spatial navigation MUST NOT prevent ordinary external command
    // execution. A script that has moved through the spatial hierarchy still runs programs, and
    // their output is theirs alone — the spatial state does not leak into what they print.
    let run = ono("home; enter compute; enter processes; exec:printf 'still-unix\\n'");
    run.assert_success();
    assert!(
        run.stderr().trim().is_empty(),
        "§6.3/§40: the navigation before the program was carried out, not refused — a script \
         that could not move never proved that Unix survived the move, got {:?}",
        run.stderr()
    );
    assert_eq!(
        run.stdout().trim(),
        "still-unix",
        "§44.10/invariant 15: after spatial navigation an external command still runs and its \
         output is unchanged, got {:?}; stderr: {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_name_the_positional_spelling_when_a_selector_is_written_as_an_option() {
    // `near --relation process` listed the four options `near` takes and never mentioned that a
    // relation is the positional selector `near <relation>` — true, and no help at all to the
    // person who typed it (§40's "actionable next steps", ADR-0271).
    let run = ono("enter process 1; near --relation process");
    assert!(!run.status().is_success());
    let text = run.stderr();
    assert!(
        text.contains("Ono-Sendai-E0202") && text.contains("near <relation>"),
        "the refusal names the spelling that works, got {text:?}"
    );
}

#[test]
fn should_refuse_a_relation_the_place_does_not_offer_rather_than_answering_nothing() {
    // §40 and §2.17: "this place has no such exit" and "this exit is empty" are different
    // answers, and an empty stream with status 0 said both. A process has `sockets`, not
    // `socket`, and the refusal is what makes the difference visible.
    let run = ono("enter process 1; near socket");
    assert!(!run.status().is_success());
    let text = run.stderr();
    assert!(
        text.contains("Ono-Sendai-E1004") && text.contains("sockets"),
        "the refusal names the exits this place does have, got {text:?}"
    );
}

#[test]
fn should_answer_an_empty_stream_for_an_exit_that_exists_and_holds_nothing() {
    // The other half: a relation the place declares and has no neighbour in is an empty answer,
    // not a refusal — the name was understood (§40, the `find` precedent of ADR-0210).
    let run = ono("enter compute; near cgroups | count | to json");
    run.assert_success();
    assert!(
        run.stdout().lines().any(|line| line.starts_with('[')),
        "an exit that exists answers with a stream, got {:?}",
        run.output()
    );
}
