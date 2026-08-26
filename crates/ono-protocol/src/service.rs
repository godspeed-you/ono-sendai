//! The remote end of a link: what an agent implements, and the loop that drives it.
//!
//! [`serve`] is the transport half of spec §21.4's `ono-agent`. It performs the handshake,
//! multiplexes streams, enforces the bounds of ADR-0015 T7 on everything the caller sends, and
//! hands each request to a [`RemoteService`]. What the service does — which providers it consults,
//! what it is allowed to do — is the agent's business and not this crate's.
//!
//! A producer writes through a [`StreamResponder`], whose `send` waits for credit. That is where
//! spec §11.2's backpressure reaches across the machine boundary: an endless provider on the
//! remote host runs exactly as fast as the local consumer drains it, because it cannot put a
//! value on the wire without room having been granted for it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ono_core::ErrorCode;
use ono_pipeline::{CancelToken, SinkClosed};
use ono_provider_api::{ActionOutcome, ObjectEvent};
use ono_value::{ErrorValue, SchemaRegistry, Value};
use tokio::sync::Semaphore;

use crate::connection::{FrameReader, FrameSink, spawn_writer};
use crate::error::unreachable;
use crate::handshake::{Identity, Offer, ProviderDescriptor, negotiate};
use crate::message::{ActRequest, RemoteQuery};
use crate::{
    Frame, FrameKind, Limits, Message, PROTOCOL_VERSION, ProtocolError, Transport, decode_message,
    encode_message,
};

/// What an agent offers, and what it enforces on the caller.
///
/// ```
/// use ono_protocol::{Identity, ProviderDescriptor, ServerConfig};
/// let config = ServerConfig::new()
///     .with_identity(Identity::new("ono-agent"))
///     .with_provider(ProviderDescriptor::new("linux.procfs").with_targets(["process"]));
/// assert_eq!(config.providers().len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct ServerConfig {
    versions: Vec<u16>,
    providers: Vec<ProviderDescriptor>,
    capabilities: Vec<String>,
    compression: Vec<String>,
    identity: Identity,
    schemas: Arc<SchemaRegistry>,
    limits: Limits,
    pty: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            versions: vec![PROTOCOL_VERSION],
            providers: Vec::new(),
            capabilities: Vec::new(),
            compression: Vec::new(),
            identity: Identity::default(),
            schemas: Arc::new(ono_value::builtin_schemas().clone()),
            limits: Limits::default(),
            pty: false,
        }
    }
}

impl ServerConfig {
    /// The default configuration: this build's protocol version, no providers, no compression.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The link protocol versions this agent speaks.
    #[must_use]
    pub fn with_versions<I: IntoIterator<Item = u16>>(mut self, versions: I) -> Self {
        self.versions = versions.into_iter().collect();
        self
    }

    /// Announces one provider, and whether it can answer here.
    #[must_use]
    pub fn with_provider(mut self, provider: ProviderDescriptor) -> Self {
        self.providers.push(provider);
        self
    }

    /// Announces what this agent is allowed to do.
    #[must_use]
    pub fn with_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.capabilities = capabilities.into_iter().map(Into::into).collect();
        self
    }

    /// Announces the compressions this agent can write.
    #[must_use]
    pub fn with_compression<I, S>(mut self, compression: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.compression = compression.into_iter().map(Into::into).collect();
        self
    }

    /// Who the agent is running as (spec §21.5: least privilege, and visibly so).
    #[must_use]
    pub fn with_identity(mut self, identity: Identity) -> Self {
        self.identity = identity;
        self
    }

    /// The schemas the agent produces and can decode.
    #[must_use]
    pub fn with_schemas(mut self, schemas: Arc<SchemaRegistry>) -> Self {
        self.schemas = schemas;
        self
    }

    /// The bounds the agent enforces on the caller.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Declares that the agent can supply a pseudo-terminal for an interactive session.
    #[must_use]
    pub const fn with_pty(mut self, pty: bool) -> Self {
        self.pty = pty;
        self
    }

    /// The providers this agent announces.
    #[must_use]
    pub fn providers(&self) -> &[ProviderDescriptor] {
        &self.providers
    }

    fn offer(&self) -> Offer {
        Offer {
            versions: self.versions.clone(),
            providers: self.providers.clone(),
            schemas: self.schemas.ids().map(ToString::to_string).collect(),
            capabilities: self.capabilities.clone(),
            compression: self.compression.clone(),
            identity: self.identity.clone(),
            pty: self.pty,
            limits: self.limits.clone(),
        }
    }
}

