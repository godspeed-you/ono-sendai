//! `map links --depth 2`: each linked host's own links, as its agent reports them (v0.4 §19.3,
//! §19.4, issue #310, ADR-1097).
//!
//! Every peer here is a real `ono --agent --listen` process on its own loopback address with a
//! home of its own — its own identity, trust store and authorization store — and a peer that
//! holds links onward holds them because it was started with `--link`. The topologies are the
//! ones a federated map has to survive: a chain, a cycle back to this host, one host reached two
//! ways, a peer that refuses its table, a peer that is down or frozen, and a link that comes back.

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
use support::peers::{Peer, holding, pin};

/// A client home that has pinned and is authorized by each of `peers`.
fn client_of(peers: &[&Peer]) -> Scratch {
    let home = scratch();
    for peer in peers {
        pin(&home, peer);
        peer.authorize(&home);
    }
    home
}

/// `map links` at `depth` after linking to each of `peers`, as the one JSON document it writes.
fn map(home: &Scratch, peers: &[&Peer], depth: u8) -> Value {
    let links: Vec<String> = peers
        .iter()
        .map(|peer| format!("link host {} --transport tcp", peer.address))
        .collect();
    let run = support::ono_at_home(
        home,
        &format!("{}; map links --depth {depth} --json", links.join("; ")),
    );
    run.assert_success();
    support::json_lines(&run)
        .pop()
        .unwrap_or_else(|| panic!("map links writes one document, got {:?}", run.stdout()))
}

fn nodes(map: &Value) -> Vec<Value> {
    map["nodes"].as_array().cloned().unwrap_or_default()
}

fn edges(map: &Value) -> Vec<Value> {
    map["edges"].as_array().cloned().unwrap_or_default()
}

/// The edges a far side reported (hop 2).
fn far_edges(map: &Value) -> Vec<Value> {
    edges(map)
        .into_iter()
        .filter(|edge| edge["evidence"]["hop"] == 2)
        .collect()
}

fn label_of(map: &Value, id: &Value) -> String {
    nodes(map)
        .iter()
        .find(|node| node["id"] == *id)
        .and_then(|node| node["label"].as_str())
        .unwrap_or_default()
        .to_owned()
}

fn federation_of(map: &Value, link: &str) -> Value {
    map["federation"]
        .as_array()
        .and_then(|asked| asked.iter().find(|entry| entry["link"] == link))
        .cloned()
        .unwrap_or_else(|| panic!("the map says what `{link}` answered, got {map}"))
}

#[test]
fn should_draw_a_second_hop_with_the_far_sides_confidence_and_provenance() {
    // A → B → C: this session links to B, and B holds a link to C. The map draws C behind B,
    // `inferred` here — this session observed nothing of it — with B's own claim beside it and B
    // named as the one who reported it.
    let c = Peer::start("127.0.0.4", &[]);
    let b = holding("127.0.0.3", &[("c", &c)]);
    let home = client_of(&[&b]);

    let drawn = map(&home, &[&b], 2);
    let far = far_edges(&drawn);
    assert_eq!(far.len(), 1, "one far link, got {far:#?}");
    let edge = &far[0];
    assert_eq!(
        edge["confidence"], "inferred",
        "§19.4: one-sided, got {edge}"
    );
    assert_eq!(edge["evidence"]["reported_confidence"], "exact");
    assert_eq!(edge["evidence"]["reported_by"], b.address);
    assert_eq!(edge["evidence"]["verified"], false);
    assert_eq!(edge["evidence"]["state"], "connected");
    assert_eq!(
        edge["evidence"]["fingerprint"], c.fingerprint,
        "the far side says which key the next hop proved to it"
    );
    assert!(
        edge["provenance"].to_string().contains(&b.address),
        "the provenance names where the observation came from, got {}",
        edge["provenance"]
    );
    assert_eq!(
        label_of(&drawn, &edge["target"]),
        format!("{}/c", b.address),
        "a host this session holds no link to is named through the peer that reported it"
    );
    let asked = federation_of(&drawn, &b.address);
    assert_eq!(asked["status"], "listed");
    assert_eq!(asked["count"], 1);
}

