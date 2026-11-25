//! The local end of a link: the handshake, the stream table, and the credit that bounds it.
//!
//! # Multiplexing
//!
//! One connection carries many concurrent queries (spec §21.4: "typed RPC / multiplexed
//! streams"). Each has a stream id, its own inbound queue, and its own cancellation; a stream
//! that stalls or is cancelled does not touch the others, because routing happens on the frame
//! header and never needs the payload.
//!
//! # Backpressure, and why it is credit rather than the socket
//!
//! Spec §11.2 requires that a slow consumer stop an infinite producer, and ADR-0013 makes a
//! bounded channel the mechanism locally. A remote producer is on the other side of a socket, and
//! the socket's own flow control is per *connection*: letting it be the bound would mean a
//! consumer that stopped reading one query would stall every other query on the same link, and
//! would still allow the peer to buffer whatever the network path holds.
//!
//! So each stream carries a **credit window**, negotiated in the handshake. Opening a stream
//! grants the remote `window` messages. The consumer grants more only as it takes messages out,
//! so at any moment
//!
//! ```text
//! messages the remote has sent  ≤  messages the consumer has taken  +  window
//! ```
//!
//! which is the bound `crates/ono-protocol/tests/streams.rs` asserts directly against a producer
//! that never stops. The inbound queue is sized to the window, so a peer that respects its credit
//! never fills it; one that does not is refusing the protocol and the link fails with
//! `remote.protocol_mismatch` rather than growing.
//!
//! Credit is returned in halves rather than one at a time: a grant per message would double the
//! frame count of every stream for no gain, and half a window is early enough that a peer with a
//! round trip of latency never runs dry.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{ActionOutcome, EventStream, ObjectEvent};
use ono_value::{ErrorValue, SchemaRegistry, Value};
use tokio::sync::mpsc;

use crate::connection::{FrameReader, FrameSink, spawn_writer};
use crate::error::unreachable;
use crate::handshake::{Negotiated, PeerClock, hello};
use crate::message::{ActRequest, AdaptRequest, RemoteQuery};
use crate::trust::{TrustPolicy, TrustStore, decide};
use crate::{
    Frame, FrameKind, Identity, Limits, Message, PROTOCOL_VERSION, ProtocolError, Transport,
    decode_message, encode_message,
};

/// How a link is opened: what to offer, what to demand of the peer's identity, and what to
/// enforce on what it sends.
///
/// ```
/// use ono_protocol::{ClientConfig, Identity};
/// let config = ClientConfig::new("db.example.com").with_identity(Identity::new("william"));
/// assert_eq!(config.host(), "db.example.com");
/// ```
#[derive(Debug, Clone)]
pub struct ClientConfig {
    host: String,
    versions: Vec<u16>,
    providers: Vec<String>,
    capabilities: Vec<String>,
    compression: Vec<String>,
    identity: Identity,
    schemas: Arc<SchemaRegistry>,
    limits: Limits,
    credit_window: u32,
    trust: TrustStore,
    policy: TrustPolicy,
    pty: bool,
    clock: Option<PeerClock>,
}

impl ClientConfig {
    /// A configuration for a link to `host`, which is also the name it is pinned under.
    #[must_use]
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            versions: vec![PROTOCOL_VERSION],
            providers: Vec::new(),
            capabilities: Vec::new(),
            compression: Vec::new(),
            identity: Identity::default(),
            schemas: Arc::new(ono_value::builtin_schemas().clone()),
            limits: Limits::default(),
            credit_window: crate::DEFAULT_CREDIT,
            trust: TrustStore::in_memory(),
            policy: TrustPolicy::default(),
            clock: None,
            pty: false,
        }
    }

    /// States this end's own clock identity, so the far side can tell which clock domain the
    /// events it receives from here belong to (v0.5 §24.2, §25.5).
    #[must_use]
    pub fn with_clock(mut self, clock: PeerClock) -> Self {
        self.clock = Some(clock);
        self
    }

    /// This end's own clock identity, where one was stated.
    #[must_use]
    pub const fn clock(&self) -> Option<&PeerClock> {
        self.clock.as_ref()
    }

    /// The link protocol versions this end will accept.
    #[must_use]
    pub fn with_versions<I: IntoIterator<Item = u16>>(mut self, versions: I) -> Self {
        self.versions = versions.into_iter().collect();
        self
    }

    /// Asks for these providers only. Left unset, the link takes whatever the remote has.
    #[must_use]
    pub fn with_providers<I, S>(mut self, providers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.providers = providers.into_iter().map(Into::into).collect();
        self
    }

    /// Asks for these capabilities only. Left unset, the link takes whatever the remote offers.
    #[must_use]
    pub fn with_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.capabilities = capabilities.into_iter().map(Into::into).collect();
        self
    }

    /// The compressions this end can read, best first.
    #[must_use]
    pub fn with_compression<I, S>(mut self, compression: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.compression = compression.into_iter().map(Into::into).collect();
        self
    }

    /// Who this end is running as.
    #[must_use]
    pub fn with_identity(mut self, identity: Identity) -> Self {
        self.identity = identity;
        self
    }

    /// The schemas this end can decode. A record naming a schema that is not here is refused.
    #[must_use]
    pub fn with_schemas(mut self, schemas: Arc<SchemaRegistry>) -> Self {
        self.schemas = schemas;
        self
    }

    /// The bounds this end enforces on what the remote sends.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// How many messages per stream the remote may send before waiting for credit.
    #[must_use]
    pub const fn with_credit_window(mut self, window: u32) -> Self {
        self.credit_window = window;
        self
    }

    /// The trust store host keys are pinned in.
    #[must_use]
    pub fn with_trust_store(mut self, store: TrustStore) -> Self {
        self.trust = store;
        self
    }

    /// How much this link demands of the peer's identity.
    #[must_use]
    pub const fn with_trust_policy(mut self, policy: TrustPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Declares that this end can host a pseudo-terminal for an interactive remote session.
    #[must_use]
    pub const fn with_pty(mut self, pty: bool) -> Self {
        self.pty = pty;
        self
    }

    /// The host this link is to, as the trust store names it.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The schemas this end can decode.
    #[must_use]
    pub fn schemas(&self) -> &Arc<SchemaRegistry> {
        &self.schemas
    }
}

