//! `map links --connections`: which of this host's connections reach a host this session links
//! to, on what evidence (v0.4 §14.5, §19.4, §43.3, issue #309, ADR-1098).
//!
//! Real peers — `ono --agent --listen` on their own loopback addresses, with homes and identities
//! of their own — and real TCP connections: the shell's own link to a peer, and one the test opens
//! to a listener it owns. Every peer on one machine shares the kernel's socket table and its boot,
//! so every peer holds the mirror of every connection and is proven to be this machine: a single
//! link shows the shell's wiring end to end, two that the map says "ambiguous" instead of picking
//! one.
//!
//! Those peers cannot show the evidence being weighed, because they always hold it. The second
//! half of the suite links far agents whose connection tables and boot the test writes
//! (`support::far_side`): the mirror held or not, the same service with another client, a
//! loopback mirror from another machine, and a partial table (review TST-1, ADR-1098). What no
//! test here proves is two real machines: a non-loopback 4-tuple is graded by the unit contract
//! in `correlate.rs`.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "remote")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::{Scratch, scratch};
use serde_json::Value;

mod support;
use support::peers::{Peer, pin};

fn client_of(peers: &[&Peer]) -> Scratch {
    let home = scratch();
    for peer in peers {
        pin(&home, peer);
        peer.authorize(&home);
    }
    home
}

fn correlated_map(home: &Scratch, peers: &[&Peer]) -> Value {
    let links: Vec<String> = peers
        .iter()
        .map(|peer| format!("link host {} --transport tcp", peer.address))
        .collect();
    let run = support::ono_at_home(
        home,
        &format!("{}; map links --connections --json", links.join("; ")),
    );
    run.assert_success();
    support::json_lines(&run)
        .pop()
        .unwrap_or_else(|| panic!("map links writes one document, got {:?}", run.stdout()))
}

fn entries(map: &Value, subject: &str) -> Vec<Value> {
    map["correlations"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry["subject"] == subject)
        .collect()
}

fn reaching(map: &Value) -> Vec<Value> {
    map["edges"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|edge| edge["relation"] == "connection.reaches_host")
        .collect()
}

#[test]
fn should_correlate_a_connection_with_the_linked_host_whose_socket_mirrors_it() {
    // A real connection the test owns: a listener on 127.0.0.31 and a client connected to it.
    // The far agent's socket table holds the mirror, so the connection reaches that host on two
    // sides' evidence: `strong`, with the evidence on the edge.
    let far = Peer::start("127.0.0.30", &[]);
    let home = client_of(&[&far]);
    let listener = std::net::TcpListener::bind("127.0.0.31:0").expect("a listener");
    let port = listener.local_addr().expect("bound").port();
    let client = std::net::TcpStream::connect(("127.0.0.31", port)).expect("connected");
    let (_accepted, _) = listener.accept().expect("accepted");
    let ours = client.local_addr().expect("the client's end");

    let map = correlated_map(&home, &[&far]);
    let edges = reaching(&map);
    let wanted = format!("{ours}");
    let edge = edges
        .iter()
        .find(|edge| edge["evidence"]["local"] == wanted.as_str())
        .unwrap_or_else(|| panic!("the test's connection {wanted} is correlated, got {edges:#?}"));
    assert_eq!(
        edge["confidence"], "strong",
        "two sides observed it, got {edge}"
    );
    assert_eq!(edge["evidence"]["basis"], "two_sided");
    assert_eq!(edge["evidence"]["link"], far.address);
    assert_eq!(edge["evidence"]["remote"], format!("127.0.0.31:{port}"));
    assert!(
        edge["evidence"]["far_socket"].is_string(),
        "the edge names the far socket it mirrors, got {edge}"
    );
    assert_eq!(edge["provider"], "ono.link.correlate");
    let link_read = entries(&map, "link");
    assert_eq!(link_read[0]["status"], "read", "got {link_read:#?}");
}

