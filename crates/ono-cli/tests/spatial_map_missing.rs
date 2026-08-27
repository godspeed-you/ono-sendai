//! Outcome tests for the v0.4 map: the `SpatialMap` data contract, its text rendering, semantic
//! zoom, clustering, landmarks and the `look` rendering rules — none of which this build has.
//!
//! Narrative: `docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` §22 (map data
//! contract), §23 (map rendering), §24 (`look` rendering rules), §8 (semantic zoom, clustering,
//! expansion), §26 and §3.7 (landmark engine and landmark reasons), §34.2 (view budgets),
//! §39 (accessibility and terminal capability), §29.1/§29.4 (`map --json` is a bounded graph
//! value that works without a TTY), §43.2 (the map properties), §43.5 (snapshots are
//! presentation, never a semantic contract), §52.1 (map text rendering works without a
//! full-screen TUI) and §53 (the map default is bounded, relevance-ranked, semantically
//! clustered — never the entire graph).
//!
//! Everything here is a RED suite for a subsystem that does not exist yet: today `map` is not a
//! command at all (`Ono-Sendai-E0101 command not found: map`, exit 127) and `look` resolves to
//! `/usr/bin/look`, the Unix word-list tool. Each test therefore carries `#[ignore]` naming the
//! section that governs it, and the increment that delivers that section removes the ignore.
//!
//! Readings this suite fixes where the specification leaves a choice (each is repeated at the
//! test that depends on it, so the implementing increment can turn it into an ADR):
//!
//! * §22 calls the `SpatialMap` shape a "recommended contract". This suite treats its field
//!   names as *the* JSON keys of `map --json`; a renderer-independent contract that every
//!   consumer must guess is not a contract.
//! * `map --json` writes one JSON document to stdout (§6.9: "returns `SpatialMap` and MUST not
//!   depend on terminal rendering"; §29.1: it must work non-interactively). The helper below
//!   accepts a leading rendering from preceding navigation statements, but the map itself is
//!   JSON, not a table.
//! * §8.1 declares the L0–L4 vocabulary "normative for renderer behavior and tests" but names no
//!   flag; this suite selects a level with `map --zoom <n>` and expects `zoom_level` to echo it.
//! * §8.3 requires clusters to be expandable as a *view* action; the non-interactive spelling
//!   used here is `map --expand <cluster-id>`.
//! * §23.4/§53 require focus never to move the shell; the non-interactive spelling used here is
//!   `map --focus <node-id>`.
//! * §39.2 requires an ASCII fallback but names no switch; this suite takes a terminal that
//!   cannot promise Unicode (`TERM=dumb`, `LC_ALL=C`, `LANG=C`) together with `NO_COLOR=1` as
//!   the condition, per §39.1/§39.2.
//!
//! Every fixture is built by the test — sleeping children it spawns, a listener it binds on
//! `0.0.0.0:0` — so the assertions never depend on this machine's real services. Everything runs
//! offline, unprivileged, and asserts what a user sees: the JSON document, the rendered text, the
//! exit status (AGENTS.md §11).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::process::{Child, Stdio};
use std::time::Duration;

use ono_testkit::Shell;
use serde_yaml_ng::Value;

/// The six canonical domains of §7 and §53, as they appear at zoom level L1 (§8.1).
const CANONICAL_DOMAINS: [&str; 6] = [
    "compute",
    "network",
    "storage",
    "containers",
    "identity",
    "devices",
];

/// The built-in landmark reasons every implementation must expose (§3.7).
const LANDMARK_REASONS: [&str; 14] = [
    "high_cpu",
    "high_memory",
    "failed",
    "restarting",
    "recently_changed",
    "public_listener",
    "privileged",
    "storage_pressure",
    "connection_spike",
    "new_object",
    "removed_object",
    "security_boundary",
    "remote_boundary",
    "user_pinned",
];

/// The default visible-node budget of a text map (§34.2).
const TEXT_MAP_NODE_BUDGET: usize = 30;

/// Navigation prefix that puts a script at the canonical processes collection (§7.2, §43.3).
const AT_PROCESSES: &str = "enter compute\nenter processes\n";

/// Navigation prefix that puts a script at the canonical listeners collection (§7.3, §14.1).
const AT_LISTENERS: &str = "enter network\nenter listeners\n";

/// Runs a one-liner without a terminal, with colour off so the output is the semantic content.
fn ono(script: &str) -> ono_testkit::Run {
    ono_in(script, &[])
}

/// Runs a one-liner with additional environment, still without a terminal.
fn ono_in(script: &str, env: &[(&str, &str)]) -> ono_testkit::Run {
    let mut shell = Shell::new()
        .args(["-c", script])
        .env("NO_COLOR", "1")
        .timeout(Duration::from_secs(60));
    for (name, value) in env {
        shell = shell.env(*name, *value);
    }
    shell.run()
}

/// The JSON document a `--json` command wrote, tolerating a rendering that preceding navigation
/// statements printed before it.
///
/// §22/§6.1: `map --json` and `look --json` return one structured value, not a table.
fn document(run: &ono_testkit::Run, script: &str) -> Value {
    run.assert_success();
    let stdout = run.stdout();
    let start = stdout.find('{').unwrap_or_else(|| {
        panic!(
            "spec §29.1: `{script}` must write a JSON document without a TTY, got stdout {stdout:?}, stderr {:?}",
            run.stderr()
        )
    });
    let end = stdout
        .rfind('}')
        .expect("a closing brace follows an opening one");
    let text = &stdout[start..=end];
    let value: Value = serde_yaml_ng::from_str(text).unwrap_or_else(|error| {
        panic!("spec §22: `{script}` must write a JSON object, got {text:?} ({error})")
    });
    assert!(
        value.as_mapping().is_some(),
        "spec §22: `{script}` returns one `SpatialMap` object, got {text:?}"
    );
    value
}

