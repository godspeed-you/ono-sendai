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

/// What a peer says about its own clock (v0.5 §24.2, §25.5).
///
/// §24.2 requires a remote event to preserve "a source clock identity or host identity" beside
/// its source time, and §25.5 makes the boot boundary the thing that separates clock domains.
/// Both are settled once, here, rather than repeated on every event.
///
/// It is documented the way [`Identity`] is, and for the same reason: **self-reported context,
/// never authority.** A peer that names a boot has told this side which domain its monotonic
/// readings belong to; it has not proved anything, and nothing may be granted because of it. A
/// peer that says nothing leaves the clock unknown, which
/// [`ClockDomain::is_comparable_to`](ono_temporal_core::ClockDomain::is_comparable_to) already
/// treats as "not comparable to anything, including itself".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerClock {
    clock_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uncertainty_nanos: Option<i128>,
}

impl PeerClock {
    /// The clock of the host named `clock_id`, with nothing else stated.
    #[must_use]
    pub fn of(clock_id: impl Into<String>) -> Self {
        Self {
            clock_id: clock_id.into(),
            boot_id: None,
            reading: None,
            uncertainty_nanos: None,
        }
    }

    /// Names which boot of that host the monotonic readings belong to (§25.5).
    #[must_use]
    pub fn on_boot(mut self, boot_id: impl Into<String>) -> Self {
        self.boot_id = Some(boot_id.into());
        self
    }

    /// Records what the peer's own wall clock said while it answered.
    #[must_use]
    pub fn reading_at(mut self, now: jiff::Timestamp) -> Self {
        self.reading = Some(now.to_string());
        self
    }

    /// States how far that reading may be from true (§24.4).
    #[must_use]
    pub fn within(mut self, uncertainty: ono_value::Duration) -> Self {
        self.uncertainty_nanos = Some(uncertainty.nanoseconds());
        self
    }

    /// The clock identity the peer reported — a host name, a runtime id, whatever it calls
    /// itself. Never checked against the transport's own idea of who the peer is.
    #[must_use]
    pub fn clock_id(&self) -> &str {
        &self.clock_id
    }

    /// Which boot of that host, where the peer said (§25.5). `None` separates the domain from
    /// every other, including another `None`.
    #[must_use]
    pub fn boot_id(&self) -> Option<&str> {
        self.boot_id.as_deref()
    }

    /// What the peer's wall clock said at the handshake, where it said anything.
    #[must_use]
    pub fn reading(&self) -> Option<jiff::Timestamp> {
        self.reading.as_deref().and_then(|text| text.parse().ok())
    }

    /// How far that reading may be from true, where the peer stated a bound (§24.4).
    ///
    /// `None` is unmeasured, which is not a measured zero.
    #[must_use]
    pub fn uncertainty(&self) -> Option<ono_value::Duration> {
        self.uncertainty_nanos
            .map(ono_value::Duration::from_nanoseconds)
    }

    /// The clock domain a remote event's monotonic reading belongs to (§25.5).
    #[must_use]
    pub fn domain(&self) -> ono_temporal_core::ClockDomain {
        ono_temporal_core::ClockDomain::new(&self.clock_id, self.boot_id.as_deref())
    }

    /// How far the peer's wall clock was from `local_now`, measured at the handshake.
    ///
    /// Positive means the peer reads later than this host. `None` where either side did not
    /// say, because an unmeasured offset is unknown rather than zero (§35.3).
    #[must_use]
    pub fn offset_from(&self, local_now: jiff::Timestamp) -> Option<ono_value::Duration> {
        let peer = self.reading()?;
        Some(ono_value::Duration::from_nanoseconds(
            peer.as_nanosecond() - local_now.as_nanosecond(),
        ))
    }
}

/// What a linked host can answer about time, as §24.1 enumerates it.
///
/// The four cases are ordered, and the link reports the strongest one any negotiated provider
/// supports: a peer with one journal and twenty snapshot providers still has persisted history.
/// [`RemoteTemporal::None`] is the answer §24.5 requires a caller to be able to act on — "the
/// remote has none" is a fact to state, never a reason to show current state as past state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RemoteTemporal {
    /// The peer claims nothing about time.
    None,
    /// The peer can say what is true now, and nothing about what was (§24.1, §21.2).
    CurrentSnapshot,
    /// The peer emits changes as they happen (§24.1, §21.3).
    LiveEvents,
    /// The peer can answer directly about the past (§24.1, §21.4).
    PersistedHistory,
}

impl RemoteTemporal {
    /// The case `capabilities` amounts to.
    #[must_use]
    pub const fn of(capabilities: &ono_provider_api::TemporalCapabilities) -> Self {
        if capabilities.historical_query {
            Self::PersistedHistory
        } else if capabilities.live_events {
            Self::LiveEvents
        } else if capabilities.current_snapshot {
            Self::CurrentSnapshot
        } else {
            Self::None
        }
    }

