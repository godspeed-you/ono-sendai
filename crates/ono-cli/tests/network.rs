//! Outcome tests for the network family the contract declares:
//! `resolve dns`, `test port`, `watch interface`, `watch route`, `trace route`, `trace interface`,
//! `enter interface`, `enter socket`, and the write paths `add/remove/set route`,
//! `set/start/stop/add/remove interface` and `stop socket`.
//!
//! Contract: `docs/spec/commands/network.yaml`, schemas `ono.interface/1`, `ono.route/1`,
//! `ono.socket/1`, `ono.endpoint/1`, `ono.graph/1`, `ono.context/1`, `ono.action-result/1`,
//! `ono.error/1`, and the deferred `ono.dns-record/1`, `ono.probe-result/1`,
//! `ono.interface-event/1`, `ono.route-event/1` (`docs/spec/schemas/deferred.yaml`).
//! Narrative: spec §9.1 (the network table), §14.1/§14.3 (object context and its implicit
//! selector), §16.5 (no collapsed failures), §17.4 (scripts never wait for a prompt), §18.2
//! (native live streams begin with a snapshot — ADR-0024, ADR-0034), §22.1/§22.3 (graphs and
//! useful traces), §23.2 (netlink), §28.4/§28.5 (Socket, Interface), §41.2/§41.5, §43 (error
//! codes). ADR-0006 makes any `failed` ActionResult row exit 1; ADR-0023 fixes what a context
//! frame may contribute.
//!
//! Everything here runs offline and unprivileged: the loopback interface, `127.0.0.1`, names
//! from `/etc/hosts`, and TCP listeners the test itself binds on `127.0.0.1:0`. Every test
//! asserts what the user sees — stdout through `| to json`, the exit status, the structured error
//! code, the system state afterwards — never how a stage is wired (AGENTS.md §11).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use ono_testkit::Shell;
use serde_yaml_ng::Value;

/// Runs a one-liner with a generous budget: nothing here may hang, and a run that does is a
/// failure of the shell, not of the test.
fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

/// Parses the JSON document `to json` wrote as the stream's values.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let text = run.stdout().trim().to_owned();
    let stderr = run.stderr();
    let document: Value = serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!("`to json` must emit a JSON document, got {text:?} ({error}); stderr: {stderr:?}")
    });
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!(
                "spec §33.5: `to json` emits the stream as an array, got {text:?}; stderr: {stderr:?}"
            )
        })
        .clone()
}

/// The one value a single-object command emits: one ActionResult, one graph, one probe result.
fn single(run: &ono_testkit::Run) -> Value {
    let mut rows = rows(run);
    assert_eq!(
        rows.len(),
        1,
        "spec §33.5: a one-value stream is a one-element array, got {:?}",
        run.stdout()
    );
    rows.remove(0)
}

fn text(row: &Value, field: &str) -> String {
    row[field]
        .as_str()
        .unwrap_or_else(|| panic!("field `{field}` must be a string, got {row:?}"))
        .to_owned()
}

/// A listening socket the test owns for as long as it holds the value.
fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback listener");
    let port = listener.local_addr().expect("the bound address").port();
    (listener, port)
}

/// A loopback port that nothing listens on: bound, read, released.
fn closed_port() -> u16 {
    let (listener, port) = listener();
    drop(listener);
    port
}

fn accepts_connections(port: u16) -> bool {
    let address: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .expect("a socket address");
    TcpStream::connect_timeout(&address, Duration::from_secs(2)).is_ok()
}

/// Whether the test runs without the capability the write paths need. The mutation tests
/// assert that the operating system refuses them; a root run would carry them out — on the
/// loopback interface — so they stand down rather than reconfigure the machine.
fn unprivileged() -> bool {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    let uid = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().next())
        .expect("the Uid line of /proc/self/status");
    if uid == "0" {
        eprintln!("skipped: the network write paths must not be exercised as root");
        return false;
    }
    true
}

/// The explain plan of a network mutation, in the wording the shell prints for the mutations
/// it already implements (`explain stop service nginx`).
fn assert_explained_as_privileged_mutation(script: &str, risk: &str) {
    let run = ono(&format!("explain {script}"));
    run.assert_success();
    let plan = run.stdout();
    assert!(
        plan.contains("privilege    elevated"),
        "network.yaml declares `privilege: elevated` for `{script}`, and spec §17.1/§43 make \
         `explain` report it before anything runs; got {plan:?}"
    );
    assert!(
        plan.contains(&format!("risk         {risk}")),
        "spec §17.1: `explain` reports the risk descriptor `{risk}` for `{script}`, got {plan:?}"
    );
}

fn assert_failed_row(row: &Value, operation: &str, code: &str) {
    assert_eq!(
        text(row, "operation"),
        operation,
        "spec §11.5: `operation` is the command id, got {row:?}"
    );
    assert_eq!(
        text(row, "status"),
        "failed",
        "the mutation reports a failure as a `failed` row, not as text, got {row:?}"
    );
    assert_eq!(
        row["changed"].as_bool(),
        Some(false),
        "a failed mutation changed nothing, got {row:?}"
    );
    assert_eq!(
        row["error"]["code"].as_str(),
        Some(code),
        "spec §43: the failed row carries the structured error {code}, got {row:?}"
    );
    assert!(
        row["duration"].is_string(),
        "action-result.v1: `duration` is required even for a failure, got {row:?}"
    );
}

