//! The accept loop of an exposed agent, and the ceilings it keeps (v0.4.1 §11, §12).
//!
//! An agent a person starts with `--listen` is a service strangers can reach. §12 is one section
//! about that fact, in six parts: a global ceiling on connections (§12.1), a separate ceiling and
//! a timeout on connections that are still negotiating (§12.2), a ceiling per authenticated
//! client key (§12.3), the bounds the wire already had (§12.4), what an operator's revocation
//! does to a session already running (§12.5), and the rule that one bad peer never takes the
//! listener with it (§12.6).
//!
//! # The two gates, and why there are two
//!
//! A peer costs the agent something the moment it completes TCP, and costs it a great deal more
//! once TLS starts. So the ceilings are applied at the two different moments, for two different
//! reasons:
//!
//! - **At accept**, before any cryptography is spent: is there a pending-handshake slot? Sixteen
//!   by default. A peer refused here is dropped without an answer, because a peer that has not
//!   completed TLS has no authenticated channel to be told anything over (§13.1). This is the
//!   gate that stops a flood.
//! - **After TLS**, when the peer's fingerprint is a fact this process verified: is the agent
//!   under its global ceiling, and is this fingerprint under its own? A peer refused here gets a
//!   stable `remote.connection_limit`, because §54.1 wants a refusal that says which boundary
//!   decided.
//!
//! §12.1 asks that the global ceiling "include connections that completed TCP accept but are
//! still in TLS/protocol handshake state, using a separate handshake semaphore if required to
//! prevent handshake exhaustion". The separate semaphore is what the sentence sanctions and what
//! this module uses, so the sockets an agent holds are bounded by
//! `max_connections + max_pending_handshakes` and never by the number of peers that dialled.
//!
//! # The registry is not only a counter
//!
//! Counting would be enough for §12.1 and §12.3. §12.5 asks for more: when an operator removes a
//! client key, the sessions that key already has should end within five seconds. That needs a
//! handle on each live connection, not a number — which is why the per-fingerprint ceiling and
//! live revocation arrive together (ADR-0505).

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

use ono_core::ErrorCode;
use ono_protocol::{
    Audit, AuditEvent, AuditKind, AuthorizedClients, Fingerprint, Limits, NoAudit,
    ServerAuthorization,
};
use ono_value::{ErrorValue, Value};
use tokio::sync::oneshot;

use crate::agent::{AgentConfig, serve_registry};
use crate::tls::TlsListener;

/// Where a ceiling was reached, for the diagnostic and the audit record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ceiling {
    /// The agent is holding as many connections as it may (§12.1).
    Agent,
    /// This fingerprint is holding as many as it may (§12.3).
    Client,
    /// As many peers are negotiating as may negotiate at once (§12.2).
    PendingHandshakes,
}

impl Ceiling {
    /// The stable word the refusal and the audit record use.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Client => "client",
            Self::PendingHandshakes => "pending_handshakes",
        }
    }
}

/// One live connection, as the registry holds it.
#[derive(Debug)]
struct Live {
    fingerprint: Fingerprint,
    /// Dropped or sent on to end the session (§12.5). `None` once it has been used.
    close: Option<oneshot::Sender<()>>,
}

#[derive(Debug, Default)]
struct State {
    next_id: u64,
    live: HashMap<u64, Live>,
    pending: u32,
}

/// The connections one listening agent is holding, and the ceilings it holds them under.
///
/// Shared by the accept loop, every connection task and the revocation sweep, so the counts a
/// refusal is decided from are the counts the sessions maintain.
#[derive(Debug)]
pub struct ConnectionRegistry {
    limits: Limits,
    state: Mutex<State>,
}