/// One message arriving on a stream.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteMessage {
    /// A value the remote produced.
    Value(Value),
    /// An object event the remote observed (spec §31.14).
    Event(ObjectEvent),
    /// A failure concerning one item, leaving the stream running (spec §16.5).
    Failure(ErrorValue),
}

/// What the reader task hands to a stream.
///
/// A value travels unboxed because it is the hot path — every object of every remote query goes
/// through here — while the rarer and much larger payloads are boxed, so that a stream of values
/// does not pay for the size of an error record it is not carrying.
#[derive(Debug)]
enum Inbound {
    Value(Value),
    Event(Box<ObjectEvent>),
    Failure(Box<ErrorValue>),
    Outcome(Box<ActionOutcome>),
    LinkTable(Box<crate::LinkTable>),
    End,
}

#[derive(Debug)]
struct Registered {
    sender: mpsc::Sender<Inbound>,
    opened: OpenStream,
}

/// One stream open on a link right now, as the link itself knows it (ADR-0105, ADR-1099).
///
/// The streams a link multiplexes are objects with an identity: the id both ends route by, what
/// opened it, what it asked about and when. `trace link` draws them, so a reader can see what a
/// link is carrying instead of a count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenStream {
    /// The id both ends route the stream's frames by.
    pub id: u32,
    /// What opened it: `query`, `subscribe`, `act`, `adapt` or `probe`.
    pub kind: &'static str,
    /// The target it asked about, where it asked about one.
    pub target: Option<String>,
    /// When this side opened it.
    pub opened_at: jiff::Timestamp,
}

impl OpenStream {
    fn of(id: u32, message: &Message) -> Self {
        let (kind, target) = match message {
            Message::StartQuery(query) => ("query", Some(query.target_name().to_owned())),
            Message::StartSubscribe(query) => ("subscribe", Some(query.target_name().to_owned())),
            Message::Act(request) => ("act", Some(request.target_name().to_owned())),
            Message::StartAdapt(request) => ("adapt", request.argv().first().cloned()),
            Message::Probe => ("probe", None),
            Message::ListLinks => ("links", None),
            other => (other.kind().as_str(), None),
        };
        Self {
            id,
            kind,
            target,
            opened_at: jiff::Timestamp::now(),
        }
    }
}

#[derive(Debug)]
struct LinkInner {
    frames: FrameSink,
    streams: Mutex<HashMap<u32, Registered>>,
    next_id: AtomicU32,
    alive: AtomicBool,
    failure: Mutex<Option<ErrorValue>>,
    limits: Limits,
    window: u32,
    /// Woken when the link fails, for whoever waits on [`Link::closed`].
    gone: tokio::sync::Notify,
}

impl LinkInner {
    fn fail(&self, error: &ErrorValue) {
        self.alive.store(false, Ordering::SeqCst);
        self.gone.notify_waiters();
        if let Ok(mut slot) = self.failure.lock()
            && slot.is_none()
        {
            *slot = Some(error.clone());
        }
        let registered: Vec<Registered> =
            self.with_streams(|streams| streams.drain().map(|(_, r)| r).collect());
        for stream in registered {
            // Best effort: a consumer that is not reading learns of the failure when it does.
            let _ = stream
                .sender
                .try_send(Inbound::Failure(Box::new(error.clone())));
        }
    }

    fn with_streams<T>(&self, body: impl FnOnce(&mut HashMap<u32, Registered>) -> T) -> T {
        let mut guard = match self.streams.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        body(&mut guard)
    }