/// Runs an unprivileged network mutation and asserts the one honest outcome: one ActionResult
/// row, `failed`, `io.permission_denied`, exit 1 — never `E0101`/`E0102` "not implemented".
fn assert_refused_by_the_kernel(script: &str, operation: &str) -> Value {
    let run = ono(&format!("{script} | to json"));
    assert!(
        !run.stderr().contains("Ono-Sendai-E0101") && !run.stderr().contains("Ono-Sendai-E0102"),
        "`{script}` is delivered, not declared: the shell attempts it, got {:?}",
        run.stderr()
    );
    let row = single(&run);
    // rtnetlink refuses every RTM_NEW*/DEL*/SET* message from a caller without CAP_NET_ADMIN
    // before it looks at the payload, so permission is the first thing the OS says — E0302,
    // not a not-found for a prefix or interface that happens not to exist.
    assert_failed_row(&row, operation, "Ono-Sendai-E0302");
    run.assert_status(1);
    row
}

/// The nodes of a graph, as `ono.graph-node/1` records.
fn nodes(graph: &Value) -> Vec<Value> {
    graph["nodes"]
        .as_sequence()
        .unwrap_or_else(|| panic!("graph.v1: `nodes` is a required list, got {graph:?}"))
        .clone()
}

/// The first node of the given kind whose summary satisfies the predicate.
fn node_where(graph: &Value, kind: &str, predicate: impl Fn(&Value) -> bool) -> Option<Value> {
    nodes(graph)
        .into_iter()
        .find(|node| node["kind"].as_str() == Some(kind) && predicate(&node["value"]))
}

fn interface_node(graph: &Value, name: &str) -> Option<Value> {
    node_where(graph, "ono.interface/1", |value| {
        value["name"].as_str() == Some(name)
    })
}

/// The frames of the context stack, ground first.
fn context_frames(run: &ono_testkit::Run) -> Vec<Value> {
    rows(run)
}

// --- resolve dns ------------------------------------------------------------------------------

#[test]
fn should_resolve_localhost_to_a_loopback_record_when_resolving_dns() {
    // Spec §9.1: `resolve dns <name>` yields `Stream<DnsRecord>`. `localhost` comes from
    // /etc/hosts, so the answer needs no network and is the same on every machine.
    let run = ono("resolve dns localhost | to json");
    run.assert_success();
    let records = rows(&run);
    assert!(
        !records.is_empty(),
        "spec §9.1: resolving a name that /etc/hosts answers yields at least one record, got {:?}",
        run.stdout()
    );
    for record in &records {
        assert_eq!(
            record["name"].as_str(),
            Some("localhost"),
            "ono.dns-record/1: each record names the query it answers, got {record:?}"
        );
        assert!(
            matches!(record["type"].as_str(), Some("A" | "AAAA")),
            "ono.dns-record/1: an address record carries its record type (the type `--type` \
             selects on), got {record:?}"
        );
    }
    assert!(
        records
            .iter()
            .any(|record| { matches!(record["address"].as_str(), Some("127.0.0.1" | "::1")) }),
        "localhost resolves to a loopback address (spec §41.2 names 127.0.0.1 and ::1), got {records:?}"
    );
}

#[test]
fn should_return_only_the_requested_record_type_when_resolving_dns_with_a_type() {
    // network.yaml: `--type` restricts the answer to one record type.
    let run = ono("resolve dns localhost --type A | to json");
    run.assert_success();
    let records = rows(&run);
    assert!(
        !records.is_empty(),
        "/etc/hosts maps localhost to 127.0.0.1, so an A query has an answer, got {:?}",
        run.stdout()
    );
    for record in &records {
        assert_eq!(
            record["type"].as_str(),
            Some("A"),
            "network.yaml: `--type A` yields A records only, got {record:?}"
        );
        assert_eq!(
            record["address"].as_str(),
            Some("127.0.0.1"),
            "an A record for localhost is the IPv4 loopback address, got {record:?}"
        );
    }
}

#[test]
fn should_perform_a_reverse_lookup_when_the_query_is_an_address() {
    // network.yaml, selector `query`: "An address performs a reverse lookup." 127.0.0.1 is
    // `localhost` in /etc/hosts everywhere.
    let run = ono("resolve dns 127.0.0.1 | to json");
    run.assert_success();
    let records = rows(&run);
    assert!(
        records
            .iter()
            .any(|record| record["name"].as_str() == Some("localhost")),
        "a reverse lookup of 127.0.0.1 names localhost, got {records:?}"
    );
}

#[test]
fn should_fail_with_a_structured_error_and_not_hang_when_the_name_does_not_exist() {
    // `.invalid` is reserved never to resolve (RFC 2606). The resolver answers "no such name"
    // where it is reachable and "cannot ask" where the container has no network; both are
    // structured, non-zero, and bounded in time — never a hang, never "not implemented".
    let started = Instant::now();
    let run = Shell::new()
        .args(["-c", "resolve dns definitely.invalid. | to json"])
        .timeout(Duration::from_secs(20))
        .run();
    assert!(
        !run.status().is_success(),
        "spec §16: a name that does not resolve is a failure, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0301") || run.stderr().contains("Ono-Sendai-E0401"),
        "spec §43: the failure is `io.not_found` (no such name) or `provider.unavailable` (no \
         resolver reachable), got {:?}",
        run.stderr()
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "a failed resolution returns within the resolver's own timeout, not never"
    );
}

