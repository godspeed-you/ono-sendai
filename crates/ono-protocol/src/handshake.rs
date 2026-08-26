//! The link handshake of spec §21.2.
//!
//! Spec §21.2 lists what a link "could negotiate": protocol version, remote OS/arch, whether an
//! Ono agent is present, available providers, schema versions, terminal capabilities, identity
//! and privilege, latency and compression. All of it except latency is settled here, in one
//! round trip: the client offers, the server answers, and nothing else on the link is optional
//! afterwards.
//!
//! Latency is deliberately left out. It is a measurement, not a negotiation, and measuring it
//! during the handshake would only produce a number that is stale by the first query.
//!
//! # The shape of the exchange
//!
//! ```text
//! client ──── Hello   {versions, wanted providers, capabilities, compression, identity} ──▶ server
//! client ◀─── Accept  {version, providers with availability, capabilities, compression,  ─── server
//!                      identity, credit window}
//!         or  Reject  {code, message}
//! ```
//!
//! Both directions are a single frame on stream `0`, so a peer that is not speaking this protocol
//! is discovered before any stream exists.
//!
//! # What an empty list means
//!
//! A `Hello` that names no providers and no capabilities is asking for whatever the remote has.
//! A `Hello` that names some is asking for those, and gets the intersection. The asymmetry is
//! intentional: a client usually wants everything, and a client that wants something specific
//! must not silently receive more.

use serde::{Deserialize, Serialize};

use crate::trust::TrustDecision;
use crate::{Fingerprint, Limits};

/// The link protocol version this build speaks.
///
/// Distinct from [`FRAME_VERSION`](crate::FRAME_VERSION): the envelope has to be readable before
/// anything can be negotiated, so the two are versioned independently.
pub const PROTOCOL_VERSION: u16 = 1;

/// Who the shell is running as, at one end of a link (spec §21.2: "identity and privilege").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    user: String,
    uid: Option<u32>,
    elevated: bool,
}

impl Identity {
    /// The identity of `user`, unelevated.
    #[must_use]
    pub fn new(user: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            uid: None,
            elevated: false,
        }
    }

    /// Records the numeric user id.
    #[must_use]
    pub const fn with_uid(mut self, uid: u32) -> Self {
        self.uid = Some(uid);
        self
    }

    /// Marks the identity as holding elevated privilege (spec §17.2).
    #[must_use]
    pub const fn elevated(mut self) -> Self {
        self.elevated = true;
        self
    }

    /// The user name.
    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The numeric user id, where one is known.
    #[must_use]
    pub const fn uid(&self) -> Option<u32> {
        self.uid
    }

    /// Whether this end holds elevated privilege.
    #[must_use]
    pub const fn is_elevated(&self) -> bool {
        self.elevated
    }
}

impl Default for Identity {
    fn default() -> Self {
        Self::new("unknown")
    }
}

/// A provider the remote end has, and whether it can actually answer there.
///
/// An unavailable provider is still described. Spec §35.3 and §21.3 both turn on the same point:
/// a capability that is missing must be visibly missing, because "there are none" and "I could
/// not look" are different answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    id: String,
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    unavailable: Option<String>,
}

impl ProviderDescriptor {
    /// A provider that can answer.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            targets: Vec::new(),
            capabilities: Vec::new(),
            unavailable: None,
        }
    }

    /// Declares the targets it answers about.
    #[must_use]
    pub fn with_targets<I, S>(mut self, targets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.targets = targets.into_iter().map(Into::into).collect();
        self
    }

    /// Declares what it must be allowed to do.
    #[must_use]
    pub fn with_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.capabilities = capabilities.into_iter().map(Into::into).collect();
        self
    }

    /// Marks the provider as unable to answer here, with the reason a user needs.
    #[must_use]
    pub fn unavailable(mut self, reason: impl Into<String>) -> Self {
        self.unavailable = Some(reason.into());
        self
    }

    /// The provider's id, such as `linux.procfs`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The targets it answers about.
    #[must_use]
    pub fn targets(&self) -> &[String] {
        &self.targets
    }

    /// What it must be allowed to do.
    #[must_use]
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    /// Whether it can answer on the remote machine.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        self.unavailable.is_none()
    }

    /// Why it cannot answer, when it cannot.
    #[must_use]
    pub fn unavailable_reason(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }
}

/// What the local end offers when it opens a link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    versions: Vec<u16>,
    agent: String,
    os: String,
    arch: String,
    #[serde(default)]
    providers: Vec<String>,
    #[serde(default)]
    schemas: Vec<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    compression: Vec<String>,
    #[serde(default)]
    pty: bool,
    identity: Identity,
    credit_window: u32,
}