    fn status(&self) -> Result<(), ErrorValue> {
        if self.alive.load(Ordering::SeqCst) {
            return Ok(());
        }
        let recorded = match self.failure.lock() {
            Ok(slot) => slot.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        Err(recorded.unwrap_or_else(|| unreachable("the link is closed")))
    }

    fn release(&self, id: u32) {
        self.with_streams(|streams| streams.remove(&id));
    }

    fn cancel_stream(&self, id: u32) {
        self.release(id);
        if let Ok(payload) = encode_message(&Message::Cancel, &self.limits) {
            let _ = self.frames.send(Frame::new(FrameKind::Cancel, id, payload));
        }
    }

    fn grant(&self, id: u32, credit: u32) {
        if let Ok(payload) = encode_message(&Message::Credit(credit), &self.limits) {
            let _ = self.frames.send(Frame::new(FrameKind::Credit, id, payload));
        }
    }

    fn open(self: &Arc<Self>, kind: FrameKind, message: &Message) -> Result<Opened, ErrorValue> {
        self.status()?;
        // One more slot than the window, so the `end` that costs no credit always has somewhere
        // to land; a peer that overruns its window still fills the queue and fails the link.
        let (sender, receiver) = mpsc::channel(self.window as usize + 1);
        let id = self.with_streams(|streams| {
            // Checked again under the lock `fail` drains under: `fail` marks the link dead
            // before it drains, so a stream registered here is either drained and told, or
            // never registered. Checked only before the lock, a stream could register just
            // after the drain, on a dead link, and wait for ever.
            if !self.alive.load(Ordering::SeqCst) {
                return Err(self
                    .status()
                    .err()
                    .unwrap_or_else(|| unreachable("the link is closed")));
            }
            if streams.len() >= self.limits.max_streams() {
                return Err(ErrorValue::from(ProtocolError::TooManyStreams {
                    limit: self.limits.max_streams(),
                }));
            }
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);
            streams.insert(
                id,
                Registered {
                    sender,
                    opened: OpenStream::of(id, message),
                },
            );
            Ok(id)
        })?;
        let payload = encode_message(message, &self.limits).map_err(ErrorValue::from)?;
        if self.frames.send(Frame::new(kind, id, payload)).is_err() {
            self.release(id);
            return Err(self
                .status()
                .err()
                .unwrap_or_else(|| unreachable("the link is closed")));
        }
        Ok(Opened {
            id,
            receiver,
            guard: StreamGuard {
                inner: Arc::clone(self),
                id,
                armed: true,
            },
        })
    }
}

/// A stream this side opened and is waiting on.
///
/// Whoever awaits the answer may be dropped mid-wait — a Ctrl-C during `look` or `map links`
/// drops the command's future — and the stream must go with it: the guard cancels it unless the
/// waiter disarmed it by finishing (review PRF-1). Without it the stream stays registered, holds
/// one of `max_streams`, and its late answer meets a receiver that is gone.
struct Opened {
    id: u32,
    receiver: mpsc::Receiver<Inbound>,
    guard: StreamGuard,
}

impl Opened {
    /// The answer arrived: forget the stream; nothing is sent.
    fn release(mut self) {
        self.guard.armed = false;
        self.guard.inner.release(self.id);
    }

    /// The waiter gives up: forget the stream and tell the far side.
    fn cancel(mut self) {
        self.guard.armed = false;
        self.guard.inner.cancel_stream(self.id);
    }

    /// Hands the stream to an owner that cancels it on drop itself ([`RemoteStream`]).
    fn into_parts(mut self) -> (u32, mpsc::Receiver<Inbound>) {
        self.guard.armed = false;
        (self.id, self.receiver)
    }
}

struct StreamGuard {
    inner: Arc<LinkInner>,
    id: u32,
    armed: bool,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        if self.armed {
            self.inner.cancel_stream(self.id);
        }
    }
}

/// The local end of a remote link.
///
/// A link is opened once, and then drives as many concurrent queries, subscriptions and actions
/// as the peer's stream limit allows. Dropping it hangs up: pending control frames are flushed,
/// the transport is shut down, and the remote end observes an ordinary end of session — which
/// is what lets an agent loop finish with success when its caller simply goes away.
#[derive(Debug)]
pub struct Link {
    inner: Arc<LinkInner>,
    /// Unique among the links this process made (see [`Prober::link`]).
    id: u64,
    negotiated: Negotiated,
    /// How long the handshake's one exchange took: the offer out, the answer back.
    round_trip: std::time::Duration,
}