/// The `SpatialMap` of `map <args>`, evaluated after `place` has been navigated to.
fn map_at(place: &str, args: &str) -> Value {
    let script = format!("{place}map --json {args}");
    let run = ono(&script);
    document(&run, &script)
}

/// The `SpatialMap` of the default place (§5, §7.1: `home` is `SYSTEM`).
fn map(args: &str) -> Value {
    map_at("", args)
}

/// The list under `field`, which the `SpatialMap` contract declares as a list (§22).
fn list<'a>(document: &'a Value, field: &str) -> &'a Vec<Value> {
    document[field].as_sequence().unwrap_or_else(|| {
        panic!("spec §22: `{field}` is a list in `SpatialMap`, got {document:?}")
    })
}

fn nodes(document: &Value) -> &Vec<Value> {
    list(document, "nodes")
}

fn edges(document: &Value) -> &Vec<Value> {
    list(document, "edges")
}

fn clusters(document: &Value) -> &Vec<Value> {
    list(document, "clusters")
}

fn landmarks(document: &Value) -> &Vec<Value> {
    list(document, "landmarks")
}

/// A string field of an object, or a panic naming the contract that requires it.
fn text(value: &Value, field: &str, section: &str) -> String {
    value[field]
        .as_str()
        .unwrap_or_else(|| panic!("spec {section}: `{field}` is a string, got {value:?}"))
        .to_owned()
}

/// The `id` of every element of a list of nodes, edges or clusters.
fn ids(values: &[Value]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| text(value, "id", "§22"))
        .collect()
}

/// Every key that appears anywhere in a document, at any depth.
fn keys(value: &Value, found: &mut BTreeSet<String>) {
    match value {
        Value::Mapping(mapping) => {
            for (key, child) in mapping {
                if let Some(name) = key.as_str() {
                    found.insert(name.to_owned());
                }
                keys(child, found);
            }
        }
        Value::Sequence(items) => {
            for item in items {
                keys(item, found);
            }
        }
        _ => {}
    }
}

/// The largest integer anywhere inside a value — how a `HiddenSummary` reports its count (§22,
/// §8.2: "A cluster MUST report the number of hidden objects"), whose field layout §22 leaves open.
fn max_integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::Mapping(mapping) => mapping.values().filter_map(max_integer).max(),
        Value::Sequence(items) => items.iter().filter_map(max_integer).max(),
        _ => None,
    }
}

/// Whether the JSON text of a value mentions `needle` — used to find the node standing for a
/// fixture this test created, whichever field of `MapNode` carries the object reference (§22).
fn mentions(value: &Value, needle: &str) -> bool {
    serde_yaml_ng::to_string(value)
        .expect("a value that was parsed can be printed")
        .contains(needle)
}

/// §43.2: "all rendered edges reference existing rendered nodes or explicit off-map endpoints".
///
/// An endpoint is resolved when it is the id of a node in the same document, the id of a cluster
/// that stands for it (§8.2), or when the edge itself marks that endpoint as off-map — the
/// "explicit" of the property. Nothing else may appear as an edge endpoint.
fn assert_edges_resolve(document: &Value, what: &str) {
    let known: BTreeSet<String> = ids(nodes(document))
        .union(&ids(clusters(document)))
        .cloned()
        .collect();
    for edge in edges(document) {
        let source = text(edge, "source", "§22");
        let target = text(edge, "target", "§22");
        for endpoint in [&source, &target] {
            let off_map = serde_yaml_ng::to_string(edge)
                .expect("a value that was parsed can be printed")
                .contains("off_map");
            assert!(
                known.contains(endpoint.as_str()) || off_map,
                "spec §43.2: every rendered edge references a rendered node or an explicit \
                 off-map endpoint; `{endpoint}` is neither, in {what} edge {edge:?}"
            );
        }
    }
}

/// Sleeping children the test owns, killed when the guard is dropped.
struct Children(Vec<Child>);

impl Children {
    /// Spawns `count` children that outlive the shell runs of one test.
    fn spawn(count: usize) -> Self {
        let children = (0..count)
            .map(|_| {
                std::process::Command::new("sleep")
                    .arg("120")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("`sleep` is available to spawn a process fixture")
            })
            .collect();
        Self(children)
    }

    /// The process id of the first child — the object the map must be able to show.
    fn first_pid(&self) -> u32 {
        self.0.first().expect("at least one child").id()
    }
}

impl Drop for Children {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A listener bound on every interface, which §26.2 and §3.7 call a `public_listener` landmark.
/// Binding needs no network and no privilege on a port the kernel picks.
fn public_listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("0.0.0.0:0").expect("bind a public loopback-visible listener");
    let port = listener.local_addr().expect("the bound address").port();
    (listener, port)
}

