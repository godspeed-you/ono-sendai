//! Outcome tests for the relationship half of the v0.4 spatial systems interface: the edges
//! `follow`, `near` and `map` traverse, the distinction between a canonical hierarchy edge and a
//! relationship edge, and the live spatial state that makes an edge appear and vanish.
//!
//! Specification: `docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` — §2
//! (invariants 3, 5, 6, 12, 16, 17), §3.5 (RelationshipEdge), §6.2/§6.4/§6.6 (`near`, `follow`,
//! `back`/`up`), §11 (hierarchy versus graph, explainability, confidence), §12/§13/§14 (the
//! process, service and network spaces and the edges they promise), §16.2 (namespace boundary),
//! §22 (the `SpatialMap` contract), §25 (live spatial state), §29 (scripting semantics), §31
//! (`trace` versus spatial navigation), §32 (relationship provider requirements), §35.2
//! (permission states stay distinct from empty), §40 (structured spatial errors), §43.6 (a live
//! test must be caused by a real change), §44.4/§44.5/§44.6/§44.9 (acceptance scenarios).
//!
//! None of these commands exist in this build: `follow`, `near`, `map`, `home`, `back`, `up` and
//! `trail` are not commands at all, and `look`/`find` are answered by the external programs of
//! the same name. Every test therefore begins by refusing that answer — a spatial assertion may
//! never be satisfied by `/usr/bin/look`, and a missing command must fail as a missing command.
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

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use ono_testkit::{Scratch, Shell, scratch};
use serde_yaml_ng::Value;

/// Runs a one-liner with a generous budget: nothing here may hang, and a run that does is a
/// failure of the shell, not of the test.
fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

/// Whether the run is unprivileged. The permission-honesty test asserts what a normal user sees
/// when the kernel refuses; as root the kernel would answer and there would be nothing to assert.
fn unprivileged() -> bool {
    if ono_process::effective_uid() == 0 {
        eprintln!("skipped: this test asserts what an unprivileged user is refused");
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

/// A `sleep` child: a process with a parent, a cgroup and no socket of its own.
struct SleepChild(Child);

impl SleepChild {
    fn spawn() -> Self {
        let child = Command::new("sleep")
            .arg("30")
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

/// A listening TCP socket the test process owns, so the shell can attribute it to a pid the test
/// knows without any privilege.
fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback listener");
    let port = listener.local_addr().expect("the bound address").port();
    (listener, port)
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §44.4); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §44.4); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §11.4); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §31.3); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §44.5); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §44.5); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §40); un-ignored by the increment that delivers it"]
fn should_refuse_the_traversal_with_no_relation_when_the_process_owns_no_socket() {
    // §6.4: `follow` traverses a real relationship edge. Where no such edge exists the answer is
    // the structured `spatial.no_relation` of §40, never an empty place and never a silent
    // success — a `sleep` child owns no socket at all.
    let child = SleepChild::spawn();
    let run = ono(&format!("enter process {}; follow socket", child.pid()));
    assert_spatial_error(&run, "follow socket", "spatial.no_relation");
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §11.1); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §44.6); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §44.6); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §6.2); un-ignored by the increment that delivers it"]
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

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §35.2); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §44.9); un-ignored by the increment that delivers it"]
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
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §20.1); un-ignored by the increment that delivers it"]
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