impl ConnectionRegistry {
    /// A registry enforcing `limits`.
    pub(crate) fn new(limits: Limits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            state: Mutex::new(State::default()),
        })
    }

    /// The ceilings this registry keeps.
    #[must_use]
    pub const fn limits(&self) -> &Limits {
        &self.limits
    }

    /// How many authenticated connections are live.
    #[must_use]
    pub fn live(&self) -> usize {
        self.locked().live.len()
    }

    /// How many peers are negotiating.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.locked().pending as usize
    }

    /// How many live connections `fingerprint` holds.
    #[must_use]
    pub fn live_for(&self, fingerprint: Fingerprint) -> usize {
        self.locked()
            .live
            .values()
            .filter(|live| live.fingerprint == fingerprint)
            .count()
    }

    /// Takes a pending-handshake slot, or reports that there is none (§12.2).
    ///
    /// The slot is released when the returned guard is dropped, whatever ended the handshake —
    /// success, refusal, timeout or a panic on the task holding it (§12.6).
    pub(crate) fn begin_handshake(self: &Arc<Self>) -> Result<HandshakeSlot, ErrorValue> {
        let mut state = self.locked();
        if state.pending >= self.limits.max_pending_handshakes() {
            return Err(at_ceiling(
                Ceiling::PendingHandshakes,
                self.limits.max_pending_handshakes(),
            ));
        }
        state.pending += 1;
        drop(state);
        Ok(HandshakeSlot {
            registry: Arc::clone(self),
        })
    }

    /// Admits an authenticated peer, or reports which ceiling refuses it (§12.1, §12.3).
    ///
    /// The returned receiver resolves when the connection is asked to end — because the operator
    /// revoked the client's authorization, or because the agent is shutting the session down.
    ///
    /// # Errors
    ///
    /// `remote.connection_limit` (E1501), naming the ceiling that was reached and its figure.
    pub(crate) fn admit(
        self: &Arc<Self>,
        fingerprint: Fingerprint,
    ) -> Result<(ConnectionSlot, oneshot::Receiver<()>), ErrorValue> {
        let mut state = self.locked();
        if state.live.len() >= self.limits.max_connections() as usize {
            return Err(at_ceiling(Ceiling::Agent, self.limits.max_connections()));
        }
        let held = state
            .live
            .values()
            .filter(|live| live.fingerprint == fingerprint)
            .count();
        if held >= self.limits.max_connections_per_client() as usize {
            return Err(at_ceiling(
                Ceiling::Client,
                self.limits.max_connections_per_client(),
            ));
        }
        state.next_id += 1;
        let id = state.next_id;
        let (close, closed) = oneshot::channel();
        state.live.insert(
            id,
            Live {
                fingerprint,
                close: Some(close),
            },
        );
        drop(state);
        Ok((
            ConnectionSlot {
                registry: Arc::clone(self),
                id,
            },
            closed,
        ))
    }

    /// Ends every live connection whose fingerprint `store` no longer authorizes (§12.5).
    ///
    /// Returns the fingerprints it closed, so a caller can record what it did. Idempotent: a
    /// connection already asked to end is not asked twice.
    pub(crate) fn revoke_absent(&self, store: &AuthorizedClients) -> Vec<Fingerprint> {
        let mut state = self.locked();
        let mut closed = Vec::new();
        for live in state.live.values_mut() {
            if store.client(live.fingerprint).is_some() {
                continue;
            }
            if let Some(close) = live.close.take() {
                let _ = close.send(());
                closed.push(live.fingerprint);
            }
        }
        closed
    }

    fn release(&self, id: u64) {
        self.locked().live.remove(&id);
    }

    fn release_handshake(&self) {
        let mut state = self.locked();
        state.pending = state.pending.saturating_sub(1);
    }

    /// The state, recovering from a task that panicked while holding it.
    ///
    /// §12.6: one connection failing must not take the listener down, and a poisoned lock that
    /// refused every later accept would be exactly that failure wearing a different name. Nothing
    /// under the lock is left half-written — every mutation is a single insert, remove or
    /// increment — so the state a panicking holder leaves behind is consistent.
    fn locked(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A pending-handshake slot, released when it is dropped (§12.2).
#[derive(Debug)]
pub(crate) struct HandshakeSlot {
    registry: Arc<ConnectionRegistry>,
}

impl Drop for HandshakeSlot {
    fn drop(&mut self) {
        self.registry.release_handshake();
    }
}

/// A live connection's place in the registry, released when it is dropped (§12.1, §12.3).
#[derive(Debug)]
pub(crate) struct ConnectionSlot {
    registry: Arc<ConnectionRegistry>,
    id: u64,
}

impl ConnectionSlot {
    /// The connection id this slot was given, as the audit trail names it.
    fn connection_id(&self) -> String {
        format!("conn-{}", self.id)
    }
}

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.registry.release(self.id);
    }
}