/// How many processes the host has, from the shell's own process provider — the number the map's
/// bounded default is measured against (§53).
fn host_process_count() -> u64 {
    let run = ono("get process | count | to json");
    run.assert_success();
    let document: Value =
        serde_yaml_ng::from_str(run.stdout().trim()).expect("`count` emits a JSON document");
    max_integer(&document).expect("`count` emits a number") as u64
}

// ---------------------------------------------------------------------------------------------
// §22 — the map data contract
// ---------------------------------------------------------------------------------------------

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §22, §29.1); un-ignored by the increment that delivers it"]
fn should_return_a_spatial_map_document_when_map_json_runs_without_a_tty() {
    let document = map("");

    for field in [
        "map_id",
        "center",
        "scope",
        "zoom_level",
        "nodes",
        "edges",
        "clusters",
        "landmarks",
        "hidden",
        "generated_at",
        "completeness",
        "live_capable",
    ] {
        assert!(
            !document[field].is_null(),
            "spec §22: `SpatialMap` carries `{field}`, got {document:?}"
        );
    }
    assert!(
        !text(&document, "center", "§22").is_empty(),
        "spec §22: `center` is the `SpatialId` of the current place, got {document:?}"
    );
    assert!(
        document["zoom_level"].as_i64().is_some(),
        "spec §22: `zoom_level` is an integer, got {document:?}"
    );
    assert!(
        document["live_capable"].as_bool().is_some(),
        "spec §22: `live_capable` is a boolean, got {document:?}"
    );
    for field in ["nodes", "edges", "clusters", "landmarks"] {
        list(&document, field);
    }
    assert!(
        !nodes(&document).is_empty(),
        "spec §6.9: the default map shows the current place and its canonical children, got {document:?}"
    );
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §22); un-ignored by the increment that delivers it"]
fn should_describe_every_node_with_the_map_node_contract_when_map_json_returns_a_map() {
    let document = map("");
    let nodes = nodes(&document);

    for node in nodes {
        for field in ["id", "object_ref", "label", "type"] {
            assert!(
                !node[field].is_null(),
                "spec §22: `MapNode` carries `{field}`, got {node:?}"
            );
        }
        assert!(
            !text(node, "label", "§22").is_empty(),
            "spec §3.1: a node has a display name, got {node:?}"
        );
        assert!(
            node["landmark_reasons"].as_sequence().is_some(),
            "spec §22: `MapNode.landmark_reasons` is a list, empty when the node is no landmark, got {node:?}"
        );
    }
    assert_eq!(
        ids(nodes).len(),
        nodes.len(),
        "spec §3.1: a `SpatialId` identifies one object, so node ids are unique, got {nodes:?}"
    );
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §22, §11.5); un-ignored by the increment that delivers it"]
fn should_describe_every_edge_with_the_map_edge_contract_when_map_json_returns_a_map() {
    let document = map_at(AT_PROCESSES, "");

    let edges = edges(&document);
    assert!(
        !edges.is_empty(),
        "spec §6.9: a map of a collection shows the relationships around it, got {document:?}"
    );
    for edge in edges {
        for field in [
            "id",
            "source",
            "target",
            "relation",
            "confidence",
            "direction",
        ] {
            assert!(
                !edge[field].is_null(),
                "spec §22: `MapEdge` carries `{field}`, got {edge:?}"
            );
        }
        assert!(
            !text(edge, "relation", "§22").is_empty(),
            "spec §23.5: an edge exposes its relation, got {edge:?}"
        );
        let confidence = text(edge, "confidence", "§11.5");
        assert!(
            ["exact", "strong", "inferred", "user_declared", "unknown"]
                .contains(&confidence.as_str()),
            "spec §11.5: an edge carries a confidence from the v0.2-compatible model so an \
             inferred edge stays distinguishable from an exact one, got {confidence} in {edge:?}"
        );
    }
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §22, §43.2); un-ignored by the increment that delivers it"]
fn should_omit_screen_coordinates_when_map_json_returns_the_semantic_contract() {
    // §22: "Screen coordinates MUST NOT appear in the semantic `SpatialMap` contract. Layout
    // coordinates belong to the renderer." §43.2: "map coordinates never affect semantic identity".
    let document = map_at(AT_PROCESSES, "");

    let mut present = BTreeSet::new();
    keys(&document, &mut present);
    for forbidden in [
        "x",
        "y",
        "row",
        "col",
        "column",
        "position",
        "coordinate",
        "coordinates",
        "layout",
        "screen",
        "px",
        "width",
        "height",
    ] {
        assert!(
            !present.contains(forbidden),
            "spec §22: layout belongs to the renderer, so `{forbidden}` must not be a key of the \
             semantic map; keys were {present:?}"
        );
    }
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §43.2, §22); un-ignored by the increment that delivers it"]
fn should_resolve_every_edge_endpoint_to_a_rendered_node_when_map_json_returns_a_map() {
    let _children = Children::spawn(12);

    assert_edges_resolve(&map(""), "the default map");
    assert_edges_resolve(&map_at(AT_PROCESSES, ""), "the processes map");
    assert_edges_resolve(&map_at(AT_PROCESSES, "--all"), "the complete processes map");
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §6.9, §43.2); un-ignored by the increment that delivers it"]
fn should_return_the_same_node_identities_when_the_terminal_width_changes() {
    // §6.9: "`map --json` returns `SpatialMap` and MUST not depend on terminal rendering", and
    // §43.2: "map coordinates never affect semantic identity". A width is a rendering decision:
    // it may not change which objects the structured map calls out, nor what they are called.
    let script = "map --json";
    let narrow = ono_in(script, &[("COLUMNS", "40")]);
    let wide = ono_in(script, &[("COLUMNS", "200")]);

    let narrow = document(&narrow, script);
    let wide = document(&wide, script);
    assert_eq!(
        ids(nodes(&narrow)),
        ids(nodes(&wide)),
        "spec §6.9: the structured map is independent of terminal rendering, so 40 and 200 \
         columns name the same nodes"
    );
    assert_eq!(
        text(&narrow, "center", "§22"),
        text(&wide, "center", "§22"),
        "spec §43.2: the current place is identity, not layout"
    );
}

