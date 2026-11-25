//! A peer's link table over the link (`ono.link.links/1`, v0.4 §19.3, issue #310, ADR-1097).
//!
//! The far side's links are the far side's information. The request is authorized on the agent
//! as `get link` is — observe — and refused, recorded, where the connection may not observe; a
//! refusal reaches the client as the agent's structured error, never as an empty table. It is
//! extended by ADR-1056's one rule, and a hostile agent's oversized or malformed table is refused
//! before anything of it is believed.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;
use std::time::Duration;

use ono_core::ErrorCode;
use ono_protocol::{
    AuthorizedClient, AuthorizedClients, FrameKind, LINKS_CAPABILITY, LinkEntry, LinkTable,
    MAX_LINK_FIELD_BYTES, MAX_LINK_TABLE_ENTRIES, PROBE_CAPABILITY, ServerAuthorization,
    ServerConfig,
};

mod common;
use common::{
    DemoService, Observed, Recording, client_config, client_key, server_config, tap, within,
};

const WAIT: Duration = Duration::from_secs(5);

/// An agent that announces both protocol features, as this build's does.
fn agent() -> ServerConfig {
    server_config().with_capabilities([
        "process.list",
        "process.signal",
        PROBE_CAPABILITY,
        LINKS_CAPABILITY,
    ])
}

fn entry(name: &str, state: &str) -> LinkEntry {
    LinkEntry {
        name: name.to_owned(),
        host: format!("{name}.example:7022"),
        transport: "tcp".to_owned(),
        state: state.to_owned(),
        fingerprint: Some(format!("sha256:{name}")),
        boot_id: None,
        round_trip_ns: Some(1_000_000),
        latency_ns: None,
        measured_at: None,
        since: None,
    }
}

fn service(table: LinkTable) -> DemoService {
    DemoService::new(Arc::new(Observed::default())).with_links(table)
}

#[tokio::test]
async fn should_carry_the_agents_link_table_as_the_agent_observed_it() {
    let table = LinkTable::of(vec![
        entry("db01", "connected"),
        entry("nas01", "disconnected"),
    ]);
    let tapped = tap::connect(client_config("testhost"), agent(), service(table.clone()))
        .await
        .expect("the handshake succeeds");

    let answered = within(tapped.link.links(WAIT))
        .await
        .expect("an agent that keeps a link table answers with it");
    assert_eq!(answered.links, table.links);
    assert!(!answered.truncated);
    assert!(
        tapped.link.open_streams().is_empty(),
        "an answered request leaves no stream behind"
    );
}

#[tokio::test]
async fn should_say_when_the_agent_left_links_out() {
    let many: Vec<LinkEntry> = (0..MAX_LINK_TABLE_ENTRIES + 10)
        .map(|index| entry(&format!("h{index}"), "connected"))
        .collect();
    let tapped = tap::connect(
        client_config("testhost"),
        agent(),
        service(LinkTable::of(many)),
    )
    .await
    .expect("the handshake succeeds");

    let answered = within(tapped.link.links(WAIT)).await.expect("answered");
    assert_eq!(answered.links.len(), MAX_LINK_TABLE_ENTRIES);
    assert!(
        answered.truncated,
        "a table cut to the bound says it was cut: partial is not complete"
    );
}

#[tokio::test]
async fn should_refuse_locally_when_the_agent_does_not_offer_its_link_table() {
    // An older agent never announces the request; the client must not send a frame kind such an
    // agent would fail the link on.
    let tapped = tap::connect(
        client_config("testhost"),
        server_config(),
        service(LinkTable::default()),
    )
    .await
    .expect("the handshake succeeds");

    let refusal = within(tapped.link.links(WAIT))
        .await
        .expect_err("a request the agent did not offer is refused");
    assert_eq!(refusal.code(), ErrorCode::RemoteProtocolMismatch);
    assert!(refusal.message().contains(LINKS_CAPABILITY));
    assert!(
        !tapped.tap.seen().contains(&FrameKind::ListLinks),
        "nothing was sent, got {:?}",
        tapped.tap.seen()
    );
}

