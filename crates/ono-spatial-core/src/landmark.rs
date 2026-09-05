//! Landmarks (spec v0.4 §3.7, §26).
//!
//! "A landmark is an object or condition promoted because it helps orientation or deserves
//! attention", and "A landmark MUST always expose its reason" (§3.7). The reason is therefore
//! not a string: it is one of the fourteen §3.7 fixes, or a reason a KUANG/11 package contributed
//! and identified itself as the source of (§26.5).
//!
//! What the thresholds are, and which setting changes each, is `docs/contracts/spatial/landmarks.yaml`;
//! evaluating them is the landmark engine's job, not this crate's.

use std::fmt;
use std::sync::Arc;

use crate::SpatialId;

/// Why an object was promoted (§3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum LandmarkReason {
    /// CPU share at or above the configured threshold (§26.2).
    HighCpu,
    /// Memory at or above its share of the host or cgroup budget (§26.2).
    HighMemory,
    /// A service the service manager reports as failed (§26.2).
    Failed,
    /// A unit restarting often enough to be a loop rather than a restart (§26.2).
    Restarting,
    /// Changed within the change window (§24.3, §26.2).
    RecentlyChanged,
    /// A listener reachable from outside this host (§26.2).
    PublicListener,
    /// Running with privilege where the context makes that worth seeing (§26.2).
    Privileged,
    /// A filesystem at or above its pressure threshold (§26.2).
    StoragePressure,
    /// A burst of new connections within the change window (§26.2).
    ConnectionSpike,
    /// Appeared within the change window (§3.7, §25.4).
    NewObject,
    /// Went away within the change window; shown as a tombstone (§3.7, §10.3).
    RemovedObject,
    /// A container, namespace or mount boundary between here and the object (§26.2, §2.18).
    SecurityBoundary,
    /// The object belongs to another host's scope (§26.2, §19).
    RemoteBoundary,
    /// The user pinned it, and a pin outranks every heuristic (§26.4).
    UserPinned,
}

impl LandmarkReason {
    /// The fourteen built-in reasons, in the order §3.7 lists them.
    pub const ALL: &'static [LandmarkReason] = &[
        LandmarkReason::HighCpu,
        LandmarkReason::HighMemory,
        LandmarkReason::Failed,
        LandmarkReason::Restarting,
        LandmarkReason::RecentlyChanged,
        LandmarkReason::PublicListener,
        LandmarkReason::Privileged,
        LandmarkReason::StoragePressure,
        LandmarkReason::ConnectionSpike,
        LandmarkReason::NewObject,
        LandmarkReason::RemovedObject,
        LandmarkReason::SecurityBoundary,
        LandmarkReason::RemoteBoundary,
        LandmarkReason::UserPinned,
    ];

    /// The name §3.7 and `docs/contracts/spatial/landmarks.yaml` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LandmarkReason::HighCpu => "high_cpu",
            LandmarkReason::HighMemory => "high_memory",
            LandmarkReason::Failed => "failed",
            LandmarkReason::Restarting => "restarting",
            LandmarkReason::RecentlyChanged => "recently_changed",
            LandmarkReason::PublicListener => "public_listener",
            LandmarkReason::Privileged => "privileged",
            LandmarkReason::StoragePressure => "storage_pressure",
            LandmarkReason::ConnectionSpike => "connection_spike",
            LandmarkReason::NewObject => "new_object",
            LandmarkReason::RemovedObject => "removed_object",
            LandmarkReason::SecurityBoundary => "security_boundary",
            LandmarkReason::RemoteBoundary => "remote_boundary",
            LandmarkReason::UserPinned => "user_pinned",
        }
    }

    /// The reason with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|reason| reason.as_str() == name)
    }
}

impl fmt::Display for LandmarkReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who says the object deserves attention (§26.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandmarkSource {
    /// One of the built-in rules of §26.2.
    BuiltIn,
    /// A KUANG/11 package, which must identify itself (§26.5, §36.1).
    Package(Arc<str>),
}

/// A promoted object and why (§3.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landmark {
    subject: SpatialId,
    reason: LandmarkReason,
    evidence: String,
    source: LandmarkSource,
}

impl Landmark {
    /// A landmark one of the built-in rules produced.
    ///
    /// `evidence` is the fact behind it, in the user's terms — "cpu 87%", "failed 2m ago". It is
    /// not optional: §26.3 warns against pretending a local heuristic is an incident, and showing
    /// what was measured is what keeps the promotion honest.
    #[must_use]
    pub fn built_in(
        subject: SpatialId,
        reason: LandmarkReason,
        evidence: impl Into<String>,
    ) -> Self {
        Self {
            subject,
            reason,
            evidence: evidence.into(),
            source: LandmarkSource::BuiltIn,
        }
    }

    /// A landmark a KUANG/11 package contributed (§26.5).
    #[must_use]
    pub fn from_package(
        subject: SpatialId,
        reason: LandmarkReason,
        evidence: impl Into<String>,
        package: &str,
    ) -> Self {
        Self {
            subject,
            reason,
            evidence: evidence.into(),
            source: LandmarkSource::Package(package.into()),
        }
    }

    /// The object promoted.
    #[must_use]
    pub fn subject(&self) -> &SpatialId {
        &self.subject
    }

    /// Why it was promoted.
    #[must_use]
    pub fn reason(&self) -> LandmarkReason {
        self.reason
    }

    /// The fact behind the promotion.
    #[must_use]
    pub fn evidence(&self) -> &str {
        &self.evidence
    }

    /// Who says so (§26.5).
    #[must_use]
    pub fn source(&self) -> &LandmarkSource {
        &self.source
    }
}