// ---------------------------------------------------------------------------------------------
// §53, §34.2 — the default map is bounded, never the whole graph
// ---------------------------------------------------------------------------------------------

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §53, §34.2); un-ignored by the increment that delivers it"]
fn should_bound_the_default_map_when_the_host_holds_more_objects_than_the_view_budget() {
    let children = Children::spawn(12);
    let processes = host_process_count();

    let document = map_at(AT_PROCESSES, "");
    let shown = nodes(&document).len();
    assert!(
        shown <= TEXT_MAP_NODE_BUDGET,
        "spec §34.2: the text map's default visible-node budget is about {TEXT_MAP_NODE_BUDGET} \
         nodes and unbounded graph rendering is prohibited, got {shown} nodes"
    );
    assert!(
        !document["hidden"].is_null(),
        "spec §23.6: a bounded map states what it left out rather than silently dropping it, \
         got hidden {:?}",
        document["hidden"]
    );
    if processes > TEXT_MAP_NODE_BUDGET as u64 {
        assert!(
            (shown as u64) < processes,
            "spec §53: the map default is bounded and relevance-ranked, never the entire graph; \
             the host has {processes} processes ({} of them this test's own) and the default map \
             showed {shown} nodes",
            children.0.len()
        );
        let hidden = max_integer(&document["hidden"]).unwrap_or_default();
        let clustered: i64 = clusters(&document)
            .iter()
            .filter_map(|cluster| cluster["members"].as_i64())
            .sum();
        assert!(
            hidden > 0 || clustered > 0,
            "spec §8.2/§23.6: what does not fit is clustered or reported as hidden, never \
             truncated arbitrarily; got hidden {:?} and clusters {:?}",
            document["hidden"],
            clusters(&document)
        );
    }
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §6.9, §53); un-ignored by the increment that delivers it"]
fn should_show_more_than_the_default_when_the_map_is_asked_for_all() {
    let children = Children::spawn(12);
    let pid = children.first_pid();

    let default = map_at(AT_PROCESSES, "");
    let all = map_at(AT_PROCESSES, "--all");

    assert!(
        nodes(&all).len() > nodes(&default).len(),
        "spec §6.9/§53: `--all` is the explicit larger bound the default is not — default showed \
         {} nodes, `--all` showed {}",
        nodes(&default).len(),
        nodes(&all).len()
    );
    assert!(
        nodes(&all)
            .iter()
            .any(|node| mentions(node, &pid.to_string())),
        "spec §22: every node is inspectable data, so the complete map of the processes \
         collection contains the process this test spawned (pid {pid}), got {:?}",
        nodes(&all)
    );
}

// ---------------------------------------------------------------------------------------------
// §43.2 — filtering removes, it never invents
// ---------------------------------------------------------------------------------------------

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §43.2, §6.9); un-ignored by the increment that delivers it"]
fn should_only_remove_edges_when_a_relation_filter_narrows_the_map() {
    // §43.2: "filtering cannot create unknown edges". The relation filtered on is taken from the
    // unfiltered map itself, so the test never hard-codes a relation vocabulary.
    let _children = Children::spawn(12);
    let complete = map_at(AT_PROCESSES, "--all");
    let relation = text(
        edges(&complete).first().unwrap_or_else(|| {
            panic!("spec §6.9: a processes map has relationships, got {complete:?}")
        }),
        "relation",
        "§22",
    );

    let filtered = map_at(AT_PROCESSES, &format!("--all --relations {relation}"));

    let known = ids(edges(&complete));
    for edge in edges(&filtered) {
        assert_eq!(
            text(edge, "relation", "§22"),
            relation,
            "spec §6.9: `--relations` keeps only the requested relations, got {edge:?}"
        );
        assert!(
            known.contains(&text(edge, "id", "§22")),
            "spec §43.2: filtering cannot create unknown edges; {edge:?} is absent from the \
             unfiltered map"
        );
    }
    assert_edges_resolve(&filtered, "the relation-filtered map");
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §43.2, §6.9); un-ignored by the increment that delivers it"]
fn should_only_remove_nodes_and_leave_no_dangling_edge_when_a_type_filter_narrows_the_map() {
    let _children = Children::spawn(12);
    let complete = map_at(AT_PROCESSES, "--all");
    let node_type = text(
        nodes(&complete)
            .first()
            .unwrap_or_else(|| panic!("spec §6.9: a processes map has nodes, got {complete:?}")),
        "type",
        "§22",
    );

    let filtered = map_at(AT_PROCESSES, &format!("--all --type {node_type}"));

    let known = ids(nodes(&complete));
    for node in nodes(&filtered) {
        assert_eq!(
            text(node, "type", "§22"),
            node_type,
            "spec §6.9: `--type` keeps only the requested types, got {node:?}"
        );
        assert!(
            known.contains(&text(node, "id", "§22")),
            "spec §43.2: filtering removes objects, it never creates them; {node:?} is absent \
             from the unfiltered map"
        );
    }
    assert_edges_resolve(&filtered, "the type-filtered map");
}