// --- test port --------------------------------------------------------------------------------

#[test]
fn should_report_a_reachable_port_when_probing_a_listening_socket() {
    let (_listener, port) = listener();
    // Spec §9.1: `test port <host> <port>` → `ProbeResult`, "reachability with timing and error
    // detail". The listener is the test's own, so the answer is yes and needs no network.
    let run = ono(&format!("test port 127.0.0.1 {port} | to json"));
    run.assert_success();
    let probe = single(&run);
    assert_eq!(
        probe["reachable"].as_bool(),
        Some(true),
        "ono.probe-result/1: a listening port is reachable, got {probe:?}"
    );
    assert_eq!(
        probe["host"].as_str(),
        Some("127.0.0.1"),
        "the result names the host it probed, got {probe:?}"
    );
    assert_eq!(
        probe["port"].as_u64(),
        Some(u64::from(port)),
        "the result names the port it probed, got {probe:?}"
    );
    assert!(
        probe["duration"].is_string(),
        "spec §9.1: the probe carries its timing as a duration, got {probe:?}"
    );
    assert!(
        probe["error"].is_null(),
        "a reachable probe has no error detail, got {probe:?}"
    );
}

#[test]
fn should_report_the_refusal_when_probing_a_closed_port() {
    let port = closed_port();
    // A port nothing listens on is refused immediately by the loopback stack. The probe did
    // its job — the answer is data, so the run succeeds and the detail says why.
    let run = ono(&format!("test port 127.0.0.1 {port} | to json"));
    run.assert_success();
    let probe = single(&run);
    assert_eq!(
        probe["reachable"].as_bool(),
        Some(false),
        "ono.probe-result/1: a closed port is not reachable, got {probe:?}"
    );
    assert!(
        !probe["error"].is_null(),
        "spec §9.1: an unreachable probe carries error detail, got {probe:?}"
    );
    let detail = serde_yaml_ng::to_string(&probe["error"]).expect("serialise the error detail");
    assert!(
        detail.to_lowercase().contains("refused"),
        "the error detail names the refusal the OS reported, got {detail:?}"
    );
    assert!(
        probe["duration"].is_string(),
        "spec §9.1: timing is reported for a failed probe too, got {probe:?}"
    );
}

#[test]
fn should_accept_the_timeout_and_protocol_options_when_probing() {
    let (_listener, port) = listener();
    // network.yaml: `--timeout <duration>` and `--protocol <string>`; the contract's own example
    // is `test port 10.4.2.11 5432 --timeout 2s`.
    let run = ono(&format!(
        "test port 127.0.0.1 {port} --timeout 2s --protocol tcp | to json"
    ));
    run.assert_success();
    let probe = single(&run);
    assert_eq!(
        probe["reachable"].as_bool(),
        Some(true),
        "the options narrow the probe without changing its answer, got {probe:?}"
    );
    assert_eq!(
        probe["protocol"].as_str(),
        Some("tcp"),
        "the result names the transport it probed, got {probe:?}"
    );
}

// --- watch interface / watch route ------------------------------------------------------------

#[test]
fn should_begin_with_a_snapshot_when_watching_interfaces() {
    // Spec §18.2/§18.3: piped `watch` emits ordinary event values, and the stream begins with the
    // current state (ADR-0024, ADR-0034). `take 1` bounds it so the document can end.
    let run = ono("watch interface | take 1 | select kind | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"kind":"snapshot"}]"#,
        "the interface stream begins with a snapshot event (ADR-0024)"
    );
}

#[test]
fn should_carry_the_loopback_interface_in_the_snapshot_when_watching_it() {
    // network.yaml: the `name` selector watches one interface. `lo` exists everywhere.
    let run = ono("watch interface lo | take 1 | to json");
    run.assert_success();
    let event = single(&run);
    assert_eq!(
        event["kind"].as_str(),
        Some("snapshot"),
        "the first event is the current state (ADR-0024), got {event:?}"
    );
    assert_eq!(
        event["interface"]["name"].as_str(),
        Some("lo"),
        "ono.interface-event/1 carries the interface record under `interface`, as the socket \
         and process events carry theirs, got {event:?}"
    );
    assert!(
        matches!(event["source"].as_str(), Some("poll" | "subscription")),
        "spec §18.2/ADR-0034: how the event was obtained is explicit, got {event:?}"
    );
}

#[test]
fn should_watch_interfaces_through_the_kernel_rather_than_by_asking_it_again() {
    // ADR-0034 left every watch polling; ADR-0235 binds the rtnetlink multicast groups
    // `rtnetlink(7)` provides for exactly this, so the kernel says when a link or an address
    // moved. §18.2 requires the cost of a watch to be explicit, and `source` is where a consumer
    // reads which of the two it is getting.
    let run = ono("watch interface | take 1 | select source | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"source":"subscription"}]"#,
        "the interface watch is driven by RTMGRP_LINK and the address groups, not by a timer; \
         got {:?}",
        run.output()
    );
}

