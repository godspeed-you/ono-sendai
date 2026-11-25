//! A tap between a client and an agent: frames pass through it unchanged unless a test asks it
//! to drop a kind or to put a frame of its own on the wire.
//!
//! It stands for the network between two ends of a link, which is the only "outside world" the
//! probe and link-table suites need to fake: an agent that does not answer in time, an older peer
//! that never announced a request, a client that sends a request nobody offered (AGENTS.md §11).

#![allow(
    dead_code,
    clippy::expect_used,
    reason = "a shared test fixture states preconditions the same way a #[test] body does"
)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use bytes::BytesMut;
use ono_protocol::{
    ClientConfig, Frame, FrameKind, Limits, RemoteService, ServerConfig, UnauthenticatedTransport,
};
use ono_value::ErrorValue;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::sync::mpsc;

/// What a test can do to the frames a client sends.
#[derive(Debug, Clone)]
pub struct Tap {
    dropped: Arc<Mutex<HashSet<FrameKind>>>,
    seen: Arc<Mutex<Vec<FrameKind>>>,
    inject: Option<mpsc::UnboundedSender<Option<Frame>>>,
    /// While true, nothing the agent writes is read: a client that stopped reading.
    holding: Arc<tokio::sync::watch::Sender<bool>>,
    /// Bytes the agent wrote that reached the client.
    delivered: Arc<std::sync::atomic::AtomicUsize>,
    /// Set once the agent's sending direction has ended.
    closed: Arc<tokio::sync::watch::Sender<bool>>,
}

impl Default for Tap {
    fn default() -> Self {
        Self {
            dropped: Arc::default(),
            seen: Arc::default(),
            inject: None,
            holding: Arc::new(tokio::sync::watch::channel(false).0),
            delivered: Arc::default(),
            closed: Arc::new(tokio::sync::watch::channel(false).0),
        }
    }
}

impl Tap {
    /// Swallows every client frame of `kind` from now on, as a peer that never answers would.
    pub fn drop_kind(&self, kind: FrameKind) {
        self.dropped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(kind);
    }