/// What the remote end of a link answers with.
///
/// The three methods are the primitives of spec §31.14 as a link sees them: state now, changes
/// over time, and a change to make. An implementation usually forwards each to a local
/// [`Provider`](ono_provider_api::Provider); nothing here assumes it does.
#[async_trait::async_trait]
pub trait RemoteService: Send + Sync + 'static {
    /// Answers a query, sending each object through `responder`.
    ///
    /// # Errors
    ///
    /// Returns a structured error when the query cannot be answered at all. A failure concerning
    /// one object belongs on [`StreamResponder::fail`] instead, so the objects that could be read
    /// still arrive (spec §16.5).
    async fn query(
        &self,
        query: RemoteQuery,
        responder: &StreamResponder,
    ) -> Result<(), ErrorValue>;

    /// Answers a subscription, sending each change through `responder`.
    ///
    /// # Errors
    ///
    /// The default reports `provider.unsupported`: an agent that cannot watch must say so rather
    /// than quietly answering nothing (spec §18.2).
    async fn subscribe(
        &self,
        query: RemoteQuery,
        responder: &StreamResponder,
    ) -> Result<(), ErrorValue> {
        let _ = (query, responder);
        Err(ErrorValue::new(
            ErrorCode::ProviderUnsupported,
            "this agent answers queries only; it cannot watch for changes",
        ))
    }

    /// Performs one action and reports exactly what happened (spec §11.5).
    ///
    /// # Errors
    ///
    /// Returns a structured error only when the action could not be attempted at all; an action
    /// that was attempted and failed is an [`ActionOutcome`], not an error.
    async fn act(&self, request: ActRequest) -> Result<ActionOutcome, ErrorValue> {
        let _ = request;
        Err(ErrorValue::new(
            ErrorCode::ProviderUnsupported,
            "this agent answers queries only; it does not change anything",
        ))
    }
}

/// Where a remote producer writes, and where it waits when its consumer is behind.
#[derive(Debug, Clone)]
pub struct StreamResponder {
    id: u32,
    frames: FrameSink,
    credit: Arc<Semaphore>,
    cancel: CancelToken,
    limits: Limits,
}

impl StreamResponder {
    /// The stream this responder writes to.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }

    /// Whether the caller has stopped reading this stream.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// The cancellation scope of this stream, so a provider can join its own work to it.
    #[must_use]
    pub const fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// Sends one value, waiting while the caller is behind.
    ///
    /// # Errors
    ///
    /// Returns [`SinkClosed`] when the stream was cancelled or the link is gone — the signal for
    /// a provider to stop producing, exactly as it is for a local one.
    pub async fn send(&self, value: Value) -> Result<(), SinkClosed> {
        self.emit(Message::Value(value)).await
    }

    /// Sends one object event, waiting while the caller is behind.
    ///
    /// # Errors
    ///
    /// As [`send`](Self::send).
    pub async fn send_event(&self, event: ObjectEvent) -> Result<(), SinkClosed> {
        self.emit(Message::Event(event)).await
    }

    /// Reports a failure concerning one item, leaving the stream running (spec §16.5).
    ///
    /// # Errors
    ///
    /// As [`send`](Self::send).
    pub async fn fail(&self, error: ErrorValue) -> Result<(), SinkClosed> {
        self.emit(Message::Failure(error)).await
    }

