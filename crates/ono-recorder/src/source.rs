//! What one source is, and what the recorder may claim on its behalf (v0.5 §21, §22, §8.1).
//!
//! A profile is the recorder's own view of a provider: the §7.1 evidence class its events are
//! filed under, the kind of object it reports, whether it is subscribed to or polled, and what
//! §21.1 capabilities it advertises. Two rules turn that into coverage nobody may overstate:
//!
//! - **A polled source never claims completeness.** §21.5: "providers MUST NOT advertise
//!   `exhaustive_events` merely because events usually arrive", and §22.1 requires that a
//!   snapshot source's "coverage MUST reflect polling limitations". [`SourceProfile::polled`]
//!   therefore clears the flag and fixes the sampling interval, and no builder can put it back.
//! - **Absence needs the source to have promised it.** §6.3 forbids reading a polling gap as a
//!   disappearance unless the provider contract makes missing-from-a-complete-snapshot
//!   meaningful, so that is a declaration ([`SourceProfile::meaningful_disappearance`]) rather
//!   than an inference.

use std::sync::Arc;

use jiff::Timestamp;
use ono_provider_api::TemporalCapabilities;
use ono_spatial_core::{PermissionState, SpatialScope, SpatialType};
use ono_temporal_core::{EvidenceSource, TemporalCompleteness, TemporalCoverage};
use ono_value::Duration;

use crate::settings::DEFAULT_INTAKE_CAPACITY;

/// How the recorder receives a source's events (§21.3, §22.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// The provider pushes changes as its source produces them (`Provider::subscribe`).
    Subscribed,
    /// The recorder asks at an interval and compares snapshots (§22.1's `snapshot_diff`).
    Polled {
        /// How often the recorder looks, which is what coverage's sampling interval reports.
        interval: Duration,
    },
}

impl Delivery {
    /// The interval a polled source is sampled at; `None` for a subscription (§3.5).
    #[must_use]
    pub const fn sampling_interval(self) -> Option<Duration> {
        match self {
            Delivery::Polled { interval } => Some(interval),
            Delivery::Subscribed => None,
        }
    }
}

/// One source the recorder collects from (§7.1, §21.1).
#[derive(Debug, Clone, PartialEq)]
pub struct SourceProfile {
    /// The §7.1 evidence class an event from this source is filed under.
    pub source: EvidenceSource,
    /// The provider's id, as `get provider` names it.
    pub provider: Arc<str>,
    /// The kind of object it reports, which fixes the capability its coverage is filed under.
    pub object_type: SpatialType,
    /// What §21.1 says it can answer.
    pub capabilities: TemporalCapabilities,
    /// How its events reach the recorder.
    pub delivery: Delivery,
    /// Whether missing from a complete snapshot means gone (§6.3).
    pub disappearance_is_meaningful: bool,
    /// How many of its events the bounded queue holds (§43.1).
    pub intake_capacity: usize,
}

impl SourceProfile {
    /// A source that is polled by default, because most of §22's are (§22.1, §22.6, §22.7).
    #[must_use]
    pub fn new(source: EvidenceSource, provider: &str, object_type: SpatialType) -> Self {
        Self {
            source,
            provider: Arc::from(provider),
            object_type,
            capabilities: TemporalCapabilities::snapshot_only(),
            delivery: Delivery::Polled {
                interval: Duration::from_nanoseconds(5_000_000_000),
            },
            disappearance_is_meaningful: false,
            intake_capacity: DEFAULT_INTAKE_CAPACITY,
        }
    }

    /// The same source, sampled at `interval` (§22.1).
    ///
    /// Polling clears `exhaustive_events` whatever the provider said: §21.5 makes that flag a
    /// claim about sequence continuity, and a source that is asked every five seconds has none.
    #[must_use]
    pub fn polled(mut self, interval: Duration) -> Self {
        self.delivery = Delivery::Polled { interval };
        self.capabilities.exhaustive_events = false;
        self.capabilities.current_snapshot = true;
        self
    }

    /// The same source, subscribed to through `Provider::subscribe` (§21.3).
    #[must_use]
    pub fn subscribing(mut self) -> Self {
        self.delivery = Delivery::Subscribed;
        self.capabilities.live_events = true;
        self
    }

