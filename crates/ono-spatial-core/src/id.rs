//! Stable identity: `SpatialId` and the three tiers of spec v0.4 §10.
//!
//! §3.1 requires a `SpatialId` to be "opaque to users and stable for as long as the
//! implementation can truthfully identify the same conceptual object", and §10.1 splits that
//! truthfulness into three tiers. §2.8 states the consequence a PID is an attribute, not a
//! lifetime identity — so the identity of a process is built from boot identity, pid, start time
//! and pid namespace (§10.2), and a reused pid produces a different id rather than silently
//! resolving an old place to a new process (§42.2).
//!
//! The id is a digest, not a rendering: ADR-0129. What went into it stays on the
//! [`SpatialIdentity`] that produced it, where an `identity_conflict` diagnostic can show it.

use std::fmt;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::SpatialType;

/// How long an identity can be trusted (§10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IdentityTier {
    /// Tier A: a stable conceptual identity — a unit name, a filesystem UUID, a uid, a host.
    Stable,
    /// Tier B: a lifetime identity — a process, a socket, a connection. The identifier is reused
    /// after the object ends, so the identity carries what makes the lifetime unique.
    Lifetime,
    /// Tier C: an observation identity, used only where a provider can guarantee nothing
    /// stronger. The renderer MUST NOT imply stable persistence for such an object (§10.1).
    Observation,
}

impl IdentityTier {
    /// Every tier, strongest first.
    pub const ALL: &'static [IdentityTier] = &[
        IdentityTier::Stable,
        IdentityTier::Lifetime,
        IdentityTier::Observation,
    ];

    /// The name `docs/contracts/spatial/spatial.yaml` and the provider claims of §42 spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            IdentityTier::Stable => "stable",
            IdentityTier::Lifetime => "lifetime",
            IdentityTier::Observation => "observation",
        }
    }

    /// The tier with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|tier| tier.as_str() == name)
    }

    /// Whether an object of this tier may be rendered as if it persisted (§10.1).
    #[must_use]
    pub fn implies_persistence(self) -> bool {
        !matches!(self, IdentityTier::Observation)
    }
}

impl fmt::Display for IdentityTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which boot of which host an identity belongs to (§10.2).
///
/// Two processes with the same pid and start time on either side of a reboot are not the same
/// process, and neither are two on different hosts. The boot identity is what keeps them apart.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BootIdentity(Arc<str>);

impl BootIdentity {
    /// The boot identity of `host`, distinguished by `boot`.
    ///
    /// `boot` is whatever the host can offer that changes across a reboot — on Linux,
    /// `/proc/sys/kernel/random/boot_id`. Reading it is the caller's job: this crate composes
    /// facts, it does not gather them (§2.16).
    #[must_use]
    pub fn new(host: &str, boot: &str) -> Self {
        Self(format!("{host}/{boot}").into())
    }

    /// A boot identity for a host that cannot tell us which boot this is.
    ///
    /// Identities built on it are still stable within one observation, but nothing may claim
    /// they survive a reboot, so they are Tier C at best.
    #[must_use]
    pub fn unknown_boot(host: &str) -> Self {
        Self(format!("{host}/?").into())
    }

    /// The identity as text, for a diagnostic.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the boot this identity names is known (§2.17: unknown is visible).
    #[must_use]
    pub fn is_known(&self) -> bool {
        !self.0.ends_with("/?")
    }
}

impl fmt::Display for BootIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What makes an object the object it is, before it is reduced to a [`SpatialId`].
///
/// The components are ordered and named, so the same object always produces the same digest and
/// a conflict can say which component disagreed (§40 `spatial.identity_conflict`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpatialIdentity {
    tier: IdentityTier,
    object_type: SpatialType,
    components: Vec<(String, String)>,
}

impl SpatialIdentity {
    /// An identity of `tier` for an object of `object_type`, from named components.
    ///
    /// The components are the facts that make the object that object — never its display name,
    /// which §3.1 keeps out of identity.
    #[must_use]
    pub fn new(
        tier: IdentityTier,
        object_type: SpatialType,
        components: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self {
            tier,
            object_type,
            components: components
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        }
    }

    /// A Tier A identity: one that names a conceptual object directly (§10.1).
    #[must_use]
    pub fn stable(
        object_type: SpatialType,
        components: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self::new(IdentityTier::Stable, object_type, components)
    }

    /// A Tier B identity: one that is only as long-lived as the object (§10.1).
    #[must_use]
    pub fn lifetime(
        object_type: SpatialType,
        components: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self::new(IdentityTier::Lifetime, object_type, components)
    }

    /// A Tier C identity: all a provider could offer (§10.1).
    #[must_use]
    pub fn observation(
        object_type: SpatialType,
        components: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self::new(IdentityTier::Observation, object_type, components)
    }

    /// The identity of a canonical space, which is the same in every session on the same host
    /// (§7.1: the root is an orientation anchor, not an observation).
    #[must_use]
    pub fn space(space_id: &str) -> Self {
        Self::stable(SpatialType::System, [("space", space_id)])
    }

    /// The identity of a canonical space in `scope`'s host geography (§19.2, §43.7).
    ///
    /// The host is part of what the place *is*: `prod/web01`'s `COMPUTE` and this machine's are
    /// two conceptual objects, and §43.7 forbids the accidental merge that one shared id would
    /// be. A local scope adds nothing, so every id built before hosts existed is unchanged.
    #[must_use]
    pub fn space_in(space_id: &str, scope: Option<&crate::SpatialScope>) -> Self {
        match scope.filter(|scope| scope.is_remote()) {
            Some(scope) => Self::stable(
                SpatialType::System,
                [
                    ("host".to_owned(), scope.host_scope().to_string()),
                    ("space".to_owned(), space_id.to_owned()),
                ],
            ),
            None => Self::space(space_id),
        }
    }