#[test]
fn should_watch_routes_through_the_kernel_rather_than_by_asking_it_again() {
    let run = ono("watch route --table local | take 1 | select source | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"source":"subscription"}]"#,
        "the route watch is driven by the RTMGRP_IPV4_ROUTE and RTMGRP_IPV6_ROUTE groups; got \
         {:?}",
        run.output()
    );
}

#[test]
fn should_begin_with_a_snapshot_when_watching_routes() {
    // `--table local` names the one table every Linux machine populates: the loopback routes.
    let run = ono("watch route --table local | take 1 | select kind | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"kind":"snapshot"}]"#,
        "the route stream begins with a snapshot event (ADR-0024)"
    );
}

#[test]
fn should_carry_the_loopback_route_in_the_snapshot_when_watching_the_local_table() {
    let run = ono("watch route --table local | where route.interface == \"lo\" | take 1 | to json");
    run.assert_success();
    let event = single(&run);
    assert_eq!(
        event["kind"].as_str(),
        Some("snapshot"),
        "the loopback route is part of the initial state, not a later change, got {event:?}"
    );
    assert_eq!(
        event["route"]["interface"].as_str(),
        Some("lo"),
        "ono.route-event/1 carries the route record under `route`, got {event:?}"
    );
    assert_eq!(
        event["route"]["table"].as_str(),
        Some("local"),
        "network.yaml: `--table` watches the named table, got {event:?}"
    );
}

// --- trace route / trace interface -----------------------------------------------------------

#[test]
fn should_name_the_loopback_interface_when_tracing_the_loopback_route() {
    // network.yaml: `trace route <destination>` shows "which interface, gateway and neighbour a
    // route depends on". 127.0.0.0/8 is the loopback route the kernel installs in the local
    // table; the selector names the route as `get route` shows it, whichever table holds it.
    let run = ono("trace route 127.0.0.0/8 | to json");
    run.assert_success();
    let graph = single(&run);
    assert_eq!(
        graph["root"]["schema"].as_str(),
        Some("ono.route/1"),
        "graph.v1: the trace starts from the route, got {graph:?}"
    );
    assert!(
        interface_node(&graph, "lo").is_some(),
        "the loopback route depends on the interface `lo`, so the graph has an \
         ono.interface/1 node for it, got {graph:?}"
    );
    assert!(
        graph["edges"]
            .as_sequence()
            .is_some_and(|edges| edges.iter().any(|edge| {
                edge["to"]["schema"].as_str() == Some("ono.interface/1")
                    || edge["from"]["schema"].as_str() == Some("ono.interface/1")
            })),
        "spec §22.1: the dependency is an edge, not an implication of adjacency, got {graph:?}"
    );
}

#[test]
fn should_trace_the_route_that_arrives_through_the_pipeline() {
    // network.yaml: `input: null | ono.route/1` — the route to trace may come from `get route`.
    let run = ono(
        "get route --table local | where interface == \"lo\" | where family == \"inet\" | take 1 \
         | trace route | to json",
    );
    run.assert_success();
    let graph = single(&run);
    assert!(
        interface_node(&graph, "lo").is_some(),
        "a route over `lo` traces to the interface `lo`, got {graph:?}"
    );
}

#[test]
fn should_show_the_interface_and_its_address_when_tracing_an_interface() {
    // network.yaml: `trace interface` shows "the routes, addresses, neighbours and sockets bound
    // to an interface". `lo` carries 127.0.0.1/8 and the kernel's loopback routes everywhere.
    let run = ono("trace interface lo | to json");
    run.assert_success();
    let graph = single(&run);
    assert_eq!(
        graph["root"]["schema"].as_str(),
        Some("ono.interface/1"),
        "graph.v1: the trace starts from the interface, got {graph:?}"
    );
    let lo = interface_node(&graph, "lo").unwrap_or_else(|| {
        panic!("the traced interface is a node of its own graph, got {graph:?}")
    });
    assert!(
        lo["value"]["addresses"]
            .as_sequence()
            .is_some_and(|addresses| addresses.iter().any(|a| a.as_str() == Some("127.0.0.1/8"))),
        "spec §28.5: the interface node summarises its addresses, got {lo:?}"
    );
    assert!(
        node_where(&graph, "ono.route/1", |route| route["interface"].as_str()
            == Some("lo"))
        .is_some(),
        "the loopback routes are bound to `lo`, so the graph has an ono.route/1 node over it, \
         got {graph:?}"
    );
}

#[test]
fn should_include_the_listening_socket_when_tracing_its_interface() {
    let (_listener, port) = listener();
    // A socket bound to 127.0.0.1 is bound to the interface that owns that address.
    let run = ono("trace interface lo | to json");
    run.assert_success();
    let graph = single(&run);
    assert!(
        node_where(&graph, "ono.socket/1", |socket| socket["local"]["port"]
            .as_u64()
            == Some(u64::from(port)))
        .is_some(),
        "network.yaml: the sockets bound to the interface are part of its graph; the test's \
         listener on 127.0.0.1:{port} is missing from {graph:?}"
    );
}

