//! The multiplexed streams of one link, each with its own identity, cancellation and failures
//! (spec §21.4, §16.5, §18.5; issue #219 (b), ADR-1099).
//!
//! One connection carries many streams. Each one's values, failures and end belong to it alone:
//! a failure on one stream is not lost, swallowed or delivered to another; a cancelled stream
//! does not take a live one with it; and a peer that dies mid-stream ends every open stream with
//! the link's failure — never with what reads as a complete answer.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_core::ErrorCode;
use ono_protocol::{RemoteMessage, RemoteQuery, RemoteStream};
use ono_value::Value;

mod common;
use common::{connect, settle, within};

/// Everything a stream carried until it ended: values and failures in order.
async fn drain(stream: &mut RemoteStream) -> Vec<RemoteMessage> {
    let mut seen = Vec::new();
    while let Some(message) = stream.recv().await {
        seen.push(message);
    }
    seen
}

fn failures(seen: &[RemoteMessage]) -> Vec<ErrorCode> {
    seen.iter()
        .filter_map(|message| match message {
            RemoteMessage::Failure(error) => Some(error.code()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn should_keep_each_streams_own_failures_on_that_stream_when_they_run_side_by_side() {
    // Four streams at once: two that fail one item each, one the remote cannot answer at all,
    // and one that succeeds. Every failure arrives on the stream it belongs to, and only there.
    let fixture = connect().await;
    let mut flaky = fixture
        .link
        .query(&RemoteQuery::target("flaky"))
        .expect("opens");
    let mut also_flaky = fixture
        .link
        .query(&RemoteQuery::target("flaky"))
        .expect("opens");
    let mut unknown = fixture
        .link
        .query(&RemoteQuery::target("nonexistent"))
        .expect("opens");
    let mut clean = fixture
        .link
        .query(&RemoteQuery::target("demo").limit(3))
        .expect("opens");

    let (a, b, c, d) = within(async {
        tokio::join!(
            drain(&mut flaky),
            drain(&mut also_flaky),
            drain(&mut unknown),
            drain(&mut clean)
        )
    })
    .await;

    for flaky in [&a, &b] {
        assert_eq!(
            failures(flaky),
            [ErrorCode::IoPermissionDenied],
            "each flaky stream carries its own one failure between its values, got {flaky:?}"
        );
        assert_eq!(flaky.len(), 3, "the values around the failure still arrive");
    }
    assert_eq!(failures(&c), [ErrorCode::ResolveTargetNotFound]);
    assert!(
        failures(&d).is_empty() && d.len() == 3,
        "the clean stream saw none of the others' failures, got {d:?}"
    );
}

#[tokio::test]
async fn should_end_every_open_stream_with_the_links_failure_when_the_peer_dies_mid_stream() {
    // Two endless producers and a finished stream. The peer dies while the endless ones are
    // mid-flight: each of them ends with `remote.unreachable` after the values it already
    // received, so no reader can mistake a dead peer for an answer that was complete.
    let fixture = connect().await;
    let mut one = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect("opens");
    let mut two = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect("opens");
    // Both have started producing.
    assert!(matches!(
        within(one.recv()).await,
        Some(RemoteMessage::Value(_))
    ));
    assert!(matches!(
        within(two.recv()).await,
        Some(RemoteMessage::Value(_))
    ));

    fixture.server.abort();
    let _ = fixture.server.await;
    settle().await;

    for stream in [&mut one, &mut two] {
        let rest = within(drain(stream)).await;
        let last = rest
            .last()
            .expect("a dead peer ends the stream with something");
        assert!(
            matches!(last, RemoteMessage::Failure(error) if error.code() == ErrorCode::RemoteUnreachable),
            "the stream ends with the link's failure, not with a clean end, got {last:?}"
        );
        assert!(
            rest[..rest.len() - 1]
                .iter()
                .all(|message| matches!(message, RemoteMessage::Value(Value::Int(_)))),
            "everything before it is what the peer did send"
        );
    }
}

#[tokio::test]
async fn should_keep_a_live_stream_answering_beside_a_cancelled_one_and_list_only_the_live_one() {
    let fixture = connect().await;
    let cancelled = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect("opens");
    let mut live = fixture
        .link
        .query(&RemoteQuery::target("endless"))
        .expect("opens");
    let open: Vec<u32> = fixture
        .link
        .open_streams()
        .iter()
        .map(|stream| stream.id)
        .collect();
    assert_eq!(open, [cancelled.id(), live.id()], "both are open, by id");
    assert!(
        fixture
            .link
            .open_streams()
            .iter()
            .all(|stream| stream.kind == "query" && stream.target.as_deref() == Some("endless")),
        "each says what it is"
    );

    cancelled.cancel();
    for _ in 0..100 {
        assert!(
            matches!(within(live.recv()).await, Some(RemoteMessage::Value(_))),
            "the live stream keeps answering"
        );
    }
    assert_eq!(
        fixture
            .link
            .open_streams()
            .iter()
            .map(|stream| stream.id)
            .collect::<Vec<_>>(),
        [live.id()],
        "a cancelled stream is no longer carried"
    );
}