impl Link {
    /// Performs the handshake over `transport` and returns the established link.
    ///
    /// The peer's key is checked **before** the handshake is even offered: spec §21.5 and
    /// ADR-0015 T5 both require identity to be settled before anything else crosses the link, and
    /// a key checked afterwards would already have accepted whatever the peer said.
    ///
    /// # Errors
    ///
    /// - `remote.host_key_changed` (E0603) when the peer presents a key other than the pinned one.
    ///   There is no way past this, by design (ADR-0015 standing rule 4).
    /// - `safety.policy_denied` (E0702) when the trust policy will not accept this peer.
    /// - `remote.protocol_mismatch` (E0602) when no protocol version is shared, or the peer is
    ///   not speaking this protocol at all.
    /// - `remote.unreachable` (E0601) when the transport fails or ends.
    pub async fn connect<T: Transport>(
        transport: T,
        config: ClientConfig,
    ) -> Result<Self, ErrorValue> {
        let key = transport.peer_key().cloned();
        let trust = decide(config.policy, &config.trust, &config.host, key.as_ref())?;
        let fingerprint = key.as_ref().map(crate::HostKey::fingerprint);

        let limits = config.limits.clone();
        let (reader, writer) = tokio::io::split(transport);
        let mut reader = FrameReader::new(reader, limits.clone());
        let frames = spawn_writer(writer, limits.clone());

        let offer = hello(
            config.versions.clone(),
            config.providers.clone(),
            config.schemas.ids().map(ToString::to_string).collect(),
            config.capabilities.clone(),
            config.compression.clone(),
            config.identity.clone(),
            config.credit_window.clamp(1, limits.max_credit()),
            config.pty,
        )
        .announcing(config.clock.clone());
        let payload = encode_message(&Message::Hello(offer), &limits).map_err(ErrorValue::from)?;
        let offered = std::time::Instant::now();
        frames
            .send(Frame::new(FrameKind::Hello, 0, payload))
            .map_err(|_| unreachable("the link closed before the handshake was sent"))?;

        let answer = reader
            .next()
            .await?
            .ok_or_else(|| unreachable("the peer closed the link without answering"))?;
        let round_trip = offered.elapsed();
        let message = decode_message(answer.kind(), answer.payload(), &config.schemas, &limits)
            .map_err(ErrorValue::from)?;
        let accept = match message {
            Message::Accept(accept) => accept,
            Message::Reject(reject) => {
                // The wire carries a code, a sentence and the details the refusal census lets
                // cross it (ADR-1056); the remediation is the same for everyone who meets the
                // code, so this side supplies it rather than the far side describing its own
                // configuration to a peer it just refused (§59.1).
                let refused = reject.to_error();
                return Err(
                    match crate::authorization::refusal_guidance(refused.code()) {
                        Some(help) => refused.with_help(help),
                        None => refused,
                    },
                );
            }
            other => {
                return Err(ErrorValue::from(ProtocolError::MalformedPayload {
                    kind: other.kind(),
                    detail: "a handshake is answered with an accept or a reject".to_owned(),
                }));
            }
        };
        if !config.versions.contains(&accept.version()) {
            return Err(crate::error::version_mismatch(
                &config.versions,
                &[accept.version()],
            ));
        }

        let window = accept.credit_window().clamp(1, limits.max_credit());
        let negotiated = Negotiated::from_accept(accept, trust, fingerprint);
        let inner = Arc::new(LinkInner {
            frames,
            streams: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
            alive: AtomicBool::new(true),
            failure: Mutex::new(None),
            limits: limits.clone(),
            window,
            gone: tokio::sync::Notify::new(),
        });
        spawn_reader(
            Arc::clone(&inner),
            reader,
            Arc::clone(&config.schemas),
            limits,
        );
        static LINKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Ok(Self {
            inner,
            id: LINKS.fetch_add(1, Ordering::Relaxed),
            negotiated,
            round_trip,
        })
    }

    /// How long the handshake's exchange took — the offer written, the answer read.
    ///
    /// The one round trip this side times. It bounds the link's latency from above at the
    /// moment the link was made: it includes the far side beginning to answer, so a link to an
    /// agent that was still starting reads longer than the wire alone would.
    #[must_use]
    pub const fn handshake_round_trip(&self) -> std::time::Duration {
        self.round_trip
    }

    /// Everything the handshake settled (spec §21.2).
    #[must_use]
    pub const fn negotiated(&self) -> &Negotiated {
        &self.negotiated
    }

    /// Opens a stream of the objects `query` matches on the remote machine.
    ///
    /// The stream is returned without a round trip: a query the remote cannot answer arrives as
    /// the stream's first failure rather than as an error from this call, because asking first
    /// would cost a full round trip on every query to learn something the first frame carries
    /// anyway.
    ///
    /// # Errors
    ///
    /// Returns `remote.unreachable` when the link is gone, and `remote.protocol_mismatch` when
    /// the link already has as many streams open as it allows.
    pub fn query(&self, query: &RemoteQuery) -> Result<RemoteStream, ErrorValue> {
        self.start(FrameKind::StartQuery, Message::StartQuery(query.clone()))
    }

    /// Asks the agent to adapt an external invocation (spec v0.3 §1.54): the stream carries
    /// the records the remote decoded, or its refusal as a failure.
    ///
    /// # Errors
    ///
    /// As [`Link::query`].
    pub fn adapt(&self, request: &AdaptRequest) -> Result<RemoteStream, ErrorValue> {
        self.start(FrameKind::StartAdapt, Message::StartAdapt(request.clone()))
    }