    /// Lets frames of `kind` through again.
    pub fn pass_kind(&self, kind: FrameKind) {
        self.dropped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&kind);
    }

    /// Every kind the client put on the wire, in order — the handshake included.
    pub fn seen(&self) -> Vec<FrameKind> {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Stops reading what the agent writes, as a client that stopped reading would.
    pub fn hold_answers(&self) {
        self.holding.send_replace(true);
    }

    /// Reads what the agent writes again.
    pub fn release_answers(&self) {
        self.holding.send_replace(false);
    }

    /// How many bytes the agent wrote reached the client so far.
    pub fn delivered(&self) -> usize {
        self.delivered.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Resolves once the agent's sending direction has ended: it let the socket go.
    pub async fn agent_closed(&self) {
        let mut closed = self.closed.subscribe();
        while !*closed.borrow_and_update() {
            if closed.changed().await.is_err() {
                return;
            }
        }
    }

    /// Ends the client's sending direction: the agent reads end of input.
    pub fn hang_up(&self) {
        // Through the same queue as injected frames, so every frame injected before it is sent.
        if let Some(inject) = &self.inject {
            let _ = inject.send(None);
        }
    }

    /// Puts `frame` on the wire to the agent, as if the client had sent it.
    pub fn inject(&self, frame: Frame) {
        if let Some(inject) = &self.inject {
            let _ = inject.send(Some(frame));
        }
    }
}

/// A link whose client frames pass through a [`Tap`].
pub struct Tapped {
    /// The client end.
    pub link: ono_protocol::Link,
    /// The tap.
    pub tap: Tap,
    /// The task running the agent.
    pub server: tokio::task::JoinHandle<Result<(), ErrorValue>>,
}

/// Connects `client` to `service` under `server`, through a tap.
pub async fn connect<S: RemoteService>(
    client: ClientConfig,
    server: ServerConfig,
    service: S,
) -> Result<Tapped, ErrorValue> {
    connect_proving(client, server, service, None).await
}

/// The same, with the agent's transport reporting that the client proved `proves` — what a
/// listening agent's TLS layer reports and an authorization policy is resolved from.
pub async fn connect_proving<S: RemoteService>(
    client: ClientConfig,
    server: ServerConfig,
    service: S,
    proves: Option<ono_protocol::HostKey>,
) -> Result<Tapped, ErrorValue> {
    connect_through(client, server, service, proves, 64 * 1024).await
}

/// The same, with `buffer` bytes of room on each side of the tap: a small buffer is a client
/// whose socket fills as soon as it stops reading.
pub async fn connect_through<S: RemoteService>(
    client: ClientConfig,
    server: ServerConfig,
    service: S,
    proves: Option<ono_protocol::HostKey>,
    buffer: usize,
) -> Result<Tapped, ErrorValue> {
    let (client_near, client_far) = tokio::io::duplex(64 * 1024);
    let (server_near, server_far) = tokio::io::duplex(buffer);
    let (inject, mut injected) = mpsc::unbounded_channel::<Option<Frame>>();
    let tap = Tap {
        inject: Some(inject),
        ..Tap::default()
    };
    let mut listening = UnauthenticatedTransport::new(server_near);
    if let Some(key) = proves {
        listening = listening.with_peer_key(key);
    }
    let handle = tokio::spawn(ono_protocol::serve(listening, server, service));

    let (mut to_server_read, mut to_server) = tokio::io::split(server_far);
    let (mut from_client, mut to_client) = tokio::io::split(client_far);
    // Agent → client: unchanged, unless the test holds the answers back.
    let reading = tap.clone();
    tokio::spawn(async move {
        let mut holding = reading.holding.subscribe();
        let mut chunk = vec![0u8; 16 * 1024];
        loop {
            while *holding.borrow_and_update() {
                if holding.changed().await.is_err() {
                    return;
                }
            }
            let Ok(read) = to_server_read.read(&mut chunk).await else {
                break;
            };
            if read == 0 {
                break;
            }
            reading
                .delivered
                .fetch_add(read, std::sync::atomic::Ordering::SeqCst);
            if to_client.write_all(&chunk[..read]).await.is_err() {
                break;
            }
        }
        reading.closed.send_replace(true);
        let _ = to_client.shutdown().await;
    });
    // Client → agent: frame by frame, so a kind can be dropped and a frame injected.
    let forward = tap.clone();
    tokio::spawn(async move {
        let limits = Limits::default();
        let mut buffer = BytesMut::new();
        let mut chunk = vec![0u8; 16 * 1024];
        loop {
            tokio::select! {
                read = from_client.read(&mut chunk) => {
                    let Ok(read) = read else { break };
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    while let Ok(Some(frame)) = ono_protocol::decode(&mut buffer, &limits) {
                        forward
                            .seen
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(frame.kind());
                        let dropped = forward
                            .dropped
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .contains(&frame.kind());
                        if dropped {
                            continue;
                        }
                        if write(&mut to_server, &frame, &limits).await.is_err() {
                            return;
                        }
                    }
                }
                Some(frame) = injected.recv() => {
                    let Some(frame) = frame else { break };
                    if write(&mut to_server, &frame, &limits).await.is_err() {
                        return;
                    }
                }
            }
        }
        let _ = to_server.shutdown().await;
    });
    let link = super::within(ono_protocol::Link::connect(
        UnauthenticatedTransport::new(client_near),
        client,
    ))
    .await?;
    Ok(Tapped {
        link,
        tap,
        server: handle,
    })
}

async fn write<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &Frame,
    limits: &Limits,
) -> std::io::Result<()> {
    let mut out = BytesMut::new();
    ono_protocol::encode(frame, limits, &mut out).expect("a forwarded frame re-encodes");
    writer.write_all(&out).await?;
    writer.flush().await
}