    /// The same source with the §21.1 capabilities the provider advertises.
    ///
    /// A polled source keeps its cleared `exhaustive_events`, because §21.5 is a prohibition
    /// rather than a default.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: TemporalCapabilities) -> Self {
        let polled = self.is_polled();
        self.capabilities = capabilities;
        if polled {
            self.capabilities.exhaustive_events = false;
        }
        self
    }

    /// The same source claiming, or not, that its sequence supports absence claims (§21.5).
    ///
    /// A polled source cannot claim it, and asking is not an error: the answer is no.
    #[must_use]
    pub fn exhaustive(mut self, exhaustive: bool) -> Self {
        self.capabilities.exhaustive_events = exhaustive && !self.is_polled();
        self
    }

    /// The same source declaring whether missing from a complete snapshot means gone (§6.3).
    #[must_use]
    pub const fn meaningful_disappearance(mut self, meaningful: bool) -> Self {
        self.disappearance_is_meaningful = meaningful;
        self
    }

    /// The same source with a queue of `capacity` events (§43.1).
    #[must_use]
    pub const fn with_intake_capacity(mut self, capacity: usize) -> Self {
        self.intake_capacity = if capacity == 0 { 1 } else { capacity };
        self
    }

    /// Whether the recorder polls this source rather than being pushed to (§22.1).
    #[must_use]
    pub const fn is_polled(&self) -> bool {
        matches!(self.delivery, Delivery::Polled { .. })
    }

    /// Whether this source promises sequence continuity, which is what makes a hole a loss.
    ///
    /// [`ono_temporal_ledger::LedgerStore::declare_contiguous`] is called for exactly these, and
    /// the recorder is the only component that knows which sources made the promise (§43.2).
    #[must_use]
    pub const fn promises_continuity(&self) -> bool {
        self.capabilities.exhaustive_events
    }

    /// The capability this source's existence claims are filed under (§8.1).
    ///
    /// `ono_temporal_reconstruct::capability` builds the name and nothing else spells it: object
    /// presence is gated on this exact string, and a coverage interval filed under a different
    /// one reconstructs to `Presence::Unknown`.
    #[must_use]
    pub fn existence_capability(&self) -> Arc<str> {
        ono_temporal_reconstruct::capability::existence(self.object_type)
    }

    /// The capability one of this source's fields is filed under (§8.1).
    #[must_use]
    pub fn field_capability(&self, field: &str) -> Arc<str> {
        ono_temporal_reconstruct::capability::field(self.object_type, field)
    }

    /// What this source could observe over an interval (§8.2, §8.3, §21.5).
    ///
    /// `complete` only where the provider advertised `exhaustive_events`; everything else is
    /// `partial`, because §8.3 is what "useful evidence exists and absence proves nothing" means
    /// and §7.4 builds negative claims on the difference.
    #[must_use]
    pub const fn completeness(&self) -> TemporalCompleteness {
        if self.capabilities.exhaustive_events {
            TemporalCompleteness::Complete
        } else {
            TemporalCompleteness::Partial
        }
    }

    /// The coverage interval this source's existence claims cover (§8.1, §22.1).
    #[must_use]
    pub fn coverage(
        &self,
        scope: &SpatialScope,
        from: Timestamp,
        until: Timestamp,
        permission: PermissionState,
    ) -> TemporalCoverage {
        self.coverage_of(self.existence_capability(), scope, from, until, permission)
    }

    /// The coverage interval for one named capability (§8.1).
    #[must_use]
    pub fn coverage_of(
        &self,
        capability: Arc<str>,
        scope: &SpatialScope,
        from: Timestamp,
        until: Timestamp,
        permission: PermissionState,
    ) -> TemporalCoverage {
        TemporalCoverage {
            scope: scope.clone(),
            capability,
            from,
            until,
            completeness: self.completeness(),
            sampling_interval: self.delivery.sampling_interval(),
            source: self.source.clone(),
            permission,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_refuse_to_claim_exhaustive_events_when_the_source_is_polled() {
        let profile = SourceProfile::new(
            EvidenceSource::recorder(),
            "linux.procfs",
            SpatialType::Process,
        )
        .polled(Duration::from_nanoseconds(1_000_000_000))
        .exhaustive(true);

        assert!(!profile.promises_continuity());
        assert_eq!(profile.completeness(), TemporalCompleteness::Partial);
    }

    #[test]
    fn should_file_existence_under_the_name_reconstruction_looks_it_up_by() {
        let profile = SourceProfile::new(
            EvidenceSource::recorder(),
            "linux.procfs",
            SpatialType::Process,
        );
        assert_eq!(&*profile.existence_capability(), "process.existence");
    }
}