#[tokio::test]
async fn should_refuse_and_record_a_link_table_the_connection_may_not_observe() {
    // §10.2: the table is served under the check `get link` is served under. A connection that
    // is authorized but may not observe is told it is not permitted — a structured refusal, not
    // an empty table — and the agent records the denial (§14.1).
    let audit = Arc::new(Recording::default());
    let store = Arc::new(AuthorizedClients::of([AuthorizedClient::observing(
        client_key().fingerprint(),
    )
    .with_observe(false)]));
    let tapped = tap::connect_proving(
        client_config("testhost"),
        agent()
            .with_authorization(ServerAuthorization::Store(store))
            .with_audit(Arc::clone(&audit) as ono_protocol::Audit),
        service(LinkTable::of(vec![entry("db01", "connected")])),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served");

    assert!(
        tapped.link.offers(LINKS_CAPABILITY),
        "the request is announced to every authorized connection: what it may read is decided \
         when it arrives, so a refusal says `not permitted`, never `not offered`"
    );
    let refusal = within(tapped.link.links(WAIT))
        .await
        .expect_err("a connection that may not observe is refused the table");
    assert_eq!(refusal.code(), ErrorCode::RemoteCapabilityDenied);
    let recorded = audit.lines().join("\n");
    assert!(
        recorded.contains("authorization.denied") && recorded.contains("link.list"),
        "the denial is recorded with the capability `get link` declares, got {recorded}"
    );
    assert!(
        !recorded.contains("db01"),
        "and nothing of the table reaches the audit trail or the peer"
    );
}

#[tokio::test]
async fn should_fail_the_link_as_a_mismatch_when_a_hostile_agent_sends_an_oversized_table() {
    // A table no honest agent builds: more entries than the bound, bypassing `LinkTable::of`.
    let hostile = LinkTable {
        links: (0..MAX_LINK_TABLE_ENTRIES * 4)
            .map(|index| entry(&format!("h{index}"), "connected"))
            .collect(),
        truncated: false,
        observed_at: None,
    };
    let tapped = tap::connect(client_config("testhost"), agent(), service(hostile))
        .await
        .expect("the handshake succeeds");

    let refusal = within(tapped.link.links(WAIT))
        .await
        .expect_err("an oversized table is not believed");
    assert_eq!(refusal.code(), ErrorCode::RemoteProtocolMismatch);
}

#[tokio::test]
async fn should_fail_the_link_as_a_mismatch_when_a_field_is_longer_than_the_bound() {
    let mut long = entry("db01", "connected");
    long.name = "x".repeat(MAX_LINK_FIELD_BYTES + 1);
    let tapped = tap::connect(
        client_config("testhost"),
        agent(),
        service(LinkTable {
            links: vec![long],
            truncated: false,
            observed_at: None,
        }),
    )
    .await
    .expect("the handshake succeeds");

    let refusal = within(tapped.link.links(WAIT))
        .await
        .expect_err("a field over the bound is not believed");
    assert_eq!(refusal.code(), ErrorCode::RemoteProtocolMismatch);
}

#[tokio::test]
async fn should_refuse_a_link_table_payload_that_is_not_one() {
    for payload in [
        &br#"{"links":"nope"}"#[..],
        br#"{"links":[{"name":1}]}"#,
        br#"[]"#,
        br#"null"#,
    ] {
        let decoded = ono_protocol::decode_message(
            FrameKind::LinkTable,
            payload,
            &ono_value::SchemaRegistry::new(),
            &ono_protocol::Limits::default(),
        );
        assert!(
            decoded.is_err(),
            "{} is not a link table",
            String::from_utf8_lossy(payload)
        );
    }
}

#[tokio::test(start_paused = true)]
async fn should_report_a_link_table_that_did_not_arrive_in_time_and_release_it() {
    let tapped = tap::connect(
        client_config("testhost"),
        agent(),
        service(LinkTable::default()),
    )
    .await
    .expect("the handshake succeeds");
    tapped.tap.drop_kind(FrameKind::ListLinks);

    let refusal = tapped
        .link
        .links(Duration::from_millis(500))
        .await
        .expect_err("nobody answered");
    assert_eq!(refusal.code(), ErrorCode::RemoteUnreachable);
    assert!(tapped.link.open_streams().is_empty());
}

#[tokio::test]
async fn should_fail_the_link_as_a_mismatch_when_an_agent_receives_a_request_it_did_not_offer() {
    let tapped = tap::connect(
        client_config("testhost"),
        server_config(),
        service(LinkTable::default()),
    )
    .await
    .expect("the handshake succeeds");
    tapped.tap.inject(ono_protocol::Frame::new(
        FrameKind::ListLinks,
        41,
        b"null".to_vec(),
    ));
    let outcome = within(tapped.server).await.expect("no panic");
    assert_eq!(
        outcome.expect_err("not served").code(),
        ErrorCode::RemoteProtocolMismatch
    );
}

#[tokio::test]
async fn should_forget_a_request_whose_caller_went_away_and_drop_its_late_answer() {
    // A caller that stops waiting — a Ctrl-C during `map links --depth 2`, a `look` given up —
    // drops the future mid-request. The stream must go with it: a late answer is a message that
    // crossed with a cancellation, not a peer overrunning its credit, and it must not take the
    // whole link down (review PRF-1).
    let tapped = tap::connect(
        client_config("testhost"),
        agent(),
        service(LinkTable::of(vec![entry("db01", "connected")])),
    )
    .await
    .expect("the handshake succeeds");
    // The agent does not see the request yet: its answer is held back.
    tapped.tap.drop_kind(FrameKind::ListLinks);
    let id = {
        let asking = tapped.link.links(WAIT);
        tokio::pin!(asking);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), asking.as_mut())
                .await
                .is_err(),
            "nothing has answered yet"
        );
        let open = tapped.link.open_streams();
        assert_eq!(open.len(), 1, "the request holds one stream, got {open:?}");
        open[0].id
        // The future is dropped here, unanswered.
    };
    assert!(
        tapped.link.open_streams().is_empty(),
        "a request nobody waits for any more leaves no stream behind, got {:?}",
        tapped.link.open_streams()
    );

    // Now the answer arrives, late: the agent is asked under the abandoned stream's id.
    tapped.tap.inject(ono_protocol::Frame::new(
        FrameKind::ListLinks,
        id,
        b"null".to_vec(),
    ));
    tapped.tap.pass_kind(FrameKind::ListLinks);
    let answered = within(tapped.link.links(WAIT))
        .await
        .expect("the link survived the late answer and serves the next request");
    assert_eq!(answered.links.len(), 1);
    assert!(tapped.link.is_alive());
}