/// The refusal a peer over a ceiling receives (§53.1, §54.1).
fn at_ceiling(ceiling: Ceiling, limit: u32) -> ErrorValue {
    let message = match ceiling {
        Ceiling::Agent => {
            format!("this agent is already holding the {limit} connections it may hold")
        }
        Ceiling::Client => {
            format!("this client key is already holding the {limit} connections one key may hold")
        }
        Ceiling::PendingHandshakes => {
            format!("{limit} peers are already negotiating with this agent")
        }
    };
    ErrorValue::new(ErrorCode::RemoteConnectionLimit, message)
        // A slot is released whenever a session ends, so this is the one remote refusal that is
        // worth trying again — unlike every refusal in §9, which is about who you are.
        .with_retryable(true)
        .with_metadata("ceiling", Value::string(ceiling.as_str()))
        .with_metadata("limit", Value::Int(i128::from(limit)))
        .with_help(
            "a listening agent bounds concurrent connections globally and per client key \
             (v0.4.1 section 12.1, section 12.3). Wait for a session to end, or raise \
             `limits.remote_connections` / `limits.remote_connections_per_client` on the \
             agent's host.",
        )
}

/// The refusal a peer that never finished negotiating receives (§12.2).
fn handshake_timed_out(timeout: std::time::Duration) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RemoteHandshakeTimeout,
        format!(
            "the handshake did not complete within {} seconds",
            timeout.as_secs_f64()
        ),
    )
    .with_retryable(true)
    .with_metadata(
        "timeout_ms",
        Value::Int(i128::from(
            u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
        )),
    )
}

/// Where a listening agent gets the authorization store for each connection.
///
/// Read once per accepted connection and fixed for that connection's life (§10.3), and read again
/// by the revocation sweep (§12.5). A closure rather than a value, because the file changes while
/// the agent runs: `add client-key` on the host must reach the next connection without a restart.
type AuthorizationSource =
    Arc<dyn Fn() -> Result<AuthorizedClients, ErrorValue> + Send + Sync + 'static>;

/// How often the agent re-reads the store to notice a revoked client (§12.5).
///
/// §12.5 asks for existing connections to close "within 5 seconds"; one second leaves room for
/// the read, the sweep and the session noticing, and costs one file read per second on a host
/// that is already serving a network socket.
const REVOCATION_SWEEP: std::time::Duration = std::time::Duration::from_secs(1);

/// An agent serving a bound socket, under the ceilings of §12.
pub struct ListeningAgent {
    listener: Arc<TlsListener>,
    config: AgentConfig,
    limits: Limits,
    audit: Audit,
    authorization: AuthorizationSource,
    registry: Arc<ConnectionRegistry>,
}