    /// Opens a stream of the changes to the objects `query` matches (spec §21.4).
    ///
    /// # Errors
    ///
    /// As [`query`](Self::query).
    pub fn subscribe(&self, query: &RemoteQuery) -> Result<RemoteStream, ErrorValue> {
        self.start(
            FrameKind::StartSubscribe,
            Message::StartSubscribe(query.clone()),
        )
    }

    /// Performs one action on the remote machine and waits for its outcome.
    ///
    /// # Errors
    ///
    /// Returns whatever the remote refused with, or `remote.unreachable` when the link failed
    /// before an answer arrived.
    pub async fn act(&self, request: &ActRequest) -> Result<ActionOutcome, ErrorValue> {
        let mut opened = self
            .inner
            .open(FrameKind::Act, &Message::Act(request.clone()))?;
        let mut refusal = None;
        while let Some(inbound) = opened.receiver.recv().await {
            match inbound {
                Inbound::Outcome(outcome) => {
                    opened.release();
                    return Ok(*outcome);
                }
                Inbound::Failure(error) => refusal = Some(*error),
                Inbound::Value(_) | Inbound::Event(_) | Inbound::LinkTable(_) => {}
                Inbound::End => break,
            }
        }
        opened.release();
        Err(refusal.unwrap_or_else(|| unreachable("the remote ended the action without answering")))
    }

    /// Measures one round trip on the open link (`ono.link.probe/1`, v0.4 §19.1, ADR-1095).
    ///
    /// The agent answers a probe at once and touches no provider, so the figure is the link and
    /// the far side's loop, not whatever a query happened to cost. It is sent only when the
    /// handshake negotiated the probe; otherwise the refusal is local and nothing crosses the
    /// wire (ADR-1056's one rule). A probe that is not answered within `timeout` is released —
    /// its stream is forgotten and cancelled — so an unanswered probe leaves nothing behind.
    ///
    /// # Errors
    ///
    /// - `remote.protocol_mismatch` when the far side did not offer the probe;
    /// - `remote.unreachable` when the link is gone, or no answer came within `timeout`
    ///   (retryable, with `timeout_ms`).
    pub async fn probe(
        &self,
        timeout: std::time::Duration,
    ) -> Result<std::time::Duration, ErrorValue> {
        probe(&self.inner, self.offers(crate::PROBE_CAPABILITY), timeout).await
    }

    /// A handle that probes this link from elsewhere — the link map, a watch — without holding
    /// the link open: once the link is gone, its probes report that (ADR-1095).
    #[must_use]
    pub fn prober(&self) -> Prober {
        Prober {
            inner: Arc::downgrade(&self.inner),
            offered: self.offers(crate::PROBE_CAPABILITY),
            offers_links: self.offers(crate::LINKS_CAPABILITY),
            link: self.id,
        }
    }

    /// Asks the far side for its own link table (`ono.link.links/1`, v0.4 §19.3, ADR-1097).
    ///
    /// Sent only when the handshake negotiated the request; otherwise the refusal is local. The
    /// agent checks the connection may observe before it answers, and a refusal arrives as its
    /// structured error — never as an empty table. A table larger than
    /// [`MAX_LINK_TABLE_ENTRIES`](crate::MAX_LINK_TABLE_ENTRIES), with a field longer than
    /// [`MAX_LINK_FIELD_BYTES`](crate::MAX_LINK_FIELD_BYTES) or a state or transport this build
    /// does not know is refused when it is decoded: this request fails as a protocol mismatch,
    /// and the link and its other streams are unharmed.
    ///
    /// # Errors
    ///
    /// - `remote.protocol_mismatch` when the far side did not offer the request, or answered it
    ///   with something that is not a link table;
    /// - the far side's refusal, `remote.capability_denied` where the connection may not observe;
    /// - `remote.unreachable` when the link is gone or no answer came within `timeout`.
    pub async fn links(
        &self,
        timeout: std::time::Duration,
    ) -> Result<crate::LinkTable, ErrorValue> {
        links(&self.inner, self.offers(crate::LINKS_CAPABILITY), timeout).await
    }

    /// Whether the handshake negotiated the protocol feature `id` (ADR-1056, rule 2).
    #[must_use]
    pub fn offers(&self, id: &str) -> bool {
        self.negotiated
            .capabilities()
            .iter()
            .any(|offered| offered == id)
    }

    /// The streams open on this link right now, in id order (ADR-0105, ADR-1099).
    #[must_use]
    pub fn open_streams(&self) -> Vec<OpenStream> {
        let mut open: Vec<OpenStream> = self.inner.with_streams(|streams| {
            streams
                .values()
                .map(|registered| registered.opened.clone())
                .collect()
        });
        open.sort_by_key(|stream| stream.id);
        open
    }

    /// Resolves once the link has failed or the far side has hung up — what a holder of a link
    /// waits on to know it must make it again. Sends nothing.
    pub async fn closed(&self) {
        loop {
            let gone = self.inner.gone.notified();
            if self.inner.status().is_err() {
                return;
            }
            gone.await;
        }
    }