    /// The name a link table and an error message spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::CurrentSnapshot => "current_snapshot",
            Self::LiveEvents => "live_events",
            Self::PersistedHistory => "persisted_history",
        }
    }
}

impl std::fmt::Display for RemoteTemporal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a provider says it can answer about time, as the wire writes it down (§21.1, §24.1).
///
/// The seven flags of [`ono_provider_api::TemporalCapabilities`], serialised so that a claim
/// nobody made is absent rather than false-by-omission-of-meaning. Every field defaults, so a
/// peer built before this existed decodes as a provider that claims nothing — which is exactly
/// what §24.5 needs it to be.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalDescriptor {
    #[serde(default, skip_serializing_if = "is_false")]
    current_snapshot: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    live_events: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    historical_query: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    exhaustive_events: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    causal_tokens: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    checkpointable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retained_history_nanos: Option<i128>,
}

fn is_false(flag: &bool) -> bool {
    !*flag
}

impl TemporalDescriptor {
    /// The claim as a provider makes it.
    #[must_use]
    pub fn to_capabilities(&self) -> ono_provider_api::TemporalCapabilities {
        ono_provider_api::TemporalCapabilities {
            current_snapshot: self.current_snapshot,
            live_events: self.live_events,
            historical_query: self.historical_query,
            exhaustive_events: self.exhaustive_events,
            causal_tokens: self.causal_tokens,
            checkpointable: self.checkpointable,
            retained_history: self
                .retained_history_nanos
                .map(ono_value::Duration::from_nanoseconds),
        }
    }
}

impl From<&ono_provider_api::TemporalCapabilities> for TemporalDescriptor {
    fn from(capabilities: &ono_provider_api::TemporalCapabilities) -> Self {
        Self {
            current_snapshot: capabilities.current_snapshot,
            live_events: capabilities.live_events,
            historical_query: capabilities.historical_query,
            exhaustive_events: capabilities.exhaustive_events,
            causal_tokens: capabilities.causal_tokens,
            checkpointable: capabilities.checkpointable,
            retained_history_nanos: capabilities.retained_history.map(|d| d.nanoseconds()),
        }
    }
}

/// One thing a remote provider can do, with how much it could change (spec §17.1, §21.2).
///
/// This is [`ono_provider_api::Capability`] as the wire writes it down. Risk and elevation
/// travel with the id because they are what a *local* risk display is computed from: a remote
/// `process.signal` that arrived as a bare name would be indistinguishable from a read, and a
/// destructive remote action would look safe (ADR-0015).
///
/// The risk is carried as its `docs/contracts/capabilities.yaml` name. A name this build does not
/// know decodes as [`Risk::Destructive`](ono_provider_api::Risk::Destructive): a claim that
/// cannot be understood must be over-stated, never under-stated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    id: String,
    #[serde(default = "default_risk")]
    risk: String,
    #[serde(default)]
    elevation: bool,
}

fn default_risk() -> String {
    ono_provider_api::Risk::Read.as_str().to_owned()
}

impl CapabilityDescriptor {
    /// The capability's id, as `docs/contracts/capabilities.yaml` spells it.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The risk the declaring side put on it, as `docs/contracts/capabilities.yaml` names it.
    ///
    /// A name this build does not know reads as `destructive` through
    /// [`to_capability`](Self::to_capability): a claim that cannot be understood is over-stated,
    /// never under-stated, and authorization asks the same question through the same door.
    #[must_use]
    pub fn risk(&self) -> ono_provider_api::Risk {
        self.to_capability().risk()
    }

    /// Whether the declaring side says the capability needs elevated privilege.
    #[must_use]
    pub const fn needs_elevation(&self) -> bool {
        self.elevation
    }

    /// The capability as a provider declares it, for mounting a remote provider locally.
    #[must_use]
    pub fn to_capability(&self) -> ono_provider_api::Capability {
        use ono_provider_api::Risk;
        let risk = match self.risk.as_str() {
            "read" => Risk::Read,
            "observe" => Risk::Observe,
            "mutate" => Risk::Mutate,
            // `destructive`, and every claim this build cannot understand: assume the worst.
            _ => Risk::Destructive,
        };
        let capability = ono_provider_api::Capability::new(&self.id, risk);
        if self.elevation {
            capability.needing_elevation()
        } else {
            capability
        }
    }
}

impl From<&ono_provider_api::Capability> for CapabilityDescriptor {
    fn from(capability: &ono_provider_api::Capability) -> Self {
        Self {
            id: capability.id().to_owned(),
            risk: capability.risk().as_str().to_owned(),
            elevation: capability.needs_elevation(),
        }
    }
}