// --- enter interface / enter socket -----------------------------------------------------------

#[test]
fn should_push_an_object_frame_when_entering_an_interface() {
    // Spec §14.1: a frame has a kind from the fixed set and the identity of the entered object;
    // §14.3 makes an entered object an object context.
    let run = ono("enter interface lo; get context | to json");
    run.assert_success();
    let frames = context_frames(&run);
    assert_eq!(
        frames.len(),
        2,
        "the ground frame plus the entered interface (spec §14.1), got {frames:?}"
    );
    let top = &frames[1];
    assert_eq!(
        top["kind"].as_str(),
        Some("object"),
        "context.v1: an entered interface is an `object` frame, got {top:?}"
    );
    assert_eq!(
        top["target"].as_str(),
        Some("interface"),
        "context.v1: the frame names the target it narrows to, got {top:?}"
    );
    assert_eq!(
        top["identity"].as_str(),
        Some("lo"),
        "context.v1: the frame carries the interface's identity as the prompt shows it, got {top:?}"
    );
    assert!(
        top["selector"].as_str().is_some_and(|s| s.contains("lo")),
        "ADR-0023: what the frame contributes is spelled out as an explicit selector, got {top:?}"
    );
}

#[test]
fn should_narrow_routes_to_the_entered_interface() {
    // Spec §14.3: the object context provides an implicit selector. Inside `lo`, the local
    // table shows only the loopback routes — never every route on the machine.
    let run = ono("enter interface lo; get route --table local | to json");
    run.assert_success();
    let routes = rows(&run);
    assert!(
        !routes.is_empty(),
        "the kernel keeps the loopback routes in the local table, so the narrowed query is not \
         empty, got {:?}",
        run.stdout()
    );
    for route in &routes {
        assert_eq!(
            route["interface"].as_str(),
            Some("lo"),
            "spec §14.3: `get route` inside `enter interface lo` yields routes over `lo` only, \
             got {route:?}"
        );
    }
}

#[test]
fn should_pop_the_interface_frame_when_leaving() {
    // Two documents: the stack with the frame on it, then the stack after `leave`. A `leave`
    // with nothing to pop prints a diagnostic (ADR-0023), so a quiet run proves a frame was
    // there to pop.
    let run = ono("enter interface lo; get context | to json; leave; get context | to json");
    run.assert_success();
    assert!(
        run.stderr().is_empty(),
        "entering `lo` and leaving it again are both ordinary, quiet operations, got {:?}",
        run.stderr()
    );
    let documents: Vec<Vec<Value>> = run
        .stdout()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_yaml_ng::from_str::<Value>(line)
                .unwrap_or_else(|error| {
                    panic!("a JSON document per `to json`, got {line:?} ({error})")
                })
                .as_sequence()
                .expect("spec §33.5: an array")
                .clone()
        })
        .collect();
    assert_eq!(
        documents.len(),
        2,
        "one document per `get context | to json`, got {:?}",
        run.stdout()
    );
    assert_eq!(
        documents[0].len(),
        2,
        "spec §14.1: `enter` pushed the interface frame, got {:?}",
        documents[0]
    );
    assert_eq!(
        documents[1].len(),
        1,
        "spec §14.1: `leave` pops the frame `enter` pushed, got {:?}",
        documents[1]
    );
    assert_eq!(
        documents[1][0]["kind"].as_str(),
        Some("local"),
        "the ground frame is what remains, got {:?}",
        documents[1]
    );
}

