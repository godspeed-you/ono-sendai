//! The three clocks a remote observation carries, and the domain they belong to (v0.5 §24.2,
//! §24.3, §25.5).
//!
//! §24.2 fixes what a remote event must preserve: its `source_time`, a source clock identity or
//! host identity, the local `ingested_at`, and the clock uncertainty where it is known. The
//! sentence that matters is the last one: **"The local ledger MUST NOT overwrite remote source
//! time with ingest time."** [`crate::retag`] already keeps the first half of that promise, by
//! rewriting only the provenance link and leaving `observed` verbatim. This module is the other
//! half — the distinct arrival instant, and the [`ClockDomain`] the arrival belongs to.
//!
//! Nothing here compares wall clocks. Two hosts whose clocks differ by milliseconds are two
//! clock domains, and [`ono_temporal_core::happens_before`] answers
//! [`Concurrent`](ono_temporal_core::Ordering::Concurrent) for events in different domains
//! whatever their timestamps say (§24.3, §26.3). What this module supplies is the domain, so
//! that the ordering model has the fact it needs to refuse.

use std::sync::Arc;

use jiff::Timestamp;
use ono_protocol::PeerClock;
use ono_temporal_core::{ClockDomain, EventTimes, EvidenceSource};

/// The stamp this shell puts on everything arriving over one link (§24.2).
///
/// One per link, built from what the handshake settled. It knows the link's name, which is how
/// the user spells the far side, and whatever the peer said about its own clock — self-reported
/// context, never authority, exactly as [`ono_protocol::PeerInfo::identity`] is.
#[derive(Debug, Clone)]
pub struct RemoteIngest {
    host: Arc<str>,
    clock: Option<PeerClock>,
}

impl RemoteIngest {
    /// The stamp for a link to `host` whose peer reported `clock`.
    #[must_use]
    pub fn new(host: &str, clock: Option<PeerClock>) -> Self {
        Self {
            host: Arc::from(host),
            clock,
        }
    }

    /// The link's name, as the user named the far side.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// What the peer said about its own clock, where it said anything (§24.2).
    #[must_use]
    pub const fn clock(&self) -> Option<&PeerClock> {
        self.clock.as_ref()
    }

    /// The clock domain an event arriving over this link belongs to (§25.5).
    ///
    /// The peer's own clock identity where it named one, because that is the clock its
    /// monotonic readings and sequences came from; the link name otherwise, because a host that
    /// did not name itself is still a different host from this one. An unnamed boot leaves the
    /// domain incomparable to every other, including another unnamed boot — which is what
    /// [`ClockDomain::is_comparable_to`] already decides and this module does not relitigate.
    #[must_use]
    pub fn domain(&self) -> ClockDomain {
        match &self.clock {
            Some(clock) => ClockDomain::new(clock.clock_id(), clock.boot_id()),
            None => ClockDomain::new(&self.host, None),
        }
    }

    /// The spatial scope a coverage record for this link belongs to (§24.6).
    ///
    /// §24.6: "Coverage MUST be representable per host/cluster/node rather than forcing one
    /// global label." A [`SpatialScope`](ono_spatial_core::SpatialScope) already nests, so the
    /// requirement is met by giving each link its own remote-host scope and letting coverage
    /// hang off it — one label per host, and clusters and nodes below it, rather than one label
    /// for a federation.
    #[must_use]
    pub fn scope(&self) -> ono_spatial_core::SpatialScope {
        let (host, boot) = match &self.clock {
            Some(clock) => (clock.clock_id(), clock.boot_id()),
            None => (&*self.host, None),
        };
        ono_spatial_core::SpatialScope::remote_host(
            host,
            ono_spatial_core::BootIdentity::of(host, boot, None),
        )
    }

    /// The §7.1 evidence source for `provider_id` reached across this link.
    ///
    /// `None` where the link name or the provider id carries one of the two characters that
    /// structure a composed source name, which [`EvidenceSource::remote`] refuses rather than
    /// producing a name that reads two ways.
    #[must_use]
    pub fn source(&self, provider_id: &str) -> Option<EvidenceSource> {
        EvidenceSource::remote(&self.host, provider_id)
    }

    /// How far the peer's wall clock was from this host's, as measured at the handshake.
    ///
    /// `None` where the peer did not state a reading, because an unmeasured offset is unknown
    /// and never a substituted zero (§35.3).
    #[must_use]
    pub fn offset_from(&self, local_now_at_handshake: Timestamp) -> Option<ono_value::Duration> {
        self.clock.as_ref()?.offset_from(local_now_at_handshake)
    }

    /// The three instants of §3.3 for something that arrived over this link at `ingested_at`.
    ///
    /// `source_time` is whatever the far side said, unchanged. `observed_at` is that same
    /// instant, because the component that observed the thing was over there; where the source
    /// gave no time at all it is the arrival, which is the earliest instant this shell can
    /// honestly name. `ingested_at` is separate from both, always, which is §24.2's prohibition
    /// expressed as three fields that cannot be collapsed.
    #[must_use]
    pub fn times(&self, source_time: Option<Timestamp>, ingested_at: Timestamp) -> EventTimes {
        EventTimes {
            source_time,
            observed_at: source_time.unwrap_or(ingested_at),
            ingested_at,
            source_sequence: None,
            monotonic_nanos: None,
            clock_uncertainty: self.clock.as_ref().and_then(PeerClock::uncertainty),
            domain: self.domain(),
        }
    }