    /// Whether the link is still up: no transport failure and no hang-up has been seen.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.inner.status().is_ok()
    }

    /// Says goodbye now, without waiting for the link to be dropped.
    ///
    /// Queued frames are flushed and the transport is shut down, so a peer reading a pipe sees
    /// end of input — the hang-up an agent loop ends on. An owner that must know the peer has
    /// gone before it can proceed calls this and then waits for the peer; dropping the link
    /// does the same thing, but only once every [`RemoteStream`] and mounted provider derived
    /// from it has gone too.
    pub fn hangup(&self) {
        self.inner.frames.hangup();
    }

    fn start(&self, kind: FrameKind, message: Message) -> Result<RemoteStream, ErrorValue> {
        let (id, receiver) = self.inner.open(kind, &message)?.into_parts();
        Ok(RemoteStream {
            id,
            receiver,
            link: Arc::clone(&self.inner),
            window: self.inner.window,
            unacknowledged: 0,
            finished: false,
        })
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        // The reader task shares `LinkInner` (and with it the frame sink), so the sink cannot
        // close by going out of scope; the link's owner is the one who says goodbye.
        self.hangup();
    }
}

/// Probes the link behind `inner` (see [`Link::probe`]).
async fn probe(
    inner: &Arc<LinkInner>,
    offered: bool,
    timeout: std::time::Duration,
) -> Result<std::time::Duration, ErrorValue> {
    if !offered {
        return Err(crate::features::not_offered(
            crate::PROBE_CAPABILITY,
            "the probe",
        ));
    }
    let started = std::time::Instant::now();
    let mut opened = inner.open(FrameKind::Probe, &Message::Probe)?;
    let answer = tokio::time::timeout(timeout, opened.receiver.recv()).await;
    match answer {
        Ok(Some(Inbound::End)) => {
            opened.release();
            Ok(started.elapsed())
        }
        Ok(Some(Inbound::Failure(error))) => {
            opened.release();
            Err(*error)
        }
        Ok(Some(_)) => {
            opened.cancel();
            Err(ErrorValue::from(ProtocolError::MalformedPayload {
                kind: FrameKind::Probe,
                detail: "a probe is answered with an end and nothing else".to_owned(),
            }))
        }
        Ok(None) => {
            opened.release();
            Err(inner
                .status()
                .err()
                .unwrap_or_else(|| unreachable("the link closed before the probe was answered")))
        }
        Err(_elapsed) => {
            opened.cancel();
            Err(unreachable(format!(
                "the far side did not answer a probe within {} ms",
                timeout.as_millis()
            ))
            .with_retryable(true)
            .with_metadata(
                "timeout_ms",
                Value::Int(i128::try_from(timeout.as_millis()).unwrap_or(i128::MAX)),
            ))
        }
    }
}

/// Asks the far side behind `inner` for its link table (see [`Link::links`]).
async fn links(
    inner: &Arc<LinkInner>,
    offered: bool,
    timeout: std::time::Duration,
) -> Result<crate::LinkTable, ErrorValue> {
    if !offered {
        return Err(crate::features::not_offered(
            crate::LINKS_CAPABILITY,
            "the request for its link table",
        ));
    }
    let mut opened = inner.open(FrameKind::ListLinks, &Message::ListLinks)?;
    let deadline = tokio::time::Instant::now() + timeout;
    let mut table = None;
    let mut refusal = None;
    loop {
        match tokio::time::timeout_at(deadline, opened.receiver.recv()).await {
            Ok(Some(Inbound::LinkTable(answer))) if table.is_none() => {
                table = Some(*answer);
            }
            Ok(Some(Inbound::Failure(error))) => refusal = Some(*error),
            Ok(Some(Inbound::End)) => {
                opened.release();
                break;
            }
            Ok(Some(_)) => {
                opened.cancel();
                return Err(ErrorValue::from(ProtocolError::MalformedPayload {
                    kind: FrameKind::ListLinks,
                    detail: "a link table request is answered with one table and an end".to_owned(),
                }));
            }
            Ok(None) => {
                opened.release();
                return Err(refusal.unwrap_or_else(|| {
                    inner.status().err().unwrap_or_else(|| {
                        unreachable("the link closed before the link table arrived")
                    })
                }));
            }
            Err(_elapsed) => {
                opened.cancel();
                return Err(unreachable(format!(
                    "the far side did not send its link table within {} ms",
                    timeout.as_millis()
                ))
                .with_retryable(true)
                .with_metadata(
                    "timeout_ms",
                    Value::Int(i128::try_from(timeout.as_millis()).unwrap_or(i128::MAX)),
                ));
            }
        }
    }
    if let Some(refusal) = refusal {
        return Err(refusal);
    }
    table.ok_or_else(|| {
        ErrorValue::from(ProtocolError::MalformedPayload {
            kind: FrameKind::ListLinks,
            detail: "the far side ended the request without a link table".to_owned(),
        })
    })
}

