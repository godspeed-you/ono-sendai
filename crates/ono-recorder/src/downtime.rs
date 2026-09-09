//! What a restart owes the record (v0.5 §44.1, §44.2, §44.3, §43.2, §55.5).
//!
//! §44.1 lists five steps and the order is the specification's own:
//!
//! 1. validate store metadata;
//! 2. restore source sequence checkpoints;
//! 3. mark any unobserved downtime as a coverage gap;
//! 4. take a fresh checkpoint as appropriate;
//! 5. continue without pretending the gap was covered.
//!
//! Step three is the one everything else exists for. Between the last event the store holds and
//! the instant the recorder came back, nothing was watching, and §55.5 names the failure mode of
//! leaving that implicit: a "silent gap" reads as a quiet morning. So the interval becomes a
//! [`TemporalGap`] per source, filed under the `<type>.existence` capability object presence is
//! gated on, with `ono.recorder` as the source and the words §11.7 renders.
//!
//! §44.2 is the other half: "a host reboot creates a new boot clock domain ... monotonic/sequence
//! semantics do not cross the boot boundary unless a provider supplies explicit continuity". A
//! plan therefore reports the boot change and refuses to resume a sequence across it — the wall
//! clock is continuous, the numbering is not.

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::SpatialScope;
use ono_temporal_core::{
    ClockDomain, EvidenceSource, GapReason, LedgerRead as _, TemporalGap, gap_detail,
};
use ono_temporal_ledger::{IntegrityReport, LedgerStore, SourceSequence};
use ono_value::ErrorValue;

use crate::source::SourceProfile;

/// What §44.1's procedure found, and what it asks the recorder to do next.
#[derive(Debug, Clone)]
pub struct RestartPlan {
    /// The schema version the store is at, after §44.3's migration (step 1).
    pub store_version: u32,
    /// What validating the store's metadata found (step 1, §31.7).
    pub integrity: IntegrityReport,
    /// Every source sequence the store holds, whatever clock domain it belongs to (step 2).
    pub sequences: Vec<SourceSequence>,
    /// The clock domain this run belongs to (§25.5).
    pub domain: ClockDomain,
    /// Whether the host rebooted since the last run (§44.2).
    pub boot_changed: bool,
    /// When the recorder was last observing, or `None` where it never has been.
    pub last_observed: Option<Timestamp>,
    /// The downtime, and every break a declared-contiguous source left (step 3, §43.2).
    pub gaps: Vec<TemporalGap>,
    /// Whether step 4 wants a fresh checkpoint.
    pub checkpoint_due: bool,
}

impl RestartPlan {
    /// The sequence a source may resume from, or `None` where it may not (§44.2).
    ///
    /// A number is resumable only inside the clock domain it was issued in. Across a boot there
    /// is no continuity to resume, so the answer is `None` and the source starts again — which is
    /// what stops a fresh `1` being read as a hole after `4711`.
    #[must_use]
    pub fn resumable_sequence(&self, source: &EvidenceSource) -> Option<u64> {
        self.sequences
            .iter()
            .find(|sequence| {
                &sequence.source == source && sequence.domain.is_comparable_to(&self.domain)
            })
            .map(|sequence| sequence.highest)
    }

    /// The interval nothing was watching, or `None` where there was none.
    #[must_use]
    pub fn downtime(&self) -> Option<(Timestamp, Timestamp)> {
        self.gaps
            .iter()
            .find(|gap| gap.reason == GapReason::NotRecorded)
            .map(|gap| (gap.from, gap.until))
    }
}

/// Runs §44.1's restart procedure over `store`, in the order §44.1 gives it.
///
/// `now` is a parameter, as it is everywhere below the recorder (§39.2): the recorder is what
/// reads the clock, and a downtime test that could not choose the instant would measure the
/// machine it ran on.
///
/// # Errors
///
/// Returns a §34 store refusal where the store cannot answer for its own metadata.
pub fn restart(
    store: &LedgerStore,
    scope: &SpatialScope,
    domain: &ClockDomain,
    sources: &[SourceProfile],
    now: Timestamp,
) -> Result<RestartPlan, ErrorValue> {
    // 1. Validate store metadata.
    let integrity = store.integrity();
    let store_version = store.store_version();
    let _sets = store.logical_sets()?;

    // 2. Restore source sequence checkpoints.
    let sequences = store.source_sequences()?;
    let boot_changed = sequences.iter().any(|sequence| {
        sequence.domain.host == domain.host && !sequence.domain.is_comparable_to(domain)
    });

    // 3. Mark any unobserved downtime as a coverage gap.
    let last_observed = store.retention().latest;
    let mut gaps = integrity.gaps.clone();
    gaps.extend(store.sequence_gaps()?);
    if let Some(from) = last_observed
        && from < now
    {
        gaps.extend(downtime_gaps(scope, sources, from, now));
    }

    // 4. Take a fresh checkpoint as appropriate. A restart always wants one: whatever the store
    //    holds was projected before the interval nobody watched, so §9.1 has nothing nearer.
    // 5. Continue without pretending the gap was covered — which is what `gaps` is.
    Ok(RestartPlan {
        store_version,
        integrity,
        sequences,
        domain: domain.clone(),
        boot_changed,
        last_observed,
        gaps,
        checkpoint_due: true,
    })
}

/// One gap per source's existence capability, over the interval nothing was watching (§44.1).
///
/// The source is `ono.recorder` rather than the provider: the provider was not unavailable, the
/// recorder was not running, and `gap_detail` renders exactly that distinction for §11.7.
fn downtime_gaps(
    scope: &SpatialScope,
    sources: &[SourceProfile],
    from: Timestamp,
    until: Timestamp,
) -> Vec<TemporalGap> {
    let recorder = EvidenceSource::recorder();
    let detail = gap_detail(&recorder, GapReason::NotRecorded);
    let mut capabilities: Vec<Arc<str>> = sources
        .iter()
        .map(SourceProfile::existence_capability)
        .collect();
    capabilities.push(Arc::from(RECORDER_CAPABILITY));
    capabilities.sort();
    capabilities.dedup();
    capabilities
        .into_iter()
        .map(|capability| TemporalGap {
            scope: scope.clone(),
            from,
            until,
            capability,
            reason: GapReason::NotRecorded,
            source: recorder.clone(),
            detail: detail.clone(),
        })
        .collect()
}

/// The capability the recorder's own coverage is filed under, beside each source's.
///
/// The ledger uses the same name for a sequence break and a corrupt segment, so a reader asking
/// "what could this ledger say about events here" has one capability to ask about whatever made
/// the interval empty.
pub const RECORDER_CAPABILITY: &str = "temporal.events";
