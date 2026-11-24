//! What a peer that does not say hello in time is told, and what it cannot do about it (issue
//! #181; v0.4.1 §12.2, §12.6, §54.1).
//!
//! §12.2: "A peer that does not complete TLS plus Ono protocol negotiation within the timeout
//! MUST be disconnected." The Ono half of that deadline is kept here, after the transport is up,
//! so there is a channel to say why — and `remote.handshake_timeout` (E1502) is what §54.1 asks
//! the peer to be told. The other property is the one a hostile peer probes: being told must not
//! give it a way to hold the agent longer, by never reading the refusal or by never hanging up.
//!
//! Time is paused in every test here: tokio advances the clock only when every task is waiting,
//! so a deadline fires exactly when nothing else can happen and no test sleeps.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use bytes::BytesMut;
use ono_core::ErrorCode;
use ono_protocol::{FrameKind, Limits, Message, UnauthenticatedTransport};
use ono_value::Value;
use tokio::io::AsyncReadExt as _;

mod common;
use common::{DemoService, Observed, schemas, server_config};

const DEADLINE: Duration = Duration::from_secs(2);

fn deadline_limits() -> Limits {
    Limits::default().with_handshake_timeout(DEADLINE)
}

#[tokio::test(start_paused = true)]
async fn should_tell_a_peer_that_never_says_hello_that_its_handshake_timed_out() {
    let (near, mut far) = tokio::io::duplex(64 * 1024);
    let agent = tokio::spawn(ono_protocol::serve(
        UnauthenticatedTransport::new(near),
        server_config().with_limits(deadline_limits()),
        DemoService::new(std::sync::Arc::new(Observed::default())),
    ));

    // The peer says nothing and reads what it is sent.
    let mut received = BytesMut::new();
    let frame = loop {
        if let Some(frame) =
            ono_protocol::decode(&mut received, &Limits::default()).expect("frames")
        {
            break Some(frame);
        }
        if far
            .read_buf(&mut received)
            .await
            .expect("the transport reads")
            == 0
        {
            break None;
        }
    };
    let frame = frame.expect(
        "§54.1: a peer whose handshake timed out is told so, not met with a socket that closes",
    );
    assert_eq!(frame.kind(), FrameKind::Reject);
    let Message::Reject(reject) = ono_protocol::decode_message(
        frame.kind(),
        frame.payload(),
        &schemas(),
        &Limits::default(),
    )
    .expect("a reject decodes") else {
        panic!("a handshake answer that refuses is a Reject");
    };
    let refused = reject.to_error();
    assert_eq!(refused.code(), ErrorCode::RemoteHandshakeTimeout);
    assert_eq!(
        refused.metadata().get("timeout_ms"),
        Some(&Value::Int(2_000)),
        "refusals.yaml: E1502 carries the deadline it enforced, got {refused:?}"
    );
    assert_eq!(
        refused.retryable(),
        Some(true),
        "a deadline is about the moment, so trying again is a remedy"
    );

    drop(far);
    let served = agent.await.expect("the agent task does not panic");
    assert!(
        served.is_ok(),
        "refusing a slow peer is the agent doing its job, not failing: {served:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn should_let_go_of_a_peer_that_neither_reads_its_refusal_nor_hangs_up() {
    // A slow-loris: TCP-level room for sixteen bytes and nobody reading them, so the refusal
    // itself cannot be written, and no end of stream ever comes. The agent must still be done
    // with it within a bound fixed by its own limits — the deadline it enforced plus one more for
    // saying so — rather than for as long as the peer cares to wait.
    let (near, far) = tokio::io::duplex(16);
    let started = tokio::time::Instant::now();
    let served = tokio::time::timeout(
        DEADLINE * 5,
        ono_protocol::serve(
            UnauthenticatedTransport::new(near),
            server_config().with_limits(deadline_limits()),
            DemoService::new(std::sync::Arc::new(Observed::default())),
        ),
    )
    .await
    .expect("§12.6: a peer that does not read cannot hold the agent past its bound");
    assert!(
        served.is_ok(),
        "the refusal was the agent's decision: {served:?}"
    );
    assert!(
        started.elapsed() <= DEADLINE * 2,
        "the hold is bounded by the deadline and one more for the refusal, took {:?}",
        started.elapsed()
    );
    drop(far);
}

#[tokio::test(start_paused = true)]
async fn should_let_go_of_a_refused_peer_that_keeps_talking() {
    // The other half of the slow-loris: a peer that reads the refusal and then never hangs up,
    // sending a frame now and then to look alive. Waiting for it to go is how the refusal is made
    // observable; that wait has the same bound.
    let (near, mut far) = tokio::io::duplex(64 * 1024);
    let agent = tokio::spawn(ono_protocol::serve(
        UnauthenticatedTransport::new(near),
        server_config().with_limits(deadline_limits()),
        DemoService::new(std::sync::Arc::new(Observed::default())),
    ));
    let started = tokio::time::Instant::now();
    let chatter = tokio::spawn(async move {
        let mut frame = BytesMut::new();
        ono_protocol::encode(
            &ono_protocol::Frame::new(FrameKind::Credit, 1, b"1".to_vec()),
            &Limits::default(),
            &mut frame,
        )
        .expect("a frame");
        // Silent until the refusal arrives, then never silent long enough to look gone.
        let mut sink = [0u8; 1024];
        let _ = far.read(&mut sink).await;
        loop {
            tokio::time::sleep(DEADLINE / 4).await;
            if tokio::io::AsyncWriteExt::write_all(&mut far, &frame)
                .await
                .is_err()
            {
                return;
            }
            let _ = tokio::time::timeout(Duration::from_millis(1), far.read(&mut sink)).await;
        }
    });

    let served = tokio::time::timeout(DEADLINE * 5, agent)
        .await
        .expect("§12.6: a peer that never hangs up cannot hold the agent past its bound")
        .expect("the agent task does not panic");
    assert!(served.is_ok(), "{served:?}");
    assert!(
        started.elapsed() <= DEADLINE * 2,
        "the hold is bounded by the deadline and one more for the refusal, took {:?}",
        started.elapsed()
    );
    chatter.abort();
}