#[test]
fn should_say_ambiguous_and_draw_nothing_when_two_linked_hosts_hold_the_same_evidence() {
    // Two peers on one kernel both hold the mirror of every connection: the evidence cannot tell
    // them apart, so the map says so, names both, and draws no edge for either.
    let one = Peer::start("127.0.0.32", &[]);
    let two = Peer::start("127.0.0.33", &[]);
    let home = client_of(&[&one, &two]);
    let listener = std::net::TcpListener::bind("127.0.0.34:0").expect("a listener");
    let port = listener.local_addr().expect("bound").port();
    let client = std::net::TcpStream::connect(("127.0.0.34", port)).expect("connected");
    let (_accepted, _) = listener.accept().expect("accepted");
    let ours = format!("{}", client.local_addr().expect("the client's end"));

    let map = correlated_map(&home, &[&one, &two]);
    assert!(
        !reaching(&map)
            .iter()
            .any(|edge| edge["evidence"]["local"] == ours.as_str()),
        "an ambiguous connection is not resolved by picking one"
    );
    let said = entries(&map, "connection");
    let ambiguous = said
        .iter()
        .find(|entry| {
            entry["connection"]
                .as_str()
                .is_some_and(|text| text.contains(&ours))
        })
        .unwrap_or_else(|| panic!("the connection is reported, got {said:#?}"));
    assert_eq!(ambiguous["status"], "ambiguous");
    let links = ambiguous["links"].as_array().cloned().unwrap_or_default();
    assert!(
        links.contains(&Value::from(one.address.clone()))
            && links.contains(&Value::from(two.address.clone())),
        "both candidates are named, got {ambiguous}"
    );
}

#[test]
fn should_report_a_far_side_that_refuses_its_socket_table_and_infer_nothing_from_it() {
    let far = Peer::start("127.0.0.35", &[]);
    let home = scratch();
    pin(&home, &far);
    far.authorize_without_observe(&home);

    let map = correlated_map(&home, &[&far]);
    assert!(
        reaching(&map).is_empty(),
        "nothing is inferred from a table not read"
    );
    let read = entries(&map, "link");
    assert_eq!(read[0]["status"], "not_permitted", "got {read:#?}");
}

#[test]
fn should_read_no_far_table_unless_correlation_is_asked_for() {
    let far = Peer::start("127.0.0.36", &[]);
    let home = client_of(&[&far]);
    let run = support::ono_at_home(
        &home,
        &format!(
            "link host {} --transport tcp; map links --json",
            far.address
        ),
    );
    run.assert_success();
    let map = support::json_lines(&run).pop().expect("the map");
    assert!(map["correlations"].is_null(), "got {}", map["correlations"]);
    assert!(reaching(&map).is_empty());
}

// --- A far side whose tables the test writes (review TST-1) ---------------------------------
//
// The suites above link real agents, and every real agent here reads this kernel's socket table:
// it always holds the mirror, so they cannot fail for the reason they exist. These link far
// agents whose connection tables are what the test says they are, and a connection the test
// owns, so the far side holding the mirror — or not — is the variable.

use std::net::{SocketAddr, TcpListener, TcpStream};

use support::far_side::{Boot, FarAgent, Tables};

/// A connection the test owns: a listener on `host` and a client connected to it.
struct Owned {
    /// The client's end — this host's local end of the connection.
    here: SocketAddr,
    /// The listener's end — the connection's remote end.
    there: SocketAddr,
    _streams: (TcpStream, TcpStream, TcpListener),
}

fn owned_connection(host: &str) -> Owned {
    let listener = TcpListener::bind((host, 0)).expect("a listener");
    let there = listener.local_addr().expect("bound");
    let client = TcpStream::connect(there).expect("connected");
    let (accepted, _) = listener.accept().expect("accepted");
    Owned {
        here: client.local_addr().expect("the client's end"),
        there,
        _streams: (client, accepted, listener),
    }
}