impl std::fmt::Debug for ListeningAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListeningAgent")
            .field("listener", &self.listener)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl ListeningAgent {
    /// An agent answering `listener` with `config`, under Appendix A's ceilings.
    ///
    /// Every client is refused until an authorization source is supplied: the default source is
    /// an empty store, which authorizes nobody, and §2.3 makes that the right way round.
    #[must_use]
    pub fn new(listener: TlsListener, config: AgentConfig) -> Self {
        let limits = Limits::default();
        Self {
            listener: Arc::new(listener),
            config,
            registry: ConnectionRegistry::new(limits.clone()),
            limits,
            audit: Arc::new(NoAudit),
            authorization: Arc::new(|| Ok(AuthorizedClients::empty())),
        }
    }

    /// The ceilings this agent enforces (§12.4).
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.registry = ConnectionRegistry::new(limits.clone());
        self.limits = limits;
        self
    }

    /// Where this agent's audit events go (§14.1).
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = audit;
        self
    }

    /// Where the agent reads the clients it may serve (§9.2, §10.3).
    #[must_use]
    pub fn with_authorization_source(
        mut self,
        source: impl Fn() -> Result<AuthorizedClients, ErrorValue> + Send + Sync + 'static,
    ) -> Self {
        self.authorization = Arc::new(source);
        self
    }

    /// The connections this agent is holding, for a sweep or for a diagnostic.
    #[must_use]
    pub fn connections(&self) -> Arc<ConnectionRegistry> {
        Arc::clone(&self.registry)
    }

    /// Where the agent is actually listening.
    ///
    /// # Errors
    ///
    /// `remote.unreachable` (E0601) when the socket has no address the system will report.
    pub fn local_addr(&self) -> Result<std::net::SocketAddr, ErrorValue> {
        self.listener.local_addr()
    }

    /// Serves until the listening socket itself becomes unusable (§12.6).
    ///
    /// One failing connection is never a reason to stop: a refused peer, a peer that speaks
    /// nothing, a peer that stalls and a peer whose task panics are each recorded and left
    /// behind, and the next peer is accepted. The loop ends only when the socket cannot be
    /// accepted on any more, which is the one condition §12.6 exempts.
    pub async fn run(self) -> ErrorValue {
        let sweep = tokio::spawn(revocation_sweep(
            Arc::clone(&self.registry),
            Arc::clone(&self.authorization),
            Arc::clone(&self.audit),
        ));
        let outcome = self.accept_loop().await;
        sweep.abort();
        outcome
    }

    async fn accept_loop(&self) -> ErrorValue {
        // Everything a connection task needs that is the same for every connection, built once
        // and shared: a task that had to be handed nine values would be a task nobody can add a
        // tenth to.
        let serving = Arc::new(Serving {
            listener: Arc::clone(&self.listener),
            registry: Arc::clone(&self.registry),
            audit: Arc::clone(&self.audit),
            authorization: Arc::clone(&self.authorization),
            config: self.config.clone(),
            limits: self.limits.clone(),
        });
        loop {
            let (stream, from) = match self.listener.accept_tcp().await {
                Ok(accepted) => accepted,
                Err(error) if listening_socket_is_gone(&error) => {
                    return ErrorValue::new(
                        ErrorCode::RemoteUnreachable,
                        format!("the listening socket can no longer be accepted on: {error}"),
                    );
                }
                Err(error) => {
                    // Out of file descriptors, a connection reset between the kernel's accept
                    // queue and ours, a transient refusal: the peer is gone and the socket is
                    // not (§12.6).
                    self.record(
                        AuditEvent::new(
                            AuditKind::ClientVerificationFailed,
                            "unaccepted",
                            "denied",
                        )
                        .with_error_code(ErrorCode::RemoteUnreachable),
                    );
                    eprintln!(
                        "{}: a peer could not be accepted: {error}",
                        ono_core::SHORT_NAME
                    );
                    continue;
                }
            };
            let source = from.to_string();
            let slot = match self.registry.begin_handshake() {
                Ok(slot) => slot,
                Err(refusal) => {
                    // Nothing has been spent on this peer and there is no authenticated channel
                    // to answer over, so the socket is closed and the decision is recorded.
                    drop(stream);
                    self.record(
                        AuditEvent::new(AuditKind::ConnectionLimitDenied, "unaccepted", "denied")
                            .with_source_address(Some(&source))
                            .with_error_code(refusal.code()),
                    );
                    continue;
                }
            };
            let serving = Arc::clone(&serving);
            // One task per connection, and nothing above it waits on the task's outcome: a
            // panic inside it is contained by the runtime and the loop is already back at
            // `accept` (§12.6).
            tokio::spawn(async move {
                serve_one(&serving, stream, source, slot).await;
            });
        }
    }

    fn record(&self, event: AuditEvent) {
        self.audit.record(&event);
    }
}

/// What every connection task is given, built once by the accept loop.
struct Serving {
    listener: Arc<TlsListener>,
    registry: Arc<ConnectionRegistry>,
    audit: Audit,
    authorization: AuthorizationSource,
    config: AgentConfig,
    limits: Limits,
}