#[test]
fn should_refuse_to_enter_an_interface_that_does_not_exist() {
    // On its own, the refused `enter` is the script's last statement and its status is the
    // script's; followed by another statement, the script continues — `-c 'a; b'` exits with
    // `b`'s status in this shell as in every other — and the stack is still shown.
    let alone = ono("enter interface ono-definitely-not-an-interface0");
    assert!(
        !alone.status().is_success(),
        "entering nothing must fail, got {:?}",
        alone.output()
    );
    let run = ono("enter interface ono-definitely-not-an-interface0; get context | to json");
    assert!(
        run.stderr().contains("Ono-Sendai-E1001"),
        "the refusal is `spatial.not_found` for the named interface — the same answer \
         `enter service` gives for a unit that does not exist (ADR-0191) — got {:?}",
        run.stderr()
    );
    assert_eq!(
        context_frames(&run).len(),
        1,
        "no frame is pushed for an object that does not exist, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_push_a_socket_frame_when_entering_the_listening_socket() {
    let (_listener, port) = listener();
    // network.yaml: `enter socket` takes no selector; the socket arrives as input, as in the
    // contract's own example `get socket | take 1 | enter socket`.
    let run = ono(&format!(
        "get socket {port} | enter socket; get context | to json"
    ));
    run.assert_success();
    let frames = context_frames(&run);
    assert_eq!(
        frames.len(),
        2,
        "the ground frame plus the entered socket (spec §14.1), got {frames:?}"
    );
    let top = &frames[1];
    assert_eq!(
        top["kind"].as_str(),
        Some("object"),
        "context.v1: an entered socket is an `object` frame, got {top:?}"
    );
    assert_eq!(
        top["target"].as_str(),
        Some("socket"),
        "context.v1: the frame names the target it narrows to, got {top:?}"
    );
    assert!(
        top["identity"]
            .as_str()
            .is_some_and(|identity| identity.contains(&port.to_string())),
        "context.v1: the frame identifies the socket the way the prompt shows it, by its \
         endpoint 127.0.0.1:{port}, got {top:?}"
    );
}

#[test]
fn should_trace_the_entered_socket_without_a_selector() {
    let (_listener, port) = listener();
    // Spec §14.3: inside the socket frame, `trace socket` needs no `--port` — the frame's
    // implicit selector names the socket, so the graph starts from the listener.
    let run = ono(&format!(
        "get socket {port} | enter socket; trace socket | to json"
    ));
    run.assert_success();
    assert!(
        !run.stderr().contains("Ono-Sendai-E"),
        "entering the listener is an ordinary operation and the trace inside needs no selector, \
         got {:?}",
        run.stderr()
    );
    let graph = single(&run);
    assert_eq!(
        graph["root"]["schema"].as_str(),
        Some("ono.socket/1"),
        "graph.v1: the trace starts from the entered socket, got {graph:?}"
    );
    assert!(
        node_where(&graph, "ono.socket/1", |socket| {
            socket["local"]["port"].as_u64() == Some(u64::from(port))
                && socket["state"].as_str() == Some("listen")
        })
        .is_some_and(|node| node["id"] == graph["root"]),
        "the root is the test's own listener on port {port}, not some other socket, got {graph:?}"
    );
}

// --- route write paths ------------------------------------------------------------------------

#[test]
fn should_report_a_permission_failure_when_adding_a_route_unprivileged() {
    if !unprivileged() {
        return;
    }
    let script = "add route 10.99.0.0/24 --gateway 127.0.0.1 --interface lo";
    assert_explained_as_privileged_mutation(script, "mutate");
    assert_refused_by_the_kernel(script, "ono.route.add");

    let after = ono("get route | to json");
    after.assert_success();
    assert!(
        !after.stdout().contains("10.99.0.0/24"),
        "a refused addition leaves the routing table as it was, got {:?}",
        after.stdout()
    );
}

#[test]
fn should_report_a_permission_failure_when_removing_a_route_unprivileged() {
    if !unprivileged() {
        return;
    }
    let script = "remove route 10.99.0.0/24";
    assert_explained_as_privileged_mutation(script, "mutate");
    assert_refused_by_the_kernel(script, "ono.route.remove");
}

#[test]
fn should_report_a_permission_failure_when_setting_a_route_unprivileged() {
    if !unprivileged() {
        return;
    }
    let script = "set route 10.99.0.0/24 --metric 200";
    assert_explained_as_privileged_mutation(script, "mutate");
    assert_refused_by_the_kernel(script, "ono.route.set");
}

// --- interface write paths --------------------------------------------------------------------

fn loopback() -> Value {
    let run = ono("get interface lo | to json");
    run.assert_success();
    single(&run)
}

#[test]
fn should_report_a_permission_failure_when_setting_an_interface_unprivileged() {
    if !unprivileged() {
        return;
    }
    let before = loopback();
    let script = "set interface lo --mtu 1500";
    assert_explained_as_privileged_mutation(script, "mutate");
    assert_refused_by_the_kernel(script, "ono.interface.set");
    assert_eq!(
        loopback()["mtu"],
        before["mtu"],
        "a refused change leaves the MTU as it was (spec §28.5)"
    );
}

#[test]
fn should_report_a_permission_failure_when_starting_an_interface_unprivileged() {
    if !unprivileged() {
        return;
    }
    let script = "start interface lo";
    assert_explained_as_privileged_mutation(script, "mutate");
    assert_refused_by_the_kernel(script, "ono.interface.start");
}

#[test]
fn should_report_a_permission_failure_when_stopping_an_interface_unprivileged() {
    if !unprivileged() {
        return;
    }
    let before = loopback();
    let script = "stop interface lo";
    assert_explained_as_privileged_mutation(script, "mutate");
    assert_refused_by_the_kernel(script, "ono.interface.stop");
    assert_eq!(
        loopback()["state"],
        before["state"],
        "a refused stop leaves the loopback interface in the state it had (spec §28.5)"
    );
}

#[test]
fn should_act_on_the_piped_interface_when_a_record_arrives_instead_of_a_selector() {
    // The object-in spelling of spec §11.5 and §14.3: the objects the pipeline carries are the
    // objects the mutation acts on. `stop interface` refused the record as content it could not
    // write, because its contract declared no stream input at all.
    if !unprivileged() {
        return;
    }
    let row =
        assert_refused_by_the_kernel("get interface lo | stop interface", "ono.interface.stop");
    assert!(
        text(&row, "target").contains("lo"),
        "the piped interface is the target the mutation acted on, got {row:?}"
    );
}

#[test]
fn should_act_on_the_piped_interface_when_a_record_arrives_instead_of_a_selector_for_start() {
    if !unprivileged() {
        return;
    }
    let row =
        assert_refused_by_the_kernel("get interface lo | start interface", "ono.interface.start");
    assert!(
        text(&row, "target").contains("lo"),
        "the piped interface is the target the mutation acted on, got {row:?}"
    );
}

#[test]
fn should_report_a_permission_failure_when_adding_an_interface_unprivileged() {
    if !unprivileged() {
        return;
    }
    let script = "add interface ono-dummy0 --kind dummy";
    assert_explained_as_privileged_mutation(script, "mutate");
    assert_refused_by_the_kernel(script, "ono.interface.add");

    let after = ono("get interface | to json");
    after.assert_success();
    assert!(
        !after.stdout().contains("ono-dummy0"),
        "a refused creation leaves no interface behind, got {:?}",
        after.stdout()
    );
}

#[test]
fn should_report_a_permission_failure_when_removing_an_address_unprivileged() {
    if !unprivileged() {
        return;
    }
    // The address form of the contract's own example (`remove interface eth0 --address …`),
    // against the one address every machine has.
    let script = "remove interface lo --address 127.0.0.1/8";
    assert_explained_as_privileged_mutation(script, "mutate");
    assert_refused_by_the_kernel(script, "ono.interface.remove");
    assert!(
        loopback()["addresses"]
            .as_sequence()
            .is_some_and(|addresses| addresses.iter().any(|a| a.as_str() == Some("127.0.0.1/8"))),
        "a refused removal leaves the loopback address in place (spec §28.5)"
    );
}

// --- stop socket ------------------------------------------------------------------------------

#[test]
fn should_refuse_to_stop_a_socket_in_a_script_without_confirm() {
    let (_listener, port) = listener();
    // network.yaml marks closing a socket destructive and gives `--confirm` for the
    // non-interactive case; spec §17.4 forbids a script from waiting for a prompt, so without
    // the flag it fails with `safety.confirmation_required` and closes nothing.
    let run = ono(&format!("get socket {port} | stop socket | to json"));
    assert!(
        !run.status().is_success(),
        "spec §17.4: a destructive policy violation fails, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0701"),
        "spec §17.4: the refusal is `safety.confirmation_required`, got {:?}",
        run.stderr()
    );
    assert!(
        accepts_connections(port),
        "a refused stop leaves the listener serving"
    );
}