fn tables(sockets: Vec<(SocketAddr, SocketAddr)>, boot: Boot) -> Tables {
    Tables {
        sockets,
        padding: 0,
        boot,
    }
}

fn map_with(home: &Scratch, far: &[&FarAgent]) -> Value {
    let links: Vec<String> = far
        .iter()
        .map(|agent| format!("link host {} --transport tcp", agent.address))
        .collect();
    let run = support::ono_at_home(
        home,
        &format!("{}; map links --connections --json", links.join("; ")),
    );
    run.assert_success();
    support::json_lines(&run)
        .pop()
        .unwrap_or_else(|| panic!("map links writes one document, got {:?}", run.stdout()))
}

fn edge_from(map: &Value, here: SocketAddr) -> Option<Value> {
    reaching(map)
        .into_iter()
        .find(|edge| edge["evidence"]["local"] == here.to_string().as_str())
}

#[test]
fn should_draw_no_edge_when_the_far_side_does_not_hold_the_mirror() {
    // The far side is this machine (its boot is this kernel's), so a loopback mirror would be
    // evidence — and it holds none: the connection reaches nothing it can show.
    let ours = owned_connection("127.0.0.51");
    let unrelated: SocketAddr = "127.0.0.52:7000".parse().expect("an end");
    let client = scratch();
    let far = FarAgent::start(
        "127.0.0.50",
        &client,
        tables(
            vec![(unrelated, "127.0.0.1:7001".parse().expect("an end"))],
            Boot::ThisMachine,
        ),
    );
    support::far_side::pin(&client, &far);

    let map = map_with(&client, &[&far]);
    assert_eq!(
        entries(&map, "link")[0]["status"],
        "read",
        "the far tables were read, got {}",
        map["correlations"]
    );
    assert!(
        edge_from(&map, ours.here).is_none(),
        "a far side that does not hold the mirror of {} → {} gives no edge, got {:#?}",
        ours.here,
        ours.there,
        reaching(&map)
    );
}

#[test]
fn should_not_take_the_same_service_with_another_client_as_two_sided_evidence() {
    // The far side serves the very endpoint this connection reaches — to a different client.
    let ours = owned_connection("127.0.0.54");
    let client = scratch();
    let far = FarAgent::start(
        "127.0.0.53",
        &client,
        tables(
            vec![(ours.there, "127.0.0.1:1".parse().expect("an end"))],
            Boot::ThisMachine,
        ),
    );
    support::far_side::pin(&client, &far);

    let map = map_with(&client, &[&far]);
    assert!(
        edge_from(&map, ours.here).is_none(),
        "another client's socket on the same service is not this connection's mirror, got {:#?}",
        reaching(&map)
    );
}

#[test]
fn should_correlate_on_two_sides_when_the_far_side_holds_the_mirror() {
    let ours = owned_connection("127.0.0.56");
    let client = scratch();
    let far = FarAgent::start(
        "127.0.0.55",
        &client,
        tables(vec![(ours.there, ours.here)], Boot::ThisMachine),
    );
    support::far_side::pin(&client, &far);

    let map = map_with(&client, &[&far]);
    let edge = edge_from(&map, ours.here)
        .unwrap_or_else(|| panic!("the mirrored connection is correlated, got {map}"));
    assert_eq!(edge["confidence"], "strong", "got {edge}");
    assert_eq!(edge["evidence"]["basis"], "two_sided");
    assert_eq!(edge["evidence"]["remote"], ours.there.to_string());
    assert_eq!(
        edge["evidence"]["far_socket"],
        format!("{} → {}", ours.there, ours.here)
    );
}