    async fn send_outcome(&self, outcome: ActionOutcome) -> Result<(), SinkClosed> {
        self.emit(Message::Outcome(outcome)).await
    }

    /// Spends one unit of the caller's credit, or reports the stream as closed.
    async fn spend(&self) -> Result<(), SinkClosed> {
        if self.cancel.is_cancelled() {
            return Err(SinkClosed);
        }
        tokio::select! {
            // Biased, so a cancelled stream stops rather than racing the credit for one more
            // value; cancellation that only usually wins is cancellation nobody can trust.
            biased;
            () = self.cancel.cancelled() => Err(SinkClosed),
            permit = self.credit.acquire() => match permit {
                Ok(permit) => {
                    // The grant is consumed, not returned: the caller re-grants as it reads.
                    permit.forget();
                    Ok(())
                }
                Err(_) => Err(SinkClosed),
            },
        }
    }

    async fn emit(&self, message: Message) -> Result<(), SinkClosed> {
        self.spend().await?;
        let (kind, payload) = match encode_message(&message, &self.limits) {
            Ok(payload) => (message.kind(), payload),
            Err(error) => {
                // A value too large to frame is reported as a failure of that item rather than
                // dropped, so a caller never silently receives fewer objects than exist.
                let replacement = Message::Failure(ErrorValue::from(error));
                let payload = encode_message(&replacement, &self.limits).map_err(|_| SinkClosed)?;
                (replacement.kind(), payload)
            }
        };
        self.frames
            .send(Frame::new(kind, self.id, payload))
            .map_err(|_| SinkClosed)
    }

    /// Ends the stream. Costs no credit: a caller must always be able to learn that it is over.
    fn finish(&self) {
        if let Ok(payload) = encode_message(&Message::End, &self.limits) {
            let _ = self
                .frames
                .send(Frame::new(FrameKind::End, self.id, payload));
        }
    }
}

#[derive(Debug)]
struct ServerStream {
    cancel: CancelToken,
    credit: Arc<Semaphore>,
}

type Streams = Arc<Mutex<HashMap<u32, ServerStream>>>;

fn with_streams<T>(
    streams: &Streams,
    body: impl FnOnce(&mut HashMap<u32, ServerStream>) -> T,
) -> T {
    let mut guard = match streams.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    body(&mut guard)
}

