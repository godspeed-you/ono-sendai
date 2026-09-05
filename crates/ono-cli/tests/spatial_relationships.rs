//! Outcome tests for the relationship half of the v0.4 spatial systems interface: the edges
//! `follow`, `near` and `map` traverse, the distinction between a canonical hierarchy edge and a
//! relationship edge, and the live spatial state that makes an edge appear and vanish.
//!
//! Specification: `docs/specs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` — §2
//! (invariants 3, 5, 6, 12, 16, 17), §3.5 (RelationshipEdge), §6.2/§6.4/§6.6 (`near`, `follow`,
//! `back`/`up`), §11 (hierarchy versus graph, explainability, confidence), §12/§13/§14 (the
//! process, service and network spaces and the edges they promise), §16.2 (namespace boundary),
//! §22 (the `SpatialMap` contract), §25 (live spatial state), §29 (scripting semantics), §31
//! (`trace` versus spatial navigation), §32 (relationship provider requirements), §35.2
//! (permission states stay distinct from empty), §40 (structured spatial errors), §43.6 (a live
//! test must be caused by a real change), §44.4/§44.5/§44.6/§44.9 (acceptance scenarios).
//!
//! `look` and `find` shadow the external programs of the same name (ADR-0124), so every test
//! begins by refusing an external answer — a spatial assertion may never be satisfied by
//! `/usr/bin/look`, and a missing command must fail as a missing command.
//!
//! The fixture is built by the test itself (§43.3, as far as an unprivileged offline run
//! reaches): a `sh` child holding a scratch file open, a `sleep` child that owns no socket, a TCP
//! listener bound on `127.0.0.1:0` by the test process, and a connection the test opens to its
//! own listener. Nothing here depends on a service, a name or a port that only exists on the
//! developer's machine.
//!
//! Field names are asserted flexibly on purpose: the spec fixes the *behaviour* of a place, an
//! edge and a neighborhood, not the JSON spelling of every field, so a test looks for the
//! identity it navigated to and for the fields §3.5/§11.4/§22 name, and never for an internal
//! structure (AGENTS.md §11).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use ono_testkit::ono_within;
use ono_testkit::{Scratch, Shell, SkipReason, scratch};
use serde_yaml_ng::Value;

mod support;
use support::{SleepChild, listener};

/// Runs a one-liner with a generous budget: nothing here may hang, and a run that does is a
/// failure of the shell, not of the test.
fn ono(script: &str) -> ono_testkit::Run {
    ono_within(script, Duration::from_secs(30))
}

/// Whether the run is unprivileged. The permission-honesty test asserts what a normal user sees
/// when the kernel refuses; as root the kernel would answer and there would be nothing to assert.
fn unprivileged() -> bool {
    if ono_process::effective_uid() == 0 {
        ono_testkit::skipped(
            SkipReason::MissingPrivilege,
            "this test asserts what an unprivileged user is refused",
        );
        return false;
    }
    true
}

/// Fails when the shell did not answer as the owner of the spatial command: a missing command,
/// an unenterable target, or an external program of the same name answering instead.
fn assert_spatial_command(run: &ono_testkit::Run, command: &str) {
    let stderr = run.stderr();
    assert!(
        !stderr.contains("Ono-Sendai-E0101"),
        "spec §6: `{command}` is a spatial command of the shell, not an unknown name; got stderr \
         {stderr:?}"
    );
    assert!(
        !stderr.contains("Ono-Sendai-E0102"),
        "spec §6: `{command}` resolves a spatial place; got stderr {stderr:?}"
    );
    assert!(
        !stderr.contains("/usr/bin/look") && !stderr.contains("/usr/bin/find"),
        "spec §6: `{command}` is the spatial command, never the external program of the same \
         name; got stderr {stderr:?}"
    );
}

/// The JSON document a `--json` spatial command printed.
fn document(run: &ono_testkit::Run, command: &str) -> Value {
    assert_spatial_command(run, command);
    let text = run.stdout().trim().to_owned();
    let stderr = run.stderr();
    serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!(
            "spec §29.1: `{command}` prints a JSON document without a TTY, got stdout {text:?} \
             ({error}); stderr: {stderr:?}"
        )
    })
}

/// The values a `| to json` stage printed, as a sequence.
fn rows(run: &ono_testkit::Run, command: &str) -> Vec<Value> {
    let document = document(run, command);
    let stdout = run.stdout();
    document
        .as_sequence()
        .unwrap_or_else(|| panic!("spec §29.4: `{command}` is an ordinary stream, got {stdout:?}"))
        .clone()
}

/// The `PlaceView` `look --json` printed (§6.1), unwrapped from a one-element stream if the
/// implementation chose to emit it as a stream.
fn place_view(run: &ono_testkit::Run) -> Value {
    let document = document(run, "look --json");
    match document {
        Value::Sequence(mut values) if values.len() == 1 => values.remove(0),
        other => other,
    }
}

/// The current place inside a `PlaceView`: a nested `place` object where the view carries one,
/// the view itself otherwise.
fn place(view: &Value) -> Value {
    view.get("place").cloned().unwrap_or_else(|| view.clone())
}

/// The type the current place reports (§3.1 `object_type`), whatever field carries it.
fn place_type(view: &Value) -> String {
    let place = place(view);
    for field in ["type", "object_type", "kind", "schema", "place_type"] {
        if let Some(Value::String(value)) = place.get(field) {
            return value.to_lowercase();
        }
    }
    panic!(
        "spec §3.1/§6.1: `look --json` names the type of the current place in one of \
         `type`/`object_type`/`kind`/`schema`, got {view:?}"
    )
}

