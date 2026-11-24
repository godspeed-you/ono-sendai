//! `map links --connections`: which of this host's connections reach a host this session links
//! to, on what evidence (v0.4 §14.5, §19.4, §43.3, issue #309, ADR-1098).
//!
//! Real peers — `ono --agent --listen` on their own loopback addresses, with homes and identities
//! of their own — and real TCP connections: the shell's own link to a peer, and one the test opens
//! to a listener it owns. Every peer on one machine shares the kernel's socket table, so every
//! peer holds the mirror of every connection; a single link proves a correlation, two prove that
//! the map says "ambiguous" instead of picking one.

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