/// Answers one accepted socket, from the TLS handshake to the end of the session.
async fn serve_one(
    serving: &Serving,
    stream: tokio::net::TcpStream,
    source: String,
    handshake_slot: HandshakeSlot,
) {
    let Serving {
        listener,
        registry,
        audit,
        authorization,
        config,
        limits,
    } = serving;
    let record = |event: AuditEvent| audit.record(&event.with_source_address(Some(&source)));

    // §12.2: TLS is bounded, because a peer that completes TCP and then says nothing must not
    // hold a slot for as long as it likes. The Ono negotiation that follows is bounded by the
    // same figure, inside `ono_protocol::serve`.
    let handshake = tokio::time::timeout(
        limits.handshake_timeout(),
        listener.handshake(stream, &source),
    );
    let transport = match handshake.await {
        Ok(Ok(transport)) => transport,
        Ok(Err(error)) => {
            record(
                AuditEvent::new(AuditKind::ClientVerificationFailed, "unaccepted", "denied")
                    .with_error_code(error.code()),
            );
            eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
            return;
        }
        Err(_elapsed) => {
            let refusal = handshake_timed_out(limits.handshake_timeout());
            record(
                AuditEvent::new(AuditKind::ConnectionLimitDenied, "unaccepted", "denied")
                    .with_error_code(refusal.code()),
            );
            eprintln!("{}: {}", ono_core::SHORT_NAME, refusal.message());
            return;
        }
    };

    // The peer's key is a fact this process verified, so the per-client ceiling is keyed on it
    // and never on where the connection came from (§11.3, §12.3).
    let Some(fingerprint) =
        ono_protocol::Transport::peer_key(&transport).map(ono_protocol::HostKey::fingerprint)
    else {
        // Unreachable: the listener demands a client certificate. Refusing rather than admitting
        // an unidentified peer is the direction §2.3 fixes.
        record(AuditEvent::new(
            AuditKind::ClientVerificationFailed,
            "unaccepted",
            "denied",
        ));
        return;
    };

    let (slot, closed) = match registry.admit(fingerprint) {
        Ok(admitted) => admitted,
        Err(refusal) => {
            record(
                AuditEvent::new(AuditKind::ConnectionLimitDenied, "unaccepted", "denied")
                    .with_peer(fingerprint)
                    .with_error_code(refusal.code()),
            );
            // The peer completed the cryptographic handshake, so there is a channel to tell it
            // why (§54.1). It learns nothing else: not the provider inventory, not the store.
            let _ = ono_protocol::refuse(transport, &refusal, limits).await;
            return;
        }
    };

    // The peer is authenticated and admitted, so it is no longer one of the sixteen that may be
    // negotiating: holding the pending slot for the session's whole life would make §12.2's
    // ceiling a second, much lower global ceiling (§12.1 is the one that counts established
    // connections, and it has already counted this one).
    drop(handshake_slot);

    // Read once per accepted connection, so an operator's `add client-key` reaches the next
    // connection; fixed for this connection's life, which is §10.3's rule (ADR-0470).
    let store = match authorization() {
        Ok(store) => Arc::new(store),
        Err(error) => {
            // §2.3: the control could not be applied, so the connection does not start. The
            // listener stays up, because the next connection may find a repaired file.
            record(
                AuditEvent::new(
                    AuditKind::UnknownClientRefused,
                    slot.connection_id(),
                    "denied",
                )
                .with_peer(fingerprint)
                .with_error_code(error.code()),
            );
            eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
            let _ = ono_protocol::refuse(transport, &error, limits).await;
            return;
        }
    };

    let config = config
        .clone()
        .with_authorization(ServerAuthorization::Store(store))
        .with_audit(Arc::clone(audit))
        .with_source_address(&source)
        .with_limits(limits.clone());
    // Whichever finishes first: the session, or the operator withdrawing the grant it runs
    // under. Dropping the session future drops the transport, which closes the socket — which
    // is what "the connection is terminated" means to the peer (§12.5).
    tokio::select! {
        result = serve_registry(transport, config) => {
            if let Err(error) = result {
                eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
            }
        }
        _ = closed => {
            record(
                AuditEvent::new(AuditKind::ClientDisconnected, slot.connection_id(), "ended")
                    .with_peer(fingerprint)
                    .with_error_code(ErrorCode::RemoteUnauthorized),
            );
        }
    }
    drop(slot);
}

/// Closes the sessions of every client the store no longer authorizes (§12.5).
async fn revocation_sweep(
    registry: Arc<ConnectionRegistry>,
    authorization: AuthorizationSource,
    audit: Audit,
) {
    let mut ticker = tokio::time::interval(REVOCATION_SWEEP);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        // A store that will not parse authorizes nobody (ADR-0466), and a sweep that acted on
        // that reading would close every session because a file is temporarily half-written.
        // Refusing *new* connections is the fail-closed response there; this one waits.
        let Ok(store) = authorization() else {
            continue;
        };
        for fingerprint in registry.revoke_absent(&store) {
            audit.record(
                &AuditEvent::new(AuditKind::ClientDisconnected, "revoked", "ended")
                    .with_peer(fingerprint)
                    .with_error_code(ErrorCode::RemoteUnauthorized),
            );
        }
    }
}

/// Whether an accept error means the socket itself is finished (§12.6).
///
/// Everything else — a peer that reset between the kernel's queue and ours, a process out of file
/// descriptors, a transient refusal — is about one connection and leaves the listener serving.
fn listening_socket_is_gone(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::InvalidInput | io::ErrorKind::NotConnected | io::ErrorKind::BrokenPipe
    ) || error.raw_os_error() == Some(9)
}