/// A value rendered as text, for identity assertions that must not depend on field spelling.
fn rendered(value: &Value) -> String {
    serde_yaml_ng::to_string(value).expect("a spatial value serialises")
}

/// Asserts that the place the shell reports is the object the test navigated to.
fn assert_place_is(view: &Value, type_needle: &str, identity: &str, section: &str) {
    let kind = place_type(view);
    assert!(
        kind.contains(type_needle),
        "{section}: the current place is a `{type_needle}`, and `look --json` reports its type as \
         {kind:?}"
    );
    let shown = rendered(&place(view));
    assert!(
        shown.contains(identity),
        "{section}: the current place is the object identified by {identity:?}, got {shown}"
    );
}

/// A structured spatial failure (§40): the command exists, refuses, and names which of the §40
/// conditions occurred. The shell's error model carries a dotted name beside its numeric code
/// (v0.2 §43), so the name is what a caller can act on.
fn assert_spatial_error(run: &ono_testkit::Run, command: &str, error_name: &str) {
    assert_spatial_command(run, command);
    assert!(
        !run.status().is_success(),
        "spec §40: `{command}` fails with a structured error, got exit {:?} and stdout {:?}",
        run.status(),
        run.stdout()
    );
    let seen = format!("{}{}", run.stdout(), run.stderr());
    assert!(
        seen.contains(error_name),
        "spec §40: `{command}` reports `{error_name}`, got {seen:?}"
    );
}

/// The edges of a `SpatialMap` (§22), whatever the map nests them under.
fn map_edges(run: &ono_testkit::Run) -> Vec<Value> {
    let document = document(run, "map --json");
    let map = match &document {
        Value::Sequence(values) if values.len() == 1 => values[0].clone(),
        other => other.clone(),
    };
    map.get("edges")
        .and_then(Value::as_sequence)
        .unwrap_or_else(|| {
            panic!("spec §22: `map --json` returns a SpatialMap with an `edges` list, got {map:?}")
        })
        .clone()
}

/// A `sh` child holding a scratch file open on a known descriptor, plus the `sleep` it forks —
/// both are holders of that file, and both die with the fixture.
struct FileHolder {
    /// Kept so the scratch tree outlives the child that holds a file inside it.
    _directory: Scratch,
    path: PathBuf,
    child: Child,
}