#[test]
fn should_report_a_permission_failure_when_stopping_a_socket_unprivileged_with_confirm() {
    if !unprivileged() {
        return;
    }
    let (_listener, port) = listener();
    assert_explained_as_privileged_mutation("get socket | stop socket --confirm", "destructive");
    // Destroying a socket over sock_diag needs CAP_NET_ADMIN even for one's own listener.
    let row = assert_refused_by_the_kernel(
        &format!("get socket {port} | stop socket --confirm"),
        "ono.socket.stop",
    );
    let target = serde_yaml_ng::to_string(&row["target"]).expect("serialise the target");
    assert!(
        target.contains(&port.to_string()),
        "spec §11.5: the row's `target` references the socket it acted on, got {row:?}"
    );
    assert!(
        accepts_connections(port),
        "a refused stop leaves the listener serving"
    );
}

// --- `--dry-run` on the network write paths (ADR-0238) ----------------------------------------

/// Runs a network mutation with `--dry-run` and asserts the one honest outcome: `skipped`,
/// nothing changed, exit 0, and a message saying what would have been sent.
fn assert_skipped_by_the_dry_run(script: &str, operation: &str, says: &str) {
    let run = ono(&format!("{script} --dry-run | to json"));
    assert!(
        !run.stderr().contains("Ono-Sendai-E0202"),
        "`--dry-run` is an option the mutation road has always honoured, so it must be declared \
         and accepted; got {:?}",
        run.output()
    );
    run.assert_success();
    let row = single(&run);
    assert_eq!(
        row["status"].as_str(),
        Some("skipped"),
        "spec §11.6: asking without obeying answers `skipped`, got {row:?}"
    );
    assert_eq!(
        row["changed"].as_bool(),
        Some(false),
        "a dry run changes nothing, got {row:?}"
    );
    assert_eq!(row["operation"].as_str(), Some(operation));
    let message = row["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(says),
        "the row says what would have happened, not merely that nothing did: expected \
         {says:?} in {message:?}"
    );
}

#[test]
fn should_answer_a_dry_run_of_a_route_addition_without_asking_the_kernel_for_it() {
    // The mutation road has honoured `Action::is_dry_run` since ADR-0068, and every network
    // write path checks it — and no command declared the option, so no user could reach any of
    // it (ADR-0238). Unprivileged is the point: a dry run never touches the kernel, so it
    // succeeds where the real thing is refused.
    assert_skipped_by_the_dry_run(
        "add route 10.99.250.0/24 --gateway 10.99.250.1",
        "ono.route.add",
        "would add the route 10.99.250.0/24",
    );
}

#[test]
fn should_answer_a_dry_run_of_an_interface_change_without_asking_the_kernel_for_it() {
    assert_skipped_by_the_dry_run(
        "set interface lo --mtu 9000",
        "ono.interface.set",
        "would set the MTU to 9000",
    );
}

#[test]
fn should_still_refuse_the_same_mutation_when_it_is_not_a_dry_run() {
    // The dry run is the only thing that changed: without it the kernel still refuses an
    // unprivileged caller, and the refusal is still the kernel's (ADR-0088).
    assert_refused_by_the_kernel("set interface lo --mtu 9000", "ono.interface.set");
}

