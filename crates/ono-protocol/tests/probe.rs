//! The probe of `ono.link.probe/1`: a live round trip on an established link (v0.4 §19.1,
//! issue #308, ADR-1095).
//!
//! The handshake's round trip is measured once, when the link is made. A probe is the one
//! request/response pair an agent answers without touching a provider, so a latency can be
//! measured while the link is open. It is extended by ADR-1056's one rule: the agent announces
//! the capability id, the client sends the frame only when the id was announced and otherwise
//! refuses locally, and a peer that receives a frame it did not announce fails the link as a
//! protocol mismatch, never as success.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;
use std::time::Duration;

use ono_core::ErrorCode;
use ono_protocol::{Frame, FrameKind, PROBE_CAPABILITY, RemoteQuery};

mod common;
use common::{DemoService, Observed, client_config, server_config, tap, within};

/// An agent that announces probes, as this build's agent does.
fn probing_agent() -> ono_protocol::ServerConfig {
    server_config().with_capabilities(["process.list", "process.signal", PROBE_CAPABILITY])
}

#[tokio::test]
async fn should_measure_a_live_round_trip_when_the_agent_offers_probes() {
    let observed = Arc::new(Observed::default());
    let tapped = tap::connect(
        client_config("testhost"),
        probing_agent(),
        DemoService::new(Arc::clone(&observed)),
    )
    .await
    .expect("the handshake succeeds");

    assert!(
        tapped
            .link
            .negotiated()
            .capabilities()
            .iter()
            .any(|id| id == PROBE_CAPABILITY),
        "the agent announces the probe as a capability id of the handshake"
    );
    let first = within(tapped.link.probe(Duration::from_secs(5)))
        .await
        .expect("an agent that offers probes answers one");
    let second = within(tapped.link.probe(Duration::from_secs(5)))
        .await
        .expect("and the next one");
    assert!(
        first < Duration::from_secs(5) && second < Duration::from_secs(5),
        "a probe measures the round trip, which is inside its own bound: {first:?}, {second:?}"
    );
    assert_eq!(
        observed.sent(),
        0,
        "the agent answers a probe without touching a provider"
    );
    assert!(
        tapped.link.open_streams().is_empty(),
        "an answered probe leaves no stream behind"
    );
}

#[tokio::test]
async fn should_refuse_a_probe_locally_when_the_agent_does_not_offer_it() {
    // An older agent never announces the probe. The client must not send it — an older agent
    // would fail the link on a frame kind it does not know — and says so to its caller instead.
    let tapped = tap::connect(
        client_config("testhost"),
        server_config(),
        DemoService::new(Arc::new(Observed::default())),
    )
    .await
    .expect("the handshake succeeds");

    let refusal = within(tapped.link.probe(Duration::from_secs(5)))
        .await
        .expect_err("a probe the agent did not offer is refused");
    assert_eq!(refusal.code(), ErrorCode::RemoteProtocolMismatch);
    assert!(
        refusal.message().contains(PROBE_CAPABILITY),
        "the refusal names what the agent does not offer, got {:?}",
        refusal.message()
    );
    assert!(
        !tapped.tap.seen().contains(&FrameKind::Probe),
        "nothing was sent: the refusal is local, got {:?}",
        tapped.tap.seen()
    );
    let mut stream = tapped
        .link
        .query(&RemoteQuery::target("demo").limit(1))
        .expect("the link is still up");
    assert!(within(stream.recv()).await.is_some(), "and still answers");
}

#[tokio::test]
async fn should_refuse_a_probe_locally_when_the_client_did_not_ask_for_it() {
    // A client that names the capabilities it wants and leaves the probe out has not negotiated
    // it, whatever the agent offers: the negotiated set is the intersection (spec §21.2).
    let tapped = tap::connect(
        client_config("testhost").with_capabilities(["process.list"]),
        probing_agent(),
        DemoService::new(Arc::new(Observed::default())),
    )
    .await
    .expect("the handshake succeeds");

    let refusal = within(tapped.link.probe(Duration::from_secs(5)))
        .await
        .expect_err("a probe nobody negotiated is refused");
    assert_eq!(refusal.code(), ErrorCode::RemoteProtocolMismatch);
    assert!(!tapped.tap.seen().contains(&FrameKind::Probe));
}