// ---------------------------------------------------------------------------------------------
// §8 — semantic zoom, clustering, expansion
// ---------------------------------------------------------------------------------------------

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §8.1, §22); un-ignored by the increment that delivers it"]
fn should_report_the_requested_canonical_zoom_level_when_map_json_selects_one() {
    // §8.1: "the L0-L4 vocabulary is normative for renderer behavior and tests". Every canonical
    // level is selectable non-interactively and the returned map says which level it is.
    for level in 0..=4 {
        let document = map(&format!("--zoom {level}"));
        assert_eq!(
            document["zoom_level"].as_i64(),
            Some(level),
            "spec §8.1: L{level} is a canonical zoom level and the map reports the level it is, \
             got {document:?}"
        );
        assert!(
            !nodes(&document).is_empty(),
            "spec §8: a zoom level is a projection of the same truth, never an empty view, \
             got L{level} as {document:?}"
        );
        assert!(
            nodes(&document).len() <= TEXT_MAP_NODE_BUDGET,
            "spec §8.2/§34.2: every level stays inside the view budget by aggregating, got \
             {} nodes at L{level}",
            nodes(&document).len()
        );
    }
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §8.1, §8.2); un-ignored by the increment that delivers it"]
fn should_aggregate_into_the_canonical_domains_when_the_zoom_level_is_coarse() {
    let _children = Children::spawn(12);

    let domains = map("--zoom 1");
    let entities = map_at(AT_PROCESSES, "--zoom 3");

    let labels: Vec<String> = nodes(&domains)
        .iter()
        .map(|node| text(node, "label", "§22").to_lowercase())
        .collect();
    for domain in CANONICAL_DOMAINS {
        assert!(
            labels.iter().any(|label| label.contains(domain)),
            "spec §8.1/§53: L1 is the domain level and the canonical domains are the six of §7; \
             `{domain}` is missing from {labels:?}"
        );
    }
    assert!(
        nodes(&domains).len() <= nodes(&entities).len(),
        "spec §8: zoom changes the level of conceptual aggregation — L1 ({} nodes) is coarser \
         than L3 ({} nodes)",
        nodes(&domains).len(),
        nodes(&entities).len()
    );
    assert_ne!(
        ids(nodes(&domains)),
        ids(nodes(&entities)),
        "spec §8: each level is a different projection, not the same node set relabelled"
    );
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §8.2); un-ignored by the increment that delivers it"]
fn should_report_how_many_objects_a_cluster_stands_for_when_the_view_budget_is_exceeded() {
    let children = Children::spawn(12);
    let processes = host_process_count();
    if processes <= TEXT_MAP_NODE_BUDGET as u64 {
        // Clustering is only required once the visible count exceeds the budget; a host smaller
        // than that has nothing to cluster, and asserting anyway would test the host, not Ono.
        eprintln!(
            "skipped: {processes} processes ({} of them this test's own) stay inside the \
             {TEXT_MAP_NODE_BUDGET}-node view budget, so §8.2 does not require clustering here",
            children.0.len()
        );
        return;
    }

    let document = map_at(AT_PROCESSES, "");

    let clusters = clusters(&document);
    assert!(
        !clusters.is_empty(),
        "spec §8.2: when the visible object count exceeds the view budget Ono MUST cluster \
         rather than truncate arbitrarily, got {document:?}"
    );
    for cluster in clusters {
        for field in ["id", "label", "members", "grouping", "expandable"] {
            assert!(
                !cluster[field].is_null(),
                "spec §22: `MapCluster` carries `{field}`, got {cluster:?}"
            );
        }
        let members = cluster["members"].as_i64().unwrap_or_else(|| {
            panic!("spec §8.2: a cluster reports how many objects it stands for, got {cluster:?}")
        });
        assert!(
            members >= 1,
            "spec §8.2: a cluster stands for at least one object, got {cluster:?}"
        );
        assert!(
            cluster["expandable"].as_bool().is_some(),
            "spec §8.3/§22: a cluster says whether it can be expanded, got {cluster:?}"
        );
    }
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §8.3); un-ignored by the increment that delivers it"]
fn should_yield_exactly_the_members_and_keep_the_place_when_a_cluster_is_expanded() {
    // §8.3: "An interactive cluster MUST be expandable without changing the underlying current
    // place". Expansion is a view action; `enter` is navigation, and this is not `enter`.
    let _children = Children::spawn(12);
    let collapsed = map_at(AT_PROCESSES, "");
    let expandable = clusters(&collapsed)
        .iter()
        .find(|cluster| cluster["expandable"].as_bool() == Some(true))
        .cloned();
    let Some(cluster) = expandable else {
        // Expansion is only observable where the view budget forced a cluster (§8.2); a host
        // small enough to show every process has nothing to expand.
        assert!(
            host_process_count() <= TEXT_MAP_NODE_BUDGET as u64,
            "spec §8.2: a processes collection larger than the {TEXT_MAP_NODE_BUDGET}-node view \
             budget is clustered, got {collapsed:?}"
        );
        eprintln!("skipped: the host is smaller than the view budget, so nothing is clustered");
        return;
    };
    let cluster_id = text(&cluster, "id", "§22");
    let members = cluster["members"].as_i64().expect("a member count");

    let expanded = map_at(AT_PROCESSES, &format!("--expand {cluster_id}"));

    let appeared: BTreeSet<String> = ids(nodes(&expanded))
        .difference(&ids(nodes(&collapsed)))
        .cloned()
        .collect();
    assert_eq!(
        appeared.len() as i64,
        members,
        "spec §8.3: expanding a cluster yields exactly the {members} objects it stood for, got \
         {} new nodes",
        appeared.len()
    );
    assert_eq!(
        text(&expanded, "center", "§22"),
        text(&collapsed, "center", "§22"),
        "spec §8.3: expansion is a view action and does not change the current place"
    );
    assert!(
        !ids(clusters(&expanded)).contains(&cluster_id),
        "spec §8.3: an expanded cluster no longer stands in for its members, got {:?}",
        clusters(&expanded)
    );
    assert_edges_resolve(&expanded, "the expanded map");
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §23.4, §53); un-ignored by the increment that delivers it"]
fn should_not_change_the_current_place_when_a_map_focuses_a_node() {
    // §23.4/§53: "Does focus move the shell? No. Only explicit navigation changes current place."
    // The non-interactive focus argument must obey the same rule the interactive view does.
    let unfocused = map_at(AT_PROCESSES, "");
    let target = ids(nodes(&unfocused))
        .into_iter()
        .next()
        .expect("a node to focus");

    let focused = map_at(AT_PROCESSES, &format!("--focus {target}"));

    assert_eq!(
        text(&focused, "center", "§22"),
        text(&unfocused, "center", "§22"),
        "spec §23.4: moving focus inside a map MUST NOT change the shell's current place"
    );
    assert_ne!(
        text(&focused, "center", "§22"),
        target,
        "spec §23.4: the focused node is not thereby the current place"
    );
}