/// A handle for one link that does not keep the link open: it probes the link (ADR-1095) and
/// asks it for the far side's link table (ADR-1097).
///
/// Two handles are equal when they probe the same link: a link made again after it went down is
/// another link, and nothing measured on the first one is a measurement of the second.
#[derive(Debug, Clone)]
pub struct Prober {
    inner: std::sync::Weak<LinkInner>,
    offered: bool,
    offers_links: bool,
    link: u64,
}

impl Prober {
    /// Measures one round trip, as [`Link::probe`] does.
    ///
    /// # Errors
    ///
    /// As [`Link::probe`]; a link that has been dropped is `remote.unreachable`.
    pub async fn probe(
        &self,
        timeout: std::time::Duration,
    ) -> Result<std::time::Duration, ErrorValue> {
        let Some(inner) = self.inner.upgrade() else {
            return Err(unreachable("the link is closed"));
        };
        probe(&inner, self.offered, timeout).await
    }

    /// Whether the link is still up, as far as this side has seen: no transport failure, no
    /// hang-up. Asking sends nothing.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.inner
            .upgrade()
            .is_some_and(|inner| inner.status().is_ok())
    }

    /// Asks the far side for its link table, as [`Link::links`] does (ADR-1097).
    ///
    /// # Errors
    ///
    /// As [`Link::links`]; a link that has been dropped is `remote.unreachable`.
    pub async fn links(
        &self,
        timeout: std::time::Duration,
    ) -> Result<crate::LinkTable, ErrorValue> {
        let Some(inner) = self.inner.upgrade() else {
            return Err(unreachable("the link is closed"));
        };
        links(&inner, self.offers_links, timeout).await
    }

    /// Opens a query on the link, as [`Link::query`] does, without holding the link open
    /// beyond the stream's own life — what a correlation of this host's connections with the far
    /// side's sockets reads through (ADR-1098). The far side authorizes it as any query.
    ///
    /// # Errors
    ///
    /// As [`Link::query`]; a link that has been dropped is `remote.unreachable`.
    pub fn query(&self, query: &RemoteQuery) -> Result<RemoteStream, ErrorValue> {
        let Some(inner) = self.inner.upgrade() else {
            return Err(unreachable("the link is closed"));
        };
        let (id, receiver) = inner
            .open(FrameKind::StartQuery, &Message::StartQuery(query.clone()))?
            .into_parts();
        Ok(RemoteStream {
            id,
            receiver,
            window: inner.window,
            link: inner,
            unacknowledged: 0,
            finished: false,
        })
    }

    /// The streams open on the link right now, as [`Link::open_streams`] lists them; none once
    /// the link is gone.
    #[must_use]
    pub fn open_streams(&self) -> Vec<OpenStream> {
        let Some(inner) = self.inner.upgrade() else {
            return Vec::new();
        };
        let mut open: Vec<OpenStream> = inner.with_streams(|streams| {
            streams
                .values()
                .map(|registered| registered.opened.clone())
                .collect()
        });
        open.sort_by_key(|stream| stream.id);
        open
    }

    /// Whether the far side offered its link table at all.
    #[must_use]
    pub const fn offers_links(&self) -> bool {
        self.offers_links
    }

    /// Whether the far side offered the probe at all.
    #[must_use]
    pub const fn is_offered(&self) -> bool {
        self.offered
    }

    /// Which link this handle probes: unique among the links this process made.
    #[must_use]
    pub const fn link(&self) -> u64 {
        self.link
    }
}

impl PartialEq for Prober {
    fn eq(&self, other: &Self) -> bool {
        self.link == other.link
    }
}

impl Eq for Prober {}

/// One multiplexed stream of results, and the credit that bounds its producer.
///
/// Dropping a stream cancels it, so a remote producer ends the way a local one does when its
/// consumer goes away (ADR-0013: "`yes | head -1` terminates").
#[derive(Debug)]
pub struct RemoteStream {
    id: u32,
    receiver: mpsc::Receiver<Inbound>,
    link: Arc<LinkInner>,
    window: u32,
    unacknowledged: u32,
    finished: bool,
}

impl RemoteStream {
    /// The stream's id on this link.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }

    /// The next message, or `None` when the remote has produced everything it is going to.
    pub async fn recv(&mut self) -> Option<RemoteMessage> {
        loop {
            match self.receiver.recv().await? {
                Inbound::Value(value) => {
                    self.acknowledge();
                    return Some(RemoteMessage::Value(value));
                }
                Inbound::Event(event) => {
                    self.acknowledge();
                    return Some(RemoteMessage::Event(*event));
                }
                Inbound::Failure(error) => {
                    self.acknowledge();
                    return Some(RemoteMessage::Failure(*error));
                }
                Inbound::End => {
                    self.finished = true;
                    self.link.release(self.id);
                    return None;
                }
                // An outcome belongs to an action, and a link table to its request, not to a
                // query stream; ignoring them rather than failing the link keeps one confused
                // stream from taking the others down.
                Inbound::Outcome(_) | Inbound::LinkTable(_) => self.acknowledge(),
            }
        }
    }