#[test]
fn should_ask_nobody_for_their_links_at_the_default_depth() {
    let c = Peer::start("127.0.0.5", &[]);
    let b = holding("127.0.0.6", &[("c", &c)]);
    let home = client_of(&[&b]);

    let drawn = map(&home, &[&b], 1);
    assert!(
        far_edges(&drawn).is_empty(),
        "§19.3: depth 1 stops at this session's own links"
    );
    assert!(
        drawn["federation"].is_null(),
        "nobody was asked, got {}",
        drawn["federation"]
    );
}

#[test]
fn should_refuse_a_third_hop_with_the_bound_and_its_reason() {
    let run = support::ono_at_home(
        &scratch(),
        "try { map links --depth 3 } catch e { $e | to json }",
    );
    run.assert_success();
    let line = support::last_line(&run);
    let refusals: Vec<Value> = serde_json::from_str(&line)
        .unwrap_or_else(|_| panic!("the refusal as data, got {:?}", run.stdout()));
    let refusal = &refusals[0];
    assert_eq!(refusal["name"], "type.mismatch", "got {refusal}");
    assert!(
        refusal["help"]
            .as_str()
            .is_some_and(|help| help.contains("asks nobody further")),
        "the refusal says why there is no third hop, got {refusal}"
    );
}

#[test]
fn should_say_not_permitted_rather_than_no_links_when_the_peer_refuses_its_table() {
    // The far side's links are its information: a client B authorizes to connect but not to
    // observe is told it may not see them — and the map says exactly that, not "no links".
    let c = Peer::start("127.0.0.7", &[]);
    let b = holding("127.0.0.8", &[("c", &c)]);
    let home = scratch();
    pin(&home, &b);
    b.authorize_without_observe(&home);

    let drawn = map(&home, &[&b], 2);
    assert!(
        far_edges(&drawn).is_empty(),
        "nothing of B's table was drawn"
    );
    let asked = federation_of(&drawn, &b.address);
    assert_eq!(asked["status"], "not_permitted", "got {asked}");
    assert_eq!(asked["code"], "remote.capability_denied");
    b.wait_for("event=authorization.denied", 1);
    assert!(
        b.log().contains("requested_capability=link.list"),
        "the agent records the refusal under the capability `get link` declares"
    );

    let shown = support::ono_at_home(
        &home,
        &format!(
            "link host {} --transport tcp; map links --depth 2",
            b.address
        ),
    );
    assert!(
        shown.stdout().contains("not permitted to see its links"),
        "the text map says it too, got {}",
        shown.stdout()
    );
}

#[test]
fn should_close_a_cycle_back_to_this_host_instead_of_drawing_a_new_place() {
    // A ↔ B: this shell's home also runs a listening agent (one identity for both), and B holds
    // a link back to it. B's report names this host's own key, so the far edge returns to the
    // centre of the map.
    let home = scratch();
    let a = Peer::start_in(home, "127.0.0.9", &[]);
    let b = holding("127.0.0.10", &[("a", &a)]);
    pin(&a.home, &b);
    b.authorize(&a.home);

    let drawn = map(&a.home, &[&b], 2);
    let far = far_edges(&drawn);
    assert_eq!(far.len(), 1, "got {far:#?}");
    assert_eq!(
        far[0]["target"], drawn["center"],
        "a far link back to this host leads to this host, got {far:#?}"
    );
    assert_eq!(far[0]["evidence"]["back_to_this_host"], true);
    assert_eq!(
        nodes(&drawn).len(),
        2,
        "this host and B — no third place for this host under another name"
    );
}

#[test]
fn should_draw_one_host_reached_two_ways_once() {
    // A links to B and to C itself; B also holds a link to C. B's report names the key C proved,
    // which is the key this session's own link to C proved: the far edge joins the place the map
    // already has, and says which link of this session reaches it.
    let c = Peer::start("127.0.0.11", &[]);
    let b = holding("127.0.0.12", &[("c", &c)]);
    let home = client_of(&[&b, &c]);

    let drawn = map(&home, &[&b, &c], 2);
    let far = far_edges(&drawn);
    let via_b: Vec<&Value> = far
        .iter()
        .filter(|edge| edge["evidence"]["reported_by"] == b.address)
        .collect();
    assert_eq!(via_b.len(), 1, "got {far:#?}");
    assert_eq!(via_b[0]["evidence"]["same_peer_as"], c.address);
    assert_eq!(
        label_of(&drawn, &via_b[0]["target"]),
        c.address,
        "the far edge ends at the place this session's own link to C drew"
    );
    assert_eq!(nodes(&drawn).len(), 3, "this host, B and C — C once");
    // Asymmetric: C holds no links, and says so — an empty table, which is not a refusal.
    let asked = federation_of(&drawn, &c.address);
    assert_eq!(asked["status"], "listed");
    assert_eq!(asked["count"], 0);
}