#[tokio::test(start_paused = true)]
async fn should_report_a_probe_that_was_not_answered_in_time_as_unanswered() {
    let tapped = tap::connect(
        client_config("testhost"),
        probing_agent(),
        DemoService::new(Arc::new(Observed::default())),
    )
    .await
    .expect("the handshake succeeds");
    tapped.tap.drop_kind(FrameKind::Probe);

    let refusal = tapped
        .link
        .probe(Duration::from_millis(500))
        .await
        .expect_err("a probe nobody answers times out");
    assert_eq!(refusal.code(), ErrorCode::RemoteUnreachable);
    assert!(
        refusal.metadata().get("timeout_ms").is_some(),
        "the refusal states the bound it waited for, got {refusal:?}"
    );
    assert!(
        tapped.link.open_streams().is_empty(),
        "a probe that timed out is released, not left waiting: probes cannot accumulate"
    );

    tapped.tap.pass_kind(FrameKind::Probe);
    within(tapped.link.probe(Duration::from_secs(5)))
        .await
        .expect("the link itself is unharmed by a probe that went unanswered");
}

#[tokio::test]
async fn should_fail_a_probe_when_the_link_is_gone() {
    let fixture = common::try_connect(client_config("testhost"), probing_agent(), None)
        .await
        .expect("the handshake succeeds");
    fixture.server.abort();
    let _ = fixture.server.await;
    common::settle().await;

    let refusal = within(fixture.link.probe(Duration::from_secs(5)))
        .await
        .expect_err("a link that is gone answers no probe");
    assert_eq!(refusal.code(), ErrorCode::RemoteUnreachable);
}

#[tokio::test]
async fn should_fail_the_link_as_a_mismatch_when_an_agent_receives_a_probe_it_did_not_offer() {
    // ADR-1056's last line: a peer never ignores a frame it did not announce into success.
    let tapped = tap::connect(
        client_config("testhost"),
        server_config(),
        DemoService::new(Arc::new(Observed::default())),
    )
    .await
    .expect("the handshake succeeds");
    tapped
        .tap
        .inject(Frame::new(FrameKind::Probe, 77, b"null".to_vec()));

    let outcome = within(tapped.server)
        .await
        .expect("the serving task does not panic");
    let failure = outcome.expect_err("the agent does not serve a request it did not offer");
    assert_eq!(failure.code(), ErrorCode::RemoteProtocolMismatch);
}

#[tokio::test]
async fn should_probe_through_a_handle_that_does_not_keep_the_link_open() {
    let fixture = common::try_connect(client_config("testhost"), probing_agent(), None)
        .await
        .expect("the handshake succeeds");
    let prober = fixture.link.prober();
    assert!(prober.is_offered());
    within(prober.probe(Duration::from_secs(5)))
        .await
        .expect("a handle probes the link it was taken from");

    let again = common::try_connect(client_config("testhost"), probing_agent(), None)
        .await
        .expect("a second link");
    assert_ne!(
        prober,
        again.link.prober(),
        "a handle names one link: another link is another measurement"
    );

    drop(fixture.link);
    let outcome = within(fixture.server)
        .await
        .expect("the serving task does not panic");
    assert!(
        outcome.is_ok(),
        "the handle did not keep the link open: dropping the link ended the session"
    );
    let refusal = within(prober.probe(Duration::from_secs(5)))
        .await
        .expect_err("a handle to a dropped link measures nothing");
    assert_eq!(refusal.code(), ErrorCode::RemoteUnreachable);
}

#[tokio::test]
async fn should_tell_a_holder_when_its_link_is_gone() {
    let fixture = common::try_connect(client_config("testhost"), probing_agent(), None)
        .await
        .expect("the handshake succeeds");
    assert!(fixture.link.is_alive());
    fixture.server.abort();
    within(fixture.link.closed()).await;
    assert!(!fixture.link.is_alive());
}

