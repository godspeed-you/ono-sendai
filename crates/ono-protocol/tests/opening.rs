//! What an agent does with an opening it cannot use (v0.4.1 §12.6, §13.2, §14.1, §54.1; spec
//! §21.2; ADR-1056, ADR-1057).
//!
//! The first frame of a link is the client's `Hello`. When it is something else — a frame of
//! another kind, a payload this build cannot read as a hello, a frame larger than the agent
//! accepts — the peer is not speaking this protocol, or not a version of it this agent can read.
//! That is `remote.protocol_mismatch` (E0602), and a peer that has a channel is told so rather
//! than met with a socket that closes: a newer client whose hello an older agent cannot parse
//! must learn that the two do not share a protocol, not that the host is unreachable. And each of
//! these, together with a peer that hangs up before it says anything, is a decision the agent
//! records (§14.1).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::{Arc, Mutex};

use bytes::BytesMut;
use ono_core::ErrorCode;
use ono_protocol::{
    AuditEvent, AuditSink, Frame, FrameKind, Limits, Message, UnauthenticatedTransport,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

mod common;
use common::{DemoService, Observed, client_key, schemas, server_config, within};

/// The audit sink the test reads.
#[derive(Debug, Default)]
struct Recording(Mutex<Vec<String>>);

impl AuditSink for Recording {
    fn record(&self, event: &AuditEvent) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event.render());
    }
}

impl Recording {
    fn lines(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// What the agent answered, and what it recorded, when the peer opened with exactly `opening`.
async fn opened_with(opening: Vec<u8>) -> (Option<Message>, Vec<String>) {
    let (near, mut far) = tokio::io::duplex(64 * 1024);
    let audit = Arc::new(Recording::default());
    let agent = tokio::spawn(ono_protocol::serve(
        UnauthenticatedTransport::new(near).with_peer_key(client_key()),
        server_config().with_audit(Arc::clone(&audit) as ono_protocol::Audit),
        DemoService::new(Arc::new(Observed::default())),
    ));
    if opening.is_empty() {
        // Nothing to say: the peer hangs up.
        far.shutdown().await.expect("the peer hangs up");
    } else {
        far.write_all(&opening)
            .await
            .expect("the opening is written");
    }

    let mut received = BytesMut::new();
    let answer = loop {
        if let Some(frame) =
            ono_protocol::decode(&mut received, &Limits::default()).expect("the agent sends frames")
        {
            break Some(
                ono_protocol::decode_message(
                    frame.kind(),
                    frame.payload(),
                    &schemas(),
                    &Limits::default(),
                )
                .expect("the agent's answer decodes"),
            );
        }
        if within(far.read_buf(&mut received)).await.expect("reads") == 0 {
            break None;
        }
    };
    drop(far);
    let _ = within(agent).await;
    (answer, audit.lines())
}

fn framed(kind: FrameKind, payload: &[u8]) -> Vec<u8> {
    let mut wire = BytesMut::new();
    ono_protocol::encode(
        &Frame::new(kind, 0, payload.to_vec()),
        &Limits::default(),
        &mut wire,
    )
    .expect("the test's frame fits");
    wire.to_vec()
}

fn assert_told_mismatch(answer: Option<Message>, audit: &[String], what: &str) {
    let Some(Message::Reject(reject)) = answer else {
        panic!("§54.1: {what} is answered with a refusal, got {answer:?}");
    };
    assert_eq!(
        reject.to_error().code(),
        ErrorCode::RemoteProtocolMismatch,
        "{what} is a protocol mismatch, got {reject:?}"
    );
    assert!(
        audit
            .iter()
            .any(|line| line.contains("event=connection.protocol_mismatch")
                && line.contains("error_code=remote.protocol_mismatch")
                && line.contains(&client_key().fingerprint().to_string())),
        "§14.1: the agent records the mismatch and the authenticated peer, got {audit:?}"
    );
}

#[tokio::test]
async fn should_tell_a_peer_that_opens_with_another_kind_of_frame_that_it_speaks_another_protocol()
{
    let (answer, audit) = opened_with(framed(FrameKind::Credit, b"1")).await;
    assert_told_mismatch(answer, &audit, "an opening that is not a hello");
}

#[tokio::test]
async fn should_tell_a_peer_whose_hello_it_cannot_read_that_they_share_no_protocol() {
    // What an older agent meets from a client whose hello changed shape incompatibly.
    let (answer, audit) = opened_with(framed(
        FrameKind::Hello,
        br#"{"versions":"two","identity":7}"#,
    ))
    .await;
    assert_told_mismatch(answer, &audit, "a hello this agent cannot read");
}

#[tokio::test]
async fn should_tell_a_peer_whose_opening_frame_exceeds_the_limit_that_it_was_refused() {
    // A header claiming more payload than the agent accepts is refused at the header (§12.4).
    let mut header = framed(FrameKind::Hello, b"{}");
    let claimed = u32::try_from(ono_protocol::MAX_FRAME_PAYLOAD + 1).expect("fits");
    let length_at = header.len() - 2 - 4;
    header[length_at..length_at + 4].copy_from_slice(&claimed.to_be_bytes());
    header.truncate(length_at + 4);
    let (answer, audit) = opened_with(header).await;
    assert_told_mismatch(answer, &audit, "an opening frame over the size limit");
}

#[tokio::test]
async fn should_record_a_peer_that_hangs_up_before_saying_hello() {
    let (answer, audit) = opened_with(Vec::new()).await;
    assert!(answer.is_none(), "nobody is left to answer: {answer:?}");
    assert!(
        audit
            .iter()
            .any(|line| line.contains("event=connection.disconnected")
                && line.contains("error_code=remote.unreachable")
                && line.contains(&client_key().fingerprint().to_string())),
        "§14.1: a peer that went away mid-handshake is recorded, got {audit:?}"
    );
}