#[test]
fn should_take_no_loopback_mirror_as_evidence_from_a_far_side_that_is_another_machine() {
    // Every machine has 127.0.0.1: a far side that runs another kernel and reports a loopback
    // socket reports its own, whatever its ports (ADR-1098).
    let ours = owned_connection("127.0.0.58");
    let client = scratch();
    let far = FarAgent::start(
        "127.0.0.57",
        &client,
        tables(vec![(ours.there, ours.here)], Boot::Elsewhere),
    );
    support::far_side::pin(&client, &far);

    let map = map_with(&client, &[&far]);
    assert!(
        edge_from(&map, ours.here).is_none(),
        "a loopback mirror on another machine is no evidence, got {:#?}",
        reaching(&map)
    );
}

#[test]
fn should_draw_no_strong_edge_while_another_links_table_is_partial() {
    // One link holds the mirror; the other's connection table holds more than the reader's
    // bound, so it is cut, and what it did not show — perhaps the same mirror — cannot be
    // excluded.
    let ours = owned_connection("127.0.0.61");
    let client = scratch();
    let holder = FarAgent::start(
        "127.0.0.59",
        &client,
        tables(vec![(ours.there, ours.here)], Boot::ThisMachine),
    );
    let cut = FarAgent::start(
        "127.0.0.60",
        &client,
        Tables {
            sockets: Vec::new(),
            padding: ono_cli::spatial::correlate::MAX_SOCKETS + 1,
            boot: Boot::ThisMachine,
        },
    );
    support::far_side::pin(&client, &holder);
    support::far_side::pin(&client, &cut);

    let map = map_with(&client, &[&holder, &cut]);
    assert!(
        edge_from(&map, ours.here).is_none(),
        "no edge while another table is partial, got {:#?}",
        reaching(&map)
    );
    let said = entries(&map, "connection");
    let entry = said
        .iter()
        .find(|entry| {
            entry["connection"]
                .as_str()
                .is_some_and(|text| text.contains(&ours.here.to_string()))
        })
        .unwrap_or_else(|| panic!("the connection is reported, got {said:#?}"));
    assert_eq!(entry["status"], "ambiguous", "got {entry}");
    assert_eq!(
        entry["partial"],
        serde_json::json!([cut.address]),
        "the partial table is named, got {entry}"
    );
    let read = entries(&map, "link");
    let cut_read = read
        .iter()
        .find(|entry| entry["link"] == cut.address.as_str())
        .unwrap_or_else(|| panic!("the cut link is reported, got {read:#?}"));
    assert_eq!(cut_read["status"], "partial", "got {cut_read}");
}

#[test]
fn should_name_the_request_path_that_answers_whether_a_connection_reaches_a_linked_host() {
    // v0.4.1 §34.3: "If a relationship is described as 'available on request', there MUST
    // actually be a request path" (review COR-1). With a link held, a connection's `hosts` exit
    // is read across the links, which `map links --connections` does; `follow` does not cross a
    // link (§35.4). So the exit, and `follow` by its label, its group or the relation's id —
    // with `--resolve` or without — name that request, never a `--resolve` that pays nothing.
    let far = Peer::start("127.0.0.63", &[]);
    let home = client_of(&[&far]);
    let ours = owned_connection("127.0.0.64");
    let pid = std::process::id();
    let port = ours.here.port();
    let prefix = format!(
        "link host {} --transport tcp; enter process {pid}; follow socket :{port}",
        far.address
    );
    for word in ["host", "hosts", "connection.reaches_host"] {
        for resolve in ["", " --resolve"] {
            let run = support::ono_at_home(
                &home,
                &format!(
                    "{prefix}; try {{ follow {word}{resolve} }} catch e {{ $e | select code \
                     message help | to json }}"
                ),
            );
            run.assert_success();
            assert!(
                run.stderr().is_empty(),
                "the connection place is reached, got {}",
                run.stderr()
            );
            let said = support::last_line(&run);
            assert!(
                said.contains("map links --connections"),
                "`follow {word}{resolve}` names the request that reads it, got {said}; stderr {}",
                run.stderr()
            );
            assert!(
                !said.contains("--resolve` pays"),
                "no `--resolve` that pays for nothing, got {said}"
            );
        }
    }
}
