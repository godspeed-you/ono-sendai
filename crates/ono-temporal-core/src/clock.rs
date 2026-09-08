//! The clock model of v0.5 §25: which clock an instant was read from, and which of an event's
//! three instants a human navigates by.

use std::sync::Arc;

use jiff::Timestamp;

/// The clock a monotonic reading and a source sequence belong to (§25.5).
///
/// Monotonic clocks reset across boot and mean nothing across hosts, so a reading is comparable
/// only against another from the same domain. §26.4: a causal chain crosses hosts only when the
/// evidence chain actually crosses the boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockDomain {
    /// The host whose clock this is.
    pub host: Arc<str>,
    /// Which boot of that host, where the source could say. `None` is itself a reason two
    /// events cannot be ordered.
    pub boot_id: Option<Arc<str>>,
}

impl ClockDomain {
    /// A domain for `host` on the boot `boot_id`.
    #[must_use]
    pub fn new(host: &str, boot_id: Option<&str>) -> Self {
        Self {
            host: Arc::from(host),
            boot_id: boot_id.map(Arc::from),
        }
    }

    /// Whether a monotonic reading taken in this domain may be compared with one from `other`.
    ///
    /// It may only when both name the same host *and* the same boot: §25.5 makes the boot
    /// boundary the thing that separates clock domains, and an unknown boot separates them from
    /// everything, including itself.
    #[must_use]
    pub fn is_comparable_to(&self, other: &Self) -> bool {
        self.host == other.host
            && match (&self.boot_id, &other.boot_id) {
                (Some(here), Some(there)) => here == there,
                _ => false,
            }
    }
}

/// When an event happened, was seen and was stored (§3.3), and what orders it (§25.1).
///
/// §3.3: the three timestamps "MUST NOT be silently collapsed into one field". They answer three
/// different questions, and a reconstruction that confuses them reports the ledger's own latency
/// as system behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventTimes {
    /// When the source says the event happened. `None` where the source gave no time.
    pub source_time: Option<Timestamp>,
    /// When the observing component received or detected it.
    pub observed_at: Timestamp,
    /// When it entered the Ono ledger. Never a substitute for `source_time` (§24.2).
    pub ingested_at: Timestamp,
    /// The source's own sequence number within its stream (§25.4).
    pub source_sequence: Option<u64>,
    /// A monotonic reading in nanoseconds within this clock domain (§25.1).
    pub monotonic_nanos: Option<u64>,
    /// How far the source's wall clock may be from this host's (§24.4). `None` is unmeasured,
    /// which is not a measured zero.
    pub clock_uncertainty: Option<ono_value::Duration>,
    /// The clock the monotonic reading and the sequence belong to (§25.5).
    pub domain: ClockDomain,
}

impl EventTimes {
    /// A local observation with nothing but the instant it was seen.
    #[must_use]
    pub fn observed(observed_at: Timestamp, ingested_at: Timestamp, domain: ClockDomain) -> Self {
        Self {
            source_time: None,
            observed_at,
            ingested_at,
            source_sequence: None,
            monotonic_nanos: None,
            clock_uncertainty: None,
            domain,
        }
    }

    /// The instant a human navigates by (§25.1: "wall time is used for human navigation").
    ///
    /// The source's own time where it gave one, because that is when the thing happened; the
    /// observation time otherwise, because that is the earliest instant Ono can honestly name.
    #[must_use]
    pub fn presentation_instant(&self) -> Timestamp {
        self.source_time.unwrap_or(self.observed_at)
    }

    /// How long the report took to reach the ledger, as a diagnostic for §43's backpressure.
    #[must_use]
    pub fn ingest_latency(&self) -> ono_value::Duration {
        ono_value::Duration::from_nanoseconds(
            self.ingested_at.as_nanosecond() - self.observed_at.as_nanosecond(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(text: &str) -> Timestamp {
        text.parse().unwrap_or(Timestamp::UNIX_EPOCH)
    }

    #[test]
    fn should_refuse_to_compare_when_a_boot_is_unknown() {
        let known = ClockDomain::new("web01", Some("boot-a"));
        let unknown = ClockDomain::new("web01", None);
        assert!(known.is_comparable_to(&ClockDomain::new("web01", Some("boot-a"))));
        assert!(!known.is_comparable_to(&unknown));
        assert!(
            !unknown.is_comparable_to(&ClockDomain::new("web01", None)),
            "§25.5: two unknown boots are not known to be the same boot"
        );
    }

    #[test]
    fn should_report_the_observation_instant_when_the_source_gave_no_time() {
        let times = EventTimes::observed(
            instant("2026-08-31T12:00:00Z"),
            instant("2026-08-31T12:00:02Z"),
            ClockDomain::new("web01", Some("boot-a")),
        );
        assert_eq!(
            times.presentation_instant(),
            instant("2026-08-31T12:00:00Z")
        );
        assert_eq!(
            times.ingest_latency(),
            ono_value::Duration::from_nanoseconds(2_000_000_000)
        );
    }
}
