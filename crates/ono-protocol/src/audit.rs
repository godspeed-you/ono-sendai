//! What a listening agent writes down about who reached it (v0.4.1 §14).
//!
//! §14.1 lists eight things an agent must record, and §14.2 lists the fields each record carries.
//! The point of writing them here, beside the loop that makes the decisions, is that a decision
//! and its audit record cannot drift apart: the refusal an operator reads in the log is the same
//! value the client received, with the same code.
//!
//! §14.2's last sentence is the constraint that shapes the type: events "MUST NOT include private
//! keys, full secret environment values or unredacted credentials from provider payloads." So an
//! [`AuditEvent`] has no field a payload could reach. It carries an identity, a decision and a
//! code, and nothing a provider produced — a record that could quote a value would eventually
//! quote a password, and the way to be sure it never does is to give it nowhere to put one.

use std::fmt;
use std::sync::Arc;

use ono_core::ErrorCode;

use crate::trust::Fingerprint;

/// One thing worth recording about a connection (§14.1).
///
/// The eight variants are §14.1's eight bullets, in its order. They are a closed set: an agent
/// that meets a ninth kind of event has met something §14.1 did not anticipate, and the honest
/// response is to add it here rather than to file it under a neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditKind {
    /// A client authenticated and was authorized; the session began.
    ConnectionAccepted,
    /// A client authenticated and is not in the authorization store (§9.4, §59.1).
    UnknownClientRefused,
    /// The transport could not verify the client's certificate at all (§7.1).
    ClientVerificationFailed,
    /// An authorized client asked for something its policy withholds (§10.2, §10.4).
    AuthorizationDenied,
    /// A connection was refused because a limit was reached (§12.1, §12.3).
    ConnectionLimitDenied,
    /// The peer speaks no protocol version this agent speaks (§13.2).
    ProtocolMismatch,
    /// The session ended.
    ClientDisconnected,
    /// An authorized action was requested, and what came of it (§14.1's last bullet).
    ActionRequested,
}

impl AuditKind {
    /// The stable name the event is recorded under.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConnectionAccepted => "connection.accepted",
            Self::UnknownClientRefused => "connection.unknown_client_refused",
            Self::ClientVerificationFailed => "connection.client_verification_failed",
            Self::AuthorizationDenied => "authorization.denied",
            Self::ConnectionLimitDenied => "connection.limit_denied",
            Self::ProtocolMismatch => "connection.protocol_mismatch",
            Self::ClientDisconnected => "connection.disconnected",
            Self::ActionRequested => "action.requested",
        }
    }

    /// Every kind, in §14.1's order.
    pub const ALL: &'static [Self] = &[
        Self::ConnectionAccepted,
        Self::UnknownClientRefused,
        Self::ClientVerificationFailed,
        Self::AuthorizationDenied,
        Self::ConnectionLimitDenied,
        Self::ProtocolMismatch,
        Self::ClientDisconnected,
        Self::ActionRequested,
    ];
}

impl fmt::Display for AuditKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One structured audit record, carrying the fields of §14.2 and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    kind: AuditKind,
    connection_id: String,
    peer_fingerprint: Option<Fingerprint>,
    peer_label: Option<String>,
    source_address: Option<String>,
    protocol_version: Option<u16>,
    requested_capability: Option<String>,
    result: &'static str,
    error_code: Option<ErrorCode>,
    timestamp: jiff::Timestamp,
}

impl AuditEvent {
    /// An event of `kind` on connection `connection_id`, with `result` one of `allowed`,
    /// `denied` or `ended`.
    #[must_use]
    pub fn new(kind: AuditKind, connection_id: impl Into<String>, result: &'static str) -> Self {
        Self {
            kind,
            connection_id: connection_id.into(),
            peer_fingerprint: None,
            peer_label: None,
            source_address: None,
            protocol_version: None,
            requested_capability: None,
            result,
            error_code: None,
            timestamp: jiff::Timestamp::now(),
        }
    }

    /// Records the key the peer proved it holds. Public identity material (§53.3).
    #[must_use]
    pub const fn with_peer(mut self, fingerprint: Fingerprint) -> Self {
        self.peer_fingerprint = Some(fingerprint);
        self
    }