impl Hello {
    /// The link protocol versions this end speaks, lowest first.
    #[must_use]
    pub fn versions(&self) -> &[u16] {
        &self.versions
    }

    /// Which agent this end is, such as `ono/0.0.1`.
    #[must_use]
    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// The operating system this end runs on.
    #[must_use]
    pub fn os(&self) -> &str {
        &self.os
    }

    /// The architecture this end runs on.
    #[must_use]
    pub fn arch(&self) -> &str {
        &self.arch
    }

    /// The providers this end wants; empty means whatever the remote has.
    #[must_use]
    pub fn providers(&self) -> &[String] {
        &self.providers
    }

    /// The schemas this end can decode.
    #[must_use]
    pub fn schemas(&self) -> &[String] {
        &self.schemas
    }

    /// The capabilities this end wants; empty means whatever the remote offers.
    #[must_use]
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    /// The compressions this end can read, best first.
    #[must_use]
    pub fn compression(&self) -> &[String] {
        &self.compression
    }

    /// Whether this end can host a pseudo-terminal for a remote interactive session.
    #[must_use]
    pub const fn wants_pty(&self) -> bool {
        self.pty
    }

    /// Who this end is running as.
    #[must_use]
    pub const fn identity(&self) -> &Identity {
        &self.identity
    }

    /// How many messages per stream this end is prepared to buffer.
    #[must_use]
    pub const fn credit_window(&self) -> u32 {
        self.credit_window
    }
}

/// What the remote end answers when it establishes a link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accept {
    version: u16,
    agent: String,
    os: String,
    arch: String,
    #[serde(default)]
    providers: Vec<ProviderDescriptor>,
    #[serde(default)]
    schemas: Vec<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    compression: Option<String>,
    #[serde(default)]
    pty: bool,
    identity: Identity,
    credit_window: u32,
}

impl Accept {
    /// The chosen link protocol version.
    #[must_use]
    pub const fn version(&self) -> u16 {
        self.version
    }

    /// Which agent the remote is.
    #[must_use]
    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// The providers the link may use, with their availability.
    #[must_use]
    pub fn providers(&self) -> &[ProviderDescriptor] {
        &self.providers
    }

    /// The schemas the remote produces.
    #[must_use]
    pub fn schemas(&self) -> &[String] {
        &self.schemas
    }

    /// The capabilities both ends hold.
    #[must_use]
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    /// The chosen compression, if any.
    #[must_use]
    pub fn compression(&self) -> Option<&str> {
        self.compression.as_deref()
    }

    /// Who the remote is running as.
    #[must_use]
    pub const fn identity(&self) -> &Identity {
        &self.identity
    }

    /// How many messages per stream the remote will send before waiting for credit.
    #[must_use]
    pub const fn credit_window(&self) -> u32 {
        self.credit_window
    }
}

/// What the remote end answers when it will not establish a link.
///
/// The code is the dotted selector of an [`ono_core::ErrorCode`], so a refusal a script sees is
/// the same kind of thing as any other error it can match on (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reject {
    code: String,
    message: String,
}

impl Reject {
    /// A refusal carrying a stable code and a message a user can act on.
    #[must_use]
    pub fn new(code: ono_core::ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code.name().to_owned(),
            message: message.into(),
        }
    }

    /// The stable code, where it names one this build knows.
    #[must_use]
    pub fn code(&self) -> Option<ono_core::ErrorCode> {
        ono_core::ErrorCode::from_name(&self.code)
    }

    /// What the remote said was wrong.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Everything the handshake settled, and what the trust store concluded.
#[derive(Debug, Clone, PartialEq)]
pub struct Negotiated {
    version: u16,
    peer: PeerInfo,
    providers: Vec<ProviderDescriptor>,
    schemas: Vec<String>,
    capabilities: Vec<String>,
    compression: Option<String>,
    credit_window: u32,
    trust: TrustDecision,
    fingerprint: Option<Fingerprint>,
}

impl Negotiated {
    /// The chosen link protocol version.
    #[must_use]
    pub const fn version(&self) -> u16 {
        self.version
    }

    /// Who and what is on the other end.
    #[must_use]
    pub const fn peer(&self) -> &PeerInfo {
        &self.peer
    }

    /// The providers the link may use, with their availability.
    #[must_use]
    pub fn providers(&self) -> &[ProviderDescriptor] {
        &self.providers
    }

    /// The schemas the remote produces.
    #[must_use]
    pub fn schemas(&self) -> &[String] {
        &self.schemas
    }

    /// The capabilities both ends hold.
    #[must_use]
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    /// The chosen compression, if any.
    #[must_use]
    pub fn compression(&self) -> Option<&str> {
        self.compression.as_deref()
    }