// ---------------------------------------------------------------------------------------------
// §26, §3.7 — landmarks
// ---------------------------------------------------------------------------------------------

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §3.7, §26); un-ignored by the increment that delivers it"]
fn should_expose_a_built_in_reason_for_every_landmark_when_map_json_reports_them() {
    // §3.7: "A landmark MUST always expose its reason", from the built-in vocabulary the same
    // section enumerates. A plugin-contributed reason must identify its source (§26.5), which is
    // out of scope here: this suite runs with no plugins loaded.
    let _children = Children::spawn(12);
    let documents = [map(""), map_at(AT_PROCESSES, "")];

    for document in &documents {
        for landmark in landmarks(document) {
            let reasons = serde_yaml_ng::to_string(landmark).expect("a printable landmark");
            assert!(
                LANDMARK_REASONS
                    .iter()
                    .any(|reason| reasons.contains(reason)),
                "spec §3.7: a landmark always exposes its reason, and the built-in reasons are \
                 {LANDMARK_REASONS:?}; got {landmark:?}"
            );
        }
        for node in nodes(document) {
            for reason in node["landmark_reasons"]
                .as_sequence()
                .unwrap_or(&Vec::new())
            {
                let reason = reason.as_str().unwrap_or_else(|| {
                    panic!("spec §3.7: a landmark reason is named, got {node:?}")
                });
                assert!(
                    LANDMARK_REASONS.contains(&reason),
                    "spec §3.7: `{reason}` is not one of the built-in landmark reasons \
                     {LANDMARK_REASONS:?}, and core landmarks may not invent their own"
                );
            }
        }
    }
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §26.2, §3.7); un-ignored by the increment that delivers it"]
fn should_mark_a_listener_on_every_interface_as_a_public_listener_landmark() {
    // §26.2 names "public listener" a built-in network landmark rule and §3.7 fixes its reason as
    // `public_listener`. The fixture is this test's own socket bound on 0.0.0.0, so the assertion
    // never depends on a service this machine happens to run.
    let (_listener, port) = public_listener();

    let document = map_at(AT_LISTENERS, "--all");

    let node = nodes(&document)
        .iter()
        .find(|node| mentions(node, &port.to_string()))
        .unwrap_or_else(|| {
            panic!("spec §14.1: the listeners collection contains the listener this test bound on 0.0.0.0:{port}, got {document:?}")
        });
    let reasons = serde_yaml_ng::to_string(&node["landmark_reasons"]).expect("printable reasons");
    assert!(
        reasons.contains("public_listener"),
        "spec §26.2/§3.7: a listener bound on every interface is a `public_listener` landmark, \
         got {node:?}"
    );
    assert!(
        landmarks(&document)
            .iter()
            .any(|landmark| mentions(landmark, &port.to_string())),
        "spec §22: the map's landmark list names the landmark its nodes carry, got {:?}",
        landmarks(&document)
    );
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §26.3); un-ignored by the increment that delivers it"]
fn should_expose_landmark_thresholds_as_inspectable_and_configurable_settings() {
    // §26.3: "Thresholds MUST be inspectable and configurable." The v0.2 settings surface is
    // where a user inspects and changes them; setting one back to its own default proves it is
    // writable without changing what the rest of the suite observes.
    let run = ono("get config | to json");
    run.assert_success();
    let settings: Value =
        serde_yaml_ng::from_str(run.stdout().trim()).expect("`get config` emits a JSON document");
    let settings = settings.as_sequence().expect("a list of settings").clone();

    let thresholds: Vec<&Value> = settings
        .iter()
        .filter(|setting| {
            setting["key"]
                .as_str()
                .is_some_and(|key| key.starts_with("spatial.landmark"))
        })
        .collect();
    assert!(
        !thresholds.is_empty(),
        "spec §26.3: landmark thresholds are inspectable, so `get config` lists them under \
         `spatial.landmark.*`; got keys {:?}",
        settings
            .iter()
            .filter_map(|setting| setting["key"].as_str())
            .collect::<Vec<_>>()
    );
    let threshold = thresholds[0];
    let key = text(threshold, "key", "§26.3");
    assert!(
        !threshold["type"].is_null() && !threshold["default_value"].is_null(),
        "spec §26.3: a threshold states its type and its conservative default, got {threshold:?}"
    );
    let value = serde_yaml_ng::to_string(&threshold["value"])
        .expect("a printable value")
        .trim()
        .to_owned();
    let write = ono(&format!("set config {key} {value}"));
    write.assert_success();
    assert!(
        !write.stderr().contains("Ono-Sendai-E0101"),
        "spec §26.3: a landmark threshold is configurable, got stderr {:?}",
        write.stderr()
    );
}