    /// The tier.
    #[must_use]
    pub fn tier(&self) -> IdentityTier {
        self.tier
    }

    /// The object's spatial type.
    #[must_use]
    pub fn object_type(&self) -> SpatialType {
        self.object_type
    }

    /// The named components, in the order they were given.
    #[must_use]
    pub fn components(&self) -> &[(String, String)] {
        &self.components
    }

    /// The opaque id this identity reduces to.
    ///
    /// Equal identities produce equal ids, and that is the whole contract: §42.1's "repeated
    /// observations of the same live object MUST resolve to the same `SpatialId`".
    #[must_use]
    pub fn spatial_id(&self) -> SpatialId {
        let mut hasher = Sha256::new();
        hasher.update(self.tier.as_str().as_bytes());
        hasher.update([0x1f]);
        hasher.update(self.object_type.as_str().as_bytes());
        for (name, value) in &self.components {
            hasher.update([0x1f]);
            hasher.update(name.as_bytes());
            hasher.update([0x1e]);
            hasher.update(value.as_bytes());
        }
        let digest = hasher.finalize();
        let mut token = String::with_capacity(32);
        for byte in digest.iter().take(16) {
            use fmt::Write as _;
            let _ = write!(token, "{byte:02x}");
        }
        SpatialId(format!("ono:{}:{token}", self.tier.as_str()).into())
    }
}

/// The opaque, stable identity of a spatial object (§3.1).
///
/// It is opaque on purpose: a user copies one, pins it or compares it, but never composes one,
/// so the identity rules cannot be worked around by spelling an id by hand. The tier is visible
/// in the rendering because §10.1 forbids implying persistence for an observation identity, and
/// a reader who can see the tier can see that promise being kept.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpatialId(Arc<str>);

impl SpatialId {
    /// The id of the canonical space `space_id` (`system`, `compute.services`, …).
    #[must_use]
    pub fn of_space(space_id: &str) -> Self {
        SpatialIdentity::space(space_id).spatial_id()
    }

    /// The id as text, for storing in a pin or comparing across sessions.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The tier the id was built at, read back from its own rendering.
    #[must_use]
    pub fn tier(&self) -> Option<IdentityTier> {
        let rest = self.0.strip_prefix("ono:")?;
        let (tier, _) = rest.split_once(':')?;
        IdentityTier::from_name(tier)
    }

    /// Reads back an id that was stored — in a pin, a script or a protocol frame.
    ///
    /// Returns `None` for text that is not an id this crate produced, so a hand-written string
    /// cannot become a place.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.strip_prefix("ono:")?;
        let (tier, digest) = rest.split_once(':')?;
        IdentityTier::from_name(tier)?;
        (digest.len() == 32 && digest.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| Self(text.into()))
    }
}

impl fmt::Display for SpatialId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The identity of a local process, as §10.2 requires it.
///
/// "PID alone MUST NOT be treated as a persistent spatial identity." All four parts are here:
/// which boot of which host, the pid, when the process started, and which pid namespace the pid
/// was read in — because the same number means different processes in different namespaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    boot: BootIdentity,
    pid: i64,
    start_time: u64,
    pid_namespace: Option<u64>,
}

impl ProcessIdentity {
    /// A process identity.
    ///
    /// `start_time` is the kernel's own start time for the process — the field that makes a
    /// reused pid a different process. `pid_namespace` is the namespace inode the pid was read
    /// in, or `None` where the provider could not read it: unknown is recorded as unknown rather
    /// than as the root namespace (§2.17).
    #[must_use]
    pub fn new(boot: BootIdentity, pid: i64, start_time: u64, pid_namespace: Option<u64>) -> Self {
        Self {
            boot,
            pid,
            start_time,
            pid_namespace,
        }
    }

    /// Which boot of which host.
    #[must_use]
    pub fn boot(&self) -> &BootIdentity {
        &self.boot
    }

    /// The pid — an attribute of the process, never its identity on its own (§2.8).
    #[must_use]
    pub fn pid(&self) -> i64 {
        self.pid
    }

    /// The kernel's start time for the process.
    #[must_use]
    pub fn start_time(&self) -> u64 {
        self.start_time
    }

    /// The pid namespace the pid was read in, where the provider could read it.
    #[must_use]
    pub fn pid_namespace(&self) -> Option<u64> {
        self.pid_namespace
    }

    /// The identity this process presents to the spatial layer.
    ///
    /// Tier B: it is exactly as long-lived as the process, which is what §10.2 asks for.
    #[must_use]
    pub fn identity(&self) -> SpatialIdentity {
        SpatialIdentity::lifetime(
            SpatialType::Process,
            [
                ("boot".to_owned(), self.boot.as_str().to_owned()),
                ("pid".to_owned(), self.pid.to_string()),
                ("start_time".to_owned(), self.start_time.to_string()),
                (
                    "pid_namespace".to_owned(),
                    self.pid_namespace
                        .map_or_else(|| "unknown".to_owned(), |ns| ns.to_string()),
                ),
            ],
        )
    }

    /// The opaque id of the process.
    #[must_use]
    pub fn spatial_id(&self) -> SpatialId {
        self.identity().spatial_id()
    }
}