/// Answers one link with `service` until the caller disconnects.
///
/// The handshake happens first (spec §21.2); a caller that shares no protocol version is refused
/// with a [`Reject`](crate::Reject) and the connection ends, which is a successful outcome for
/// this function — the agent did its job.
///
/// # Errors
///
/// Returns `remote.protocol_mismatch` when the caller is not speaking this protocol, and
/// `remote.unreachable` when the transport fails or ends mid-frame.
pub async fn serve<T, S>(transport: T, config: ServerConfig, service: S) -> Result<(), ErrorValue>
where
    T: Transport,
    S: RemoteService,
{
    let limits = config.limits.clone();
    let schemas = Arc::clone(&config.schemas);
    let (reader, writer) = tokio::io::split(transport);
    let mut reader = FrameReader::new(reader, limits.clone());
    let frames = spawn_writer(writer, limits.clone());

    let opening = reader
        .next()
        .await?
        .ok_or_else(|| unreachable("the caller closed the link before saying hello"))?;
    let Message::Hello(hello) =
        decode_message(opening.kind(), opening.payload(), &schemas, &limits)
            .map_err(ErrorValue::from)?
    else {
        return Err(ErrorValue::from(ProtocolError::MalformedPayload {
            kind: opening.kind(),
            detail: "a link opens with a hello".to_owned(),
        }));
    };

    let accept = match negotiate(&hello, &config.offer()) {
        Ok(accept) => accept,
        Err(reject) => {
            let payload =
                encode_message(&Message::Reject(reject), &limits).map_err(ErrorValue::from)?;
            let _ = frames.send(Frame::new(FrameKind::Reject, 0, payload));
            return Ok(());
        }
    };
    let window = accept.credit_window();
    let payload = encode_message(&Message::Accept(accept), &limits).map_err(ErrorValue::from)?;
    frames
        .send(Frame::new(FrameKind::Accept, 0, payload))
        .map_err(|_| unreachable("the link closed before the handshake was answered"))?;

    let service = Arc::new(service);
    let streams: Streams = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let Some(frame) = reader.next().await? else {
            return Ok(());
        };
        let id = frame.stream();
        let message = decode_message(frame.kind(), frame.payload(), &schemas, &limits)
            .map_err(ErrorValue::from)?;
        match message {
            Message::StartQuery(query) => {
                start(&streams, &frames, &limits, id, window, |responder| {
                    let service = Arc::clone(&service);
                    async move { service.query(query, &responder).await.err() }
                });
            }
            Message::StartSubscribe(query) => {
                start(&streams, &frames, &limits, id, window, |responder| {
                    let service = Arc::clone(&service);
                    async move { service.subscribe(query, &responder).await.err() }
                });
            }
            Message::Act(request) => {
                start(&streams, &frames, &limits, id, window, |responder| {
                    let service = Arc::clone(&service);
                    async move {
                        match service.act(request).await {
                            Ok(outcome) => {
                                let _ = responder.send_outcome(outcome).await;
                                None
                            }
                            Err(error) => Some(error),
                        }
                    }
                });
            }
            Message::Cancel => {
                if let Some(stream) = with_streams(&streams, |open| open.remove(&id)) {
                    stream.cancel.cancel();
                    // Closing the window wakes a producer that is waiting for credit it will
                    // never be granted; without it, cancelling a stalled stream would do nothing.
                    stream.credit.close();
                }
            }
            Message::Credit(granted) => {
                with_streams(&streams, |open| {
                    if let Some(stream) = open.get(&id) {
                        grant(&stream.credit, granted, &limits);
                    }
                });
            }
            other => {
                return Err(ErrorValue::from(ProtocolError::MalformedPayload {
                    kind: other.kind(),
                    detail: "an agent is asked, it is not told".to_owned(),
                }));
            }
        }
    }
}

/// Adds credit without ever letting the counter run away from a peer that keeps granting.
fn grant(credit: &Semaphore, granted: u32, limits: &Limits) {
    let ceiling = limits.max_credit() as usize;
    let room = ceiling.saturating_sub(credit.available_permits());
    credit.add_permits((granted as usize).min(room));
}

/// Registers a stream and runs `body` on it, ending the stream when the body returns.
fn start<F, Fut>(
    streams: &Streams,
    frames: &FrameSink,
    limits: &Limits,
    id: u32,
    window: u32,
    body: F,
) where
    F: FnOnce(StreamResponder) -> Fut,
    Fut: std::future::Future<Output = Option<ErrorValue>> + Send + 'static,
{
    let responder = StreamResponder {
        id,
        frames: frames.clone(),
        credit: Arc::new(Semaphore::new(window as usize)),
        cancel: CancelToken::new(),
        limits: limits.clone(),
    };
    let admitted = with_streams(streams, |open| {
        if open.len() >= limits.max_streams() || open.contains_key(&id) {
            return false;
        }
        open.insert(
            id,
            ServerStream {
                cancel: responder.cancel.clone(),
                credit: Arc::clone(&responder.credit),
            },
        );
        true
    });
    if !admitted {
        let refusal = ErrorValue::from(ProtocolError::TooManyStreams {
            limit: limits.max_streams(),
        });
        if let Ok(payload) = encode_message(&Message::Failure(refusal), limits) {
            let _ = frames.send(Frame::new(FrameKind::Failure, id, payload));
        }
        responder.finish();
        return;
    }
    let future = body(responder.clone());
    let streams = Arc::clone(streams);
    tokio::spawn(async move {
        if let Some(error) = future.await {
            let _ = responder.fail(error).await;
        }
        responder.finish();
        with_streams(&streams, |open| open.remove(&responder.id));
    });
}