#[tokio::test]
async fn should_forget_a_probe_whose_caller_went_away_and_drop_its_late_answer() {
    // A dropped probe future (a Ctrl-C during `look`) must take its stream with it, and the late
    // answer must be dropped as a frame that crossed with a cancellation — not fail the whole
    // link as a credit overrun (review PRF-1). The probe is answered in the agent's own loop, in
    // order, so the late answer reaches the client before the next probe's does.
    let tapped = tap::connect(
        client_config("testhost"),
        probing_agent(),
        DemoService::new(Arc::new(Observed::default())),
    )
    .await
    .expect("the handshake succeeds");
    tapped.tap.drop_kind(FrameKind::Probe);
    let id = {
        let probing = tapped.link.probe(Duration::from_secs(5));
        tokio::pin!(probing);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), probing.as_mut())
                .await
                .is_err(),
            "nothing has answered yet"
        );
        let open = tapped.link.open_streams();
        assert_eq!(open.len(), 1, "the probe holds one stream, got {open:?}");
        open[0].id
    };
    assert!(
        tapped.link.open_streams().is_empty(),
        "a probe nobody waits for any more leaves no stream behind, got {:?}",
        tapped.link.open_streams()
    );

    tapped
        .tap
        .inject(Frame::new(FrameKind::Probe, id, b"null".to_vec()));
    tapped.tap.pass_kind(FrameKind::Probe);
    for _ in 0..2 {
        within(tapped.link.probe(Duration::from_secs(5)))
            .await
            .expect("the link survived the late answer and is probed again");
    }
    assert!(tapped.link.is_alive());
}

#[tokio::test]
async fn should_end_the_connection_of_a_peer_that_asks_for_probes_and_never_reads_the_answers() {
    // A probe costs the peer nothing to ask and is answered without credit. A peer that asks and
    // never reads would grow the agent's outbound queue without limit (review PRF-2, SEC-1): the
    // answers it leaves unread count against an allowance, and past it the agent ends the
    // connection with a structured refusal and an audit record (ADR-1115).
    let audit = Arc::new(common::Recording::default());
    let tapped = tap::connect_through(
        client_config("testhost"),
        probing_agent().with_audit(audit.clone()),
        DemoService::new(Arc::new(Observed::default())),
        None,
        1024,
    )
    .await
    .expect("the handshake succeeds");
    tapped.tap.hold_answers();
    let asked = u32::try_from(ono_protocol::MAX_UNCREDITED_FRAMES).expect("fits") + 256;
    for id in 0..asked {
        tapped.tap.inject(Frame::new(
            FrameKind::Probe,
            1_000_000 + id * 2,
            b"null".to_vec(),
        ));
    }
    let outcome = within(tapped.server)
        .await
        .expect("the serving task does not panic");
    let refusal = outcome.expect_err("the agent ends a connection that does not read its answers");
    assert_eq!(refusal.code(), ErrorCode::RemoteProtocolMismatch);
    assert!(
        refusal.message().contains("unread"),
        "the refusal names the bound, got {}",
        refusal.message()
    );
    assert!(
        audit
            .lines()
            .iter()
            .any(|line| line.contains("connection.limit_denied")),
        "the agent records why it ended the connection, got {:?}",
        audit.lines()
    );
}

#[tokio::test]
async fn should_let_go_of_a_peer_that_stops_reading_once_the_session_has_ended() {
    // The writer must not hold the socket for a peer that stopped reading after the session it
    // belonged to has ended (review SEC-2): what is still queued gets the handshake timeout to
    // leave, then the socket is dropped and the rest is never written.
    let limits = ono_protocol::Limits::default().with_handshake_timeout(Duration::from_millis(300));
    let tapped = tap::connect_through(
        client_config("testhost"),
        probing_agent().with_limits(limits),
        DemoService::new(Arc::new(Observed::default())),
        None,
        1024,
    )
    .await
    .expect("the handshake succeeds");
    let before = tapped.tap.delivered();
    tapped.tap.hold_answers();
    // Far more answers than the 1 KiB the socket holds, far fewer than the allowance.
    let asked: u32 = 400;
    for id in 0..asked {
        tapped.tap.inject(Frame::new(
            FrameKind::Probe,
            1_000_000 + id * 2,
            b"null".to_vec(),
        ));
    }
    tapped.tap.hang_up();
    within(tapped.server)
        .await
        .expect("the serving task does not panic")
        .expect("a caller that hangs up ends the session cleanly");
    // Past the writer's last deadline the client reads again, to the end of what the agent
    // wrote.
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    tapped.tap.release_answers();
    within(tapped.tap.agent_closed()).await;
    let answer = 12 + 4; // one `end` frame: header and a `null` payload
    let delivered = tapped.tap.delivered() - before;
    assert!(
        delivered < asked as usize * answer,
        "the writer let the socket go at its deadline instead of waiting for the peer to read \
         all {asked} answers: {delivered} bytes reached it"
    );
}
