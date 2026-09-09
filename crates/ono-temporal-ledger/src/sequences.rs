//! Source sequence continuity (v0.5 §6.8, §25.4, §43.2, §44.1).
//!
//! §25.4: "NTP corrections or manual wall-clock jumps MUST NOT reorder events inside a source
//! sequence. The ledger MUST use sequence/monotonic evidence where available." Keeping the highest
//! sequence per source and clock domain is what makes that possible across a restart, and §44.1's
//! second step — "restore source sequence checkpoints" — is a read of this set.
//!
//! §43.2 is why a break matters: "when an event source exceeds configured capacity, Ono MUST prefer
//! an explicit coverage gap over pretending continuity". A source that declares its sequence
//! contiguous and then skips a number has lost events, and the loss becomes a
//! [`TemporalGap`](ono_temporal_core::TemporalGap) with the reason §7.5 gives it rather than a
//! silent join between two numbers.

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::SpatialScope;
use ono_temporal_core::{ClockDomain, EvidenceSource, GapReason, TemporalGap};

/// The capability a sequence break stops covering.
pub(crate) const SEQUENCE_CAPABILITY: &str = "temporal.events";

/// What one source's sequence looks like in one clock domain (§25.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSequence {
    /// The §7.1 source.
    pub source: EvidenceSource,
    /// The clock domain the numbers belong to. A sequence means nothing across a boot (§25.5).
    pub domain: ClockDomain,
    /// The lowest sequence number seen.
    pub lowest: u64,
    /// The highest sequence number seen — what a restart resumes from (§44.1).
    pub highest: u64,
    /// How many distinct numbers have been seen.
    pub seen: u64,
    /// Whether every number from `lowest` to `highest` arrived.
    pub contiguous: bool,
    /// Whether the source declares its sequence contiguous, which is what makes a hole a loss.
    pub declared_contiguous: bool,
    /// The presentation instant of the last event carrying this sequence.
    pub last_seen_at: Timestamp,
    /// The scope the source was reporting about, so a gap has a place.
    pub scope: SpatialScope,
}

impl SourceSequence {
    /// How many numbers are missing between the lowest and the highest seen.
    #[must_use]
    pub const fn missing(&self) -> u64 {
        let span = self.highest.saturating_sub(self.lowest).saturating_add(1);
        span.saturating_sub(self.seen)
    }

    /// Whether the record is evidence that events were lost (§43.2).
    ///
    /// A source that never claimed contiguity may legitimately skip numbers, so a hole in its
    /// sequence is not a loss. A source that did claim it has lost what is missing.
    #[must_use]
    pub const fn has_lost_events(&self) -> bool {
        self.declared_contiguous && !self.contiguous
    }

    /// The gap a break in a declared-contiguous sequence leaves (§43.2, §44.1).
    ///
    /// `reason` is `source_disconnected` where the source was known to drop, and `not_recorded`
    /// where nothing says which. §43.2 prefers either to pretended continuity.
    #[must_use]
    pub fn gap(&self, from: Timestamp, reason: GapReason) -> TemporalGap {
        TemporalGap {
            scope: self.scope.clone(),
            from,
            until: self.last_seen_at,
            capability: Arc::from(SEQUENCE_CAPABILITY),
            reason,
            source: self.source.clone(),
            detail: ono_temporal_core::gap_detail(&self.source, reason),
        }
    }
}