#[test]
fn should_draw_a_far_host_two_peers_report_as_one_place() {
    // B and D both hold a link to C, which this session does not link to: two observations of
    // one host over two paths. C is identified by the key it proved to both, so it is one place
    // with two edges.
    let c = Peer::start("127.0.0.13", &[]);
    let b = holding("127.0.0.14", &[("c", &c)]);
    let d = holding("127.0.0.15", &[("c", &c)]);
    let home = client_of(&[&b, &d]);

    let drawn = map(&home, &[&b, &d], 2);
    let far = far_edges(&drawn);
    assert_eq!(far.len(), 2, "two reports, got {far:#?}");
    assert_eq!(far[0]["target"], far[1]["target"], "one place for C");
    assert_eq!(nodes(&drawn).len(), 4, "this host, B, D and C once");
}

#[test]
fn should_report_a_far_link_that_is_down_as_down_and_bring_it_back() {
    // B holds a link to C. C goes away: B reports the link disconnected — its own claim is then
    // the user's declaration and nothing more. C comes back on the same address with the same
    // key: B makes the link again and reports it connected.
    let mut c = Peer::start("127.0.0.16", &[]);
    let b = holding("127.0.0.17", &[("c", &c)]);
    let home = client_of(&[&b]);

    c.kill();
    b.wait_for("went down", 1);
    let drawn = map(&home, &[&b], 2);
    let far = far_edges(&drawn);
    assert_eq!(far[0]["evidence"]["state"], "disconnected", "got {far:#?}");
    assert_eq!(far[0]["evidence"]["reported_confidence"], "user_declared");
    assert!(
        far[0]["evidence"]["latency"].is_null(),
        "a down link has no latency"
    );

    c.restart();
    b.wait_for("connected to", 2);
    let drawn = map(&home, &[&b], 2);
    let far = far_edges(&drawn);
    assert_eq!(far[0]["evidence"]["state"], "connected", "got {far:#?}");
    assert!(
        far[0]["evidence"]["latency"].is_string(),
        "B probed the remade link when it was asked, got {far:#?}"
    );
}

#[test]
fn should_bound_the_second_hop_when_a_peer_answers_nothing() {
    // B is frozen after the link is made: it holds the connection and answers nothing. The map
    // still arrives, within the probe's and the table request's bounds, and says B could not be
    // asked; another peer's answer is unaffected.
    let c = Peer::start("127.0.0.18", &[]);
    let b = holding("127.0.0.19", &[("c", &c)]);
    let e = Peer::start("127.0.0.20", &[]);
    let home = client_of(&[&b, &e]);

    let pid = b.pid().expect("B runs");
    // `kill` is a native verb of the shell; the program is reached by its path.
    let kill = ["/usr/bin/kill", "/bin/kill"]
        .into_iter()
        .find(|path| std::path::Path::new(path).exists())
        .expect("a kill program");
    let started = std::time::Instant::now();
    let run = support::ono_at_home(
        &home,
        &format!(
            "link host {} --transport tcp; link host {} --transport tcp; {kill} -STOP {pid}; \
             map links --depth 2 --json; {kill} -CONT {pid}",
            b.address, e.address
        ),
    );
    let took = started.elapsed();
    b.thaw();
    run.assert_success();
    let drawn = support::json_lines(&run).pop().expect("the map");
    assert_eq!(federation_of(&drawn, &b.address)["status"], "unavailable");
    assert_eq!(federation_of(&drawn, &e.address)["status"], "listed");
    assert!(
        took < std::time::Duration::from_secs(30),
        "a peer that answers nothing costs the map its bounds, not a hang: {took:?}"
    );
}