    /// Stops the stream. The remote producer observes it at its next send.
    pub fn cancel(&self) {
        if !self.finished {
            self.link.cancel_stream(self.id);
        }
    }

    /// Feeds the stream's values into a pipeline (spec §11.2, ADR-0013).
    ///
    /// The resulting [`ValueStream`] behaves exactly as a local one: its channel is bounded, its
    /// failures ride the error channel beside the values (spec §16.5), and cancelling it stops
    /// the producer — here by cancelling the remote stream, so the machine at the other end stops
    /// too.
    #[must_use]
    pub fn into_value_stream(
        self,
        config: PipelineConfig,
        boundedness: Boundedness,
    ) -> ValueStream {
        ValueStream::spawn(config, boundedness, move |sink| async move {
            let mut stream = self;
            loop {
                let message = tokio::select! {
                    biased;
                    () = sink.cancel_token().cancelled() => break,
                    message = stream.recv() => message,
                };
                let outcome = match message {
                    Some(RemoteMessage::Value(value)) => sink.send(value).await,
                    Some(RemoteMessage::Failure(error)) => sink.fail(error).await,
                    Some(RemoteMessage::Event(_)) => Ok(()),
                    None => break,
                };
                if outcome.is_err() {
                    break;
                }
            }
        })
    }

    /// Feeds the stream's events into a subscription (spec §31.14).
    ///
    /// Only events cross over: [`EventStream`] has no error channel, so a per-item failure has
    /// nowhere truthful to go. Use [`recv`](Self::recv) where the failures matter.
    #[must_use]
    pub fn into_event_stream(self, config: PipelineConfig) -> EventStream {
        EventStream::spawn(config, move |sink| async move {
            let mut stream = self;
            while let Some(message) = stream.recv().await {
                if let RemoteMessage::Event(event) = message
                    && sink.send(event).await.is_err()
                {
                    break;
                }
            }
        })
    }

    /// Grants the remote room for one more message, in half-window batches.
    fn acknowledge(&mut self) {
        self.unacknowledged += 1;
        let batch = (self.window / 2).max(1);
        if self.unacknowledged >= batch {
            self.link.grant(self.id, self.unacknowledged);
            self.unacknowledged = 0;
        }
    }
}

impl Drop for RemoteStream {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Routes every frame the peer sends to the stream it belongs to.
fn spawn_reader<R>(
    inner: Arc<LinkInner>,
    mut reader: FrameReader<R>,
    schemas: Arc<SchemaRegistry>,
    limits: Limits,
) where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
{
    tokio::spawn(async move {
        loop {
            let frame = match reader.next().await {
                Ok(Some(frame)) => frame,
                Ok(None) => {
                    inner.fail(&unreachable("the remote closed the link"));
                    return;
                }
                Err(error) => {
                    inner.fail(&error);
                    return;
                }
            };
            let message = match decode_message(frame.kind(), frame.payload(), &schemas, &limits) {
                Ok(message) => message,
                // A link table this side must not believe — over its bounds, an unknown state —
                // arrived whole in a frame that was read whole: it fails the request it answers,
                // not the link every other stream rides on (reviews COR-4, SEC-4).
                Err(error) if frame.kind() == FrameKind::LinkTable => {
                    Message::Failure(ErrorValue::from(error))
                }
                Err(error) => {
                    inner.fail(&ErrorValue::from(error));
                    return;
                }
            };
            let inbound = match message {
                Message::Value(value) => Inbound::Value(value),
                Message::Event(event) => Inbound::Event(Box::new(event)),
                Message::Failure(error) => Inbound::Failure(Box::new(error)),
                Message::Outcome(outcome) => Inbound::Outcome(Box::new(outcome)),
                Message::LinkTable(table) => Inbound::LinkTable(Box::new(table)),
                Message::End => Inbound::End,
                // Anything else on an established link is the peer restarting a conversation
                // that is already over.
                other => {
                    inner.fail(&ErrorValue::from(ProtocolError::MalformedPayload {
                        kind: other.kind(),
                        detail: "this message only belongs in a handshake".to_owned(),
                    }));
                    return;
                }
            };
            let sender = inner.with_streams(|streams| {
                streams
                    .get(&frame.stream())
                    .map(|registered| registered.sender.clone())
            });
            // A frame for a stream that is gone is a message that crossed with a cancellation.
            // Dropping it is correct; the alternative would make every cancellation a race.
            let Some(sender) = sender else {
                continue;
            };
            match sender.try_send(inbound) {
                Ok(()) => {}
                // The waiter went away between the lookup and the send: the frame crossed with
                // the stream's cancellation, exactly as a frame for a stream already gone did.
                Err(mpsc::error::TrySendError::Closed(_)) => inner.release(frame.stream()),
                Err(mpsc::error::TrySendError::Full(_)) => {
                    inner.fail(&ErrorValue::from(ProtocolError::CreditExceeded {
                        stream: frame.stream(),
                    }));
                    return;
                }
            }
        }
    });
}