    /// How many messages per stream the remote may send before waiting for credit.
    #[must_use]
    pub const fn credit_window(&self) -> u32 {
        self.credit_window
    }

    /// What the trust store concluded about the peer's key (ADR-0015 T5, T6).
    #[must_use]
    pub const fn trust(&self) -> TrustDecision {
        self.trust
    }

    /// The peer's key fingerprint, where the transport authenticated one.
    #[must_use]
    pub const fn fingerprint(&self) -> Option<Fingerprint> {
        self.fingerprint
    }

    pub(crate) fn from_accept(
        accept: Accept,
        trust: TrustDecision,
        fingerprint: Option<Fingerprint>,
    ) -> Self {
        Self {
            version: accept.version,
            peer: PeerInfo {
                agent: accept.agent,
                os: accept.os,
                arch: accept.arch,
                identity: accept.identity,
                pty: accept.pty,
            },
            providers: accept.providers,
            schemas: accept.schemas,
            capabilities: accept.capabilities,
            compression: accept.compression,
            credit_window: accept.credit_window,
            trust,
            fingerprint,
        }
    }
}

/// What the remote end said about itself (spec §21.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    agent: String,
    os: String,
    arch: String,
    identity: Identity,
    pty: bool,
}

impl PeerInfo {
    /// Which agent the remote is, such as `ono/0.0.1`. An agentless fallback names itself here
    /// too, so spec §21.3's requirement that the fallback be visible is met by construction.
    #[must_use]
    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// The remote operating system.
    #[must_use]
    pub fn os(&self) -> &str {
        &self.os
    }

    /// The remote architecture.
    #[must_use]
    pub fn arch(&self) -> &str {
        &self.arch
    }

    /// Who the remote is running as, and whether it is elevated.
    #[must_use]
    pub const fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Whether the remote can supply a pseudo-terminal for an interactive session.
    #[must_use]
    pub const fn supports_pty(&self) -> bool {
        self.pty
    }
}

/// The agent string this build announces itself with.
pub(crate) fn agent_name() -> String {
    format!("{}/{}", ono_core::SHORT_NAME, ono_core::VERSION)
}

/// Builds the offer this end opens a link with.
pub(crate) fn hello(
    versions: Vec<u16>,
    providers: Vec<String>,
    schemas: Vec<String>,
    capabilities: Vec<String>,
    compression: Vec<String>,
    identity: Identity,
    credit_window: u32,
    pty: bool,
) -> Hello {
    Hello {
        versions,
        agent: agent_name(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        providers,
        schemas,
        capabilities,
        compression,
        pty,
        identity,
        credit_window,
    }
}

/// What one end offers to answer with.
pub(crate) struct Offer {
    pub versions: Vec<u16>,
    pub providers: Vec<ProviderDescriptor>,
    pub schemas: Vec<String>,
    pub capabilities: Vec<String>,
    pub compression: Vec<String>,
    pub identity: Identity,
    pub pty: bool,
    pub limits: Limits,
}

/// Settles a `Hello` against what this end offers.
///
/// # Errors
///
/// Returns the [`Reject`] to send back when the two ends share no protocol version. Everything
/// else in the handshake narrows rather than fails: a capability only one side holds is simply
/// not part of the link.
pub(crate) fn negotiate(hello: &Hello, offer: &Offer) -> Result<Accept, Reject> {
    let Some(version) = shared_version(&hello.versions, &offer.versions) else {
        return Err(Reject::new(
            ono_core::ErrorCode::RemoteProtocolMismatch,
            format!(
                "no shared link protocol version: the caller speaks {:?}, this agent speaks {:?}",
                hello.versions, offer.versions
            ),
        ));
    };
    let providers = offer
        .providers
        .iter()
        .filter(|provider| {
            hello.providers.is_empty() || hello.providers.iter().any(|id| id == provider.id())
        })
        .cloned()
        .collect();
    let capabilities = if hello.capabilities.is_empty() {
        offer.capabilities.clone()
    } else {
        hello
            .capabilities
            .iter()
            .filter(|wanted| offer.capabilities.contains(wanted))
            .cloned()
            .collect()
    };
    let compression = hello
        .compression
        .iter()
        .find(|wanted| offer.compression.contains(wanted))
        .cloned();
    Ok(Accept {
        version,
        agent: agent_name(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        providers,
        schemas: offer.schemas.clone(),
        capabilities,
        compression,
        pty: offer.pty,
        identity: offer.identity.clone(),
        credit_window: hello
            .credit_window
            .clamp(1, offer.limits.max_credit().max(1)),
    })
}

/// The highest version both ends speak.
fn shared_version(theirs: &[u16], ours: &[u16]) -> Option<u16> {
    ours.iter()
        .filter(|version| theirs.contains(version))
        .copied()
        .max()
}
