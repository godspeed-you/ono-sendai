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

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use ono_core::ErrorCode;
use ono_pipeline::{CancelToken, SinkClosed};
use ono_provider_api::{ActionOutcome, ObjectEvent};
use ono_value::{ErrorValue, SchemaRegistry, Value};
use tokio::sync::Semaphore;

use crate::audit::{Audit, AuditEvent, AuditKind, NoAudit};
use crate::authorization::{AuthorizationContext, AuthorizedClients, PeerAuthorization};
use crate::connection::{FrameReader, FrameSink, spawn_writer};
use crate::error::unreachable;
use crate::handshake::{CapabilityDescriptor, Identity, Offer, ProviderDescriptor, negotiate};
use crate::message::{ActRequest, AdaptRequest, RemoteQuery};
use crate::{
    Frame, FrameKind, Limits, Message, PROTOCOL_VERSION, ProtocolError, Reject, Transport,
    decode_message, encode_message,
};

/// Who decides which clients an agent serves (v0.4.1 §9.2, §4.3).
///
/// Two variants and no third, because there are exactly two ways a peer arrives: through a
/// carrier that already decided who may run the agent, or through a socket this process opened
/// itself. There is no "authorize everyone" store — the fail-open default §9.2 forbids has no
/// spelling here.
#[derive(Debug, Clone, Default)]
pub enum ServerAuthorization {
    /// The carrier authorized the peer before the agent ran: `ssh <host> ono --agent`, where
    /// OpenSSH decided who may execute the command and `peer_key` is truthfully `None` (§4.3).
    #[default]
    CarriedByTransport,
    /// The operator's `authorized_clients` store decides, and an unlisted client is refused
    /// before provider negotiation (§9.4, §59.1). This is what `--agent --listen` uses.
    Store(Arc<AuthorizedClients>),
}

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
    authorization: ServerAuthorization,
    action_capabilities: BTreeMap<(String, String), String>,
    audit: Audit,
    source_address: Option<String>,
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
            authorization: ServerAuthorization::CarriedByTransport,
            action_capabilities: BTreeMap::new(),
            audit: Arc::new(NoAudit),
            source_address: None,
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

    /// Who decides which clients this agent serves (v0.4.1 §9).
    ///
    /// The default is [`ServerAuthorization::CarriedByTransport`], which is the truth for the
    /// stdio agent §4.3 keeps: it is reached through a carrier that authenticated and authorized
    /// the caller, and it can see no peer key of its own. A listening agent passes a store, and
    /// then §2.2's order holds — proof, trust, policy, negotiation, dispatch.
    #[must_use]
    pub fn with_authorization(mut self, authorization: ServerAuthorization) -> Self {
        self.authorization = authorization;
        self
    }

    /// Declares which capability an action on `target` spelled `operation` needs.
    ///
    /// The serving side resolves this itself, from its own command contracts, and never from
    /// anything the caller put in the request: a capability id a peer supplied would authorize
    /// the peer against its own claim. An action whose capability is not declared here is denied
    /// under a policy, because Appendix C denies an unknown capability id always.
    #[must_use]
    pub fn with_action_capability(
        mut self,
        target: impl Into<String>,
        operation: impl Into<String>,
        capability: impl Into<String>,
    ) -> Self {
        self.action_capabilities
            .insert((target.into(), operation.into()), capability.into());
        self
    }

    /// The capability an `Act` on this target with this operation needs, where one is declared.
    #[must_use]
    pub fn action_capability(&self, target: &str, operation: &str) -> Option<&str> {
        self.action_capabilities
            .get(&(target.to_owned(), operation.to_owned()))
            .map(String::as_str)
    }

    /// Where this connection's audit events go (v0.4.1 §14.1).
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = audit;
        self
    }

    /// Where this connection came from, for the `source_address` field of §14.2.
    #[must_use]
    pub fn with_source_address(mut self, address: impl Into<String>) -> Self {
        self.source_address = Some(address.into());
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

    /// The offer this agent negotiates with, already intersected with the peer's authorization.
    ///
    /// §10.1: "the `Offer` used to negotiate a direct link MUST first be intersected with the
    /// authenticated client's authorization", so an unauthorized capability is *absent* from the
    /// accepted contract rather than advertised and refused later. The inventory is itself
    /// information, and a client that can read the whole capability list of a machine it may only
    /// observe has learned something the policy withheld.
    ///
    /// Filtering the offer is not the enforcement — §10.2 and §65.3 are emphatic that it cannot
    /// be — it is the half that stops the offer from being a disclosure.
    pub(crate) fn offer_for(&self, peer: &PeerAuthorization) -> Offer {
        let providers = self.providers_for(peer);
        let offered: std::collections::BTreeSet<&str> = providers
            .iter()
            .flat_map(|provider| provider.capabilities())
            .map(CapabilityDescriptor::id)
            .collect();
        let capabilities = match peer {
            PeerAuthorization::CarriedByTransport => self.capabilities.clone(),
            // A bare capability name in the agent-wide list carries no risk to judge it by, so it
            // survives only when a provider that survived the filter still declares it. An id no
            // surviving provider names is dropped: fail conservative, as Appendix C's last row
            // asks.
            PeerAuthorization::Policy(_) => self
                .capabilities
                .iter()
                .filter(|id| offered.contains(id.as_str()))
                .cloned()
                .collect(),
        };
        Offer {
            versions: self.versions.clone(),
            providers,
            schemas: self.schemas.ids().map(ToString::to_string).collect(),
            capabilities,
            compression: self.compression.clone(),
            identity: self.identity.clone(),
            pty: self.pty,
            limits: self.limits.clone(),
        }
    }

    fn providers_for(&self, peer: &PeerAuthorization) -> Vec<ProviderDescriptor> {
        let PeerAuthorization::Policy(_) = peer else {
            return self.providers.clone();
        };
        self.providers
            .iter()
            .filter_map(|provider| {
                let declared = provider.capabilities().len();
                let kept: Vec<CapabilityDescriptor> = provider
                    .capabilities()
                    .iter()
                    .filter(|capability| permits(peer, capability))
                    .cloned()
                    .collect();
                // A provider that declared capabilities and kept none is withheld whole: leaving
                // its targets in the offer would advertise a machine's shape to a client that may
                // not ask about it. A provider that declared none withholds nothing either way.
                if declared > 0 && kept.is_empty() {
                    return None;
                }
                Some(provider.clone().with_exact_capabilities(kept))
            })
            .collect()
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
        peer: &PeerAuthorization,
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
        peer: &PeerAuthorization,
        query: RemoteQuery,
        responder: &StreamResponder,
    ) -> Result<(), ErrorValue> {
        let _ = (peer, query, responder);
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
    async fn act(
        &self,
        peer: &PeerAuthorization,
        request: ActRequest,
    ) -> Result<ActionOutcome, ErrorValue> {
        let _ = (peer, request);
        Err(ErrorValue::new(
            ErrorCode::ProviderUnsupported,
            "this agent answers queries only; it does not change anything",
        ))
    }

    /// Adapts an external invocation on this side and streams the records it decodes, or —
    /// when the request says so — describes what would happen (spec v0.3 §1.54).
    ///
    /// The default refuses: an agent without adapters says so rather than running anything.
    async fn adapt(
        &self,
        peer: &PeerAuthorization,
        request: AdaptRequest,
        responder: &StreamResponder,
    ) -> Result<(), ErrorValue> {
        let _ = (peer, request, responder);
        Err(ErrorValue::new(
            ErrorCode::ProviderUnsupported,
            "this agent does not adapt external commands",
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

/// Refuses a connection that completed the cryptographic handshake, saying why (§54.1, §12.1).
///
/// The peer has an authenticated channel, so it can be told which boundary decided rather than
/// meeting a socket that closes for no stated reason. It is told that and nothing else: no
/// provider, no schema, no capability, no target, which is §59.1's rule for a refusal made before
/// negotiation.
///
/// The caller's opening `Hello` is read and discarded first. A refusal written into a connection
/// whose other direction is still full would race the peer's own write into a reset, and the
/// reason for the refusal would be the thing that got lost.
///
/// # Errors
///
/// `remote.protocol_mismatch` when the refusal cannot be encoded, which is a defect on this side.
pub async fn refuse<T: Transport>(
    transport: T,
    refusal: &ErrorValue,
    limits: &Limits,
) -> Result<(), ErrorValue> {
    let (reader, writer) = tokio::io::split(transport);
    let mut reader = FrameReader::new(reader, limits.clone());
    let frames = spawn_writer(writer, limits.clone());
    let _ = tokio::time::timeout(limits.handshake_timeout(), reader.next()).await;
    let reject = Reject::new(refusal.code(), refusal.message().to_owned());
    let payload = encode_message(&Message::Reject(reject), limits).map_err(ErrorValue::from)?;
    let _ = frames.send(Frame::new(FrameKind::Reject, 0, payload));
    frames.hangup();
    // Waiting for the peer to go is what makes the refusal observable rather than a race with
    // the socket closing under it.
    let _ = tokio::time::timeout(limits.handshake_timeout(), async {
        while matches!(reader.next().await, Ok(Some(_))) {}
    })
    .await;
    Ok(())
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
    // Read before the stream is split: the peer key is what the transport verified, and it is the
    // only identity §2.2 lets a policy be resolved from.
    let peer_key = transport.peer_key().cloned();
    let (reader, writer) = tokio::io::split(transport);
    let mut reader = FrameReader::new(reader, limits.clone());
    let frames = spawn_writer(writer, limits.clone());

    // §12.2: TLS and *Ono protocol negotiation* together have a deadline. TLS is bounded by the
    // listener that accepted the socket; this is the other half — a peer that completed the
    // cryptographic handshake and then says nothing must not hold the connection open for ever.
    let opening = tokio::time::timeout(limits.handshake_timeout(), reader.next())
        .await
        .map_err(|_| {
            ErrorValue::new(
                ErrorCode::RemoteHandshakeTimeout,
                format!(
                    "the caller did not say hello within {} seconds",
                    limits.handshake_timeout().as_secs_f64()
                ),
            )
            .with_retryable(true)
        })??
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

    let audit = Arc::clone(&config.audit);
    let address = config.source_address.clone();
    let fingerprint = peer_key.as_ref().map(crate::HostKey::fingerprint);
    let record = |event: AuditEvent| {
        let mut event = event.with_source_address(address.as_deref());
        if let Some(fingerprint) = fingerprint {
            event = event.with_peer(fingerprint);
        }
        audit.record(&event);
    };

    let authorization = match resolve(&config.authorization, peer_key.as_ref()) {
        Ok(authorization) => authorization,
        Err(refusal) => {
            // §59.1: the refusal happens after the cryptographic handshake and *before* provider
            // negotiation, and it discloses nothing beyond the rejection itself — no provider,
            // no schema, no capability, no target.
            let kind = if refusal.code() == ErrorCode::RemoteUnauthenticated {
                AuditKind::ClientVerificationFailed
            } else {
                AuditKind::UnknownClientRefused
            };
            record(AuditEvent::new(kind, "unaccepted", "denied").with_error_code(refusal.code()));
            let reject = Reject::new(refusal.code(), refusal.message().to_owned());
            let payload =
                encode_message(&Message::Reject(reject), &limits).map_err(ErrorValue::from)?;
            let _ = frames.send(Frame::new(FrameKind::Reject, 0, payload));
            return Ok(());
        }
    };
    let connection_id = authorization.context().map_or_else(
        || "carried".to_owned(),
        |context| context.connection_id().to_owned(),
    );
    let label = authorization
        .context()
        .and_then(AuthorizationContext::client_label)
        .map(ToOwned::to_owned);
    let event = |kind: AuditKind, result: &'static str| {
        AuditEvent::new(kind, connection_id.clone(), result).with_label(label.as_deref())
    };

    let accept = match negotiate(&hello, &config.offer_for(&authorization)) {
        Ok(accept) => accept,
        Err(reject) => {
            record(
                event(AuditKind::ProtocolMismatch, "denied")
                    .with_error_code(ErrorCode::RemoteProtocolMismatch),
            );
            let payload =
                encode_message(&Message::Reject(reject), &limits).map_err(ErrorValue::from)?;
            let _ = frames.send(Frame::new(FrameKind::Reject, 0, payload));
            return Ok(());
        }
    };
    record(event(AuditKind::ConnectionAccepted, "allowed").with_protocol_version(accept.version()));
    let window = accept.credit_window();
    let payload = encode_message(&Message::Accept(accept), &limits).map_err(ErrorValue::from)?;
    frames
        .send(Frame::new(FrameKind::Accept, 0, payload))
        .map_err(|_| unreachable("the link closed before the handshake was answered"))?;

    let service = Arc::new(service);
    let streams: Streams = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let Some(frame) = reader.next().await? else {
            record(event(AuditKind::ClientDisconnected, "ended"));
            return Ok(());
        };
        let id = frame.stream();
        let message = decode_message(frame.kind(), frame.payload(), &schemas, &limits)
            .map_err(ErrorValue::from)?;
        match message {
            Message::StartQuery(query) => {
                // §10.2: the offer was already filtered, and that is not the enforcement. This
                // check runs on the dispatch path, from the connection's own context, whatever
                // the negotiation happened to contain — §65.3 names hiding a capability in
                // `Accept` and executing a forged request for it as a failure mode.
                let refusal = authorization
                    .require_observe(&format!("get {}", query.target_name()))
                    .err();
                if let Some(refusal) = &refusal {
                    record(
                        event(AuditKind::AuthorizationDenied, "denied")
                            .with_requested_capability(format!("get {}", query.target_name()))
                            .with_error_code(refusal.code()),
                    );
                }
                let authorization = authorization.clone();
                start(&streams, &frames, &limits, id, window, |responder| {
                    let service = Arc::clone(&service);
                    async move {
                        if let Some(refusal) = refusal {
                            return Some(refusal);
                        }
                        service.query(&authorization, query, &responder).await.err()
                    }
                });
            }
            Message::StartSubscribe(query) => {
                let refusal = authorization
                    .require_observe(&format!("watch {}", query.target_name()))
                    .err();
                if let Some(refusal) = &refusal {
                    record(
                        event(AuditKind::AuthorizationDenied, "denied")
                            .with_requested_capability(format!("watch {}", query.target_name()))
                            .with_error_code(refusal.code()),
                    );
                }
                let authorization = authorization.clone();
                start(&streams, &frames, &limits, id, window, |responder| {
                    let service = Arc::clone(&service);
                    async move {
                        if let Some(refusal) = refusal {
                            return Some(refusal);
                        }
                        service
                            .subscribe(&authorization, query, &responder)
                            .await
                            .err()
                    }
                });
            }
            Message::StartAdapt(request) => {
                // Adapting runs a program of the caller's choosing on this host. No entry in
                // `docs/contracts/capabilities.yaml` names it, so no grant can name it either, and a
                // policy-governed connection is refused: §9.4's observe-only default does not
                // include running things, and Appendix C denies what it cannot name.
                let refusal = match &authorization {
                    PeerAuthorization::CarriedByTransport => None,
                    PeerAuthorization::Policy(_) => authorization
                        .require_action(None, &format!("adapt {}", request.argv().join(" ")))
                        .err(),
                };
                if let Some(refusal) = &refusal {
                    record(
                        event(AuditKind::AuthorizationDenied, "denied")
                            .with_requested_capability("adapt")
                            .with_error_code(refusal.code()),
                    );
                }
                let authorization = authorization.clone();
                start(&streams, &frames, &limits, id, window, |responder| {
                    let service = Arc::clone(&service);
                    async move {
                        if let Some(refusal) = refusal {
                            return Some(refusal);
                        }
                        service
                            .adapt(&authorization, request, &responder)
                            .await
                            .err()
                    }
                });
            }
            Message::Act(request) => {
                // The capability is resolved from this agent's own contracts, never from the
                // request: a peer that could name the capability it needs would be authorizing
                // itself (§65.2). An action this agent cannot name is denied (Appendix C).
                let refusal = match &authorization {
                    PeerAuthorization::CarriedByTransport => None,
                    PeerAuthorization::Policy(_) => authorization
                        .require_action(
                            config.action_capability(request.target_name(), request.operation()),
                            &format!("{} {}", request.operation(), request.target_name()),
                        )
                        .err(),
                };
                // §14.1's last bullet: the request and its result, for every authorized action.
                let needed = config
                    .action_capability(request.target_name(), request.operation())
                    .map_or_else(
                        || format!("{} {}", request.operation(), request.target_name()),
                        ToOwned::to_owned,
                    );
                record(match &refusal {
                    Some(refusal) => event(AuditKind::AuthorizationDenied, "denied")
                        .with_requested_capability(needed.clone())
                        .with_error_code(refusal.code()),
                    None => event(AuditKind::ActionRequested, "allowed")
                        .with_requested_capability(needed.clone()),
                });
                let authorization = authorization.clone();
                start(&streams, &frames, &limits, id, window, |responder| {
                    let service = Arc::clone(&service);
                    async move {
                        if let Some(refusal) = refusal {
                            return Some(refusal);
                        }
                        match service.act(&authorization, request).await {
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

/// Whether one declared capability is inside what this peer may use (§9.4, §9.6).
///
/// Two rules, and the second is the one that stops a risk class from being a back door: a
/// capability that needs elevation requires an exact grant even when its risk reads as a plain
/// read, because §9.6 says elevation and destructiveness "MUST require exact explicit grant even
/// if a future policy profile otherwise allows mutations".
fn permits(peer: &PeerAuthorization, capability: &CapabilityDescriptor) -> bool {
    use ono_provider_api::Risk;
    let observation = matches!(capability.risk(), Risk::Read | Risk::Observe);
    if observation && !capability.needs_elevation() {
        return peer.allows_observe();
    }
    peer.allows_action(capability.id())
}

/// Decides what this connection may do, once, from what the transport proved (§2.2, §10.3).
///
/// # Errors
///
/// `remote.unauthenticated` when the transport authenticated nobody and a store was configured,
/// and `remote.unauthorized` when the authenticated client is not listed.
fn resolve(
    authorization: &ServerAuthorization,
    peer_key: Option<&crate::HostKey>,
) -> Result<PeerAuthorization, ErrorValue> {
    let ServerAuthorization::Store(store) = authorization else {
        return Ok(PeerAuthorization::CarriedByTransport);
    };
    let Some(key) = peer_key else {
        return Err(crate::authorization::unauthenticated_refusal());
    };
    let context: AuthorizationContext = store.authorize(key.fingerprint())?;
    Ok(PeerAuthorization::Policy(Arc::new(context)))
}