// ---------------------------------------------------------------------------------------------
// §23.2, §39, §52.1 — the text map
// ---------------------------------------------------------------------------------------------

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §23.2, §52.1); un-ignored by the increment that delivers it"]
fn should_render_a_text_map_when_stdout_is_a_pipe_and_no_full_screen_view_is_possible() {
    // §52.1: "map text rendering works without full-screen TUI"; §23.2: "Every terminal MUST have
    // a non-fullscreen textual map representation."
    let structured = map("");
    let center_label = nodes(&structured)
        .iter()
        .find(|node| text(node, "id", "§22") == text(&structured, "center", "§22"))
        .map(|node| text(node, "label", "§22"));

    let run = ono("map");
    run.assert_success();
    let output = run.stdout();

    assert!(
        !output.trim().is_empty(),
        "spec §23.2: `map` without a TTY renders the map as text, got empty stdout, stderr {:?}",
        run.stderr()
    );
    assert!(
        !output.contains('\u{1b}'),
        "spec §29.1/§39.1: a piped map carries no terminal escape sequences, got {output:?}"
    );
    assert!(
        !output.contains("[?1049h"),
        "spec §23.3/§52.1: `map` does not open a full-screen view when there is no terminal, \
         got {output:?}"
    );
    if let Some(label) = center_label {
        assert!(
            output.contains(&label),
            "spec §23.1: the text map's first priority is the current place `{label}`, got \
             {output:?}"
        );
    }
    let rendered_labels = nodes(&structured)
        .iter()
        .filter(|node| output.contains(&text(node, "label", "§22")))
        .count();
    assert!(
        rendered_labels >= 2,
        "spec §22/§23.1: every visible node corresponds to inspectable data — the text map must \
         show the nodes `map --json` reports, got {output:?} for {:?}",
        nodes(&structured)
    );
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §39.1, §39.2); un-ignored by the increment that delivers it"]
fn should_render_the_map_in_plain_ascii_when_colour_is_disabled_and_the_terminal_is_ascii_only() {
    // §39.1: colour MUST NOT be required to distinguish current node, inferred edge, failed
    // state, remote boundary, root privilege or focus. §39.2: "ASCII fallback MUST exist."
    let run = ono_in(
        "map",
        &[
            ("NO_COLOR", "1"),
            ("TERM", "dumb"),
            ("LC_ALL", "C"),
            ("LANG", "C"),
        ],
    );
    run.assert_success();
    let output = run.stdout();

    assert!(
        output.is_ascii(),
        "spec §39.2: an ASCII-only terminal gets an ASCII map, got non-ASCII bytes in {output:?}"
    );
    assert!(
        !output.contains('\u{1b}'),
        "spec §39.1: with NO_COLOR the map carries no escape sequences at all, got {output:?}"
    );
    assert!(
        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
            >= 2,
        "spec §23.2/§39.2: the ASCII fallback is still a legible map, got {output:?}"
    );
    let structured = map("");
    for node in nodes(&structured).iter().take(3) {
        let label = text(node, "label", "§22");
        assert!(
            output.contains(&label),
            "spec §39.2: the ASCII fallback shows the same nodes as the structured map, `{label}` \
             is missing from {output:?}"
        );
    }
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §39.3, §43.5); un-ignored by the increment that delivers it"]
fn should_fit_the_text_map_into_the_terminal_when_the_terminal_is_narrow() {
    // §39.3: "At narrow widths, maps MAY collapse into ranked tree/list projections rather than
    // drawing graphs. Spatial semantics remain identical." Layout is presentation and no snapshot
    // is asserted here (§43.5); the width-driven rule itself is the contract: nothing overflows.
    for columns in [40usize, 80, 120, 200] {
        let run = ono_in("map", &[("COLUMNS", &columns.to_string())]);
        run.assert_success();
        let output = run.stdout();
        assert!(
            !output.trim().is_empty(),
            "spec §39.3: a map renders at {columns} columns, got empty stdout, stderr {:?}",
            run.stderr()
        );
        for line in output.lines() {
            assert!(
                line.chars().count() <= columns,
                "spec §39.3/§23.6: at {columns} columns the map fits the terminal instead of \
                 wrapping, got a {}-character line {line:?}",
                line.chars().count()
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// §24 — `look` rendering rules
// ---------------------------------------------------------------------------------------------

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §24.1, §6.1); un-ignored by the increment that delivers it"]
fn should_describe_identity_state_exits_and_landmarks_when_look_json_reports_a_place() {
    // §24.1 fixes the priority: identity and state, direct exits, landmarks, recent relevant
    // changes, summary counts — and "MUST NOT default to dumping all properties of the
    // underlying object". `look --all` (§6.1) is where the exhaustive view lives.
    let script = "look --json";
    let run = ono(script);
    let place = document(&run, script);

    for field in ["id", "type", "label", "groups", "landmarks"] {
        assert!(
            !place[field].is_null(),
            "spec §24.1/§6.1: the `PlaceView` carries `{field}` — identity, state, exits and \
             landmarks come first, got {place:?}"
        );
    }
    let groups = place["groups"]
        .as_sequence()
        .unwrap_or_else(|| panic!("spec §24.2: `groups` is the list of exits, got {place:?}"));
    assert!(
        !groups.is_empty(),
        "spec §5/§7.1: the root place offers the canonical domains as exits, got {place:?}"
    );

    let all = ono("look --all --json");
    let all = document(&all, "look --all --json");
    let mut default_keys = BTreeSet::new();
    keys(&place, &mut default_keys);
    let mut all_keys = BTreeSet::new();
    keys(&all, &mut all_keys);
    assert!(
        default_keys.len() < all_keys.len(),
        "spec §24.1: default `look` does not dump all properties — `--all` must show strictly \
         more than the default, got {default_keys:?} against {all_keys:?}"
    );
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §24.2); un-ignored by the increment that delivers it"]
fn should_mark_a_group_as_an_exit_only_when_it_can_be_entered_when_look_lists_groups() {
    // §24.2: displayed group labels "MUST be valid navigation or query targets where practical",
    // and "If a displayed group is not navigable, the renderer MUST not visually imply that it is
    // an exit." So navigability is a property of the structured group, not a rendering guess.
    let script = "look --json";
    let run = ono(script);
    let place = document(&run, script);
    let groups = place["groups"]
        .as_sequence()
        .unwrap_or_else(|| panic!("spec §24.2: `groups` is the list of exits, got {place:?}"))
        .clone();

    for group in &groups {
        assert!(
            !group["label"].is_null(),
            "spec §24.2: a group has the label the user types, got {group:?}"
        );
        assert!(
            !group["count"].is_null() || !group["state"].is_null(),
            "spec §24.2/§35.2: a group shows a count or the permission state that replaces it, \
             got {group:?}"
        );
        assert!(
            group["navigable"].as_bool().is_some(),
            "spec §24.2: a group states whether it is an exit, so a renderer never implies one \
             that is not, got {group:?}"
        );
    }

    let navigable = groups
        .iter()
        .find(|group| group["navigable"].as_bool() == Some(true))
        .unwrap_or_else(|| panic!("spec §5: the root place has navigable exits, got {place:?}"));
    let label = text(navigable, "label", "§24.2");
    let entered = ono(&format!("enter {label}\nlook --json"));
    entered.assert_success();
    let entered_place = document(&entered, "enter <group>; look --json");
    assert_ne!(
        text(&entered_place, "id", "§3.1"),
        text(&place, "id", "§3.1"),
        "spec §24.2: a group marked navigable really is an exit — `enter {label}` moves"
    );
    assert!(
        !entered.stderr().contains("spatial.not_enterable"),
        "spec §24.2: a group marked navigable is enterable, got stderr {:?}",
        entered.stderr()
    );
}

#[test]
#[ignore = "REASON: v0.4 spatial systems interface (docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md §24.3, §53); un-ignored by the increment that delivers it"]
fn should_not_invent_a_change_section_when_no_snapshot_or_event_source_exists() {
    // §24.3: "No fake change summary may be generated when no event source or comparison snapshot
    // exists." A one-shot script has no earlier snapshot of its own, so the reading taken here is:
    // the change section is either absent, or present and explicit about why it is empty — one of
    // the §35.2 states (unknown/unsupported/stale/empty), never a fabricated list of changes.
    let script = "look --json --changes 10s";
    let run = ono(script);
    let place = document(&run, script);

    let changed = &place["changed"];
    if changed.is_null() {
        return;
    }
    let state = changed["state"].as_str().unwrap_or_default();
    let entries = changed["entries"]
        .as_sequence()
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        assert!(
            ["empty", "unknown", "unsupported", "stale", "available"].contains(&state),
            "spec §35.2/§24.3: an empty change section names its state rather than implying \
             nothing happened, got {changed:?}"
        );
    } else {
        assert_eq!(
            state, "available",
            "spec §24.3: changes are only reported when a source exists, got {changed:?}"
        );
        for entry in &entries {
            assert!(
                !entry["observed_at"].is_null(),
                "spec §24.3/§25.3: every reported change carries when it was observed, so no \
                 summary can be fabricated, got {entry:?}"
            );
            assert!(
                !entry["object"].is_null() || !entry["id"].is_null(),
                "spec §24.3: a change names the object it happened to, got {entry:?}"
            );
        }
    }
}