    /// Records what the operator called the peer, where they called it anything.
    #[must_use]
    pub fn with_label(mut self, label: Option<&str>) -> Self {
        self.peer_label = label.map(ToOwned::to_owned);
        self
    }

    /// Records where the connection came from.
    #[must_use]
    pub fn with_source_address(mut self, address: Option<&str>) -> Self {
        self.source_address = address.map(ToOwned::to_owned);
        self
    }

    /// Records the protocol version the handshake settled on.
    #[must_use]
    pub const fn with_protocol_version(mut self, version: u16) -> Self {
        self.protocol_version = Some(version);
        self
    }

    /// Records which capability the request needed.
    #[must_use]
    pub fn with_requested_capability(mut self, capability: impl Into<String>) -> Self {
        self.requested_capability = Some(capability.into());
        self
    }

    /// Records the stable code the decision answered with.
    #[must_use]
    pub const fn with_error_code(mut self, code: ErrorCode) -> Self {
        self.error_code = Some(code);
        self
    }

    /// What kind of event this is.
    #[must_use]
    pub const fn kind(&self) -> AuditKind {
        self.kind
    }

    /// The connection this happened on.
    #[must_use]
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    /// The key the peer proved it holds, where one was proved.
    #[must_use]
    pub const fn peer_fingerprint(&self) -> Option<Fingerprint> {
        self.peer_fingerprint
    }

    /// What the operator called the peer.
    #[must_use]
    pub fn peer_label(&self) -> Option<&str> {
        self.peer_label.as_deref()
    }

    /// Where the connection came from.
    #[must_use]
    pub fn source_address(&self) -> Option<&str> {
        self.source_address.as_deref()
    }

    /// The protocol version, once one was settled.
    #[must_use]
    pub const fn protocol_version(&self) -> Option<u16> {
        self.protocol_version
    }

    /// The capability the request needed, for the events that concern one.
    #[must_use]
    pub fn requested_capability(&self) -> Option<&str> {
        self.requested_capability.as_deref()
    }

    /// `allowed`, `denied` or `ended`.
    #[must_use]
    pub const fn result(&self) -> &'static str {
        self.result
    }

    /// The stable code, where a decision answered with one.
    #[must_use]
    pub const fn error_code(&self) -> Option<ErrorCode> {
        self.error_code
    }

    /// When it happened.
    #[must_use]
    pub const fn timestamp(&self) -> jiff::Timestamp {
        self.timestamp
    }

    /// The event as one line of `key=value` fields, which is what a sink writes.
    ///
    /// Every field is one of §14.2's, and a field with nothing in it is written as `-` rather
    /// than omitted, so the shape of a line does not depend on what happened.
    #[must_use]
    pub fn render(&self) -> String {
        let or_dash = |value: Option<&str>| value.unwrap_or("-").to_owned();
        format!(
            "ono-audit event={} connection_id={} peer_fingerprint={} peer_label={} \
             source_address={} protocol_version={} requested_capability={} result={} \
             error_code={} timestamp={}",
            self.kind,
            self.connection_id,
            self.peer_fingerprint
                .map_or_else(|| "-".to_owned(), |fingerprint| fingerprint.to_string()),
            or_dash(self.peer_label.as_deref()),
            or_dash(self.source_address.as_deref()),
            self.protocol_version
                .map_or_else(|| "-".to_owned(), |version| version.to_string()),
            or_dash(self.requested_capability.as_deref()),
            self.result,
            self.error_code.map_or("-", |code| code.name()),
            self.timestamp,
        )
    }
}

/// Where a listening agent's audit events go.
pub trait AuditSink: Send + Sync + fmt::Debug {
    /// Records one event. Must not block for long: it runs on the connection's own task.
    fn record(&self, event: &AuditEvent);
}

/// A sink that discards everything, for an agent nobody asked to audit.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoAudit;

impl AuditSink for NoAudit {
    fn record(&self, _event: &AuditEvent) {}
}

/// The sink a connection writes to, shared by every task on it.
pub type Audit = Arc<dyn AuditSink>;