// --- `resolve dns --server` (ADR-0240) ---------------------------------------------------------

/// A nameserver that answers exactly one question, on a port the kernel chooses.
///
/// It is the outside world, faked the way AGENTS.md §11 allows the outside world to be faked: a
/// real UDP socket speaking real DNS, so what is under test is the shell's client and not a
/// stand-in for it. `answer` is the RDATA to return, and `record_type` the QTYPE it answers.
fn nameserver_answering(record_type: u16, answer: Vec<u8>) -> (u16, std::thread::JoinHandle<()>) {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("a fixture nameserver socket");
    let port = socket.local_addr().expect("the bound port").port();
    socket
        .set_read_timeout(Some(Duration::from_secs(20)))
        .expect("a read timeout, so a test never hangs on a question that never comes");
    let handle = std::thread::spawn(move || {
        let mut buffer = [0u8; 512];
        while let Ok((read, from)) = socket.recv_from(&mut buffer) {
            let request = &buffer[..read];
            // The question ends at the root label, four bytes of QTYPE and QCLASS later.
            let Some(root) = request.iter().skip(12).position(|byte| *byte == 0) else {
                continue;
            };
            let question_end = 12 + root + 1 + 4;
            let asked = u16::from_be_bytes([request[question_end - 4], request[question_end - 3]]);
            let mut reply = request[..question_end].to_vec();
            reply[2] = 0x81; // QR=1, RD=1
            reply[3] = 0x80; // RA=1, RCODE=0
            let answers = u16::from(asked == record_type);
            reply[6..8].copy_from_slice(&answers.to_be_bytes());
            if answers == 1 {
                reply.extend_from_slice(&[0xc0, 0x0c]); // owner: pointer to the question's name
                reply.extend_from_slice(&record_type.to_be_bytes());
                reply.extend_from_slice(&1_u16.to_be_bytes()); // CLASS IN
                reply.extend_from_slice(&60_u32.to_be_bytes()); // TTL
                let length = u16::try_from(answer.len()).expect("the fixture RDATA is short");
                reply.extend_from_slice(&length.to_be_bytes());
                reply.extend_from_slice(&answer);
            }
            if socket.send_to(&reply, from).is_err() {
                return;
            }
            return;
        }
    });
    (port, handle)
}

#[test]
fn should_answer_from_the_nameserver_that_was_named_rather_than_from_the_system_resolver() {
    // ADR-0087 refused `--server` because the system resolver cannot be pointed at a server, and
    // ADR-0240 gives the crate a DNS client of its own. `fixture.example` exists in no zone
    // anywhere: an answer for it can only have come from the server the query named.
    let (port, server) = nameserver_answering(1, vec![203, 0, 113, 7]);

    let run = ono(&format!(
        "resolve dns fixture.example --server 127.0.0.1 --port {port} --type A | to json"
    ));
    run.assert_success();
    let row = single(&run);
    assert_eq!(row["name"].as_str(), Some("fixture.example"));
    assert_eq!(row["type"].as_str(), Some("A"));
    assert_eq!(
        row["address"].as_str(),
        Some("203.0.113.7"),
        "the address is the one that nameserver answered, got {row:?}"
    );
    let _ = server.join();
}

#[test]
fn should_answer_a_reverse_question_from_the_named_nameserver() {
    // An address asks for its name (network.yaml), and over a named server that is a `PTR`
    // question in the `in-addr.arpa` zone. The RDATA is the encoded name `host.fixture`.
    let name = vec![
        4, b'h', b'o', b's', b't', 7, b'f', b'i', b'x', b't', b'u', b'r', b'e', 0,
    ];
    let (port, server) = nameserver_answering(12, name);

    let run = ono(&format!(
        "resolve dns 203.0.113.7 --server 127.0.0.1 --port {port} | to json"
    ));
    run.assert_success();
    let row = single(&run);
    assert_eq!(row["type"].as_str(), Some("PTR"));
    assert_eq!(
        row["name"].as_str(),
        Some("host.fixture"),
        "a PTR answers an address with a name, got {row:?}"
    );
    assert_eq!(row["address"].as_str(), Some("203.0.113.7"));
    let _ = server.join();
}

#[test]
fn should_report_a_nameserver_that_does_not_answer_as_unavailable_and_retryable() {
    // A server that is named and does not answer is a failure of that server, not an empty
    // result: `--server` has no second server to fall back to (spec §16.5, §35.3).
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("a socket nobody reads");
    let port = socket.local_addr().expect("the bound port").port();
    drop(socket);

    let run = ono(&format!(
        "resolve dns fixture.example --server 127.0.0.1 --port {port} --type A | to json"
    ));
    assert!(
        !run.status().is_success(),
        "a nameserver that says nothing is not an answer of nothing, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0401"),
        "errors.yaml: an unreachable provider is provider.unavailable, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_refuse_a_port_without_a_server_to_go_with_it() {
    let run = ono("resolve dns example.com --port 5353 | to json");
    assert!(
        !run.status().is_success() && run.stderr().contains("Ono-Sendai-E0201"),
        "`--port` says where a named nameserver listens; without `--server` it says nothing, \
         got {:?}",
        run.output()
    );
}
