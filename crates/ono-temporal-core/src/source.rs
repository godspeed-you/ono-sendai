//! Evidence sources (v0.5 §7.1) and the temporal capabilities a source advertises (§21.1).
//!
//! §7.1: "sources MUST have stable inspectable identity". The nine classes are closed, so a
//! source is a validated name rather than free text, and a plugin cannot invent a source class
//! to make its evidence look native.

use std::fmt;
use std::sync::Arc;

use ono_spatial_core::PermissionState;

/// The six source classes that name themselves (§7.1).
const FIXED: &[&str] = &[
    "ono.session",
    "ono.recorder",
    "linux.procfs",
    "linux.netlink",
    "linux.systemd-dbus",
    "linux.journald",
];

/// Whether `segment` may stand in a composed source name.
///
/// It has to be readable, and it may not carry the two characters that structure the name, or
/// `adapter:a/b` and `remote:a/b` would be the same text under two readings.
fn is_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|c| c.is_ascii_graphic() && c != '/' && c != ':')
}

/// One of the nine evidence source classes of §7.1.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceSource(Arc<str>);

impl EvidenceSource {
    /// Every source class that names itself, in the order §7.1 lists them.
    pub const FIXED: &'static [&'static str] = FIXED;

    /// The shell's own session: an action the operator took, an observation the shell made.
    #[must_use]
    pub fn session() -> Self {
        Self(Arc::from("ono.session"))
    }

    /// The user-level recorder of §10.
    #[must_use]
    pub fn recorder() -> Self {
        Self(Arc::from("ono.recorder"))
    }

    /// One of the six classes that name themselves, or `None` for anything else.
    #[must_use]
    pub fn builtin(name: &str) -> Option<Self> {
        FIXED
            .iter()
            .find(|known| **known == name)
            .map(|known| Self(Arc::from(*known)))
    }

    /// An external command adapter of v0.3, as `adapter:<adapter-id>`.
    #[must_use]
    pub fn adapter(adapter_id: &str) -> Option<Self> {
        is_segment(adapter_id).then(|| Self(format!("adapter:{adapter_id}").into()))
    }

    /// A provider reached over a link, as `remote:<link-id>/<provider-id>` (§24.1).
    #[must_use]
    pub fn remote(link_id: &str, provider_id: &str) -> Option<Self> {
        (is_segment(link_id) && is_segment(provider_id))
            .then(|| Self(format!("remote:{link_id}/{provider_id}").into()))
    }

    /// A provider contributed by a KUANG/11 package, as `kuang:<package-id>/<provider-id>` (§37.3).
    #[must_use]
    pub fn kuang(package_id: &str, provider_id: &str) -> Option<Self> {
        (is_segment(package_id) && is_segment(provider_id))
            .then(|| Self(format!("kuang:{package_id}/{provider_id}").into()))
    }

    /// Reads a source name back, refusing anything §7.1 does not define.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(source) = Self::builtin(text) {
            return Some(source);
        }
        if let Some(adapter_id) = text.strip_prefix("adapter:") {
            return Self::adapter(adapter_id);
        }
        if let Some(rest) = text.strip_prefix("remote:") {
            let (link, provider) = rest.split_once('/')?;
            return Self::remote(link, provider);
        }
        if let Some(rest) = text.strip_prefix("kuang:") {
            let (package, provider) = rest.split_once('/')?;
            return Self::kuang(package, provider);
        }
        None
    }

    /// The source name as §7.1 spells it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the evidence came from another host, which §24.1 makes a separate evidence domain.
    #[must_use]
    pub fn is_remote(&self) -> bool {
        self.0.starts_with("remote:")
    }

    /// Whether a KUANG/11 package contributed the evidence (§37.3).
    ///
    /// The host owns attribution, so a package cannot present itself as a native source.
    #[must_use]
    pub fn is_plugin(&self) -> bool {
        self.0.starts_with("kuang:")
    }
}

impl fmt::Display for EvidenceSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a provider can answer about time (§21.1).
///
/// Every flag is a contract rather than an observation. §21.5 is explicit about the strongest of
/// them: "providers MUST NOT advertise [`exhaustive_events`](Self::exhaustive_events) merely
/// because events usually arrive", because §7.4's negative claims are built on it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TemporalCapabilities {
    /// The provider can report current state, which is what a checkpoint is built from (§21.2).
    pub current_snapshot: bool,
    /// The provider emits canonical or mappable events as they happen (§21.3).
    pub live_events: bool,
    /// The provider can answer directly about past state or events (§21.4).
    pub historical_query: bool,
    /// Sequence continuity supports absence and change claims for the declared class (§21.5).
    pub exhaustive_events: bool,
    /// The provider carries transaction, job or action identifiers (§21.6).
    pub causal_tokens: bool,
    /// The provider's snapshot can be serialised into the temporal store (§21.7).
    pub checkpointable: bool,
    /// How far back the source itself keeps material; `None` where it does not say (§21.1).
    pub retained_history: Option<ono_value::Duration>,
}

impl TemporalCapabilities {
    /// A provider that answers nothing about time — the default every existing provider keeps.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether the source can contribute to a historical answer at all.
    #[must_use]
    pub fn is_temporal(&self) -> bool {
        self.live_events || self.historical_query || self.checkpointable
    }
}

/// One source's answer to "what can you tell me about the past?" — the `ono.temporal-source/1`
/// record of §21.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalSourceDescription {
    /// The §7.1 class that names the source.
    pub source: EvidenceSource,
    /// The provider behind it, as `get provider` names it.
    pub provider: Arc<str>,
    /// What it can answer (§21.1).
    pub capabilities: TemporalCapabilities,
    /// Whether it answers for this user now. §21.8 makes a failure coverage loss, stated here.
    pub availability: PermissionState,
    /// What a reader needs beside the flags; `None` where nothing is needed.
    pub detail: Option<Arc<str>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_refuse_a_composed_source_when_a_segment_carries_the_structure() {
        assert!(EvidenceSource::adapter("ps/extra").is_none());
        assert!(EvidenceSource::remote("web:01", "procfs").is_none());
        assert!(EvidenceSource::kuang("", "flows").is_none());
    }

    #[test]
    fn should_report_the_evidence_domain_when_a_source_is_not_local() {
        let remote = EvidenceSource::remote("web01", "linux.procfs").expect("a remote source");
        assert!(remote.is_remote());
        assert!(!remote.is_plugin());
        assert!(!EvidenceSource::recorder().is_remote());
    }
}