    /// The three instants for an arriving record, taking its source time from its provenance.
    #[must_use]
    pub fn times_of_record(
        &self,
        record: &ono_value::RecordValue,
        ingested_at: Timestamp,
    ) -> EventTimes {
        self.times(record.provenance().observed(), ingested_at)
    }

    /// The three instants for an arriving object event, keeping its source sequence (§25.4).
    ///
    /// The sequence is what [`ono_temporal_core::happens_before`] orders one source's own
    /// stream by; it belongs to the far side's clock domain and means nothing outside it, which
    /// is why the domain travels with it.
    #[must_use]
    pub fn times_of_event(
        &self,
        event: &ono_provider_api::ObjectEvent,
        ingested_at: Timestamp,
    ) -> EventTimes {
        let mut times = self.times(Some(event.at()), ingested_at);
        times.source_sequence = event.sequence();
        times
    }

    /// The transaction identity that lets a causal chain cross this host boundary (§26.4).
    ///
    /// §26.4 permits a causal explanation to cross hosts "only when the evidence chain actually
    /// crosses the boundary", and its own example is a connection identity shared by a request
    /// on one host and its acceptance on another. That identity is the source's own token —
    /// [`ObjectEvent::cause`](ono_provider_api::ObjectEvent::cause) — and this is where it
    /// survives arrival. Proximity in time is not offered here, because §26.4 makes it
    /// correlation at most.
    #[must_use]
    pub fn crossing_evidence(&self, event: &ono_provider_api::ObjectEvent) -> Option<Arc<str>> {
        event.cause().map(Arc::from)
    }
}

/// This host's own clock identity, for the agent to announce at the handshake (§24.2, §25.5).
///
/// The boot is read from `/proc/sys/kernel/random/boot_id`, which is the same fact v0.4 already
/// keys a process `SpatialId` on. A kernel that does not publish it leaves the boot unstated,
/// and an unstated boot separates the domain from everything — the conservative answer, and the
/// one §25.5 needs when monotonic readings cannot be trusted across the boundary.
#[must_use]
pub fn clock_identity(host: &str) -> PeerClock {
    let clock = PeerClock::of(host);
    match boot_id() {
        Some(boot) => clock.on_boot(boot),
        None => clock,
    }
}

/// The kernel's boot identifier, or `None` where this system does not publish one.
fn boot_id() -> Option<String> {
    let text = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(text: &str) -> Timestamp {
        text.parse().unwrap_or(Timestamp::UNIX_EPOCH)
    }

    #[test]
    fn should_keep_the_arrival_apart_from_the_source_time_when_a_remote_report_lands() {
        let ingest = RemoteIngest::new("web01", Some(PeerClock::of("web01").on_boot("boot-a")));
        let times = ingest.times(
            Some(instant("2026-08-31T12:00:00Z")),
            instant("2026-08-31T12:00:05Z"),
        );

        assert_eq!(times.source_time, Some(instant("2026-08-31T12:00:00Z")));
        assert_eq!(times.ingested_at, instant("2026-08-31T12:00:05Z"));
        assert_eq!(
            times.presentation_instant(),
            instant("2026-08-31T12:00:00Z"),
            "§24.2: the ledger MUST NOT overwrite remote source time with ingest time"
        );
    }

    #[test]
    fn should_name_the_arrival_as_the_earliest_honest_instant_when_the_source_gave_none() {
        let ingest = RemoteIngest::new("web01", None);
        let times = ingest.times(None, instant("2026-08-31T12:00:05Z"));

        assert_eq!(times.source_time, None, "an unstated time stays unstated");
        assert_eq!(times.observed_at, instant("2026-08-31T12:00:05Z"));
    }

    #[test]
    fn should_give_each_linked_host_a_coverage_scope_of_its_own() {
        let web = RemoteIngest::new("web01", Some(PeerClock::of("web01").on_boot("boot-a")));
        let db = RemoteIngest::new("db01", Some(PeerClock::of("db01").on_boot("boot-b")));

        assert_ne!(
            web.scope(),
            db.scope(),
            "§24.6: coverage is representable per host rather than forced into one global label"
        );
        assert_eq!(web.scope().id(), "web01");
    }

    #[test]
    fn should_separate_the_domains_of_two_boots_of_one_host() {
        let first = RemoteIngest::new("web01", Some(PeerClock::of("web01").on_boot("boot-a")));
        let second = RemoteIngest::new("web01", Some(PeerClock::of("web01").on_boot("boot-b")));

        assert!(
            !first.domain().is_comparable_to(&second.domain()),
            "§25.5: monotonic clocks reset across boot, so the boot separates the domains"
        );
    }
}