impl From<&str> for CapabilityDescriptor {
    /// A bare name makes the weakest claim: a read that needs no elevation.
    fn from(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            risk: default_risk(),
            elevation: false,
        }
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
    capabilities: Vec<CapabilityDescriptor>,
    #[serde(default)]
    unavailable: Option<String>,
    /// What the provider says it can answer about time (v0.5 §21.1, §24.1).
    ///
    /// Absent is the honest reading of a peer built before v0.5: it claims nothing, so the link
    /// degrades to "no temporal support" rather than failing, which is §24.5's requirement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    temporal: Option<TemporalDescriptor>,
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
            temporal: None,
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
    ///
    /// Accepts bare names for the read-only common case; a capability that mutates or needs
    /// elevation goes through [`with_capability`](Self::with_capability) so its risk crosses
    /// the link.
    #[must_use]
    pub fn with_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<CapabilityDescriptor>,
    {
        self.capabilities = capabilities.into_iter().map(Into::into).collect();
        self
    }

    /// Replaces the declared capabilities with exactly these.
    ///
    /// Used by the authorization filter of §10.1, which narrows a descriptor to the capabilities
    /// one client is permitted; it never adds one, because the set it chooses from is the set the
    /// provider declared.
    #[must_use]
    pub fn with_exact_capabilities(mut self, capabilities: Vec<CapabilityDescriptor>) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Declares one capability with everything a provider says about it.
    #[must_use]
    pub fn with_capability(mut self, capability: impl Into<CapabilityDescriptor>) -> Self {
        self.capabilities.push(capability.into());
        self
    }

    /// Declares what it can answer about time (v0.5 §21.1).
    ///
    /// A provider that never calls this claims nothing, which is what §21.5 requires of a
    /// source that has not thought about the question.
    #[must_use]
    pub fn with_temporal(mut self, capabilities: ono_provider_api::TemporalCapabilities) -> Self {
        self.temporal = Some(TemporalDescriptor::from(&capabilities));
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
    pub fn capabilities(&self) -> &[CapabilityDescriptor] {
        &self.capabilities
    }

    /// What it can answer about time (v0.5 §21.1, §24.1).
    ///
    /// A peer that declared nothing answers [`TemporalCapabilities::none`], so silence is never
    /// read as coverage.
    ///
    /// [`TemporalCapabilities::none`]: ono_provider_api::TemporalCapabilities::none
    #[must_use]
    pub fn temporal(&self) -> ono_provider_api::TemporalCapabilities {
        self.temporal.as_ref().map_or_else(
            ono_provider_api::TemporalCapabilities::none,
            TemporalDescriptor::to_capabilities,
        )
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
    /// What this end says about its own clock (§24.2, §25.5). Absent from a peer that predates
    /// v0.5, and unknown is the honest reading of that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clock: Option<PeerClock>,
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

    /// What this end said about its own clock (§24.2). Self-reported context, never authority.
    #[must_use]
    pub const fn clock(&self) -> Option<&PeerClock> {
        self.clock.as_ref()
    }

    /// States this end's clock identity on an offer already built (§24.2).
    pub(crate) fn announcing(mut self, clock: Option<PeerClock>) -> Self {
        self.clock = clock;
        self
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
    /// What the answering end says about its own clock (§24.2, §25.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clock: Option<PeerClock>,
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

    /// What the remote said about its own clock (§24.2).
    #[must_use]
    pub const fn clock(&self) -> Option<&PeerClock> {
        self.clock.as_ref()
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

    /// The strongest thing any negotiated provider can answer about time (§24.1).
    ///
    /// §24.1 requires negotiation to report which of its four cases the peer is, and §24.5
    /// requires a caller standing on a remote place to be able to say "the remote has none"
    /// rather than showing current state as past state. This is the sentence that answers both.
    #[must_use]
    pub fn temporal(&self) -> RemoteTemporal {
        self.providers
            .iter()
            .filter(|provider| provider.is_available())
            .map(|provider| RemoteTemporal::of(&provider.temporal()))
            .max()
            .unwrap_or(RemoteTemporal::None)
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
                clock: accept.clock,
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
    clock: Option<PeerClock>,
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

    /// What the remote said about its own clock (§24.2, §25.5).
    ///
    /// `None` where it said nothing, which §35.3 makes unknown rather than a zero offset. Like
    /// [`PeerInfo::identity`], this is the peer describing itself: useful context, never
    /// authority, and never a reason to grant anything.
    #[must_use]
    pub const fn clock(&self) -> Option<&PeerClock> {
        self.clock.as_ref()
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
        clock: None,
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
    pub clock: Option<PeerClock>,
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
        clock: offer.clock.clone(),
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