impl FileHolder {
    fn spawn() -> Self {
        let directory = scratch();
        let path = directory.write("held.conf", b"listen 8080;\n");
        let child = Command::new("sh")
            .arg("-c")
            .arg(format!("exec 3< {}; sleep 30", path.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("`sh` is available on every test host");
        // The descriptor is open once the shell has run its first word; the poll keeps the test
        // from racing the child rather than sleeping blindly.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline
            && std::fs::read_link(format!("/proc/{}/fd/3", child.id())).is_err()
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        Self {
            _directory: directory,
            path,
            child,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn path(&self) -> String {
        self.path.display().to_string()
    }
}

impl Drop for FileHolder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A `sh` child holding two scratch files open, one of them named with an escape byte in it.
///
/// The pair makes `follow files` ambiguous, which is the refusal §29.3 requires and the one that
/// carries a list; the escape byte is there so the test can tell the shell's own line breaks
/// from the bytes a filename brought with it (ADR-0015 T1, ADR-0211).
struct TwoFileHolder {
    _directory: Scratch,
    child: Child,
}

impl TwoFileHolder {
    const HOSTILE: &'static str = "held-\u{1b}[2Jsecond.conf";

    fn spawn() -> Self {
        let directory = scratch();
        let first = directory.write("held-first.conf", b"listen 8080;\n");
        let second = directory.write(Self::HOSTILE, b"listen 8081;\n");
        let child = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "exec 3< '{}'; exec 4< '{}'; sleep 30",
                first.display(),
                second.display()
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("`sh` is available on every test host");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline
            && std::fs::read_link(format!("/proc/{}/fd/4", child.id())).is_err()
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        Self {
            _directory: directory,
            child,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for TwoFileHolder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn should_break_a_listing_refusal_into_lines_while_still_escaping_what_the_names_carry() {
    // §29.3 and §40 with ADR-0015 T1. `follow files` at a process holding several files is an
    // ambiguity the shell answers with the candidates, and the candidates are a list — one per
    // line. The render boundary used to escape every control character in the message, including
    // the newlines the shell itself had written, so the whole list arrived on one line with
    // `\u{a}` between the entries: unreadable exactly where it matters most.
    //
    // The two halves are asserted together on purpose, because separating them is the defect:
    // the structure the shell wrote survives, and the bytes a filename carries do not.
    let holder = TwoFileHolder::spawn();
    let run = ono(&format!("enter process {}; follow files", holder.pid()));
    let stderr = run.stderr();
    assert!(
        stderr.contains("spatial.ambiguous_selector"),
        "§29.3: several open files make `follow files` ambiguous, got {stderr:?}"
    );
    assert!(
        !stderr.contains("\\u{a}"),
        "ADR-0211: the line breaks the shell wrote are line breaks, not escapes, got {stderr:?}"
    );
    let listed = stderr.lines().filter(|line| line.contains("held-")).count();
    assert!(
        listed >= 2,
        "§29.3: each candidate is on its own line, got {listed} in {stderr:?}"
    );
    assert!(
        !stderr.contains('\u{1b}'),
        "ADR-0015 T1: a filename never reaches the terminal with its escape byte intact, got \
         {stderr:?}"
    );
    assert!(
        stderr.contains("\\u{1b}"),
        "ADR-0015 T1: the escape byte is shown as data rather than dropped, got {stderr:?}"
    );
}

/// A listening TCP socket the test process owns, so the shell can attribute it to a pid the test
/// knows without any privilege.
#[test]
fn should_enter_the_open_file_when_following_it_from_the_holding_process() {
    // §44.4: `process -> follow file -> file`. The edge is the one the kernel already reports
    // (process --reads--> file); `follow` traverses it and, per invariant 3, the place changes.
    let holder = FileHolder::spawn();
    let run = ono(&format!(
        "enter process {}; follow file {}; look --json",
        holder.pid(),
        holder.path()
    ));
    let view = place_view(&run);
    assert_place_is(&view, "file", &holder.path(), "spec §44.4");
}

#[test]
fn should_name_the_holding_process_among_the_file_neighbors_when_the_file_is_the_place() {
    // §44.4 in the other direction: `file -> near process` must expose the same relationship.
    // `near` is an ordinary stream (§29.4), so it composes with `to json`.
    let holder = FileHolder::spawn();
    let run = ono(&format!(
        "enter process {}; follow file {}; near --type process | to json",
        holder.pid(),
        holder.path()
    ));
    let neighbors = rows(&run, "near --type process");
    let pid = holder.pid().to_string();
    let shown = neighbors.iter().map(rendered).collect::<String>();
    assert!(
        shown.contains(&pid),
        "spec §44.4: the file's process neighbors contain the process that holds it open (pid \
         {pid}), got {shown}"
    );
    for neighbor in &neighbors {
        assert!(
            neighbor.get("relation").is_some(),
            "spec §6.2/§3.5: every neighbor names the relation it was reached by, got {neighbor:?}"
        );
    }
}

#[test]
fn should_explain_every_edge_with_relation_provider_and_confidence_when_mapping_a_process() {
    // §11.4: every displayed relationship MUST support inspection — relation, source, target,
    // direction, provider/provenance, confidence, observed_at. §22 fixes the same fields on a
    // `MapEdge`, and §11.5 fixes the confidence vocabulary. A kernel-read edge is `exact`
    // (invariant 5, and the v0.2 graph already reports `confidence: exact` for it).
    let holder = FileHolder::spawn();
    let run = ono(&format!("enter process {}; map --json", holder.pid()));
    let edges = map_edges(&run);
    assert!(
        !edges.is_empty(),
        "spec §22: the map of a process with a parent, an open file and a cgroup carries edges, \
         got {:?}",
        run.stdout()
    );
    for edge in &edges {
        for field in ["relation", "source", "target", "direction", "confidence"] {
            assert!(
                edge.get(field).is_some(),
                "spec §11.4/§22: every edge carries `{field}`, got {edge:?}"
            );
        }
        let shown = rendered(edge);
        assert!(
            shown.contains("provider") || shown.contains("provenance"),
            "spec §11.4: every edge names the provider that observed it, got {shown}"
        );
        let confidence = edge
            .get("confidence")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        assert!(
            ["exact", "strong", "inferred", "user_declared", "unknown"].contains(&&*confidence),
            "spec §11.5: confidence is one of exact/strong/inferred/user_declared/unknown, got \
             {confidence:?}"
        );
    }
    let file_edge = edges
        .iter()
        .find(|edge| rendered(edge).contains(&holder.path()));
    let file_edge = file_edge.unwrap_or_else(|| {
        panic!(
            "spec §12: the process map carries the edge to the file it holds open ({}), got {:?}",
            holder.path(),
            run.stdout()
        )
    });
    assert_eq!(
        file_edge.get("confidence").and_then(Value::as_str),
        Some("exact"),
        "spec §11.5: an edge read from the kernel is exact, not inferred, got {file_edge:?}"
    );
}

#[test]
fn should_name_the_same_relation_and_provider_as_trace_when_the_neighbor_is_the_open_file() {
    // Invariant 16 and §31.3: the spatial layer composes provider data and MUST NOT become an
    // undocumented second source of truth, and `map`/`trace` share the underlying graph. So the
    // relation and the provider a neighbor carries are the ones the v0.2 graph already reports
    // for that very edge, not a spatial re-invention of them.
    let holder = FileHolder::spawn();
    let graph = ono(&format!("trace process {} | to json", holder.pid()));
    graph.assert_success();
    let graph_edges: Vec<Value> = serde_yaml_ng::from_str::<Value>(graph.stdout().trim())
        .expect("`trace … | to json` prints JSON")
        .as_sequence()
        .and_then(|values| values.first().cloned())
        .and_then(|graph| graph.get("edges").and_then(Value::as_sequence).cloned())
        .expect("the v0.2 graph carries edges");
    let traced = graph_edges
        .iter()
        .find(|edge| rendered(edge).contains(&holder.path()))
        .unwrap_or_else(|| {
            panic!(
                "the v0.2 graph reports the open file {} of pid {}",
                holder.path(),
                holder.pid()
            )
        });
    let relation = traced
        .get("relation")
        .and_then(Value::as_str)
        .expect("a v0.2 edge names its relation")
        .to_owned();
    let provider = traced
        .get("provider")
        .and_then(Value::as_str)
        .expect("a v0.2 edge names its provider")
        .to_owned();

    let run = ono(&format!(
        "enter process {}; near --all | to json",
        holder.pid()
    ));
    let neighbors = rows(&run, "near --all");
    let file_neighbor = neighbors
        .iter()
        .find(|neighbor| rendered(neighbor).contains(&holder.path()))
        .unwrap_or_else(|| {
            panic!(
                "spec §12: the process neighborhood contains the file it holds open ({}), got {:?}",
                holder.path(),
                run.stdout()
            )
        });
    let shown = rendered(file_neighbor);
    assert!(
        shown.contains(&relation),
        "spec §31.3/invariant 16: the neighbor is reached by the relation the provider reports \
         ({relation}), got {shown}"
    );
    assert!(
        shown.contains(&provider),
        "spec §11.4/invariant 16: the neighbor names the provider that observed the edge \
         ({provider}), got {shown}"
    );
}

#[test]
fn should_enter_the_listening_socket_when_following_it_from_its_owner_process() {
    // §44.5, first hop: `process -> socket`. §12 makes it explicit — "`follow socket :443` MUST
    // traverse to the matching socket". The listener belongs to this test process, so the shell
    // can attribute it without privilege, and the port is unique to this test.
    let (_listener, port) = listener();
    let run = ono(&format!(
        "enter process {}; follow socket :{port}; look --json",
        std::process::id()
    ));
    let view = place_view(&run);
    assert_place_is(&view, "socket", &port.to_string(), "spec §44.5");
    let shown = rendered(&place(&view)).to_lowercase();
    assert!(
        shown.contains("listen"),
        "spec §14.3: the listener place reports its state, got {shown}"
    );
}

#[test]
fn should_reach_the_accepted_connection_when_following_it_from_the_listening_socket() {
    // §44.5: `process -> socket -> connection` must be navigable by relationship discovery.
    // §14.1/§14.3/§14.4 separate a listener place from a connection place, so `follow socket`
    // reaches the listener and `follow connection` reaches the connection it accepted — the one
    // this test opened, identified by the client's ephemeral port.
    let (listening, port) = listener();
    let client = TcpStream::connect(("127.0.0.1", port)).expect("connect to the test's listener");
    let (_accepted, peer) = listening
        .accept()
        .expect("accept the test's own connection");
    let client_port = peer.port();

    let run = ono(&format!(
        "enter process {}; follow socket :{port}; follow connection; look --json",
        std::process::id()
    ));
    let view = place_view(&run);
    assert_place_is(&view, "connection", &client_port.to_string(), "spec §44.5");
    let shown = rendered(&place(&view));
    assert!(
        shown.contains(&port.to_string()),
        "spec §14.4: the connection place names its local endpoint (:{port}), got {shown}"
    );
    drop(client);
}

#[test]
fn should_refuse_the_traversal_with_no_relation_when_the_process_owns_no_socket() {
    // §6.4: `follow` traverses a real relationship edge. Where no such edge exists the answer is
    // the structured `spatial.no_relation` of §40, never an empty place and never a silent
    // success — a `sleep` child owns no socket at all.
    let child = SleepChild::spawn();
    let run = ono(&format!("enter process {}; follow socket", child.pid()));
    assert_spatial_error(&run, "follow socket", "spatial.no_relation");
}

#[test]
fn should_refuse_to_follow_a_canonical_child_that_is_not_a_relationship_edge() {
    // Invariant 6 and §53: hierarchy and graph are separate concepts — `enter` navigates the
    // canonical hierarchy, `follow` traverses relationship edges (§6.4 "MUST traverse a
    // relationship edge, not a canonical hierarchy edge"). `COMPUTE` is a canonical child of the
    // root (§7.2), so it is enterable and not followable.
    let refused = ono("home; follow compute");
    assert_spatial_error(&refused, "follow compute", "spatial.no_relation");

    let entered = ono("home; enter compute; look --json");
    let view = place_view(&entered);
    let kind = place_type(&view);
    assert!(
        kind.contains("compute"),
        "spec §7.2/§11.1: the same name is a canonical child that `enter` reaches, got {kind:?}"
    );
}

#[test]
fn should_return_to_the_process_with_back_after_following_a_socket_edge() {
    // §44.6 and invariant 4: `back` follows the actual navigation trail, so after a real
    // relationship hop it returns through that hop to the process that owns the socket.
    let (_listener, port) = listener();
    let pid = std::process::id();
    let run = ono(&format!(
        "enter process {pid}; follow socket :{port}; back; look --json"
    ));
    let view = place_view(&run);
    assert_place_is(&view, "process", &pid.to_string(), "spec §44.6");
}

#[test]
fn should_leave_the_relationship_chain_with_up_after_following_a_socket_edge() {
    // §44.6 and §6.6: `up` follows the canonical hierarchy, so from the socket it reaches the
    // socket's network parent (§14.1 `NETWORK -> LISTENERS`) — never the process the
    // relationship edge came from. §43.2 states the property: `up` never traverses an arbitrary
    // graph edge.
    let (_listener, port) = listener();
    let pid = std::process::id();
    let run = ono(&format!(
        "enter process {pid}; follow socket :{port}; up; look --json"
    ));
    let view = place_view(&run);
    let shown = rendered(&place(&view)).to_lowercase();
    assert!(
        shown.contains("network") || shown.contains("listener"),
        "spec §44.6/§14.1: `up` from a socket reaches its canonical network hierarchy parent, got \
         {shown}"
    );
    assert!(
        !place_type(&view).contains("process"),
        "spec §44.6: `up` is not `back` — it must not walk the relationship edge back to pid \
         {pid}, got {shown}"
    );
}

#[test]
fn should_keep_the_current_place_when_trace_projects_the_relationship_graph() {
    // §31.1: `trace` returns a graph projection and MUST NOT automatically change the current
    // place. It is a relationship query; `follow` is the movement. The trace runs inside the
    // process context, so it takes its selector from the place (v0.2 §14.3) and prints the graph
    // of the very object the place stands for — and the place is still that object afterwards.
    let holder = FileHolder::spawn();
    let pid = holder.pid();
    let run = ono(&format!("enter process {pid}; trace process; look --json"));
    assert_spatial_command(&run, "look --json");
    let last = run
        .stdout()
        .lines()
        .rfind(|line| line.trim_start().starts_with('{') || line.trim_start().starts_with('['))
        .unwrap_or_default()
        .to_owned();
    let view: Value = serde_yaml_ng::from_str(&last).unwrap_or_else(|error| {
        panic!(
            "spec §29.1: `look --json` prints the place as JSON, got stdout {:?} ({error})",
            run.stdout()
        )
    });
    assert_place_is(&view, "process", &pid.to_string(), "spec §31.1");
}

#[test]
fn should_bound_the_neighborhood_by_default_and_widen_it_with_all() {
    // §6.2 and invariant 9: `near` ranks and bounds by default, `--all` asks for the complete
    // one-hop neighborhood, and `--limit` is exact. §32 forbids the default view from paying for
    // expensive relations, so the default may never be larger than `--all`. pid 1 is the one
    // process every Linux host has, with more than two edges.
    let limited = ono("enter process 1; near --limit 2 | count | to json");
    let counted = rows(&limited, "near --limit 2 | count");
    assert_eq!(
        counted.first().and_then(Value::as_u64),
        Some(2),
        "spec §6.2: `near --limit 2` yields exactly two neighbors, got {:?}",
        limited.stdout()
    );

    let default = ono("enter process 1; near | count | to json");
    let all = ono("enter process 1; near --all | count | to json");
    let default_count = rows(&default, "near | count")
        .first()
        .and_then(Value::as_u64)
        .expect("`count` yields a number");
    let all_count = rows(&all, "near --all | count")
        .first()
        .and_then(Value::as_u64)
        .expect("`count` yields a number");
    assert!(
        default_count <= all_count,
        "spec §6.2/§32: the default neighborhood is bounded and never larger than `--all`, got \
         {default_count} against {all_count}"
    );
}

/// One exit of a `PlaceView`, by the word `look` prints for it (§24.2).
fn exit_group(view: &Value, label: &str) -> Value {
    for container in ["exits", "groups"] {
        let Some(found) = view.get(container) else {
            continue;
        };
        if let Some(group) = found.get(label) {
            return group.clone();
        }
        if let Some(groups) = found.as_sequence() {
            for group in groups {
                if group.get("label").and_then(Value::as_str) == Some(label) {
                    return group.clone();
                }
            }
        }
    }
    panic!(
        "spec §24.2: the place view carries a `{label}` exit, got {}",
        rendered(view)
    )
}

/// The neighbour display names the last `| to json` on stdout answered with, sorted so two
/// answers compare. The last document is what is read because a script may print a place view
/// before it, and that view is for a human rather than for this assertion.
fn neighbour_names(run: &ono_testkit::Run) -> Vec<String> {
    let stdout = run.stdout();
    let document = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('['))
        .unwrap_or_else(|| {
            panic!("spec §29.4: `near | to json` prints a document, got {stdout:?}")
        });
    let parsed: Value = serde_yaml_ng::from_str(document)
        .unwrap_or_else(|error| panic!("`near | to json` prints JSON: {error} in {document:?}"));
    let mut names: Vec<String> = parsed
        .as_sequence()
        .unwrap_or_else(|| panic!("spec §29.4: `near` is an ordinary stream, got {document:?}"))
        .iter()
        .filter_map(|row| {
            row.get("display_name")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

#[test]
fn should_answer_the_same_neighbours_whether_or_not_a_look_came_first() {
    // §32.2's "available on request" is a statement about what this session has *paid for*, not
    // about what is there. `look` declines the expensive owner scan and records that decline;
    // the `near --type process` after it asks for exactly that scan, and must get the answer.
    //
    // What this fixes: the decline was recorded on the place and never lifted, so the answer the
    // scan produced was hidden behind it. A reader who looked before they walked was told the
    // place was empty — with status 0, and with nothing on stderr. §35.2 and §42.4 forbid a
    // false empty; this was the same fault reached through a stale statement rather than a
    // refusal.
    let (listening, port) = listener();

    let direct = ono(&format!(
        "enter 127.0.0.1:{port}; near --type process | to json"
    ));
    let walked = neighbour_names(&direct);
    assert!(
        !walked.is_empty(),
        "this test needs the socket it holds to have an owner to find, got {:?}",
        direct.stdout()
    );

    let after_look = ono(&format!(
        "enter 127.0.0.1:{port}; look; near --type process | to json"
    ));
    let looked = neighbour_names(&after_look);
    assert_eq!(
        looked, walked,
        "spec v0.4 §6.2/§32.2: `near --type process` answers the same neighbours whether or not \
         a `look` came first, got {:?} after a look and {:?} without one",
        looked, walked
    );

    drop(listening);
}

#[test]
fn should_not_report_the_owner_of_a_socket_nobody_looked_up_as_no_owner() {
    // §35.2, §2.17 and §32.2. Joining a socket to the process holding it means reading every
    // `/proc/<pid>/fd` on the host — `expensive` in §32.1's cost classes — so a default `look`
    // does not spend it, and §32.1 forbids it from doing so. What it may not do is print the
    // exit as a count: `empty` is "they were read and there are none", and this listener has an
    // owner, namely the process running this test. §32.2 gives the honest spelling for an exit
    // nobody has paid for yet — "available on request" — and §35.2 keeps it distinct from `0`.
    //
    // The second half is the same fact from the other side: asked for explicitly, the exit
    // answers with the owner, or says it may not be seen. Never with nothing.
    let (listening, port) = listener();
    let run = ono(&format!("enter 127.0.0.1:{port}; look --json"));
    let view = place_view(&run);
    let group = exit_group(&view, "process");
    let state = group
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    assert!(
        !matches!(state.as_str(), "empty" | "available"),
        "spec §35.2/§2.17: the owner of this socket was never looked up, so the exit may not be \
         reported as read — got state {state:?} in {}",
        rendered(&group)
    );

    let asked = ono(&format!("enter 127.0.0.1:{port}; look --all --json"));
    let view = place_view(&asked);
    let group = exit_group(&view, "process");
    let state = group
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let count = group
        .get("count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    assert!(
        (state == "available" && count >= 1) || state == "permission_denied",
        "spec §35.2/§6.2: asked for, the owner of a socket this test holds is either named or \
         refused — got state {state:?} count {count} in {}",
        rendered(&group)
    );

    // §32.1's own exception, and the half the container caught first: "unless cached or already
    // available". A socket reached *through* the process that holds it arrives with that edge
    // already observed, so its owner is named rather than offered — the decline is about the
    // scan nobody paid for, never about an answer the session already has.
    let cached = ono(&format!(
        "enter process {}; follow socket :{port}; look --json",
        std::process::id()
    ));
    let view = place_view(&cached);
    let group = exit_group(&view, "process");
    assert_eq!(
        group.get("state").and_then(Value::as_str),
        Some("available"),
        "spec §32.1: an exit the session already holds an edge for is answered, got {}",
        rendered(&group)
    );
    assert!(
        group
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            >= 1,
        "spec §32.1: the owner already observed is counted, got {}",
        rendered(&group)
    );
    drop(listening);
}

#[test]
fn should_report_the_unreadable_namespace_group_as_unknown_rather_than_absent() {
    // §16.2, §35.2 and invariant 17: unprivileged, `/proc/1/ns` and `/proc/1/fd` are refused by
    // the kernel. The namespace and files groups of pid 1 must therefore carry a permission
    // state that is distinct from empty — `permission denied for N descriptors` and not `0`
    // (§44.8). The container/namespace boundary of §16 is exactly what an unprivileged run
    // cannot read, so this is the honest degradation the spec demands, not a weaker assertion.
    if !unprivileged() {
        return;
    }
    let run = ono("enter process 1; look --all --json");
    let view = place_view(&run);
    let shown = rendered(&view).to_lowercase();
    assert!(
        shown.contains("namespace"),
        "spec §12: the process place carries a namespaces group, got {shown}"
    );
    let honest = [
        "permission_denied",
        "permission denied",
        "unknown",
        "denied",
    ];
    assert!(
        honest.iter().any(|state| shown.contains(state)),
        "spec §35.2/§44.8: a group the kernel refuses is `unknown`/`permission_denied`, never \
         rendered as absent or zero, got {shown}"
    );
    assert!(
        !shown.contains("namespaces: 0") && !shown.contains("namespaces: []"),
        "spec §35.2: an unreadable group is not reported as empty, got {shown}"
    );
}

#[test]
fn should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes() {
    // §44.9, §25.1 and §43.6: a real edge appears while a live map watches, and disappears or
    // tombstones when the connection closes. Nothing here can pass on animation — the edge is
    // identified by the ephemeral client port of a connection this test opens after the live map
    // has already produced its first value, and closes again while it is still watching. The
    // stream is bounded with `take 3` (§29.4) so the run ends on its own: the snapshot, the
    // appearance and the disappearance.
    let (listening, port) = listener();
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        let client = TcpStream::connect(("127.0.0.1", port)).expect("connect to the listener");
        let (accepted, peer) = listening
            .accept()
            .expect("accept the test's own connection");
        sender.send(peer.port()).expect("the test is still waiting");
        std::thread::sleep(Duration::from_millis(2000));
        drop(accepted);
        drop(client);
        std::thread::sleep(Duration::from_millis(2000));
        drop(listening);
    });

    let script = format!("enter socket {port}; map --live --json | take 3 | to json");
    let run = Shell::new()
        .args(["-c", &script])
        .timeout(Duration::from_secs(40))
        .try_run()
        .unwrap_or_else(|error| {
            panic!(
                "spec §29.4/§44.9: `map --live --json | take 3` ends after three values — the \
                 snapshot, the connection appearing and the connection closing: {error}"
            )
        });
    let client_port = receiver
        .recv_timeout(Duration::from_secs(20))
        .expect("the fixture opened its connection");
    worker.join().expect("the fixture thread finished");

    let maps = rows(&run, "map --live --json | take 3");
    assert_eq!(
        maps.len(),
        3,
        "spec §18.2/§25.1: the live map begins with a snapshot and then reports each real change, \
         got {:?}",
        run.stdout()
    );
    let mentions_connection: Vec<bool> = maps
        .iter()
        .map(|map| rendered(map).contains(&client_port.to_string()))
        .collect();
    assert!(
        !mentions_connection[0],
        "spec §43.6: the first live value is the state before the test connected, so it cannot \
         already carry the connection from :{client_port}; got {}",
        rendered(&maps[0])
    );
    assert!(
        mentions_connection[1],
        "spec §44.9: the edge for the connection from :{client_port} appears because the test \
         opened it, got {}",
        rendered(&maps[1])
    );
    let last = rendered(&maps[2]);
    let gone = !last.contains(&client_port.to_string());
    let tombstoned = ["removed", "tombstone", "gone", "closed"]
        .iter()
        .any(|marker| last.to_lowercase().contains(marker));
    assert!(
        gone || tombstoned,
        "spec §44.9/§10.3: when the connection closes its edge disappears or is tombstoned, got \
         {last}"
    );
    let freshness = ["event", "poll", "cache", "stale", "partial"];
    assert!(
        freshness
            .iter()
            .any(|word| rendered(&maps[1]).to_lowercase().contains(word)),
        "spec §25.3: a live view exposes whether its updates are event-driven, polled, cached, \
         stale or partial, got {}",
        rendered(&maps[1])
    );
}

#[test]
fn should_record_the_relation_it_traversed_when_a_follow_enters_the_trail() {
    // §6.4: "The relation traversed MUST be recorded in the navigation trail", and §20.1 fixes
    // the step's fields — from, to, movement, relation. §29.1 makes `trail --json` work without
    // a TTY. This is the edge half of the trail: the movement is `follow` and the step names the
    // relationship it crossed, which is what tells `back` from `up` apart later.
    let holder = FileHolder::spawn();
    let run = ono(&format!(
        "enter process {}; follow file {}; trail --json",
        holder.pid(),
        holder.path()
    ));
    let document = document(&run, "trail --json");
    let steps = document
        .as_sequence()
        .cloned()
        .or_else(|| document.get("steps").and_then(Value::as_sequence).cloned())
        .unwrap_or_else(|| {
            panic!(
                "spec §6.7/§20.1: `trail --json` returns the navigation steps, got {:?}",
                run.stdout()
            )
        });
    let followed = steps
        .iter()
        .find(|step| rendered(step).contains("follow"))
        .unwrap_or_else(|| {
            panic!(
                "spec §6.4: the trail records the `follow` movement, got {:?}",
                run.stdout()
            )
        });
    let shown = rendered(followed);
    assert!(
        followed.get("relation").is_some(),
        "spec §6.4/§20.1: a `follow` step records the relation it traversed, got {shown}"
    );
    assert!(
        shown.contains(&holder.path()) && shown.contains(&holder.pid().to_string()),
        "spec §20.1: the step records where the movement came from and where it went, got {shown}"
    );
}

#[test]
fn should_carry_the_raw_evidence_of_an_edge_when_a_neighbour_or_a_map_edge_is_read() {
    // §11.4's list ends with "raw evidence/reference where safe", and the v0.2 relationship graph
    // already reports it: `linux.open-files` names the descriptor number the fact was read from.
    // The spatial projection dropped it, so the edge said who observed it and never what they
    // saw. ADR-0164 makes the edge itself the answer to `inspect relation`, so the evidence
    // belongs on the edge rather than behind a second command.
    let holder = FileHolder::spawn();

    let neighbours = ono(&format!(
        "enter process {}; near --type file | to json",
        holder.pid()
    ));
    let rows = rows(&neighbours, "near --type file | to json");
    let held = rows
        .iter()
        .find(|row| rendered(row).contains(&holder.path()))
        .unwrap_or_else(|| {
            panic!(
                "spec §12: the process holds the file, so it is among its neighbours, got {:?}",
                neighbours.stdout()
            )
        });
    let evidence = held.get("evidence").unwrap_or_else(|| {
        panic!("spec §11.4: an edge carries the raw evidence it was read from, got {held:?}")
    });
    assert!(
        rendered(evidence).contains("fd"),
        "spec §11.4/§37.2: the evidence is what the provider saw — the descriptor of the open \
         file — not a second copy of its name, got {evidence:?}"
    );

    let mapped = ono(&format!("enter process {}; map --json", holder.pid()));
    let file_edge = map_edges(&mapped)
        .into_iter()
        .find(|edge| rendered(edge).contains(&holder.path()));
    let file_edge = file_edge.unwrap_or_else(|| {
        panic!(
            "spec §22: the map of the process carries the edge to the file it holds, got {:?}",
            mapped.stdout()
        )
    });
    assert!(
        file_edge.get("evidence").is_some(),
        "spec §11.4/§22: a map edge answers `inspect relation`, so it carries the same evidence, \
         got {file_edge:?}"
    );
}

#[test]
fn should_say_a_costly_relation_is_unknown_rather_than_unserved_when_a_look_did_not_ask_for_it() {
    // §35.2 keeps `unknown` and `unsupported` apart, and §32.1 is why a default `look` leaves the
    // openers of a file alone: finding every process that holds one file reads every process on
    // the host. "Not asked because it is expensive" is `unknown` — the group the `owner` of a
    // file already answers with, "available on request". `unsupported` says no provider answers,
    // and one does: `near --type process` reaches the same relation and names the holder.
    let holder = FileHolder::spawn();

    let looked = ono(&format!("enter {}; look --json", holder.path()));
    looked.assert_success();
    let view = place_view(&looked);
    let openers = rendered(&view);
    assert!(
        !openers.contains("no provider answers for the `openers`"),
        "§35.2/§2.17: a relation this build serves is not reported as one nobody answers for, \
         got {openers}"
    );

    let asked = ono(&format!(
        "enter {}; near --type process | select display_name | to json",
        holder.path()
    ));
    asked.assert_success();
    assert!(
        asked.stdout().contains("sleep"),
        "§6.2: asked for explicitly, the same relation names the process holding the file, got \
         {:?}",
        asked.stdout()
    );
}

#[test]
fn should_offer_the_listeners_of_a_service_as_an_exit_even_where_no_provider_joins_them() {
    // §13's minimum groups for a service place list `listeners`. The socket belongs to the
    // process and no installed provider joins the unit to it, so the exit is `unsupported` — but
    // it has to be there: a place that leaves the group off the view is quietly claiming the
    // service has no listeners, which is the count-from-nowhere §2.17 and §35.2 forbid. The
    // `cgroup` exit of the same place has answered this way since S5.
    let dir = scratch();
    dir.write(
        "systemctl",
        "#!/bin/sh\n\
         if [ \"$1\" = --version ]; then echo 'systemd 259 (259.5)'; exit 0; fi\n\
         if [ \"$1\" = list-units ]; then\n\
           printf '%s\\n' '[{\"unit\":\"ono-listener-fixture.service\",\"load\":\"loaded\",\"active\":\"active\",\"sub\":\"running\",\"description\":\"Fixture\"}]'\n\
           exit 0\n\
         fi\n\
         exit 2\n",
    );
    std::fs::set_permissions(
        dir.path().join("systemctl"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("the shim is executable");
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run = Shell::new()
        .args([
            "-c",
            "systemctl list-units | count | to text; \
             find place ono-listener-fixture | take 1 | enter; look --json",
        ])
        .env("PATH", path)
        .timeout(Duration::from_secs(30))
        .run();
    run.assert_success();
    let view = place_view(&run);
    let shown = rendered(&view);
    assert!(
        shown.contains("listeners"),
        "§13: a service place offers its listeners as an exit; got {shown}"
    );
}

// --- v0.4.1 §34.3: a relation described as requestable has a request path (issue #25) ----------

#[test]
fn should_follow_the_owner_relation_when_it_is_requested_explicitly() {
    // Issue #25: `enter 127.0.0.1:<port>; follow owner` answered
    //
    //   Ono-Sendai-E1009 spatial.unsupported the `owner` of this place is not answered here:
    //   available on request
    //
    // with and without a preceding `look`, while `near --type process` at the same place named
    // the owner outright. v0.4.1 §34.3: "If a relationship is described as 'available on request',
    // there MUST actually be a request path", and naming the relation is that path.
    //
    // The two words are one edge — `owner` is the inverse label of `process.owns_socket` and
    // `process` is its inverse group — so the two spellings must reach the same place. The
    // listener belongs to this test process, so the shell can attribute it without privilege.
    let (_listener, port) = listener();
    let mine = std::process::id().to_string();

    let by_label = ono(&format!(
        "enter 127.0.0.1:{port}; follow owner; look --json"
    ));
    let view = place_view(&by_label);
    assert_place_is(&view, "process", &mine, "v0.4.1 §34.3");

    let by_group = ono(&format!(
        "enter 127.0.0.1:{port}; follow process; look --json"
    ));
    assert_eq!(
        identity_of(&place_view(&by_group)),
        identity_of(&view),
        "§6.4 lets `follow` take either word for a relation, and both name the same edge, so \
         they must arrive at the same place"
    );
}

/// The spatial identity of the place a view describes, which is what "the same place" means.
fn identity_of(view: &Value) -> String {
    let place = place(view);
    place["spatial_id"]
        .as_str()
        .unwrap_or_else(|| panic!("§42: a place carries its spatial identity, got {place:?}"))
        .to_owned()
}

#[test]
fn should_follow_the_owner_relation_whether_or_not_a_look_came_first() {
    // Issue #25 is explicit that the refusal happened "with and without a preceding `look`", so
    // this is not the ADR-0421 defect seen from another side. Both orders are asserted.
    let (_listener, port) = listener();
    let mine = std::process::id().to_string();

    let after_look = ono(&format!("enter 127.0.0.1:{port}; look; follow owner"));
    let seen = format!("{}{}", after_look.stdout(), after_look.stderr());
    assert!(
        after_look.status().is_success(),
        "v0.4.1 §34.3: `follow owner` after a `look` traverses the relation rather than refusing; \
         got exit {:?} and {seen:?}",
        after_look.status()
    );
    // `look` itself is allowed to print "available on request" — §32.2 makes an exit nobody has
    // asked about a discoverable, unloaded one, and §34.3 requires only that something can make
    // the request. What §34.3 forbids is the *refusal*, and that is what must be gone.
    assert!(
        !seen.contains("Ono-Sendai-E1009"),
        "§34.3: `follow owner` must traverse the relation or refuse for a reason that is not \
         \"ask again\". Got {seen:?}"
    );
    let _ = &mine;
}

#[test]
fn should_pay_for_an_expensive_relation_when_follow_is_asked_to_resolve_it() {
    // §34.3's canonical shape: "follow owner --resolve". A relation classified `expensive` or
    // `external` (§34.2) is left discoverable and unloaded by an orientation query, and the flag
    // is what says the cost is acceptable. `openers` on a file is such a relation — finding every
    // process that holds one file is every process on the host (ADR-0149).
    let home = scratch();
    let held = home.path().join("held.txt");
    std::fs::write(&held, b"fixture").expect("the fixture file is writable");
    let file = std::fs::File::open(&held).expect("the fixture file opens");
    let mine = std::process::id().to_string();

    let run = ono(&format!(
        "enter {}; follow openers --resolve; look --json",
        held.display()
    ));
    assert_place_is(&place_view(&run), "process", &mine, "v0.4.1 §34.3");
    drop(file);
}
