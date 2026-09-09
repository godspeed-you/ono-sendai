//! Temporal coverage (v0.5 §8) and the gaps it leaves (§7.5).
//!
//! §8.1: "coverage MUST NOT be represented by one global boolean", and §8.5 goes further — a
//! reconstruction using several sources "MUST compute the effective coverage per field/relation
//! rather than simply selecting the strongest global label". [`CoverageSummary::compose`] is
//! therefore a per-capability computation, and [`CoverageSummary::headline`] is a *summary* of
//! that computation for a prompt, never the thing the computation was done on.

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{PermissionState, SpatialScope};

use crate::ledger::TimeRange;
use crate::source::EvidenceSource;

/// What a source was capable of observing over an interval (§3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TemporalCompleteness {
    /// The source contract could observe every relevant event or object of this class (§8.2).
    Complete,
    /// Useful evidence exists, and absence cannot be read as proof of non-existence (§8.3).
    Partial,
    /// One snapshot at one instant. It supports state there and explains no change (§8.4).
    PointSample,
    /// Completeness could not be established.
    Unknown,
    /// The source was not there to observe (§21.8).
    Unavailable,
    /// The material exists and this user may not read it (§34 `temporal.permission_denied`).
    PermissionDenied,
}

impl TemporalCompleteness {
    /// Every state, in the order §3.5 lists them.
    pub const ALL: &'static [TemporalCompleteness] = &[
        TemporalCompleteness::Complete,
        TemporalCompleteness::Partial,
        TemporalCompleteness::PointSample,
        TemporalCompleteness::Unknown,
        TemporalCompleteness::Unavailable,
        TemporalCompleteness::PermissionDenied,
    ];

    /// The name §3.5 and `ono.temporal-coverage/1` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            TemporalCompleteness::Complete => "complete",
            TemporalCompleteness::Partial => "partial",
            TemporalCompleteness::PointSample => "point_sample",
            TemporalCompleteness::Unknown => "unknown",
            TemporalCompleteness::Unavailable => "unavailable",
            TemporalCompleteness::PermissionDenied => "permission_denied",
        }
    }

    /// The state with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|state| state.as_str() == name)
    }

    /// Whether an interval in this state contributes evidence at all.
    #[must_use]
    pub const fn is_covering(self) -> bool {
        matches!(
            self,
            TemporalCompleteness::Complete | TemporalCompleteness::Partial
        )
    }
}

impl std::fmt::Display for TemporalCompleteness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why an interval is uncovered (§7.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GapReason {
    /// Nothing was collecting.
    NotRecorded,
    /// It was collected and retention has since removed it (§10.4).
    RetentionExpired,
    /// The source was not running or not reachable (§21.8).
    ProviderUnavailable,
    /// The material exists and this user may not read it.
    PermissionDenied,
    /// The link or subscription dropped mid-interval (§24.5).
    SourceDisconnected,
    /// The clocks cannot be reconciled well enough to place events here (§25.4).
    ClockUncertain,
    /// The stored segment did not read back (§31.7).
    CorruptSegment,
    /// No source can answer for this capability at all (§21.1).
    Unsupported,
}

impl GapReason {
    /// Every reason, in the order §7.5 lists them.
    pub const ALL: &'static [GapReason] = &[
        GapReason::NotRecorded,
        GapReason::RetentionExpired,
        GapReason::ProviderUnavailable,
        GapReason::PermissionDenied,
        GapReason::SourceDisconnected,
        GapReason::ClockUncertain,
        GapReason::CorruptSegment,
        GapReason::Unsupported,
    ];

    /// The name §7.5 and `ono.temporal-gap/1` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            GapReason::NotRecorded => "not_recorded",
            GapReason::RetentionExpired => "retention_expired",
            GapReason::ProviderUnavailable => "provider_unavailable",
            GapReason::PermissionDenied => "permission_denied",
            GapReason::SourceDisconnected => "source_disconnected",
            GapReason::ClockUncertain => "clock_uncertain",
            GapReason::CorruptSegment => "corrupt_segment",
            GapReason::Unsupported => "unsupported",
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

impl std::fmt::Display for GapReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One source's claim about one capability over one interval (§8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalCoverage {
    /// The v0.4 boundary the claim is about.
    pub scope: SpatialScope,
    /// The state class it covers — `process.existence`, `relation:process.owns_socket`.
    pub capability: Arc<str>,
    /// When the interval starts.
    pub from: Timestamp,
    /// When it ends. Equal to `from` for a point sample (§8.4).
    pub until: Timestamp,
    /// What the source could observe over it (§3.5).
    pub completeness: TemporalCompleteness,
    /// How often a polled source looked. `None` for an event stream (§3.5).
    pub sampling_interval: Option<ono_value::Duration>,
    /// The §7.1 source making the claim.
    pub source: EvidenceSource,
    /// What this user could be told over the interval (v0.4 §35.2).
    pub permission: PermissionState,
}

impl TemporalCoverage {
    /// Whether the interval touches `[from, until)`, treating a point sample as its own instant.
    fn overlaps(&self, from: Timestamp, until: Timestamp) -> bool {
        if self.from == self.until {
            self.from >= from && (self.from < until || from == until)
        } else {
            self.from < until && self.until > from || (from == until && self.covers_instant(from))
        }
    }

    fn covers_instant(&self, at: Timestamp) -> bool {
        self.from <= at && at <= self.until
    }
}

/// An interval in which a capability was not covered, and why (§7.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalGap {
    /// The v0.4 boundary the gap is in.
    pub scope: SpatialScope,
    /// When the gap starts.
    pub from: Timestamp,
    /// When it ends.
    pub until: Timestamp,
    /// The state class that went uncovered.
    pub capability: Arc<str>,
    /// Why nothing is known here (§7.5).
    pub reason: GapReason,
    /// The §7.1 source that would have covered the interval.
    pub source: EvidenceSource,
    /// What the producer adds to the reason, in the words §11.7 renders.
    ///
    /// §11.7's own worked example is this field: `---- coverage gap: recorder offline 4m12s ----`
    /// says `recorder offline`, which no reason word spells. The producer that knows why the
    /// interval is empty writes the phrase; `None` where the reason says everything, and a
    /// renderer then falls back to the reason.
    pub detail: Option<Arc<str>>,
}

/// The words §11.7 renders for a gap in `capability` that `source` would have covered.
///
/// The phrase is the producer's, so a renderer never has to invent one and never has to know the
/// source vocabulary. It is stated for the combinations §7.5 and §10 give words to and is `None`
/// otherwise, because a reason word on its own is honest and an invented phrase is not.
#[must_use]
pub fn gap_detail(source: &EvidenceSource, reason: GapReason) -> Option<Arc<str>> {
    let recorder = source.as_str() == EvidenceSource::recorder().as_str();
    match (recorder, reason) {
        // §10.8: the recorder is a process that can be stopped, and an interval it did not cover
        // is the interval it was not running for — which is what §11.7 prints.
        (true, GapReason::ProviderUnavailable | GapReason::SourceDisconnected) => {
            Some(Arc::from("recorder offline"))
        }
        (true, GapReason::NotRecorded) => Some(Arc::from("recorder not running")),
        (_, GapReason::RetentionExpired) => Some(Arc::from("beyond retention")),
        (false, GapReason::SourceDisconnected) => Some(Arc::from(
            format!("{} disconnected", source.as_str()).as_str(),
        )),
        (false, GapReason::ProviderUnavailable) => Some(Arc::from(
            format!("{} unavailable", source.as_str()).as_str(),
        )),
        _ => None,
    }
}

/// What a renderer or a prompt may say in one word about a whole reconstruction (§8.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeadlineCoverage {
    /// Every capability was completely covered over the window.
    Complete,
    /// Evidence exists everywhere it matters, and some of it is not complete.
    Partial,
    /// Something material is unknown, unavailable or refused.
    Uncertain,
}

impl HeadlineCoverage {
    /// The name §8.5 spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            HeadlineCoverage::Complete => "complete",
            HeadlineCoverage::Partial => "partial",
            HeadlineCoverage::Uncertain => "uncertain",
        }
    }
}

impl std::fmt::Display for HeadlineCoverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The composed coverage of a reconstruction (§8.5).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverageSummary {
    window: TimeRange,
    capabilities: Vec<(Arc<str>, TemporalCompleteness)>,
    gaps: Vec<TemporalGap>,
    sources: Vec<EvidenceSource>,
}

impl CoverageSummary {
    /// Composes `intervals` over `window`, per capability (§8.5).
    ///
    /// The window's own ends bound the answer; where it names none, the intervals' own extent
    /// stands in, because a summary over an unbounded window would be a claim about all time.
    #[must_use]
    pub fn compose(intervals: &[TemporalCoverage], window: TimeRange) -> Self {
        let Some((from, until)) = effective_window(intervals, &window) else {
            return Self {
                window,
                ..Self::default()
            };
        };

        let mut capabilities: Vec<Arc<str>> = intervals
            .iter()
            .filter(|interval| interval.overlaps(from, until))
            .map(|interval| Arc::clone(&interval.capability))
            .collect();
        capabilities.sort_unstable();
        capabilities.dedup();

        let mut composed = Vec::with_capacity(capabilities.len());
        let mut gaps = Vec::new();
        for capability in capabilities {
            let relevant: Vec<&TemporalCoverage> = intervals
                .iter()
                .filter(|interval| {
                    interval.capability == capability && interval.overlaps(from, until)
                })
                .collect();
            let (state, mut found) = compose_one(&capability, &relevant, from, until);
            composed.push((capability, state));
            gaps.append(&mut found);
        }

        let mut sources: Vec<EvidenceSource> = intervals
            .iter()
            .filter(|interval| interval.overlaps(from, until))
            .map(|interval| interval.source.clone())
            .collect();
        sources.sort_unstable();
        sources.dedup();

        Self {
            window: TimeRange::between(from, until),
            capabilities: composed,
            gaps,
            sources,
        }
    }

    /// The one word a prompt or a renderer may use (§8.5).
    ///
    /// It is derived from the per-capability composition, never from a source label, so §8.6's
    /// "a prompt MUST NOT display `[PAST]` merely because at least one event exists near that
    /// time" holds by construction: one point sample composes to `point_sample`, which is not
    /// complete.
    #[must_use]
    pub fn headline(&self) -> HeadlineCoverage {
        if self.capabilities.is_empty() {
            return HeadlineCoverage::Uncertain;
        }
        if self
            .capabilities
            .iter()
            .all(|(_, state)| *state == TemporalCompleteness::Complete)
        {
            return HeadlineCoverage::Complete;
        }
        if self.capabilities.iter().any(|(_, state)| {
            matches!(
                state,
                TemporalCompleteness::Unknown
                    | TemporalCompleteness::Unavailable
                    | TemporalCompleteness::PermissionDenied
            )
        }) {
            return HeadlineCoverage::Uncertain;
        }
        HeadlineCoverage::Partial
    }

    /// The composition, capability by capability (§8.5).
    pub fn per_capability(&self) -> impl Iterator<Item = (&str, TemporalCompleteness)> {
        self.capabilities
            .iter()
            .map(|(name, state)| (&**name, *state))
    }

    /// What one capability composed to, or `None` where nothing covered it.
    #[must_use]
    pub fn completeness_of(&self, capability: &str) -> Option<TemporalCompleteness> {
        self.capabilities
            .iter()
            .find(|(name, _)| &**name == capability)
            .map(|(_, state)| *state)
    }

    /// Whether Ono may claim that something did not exist (§7.4).
    ///
    /// "Ono MAY claim `process X did not exist at time T` only if an authoritative or
    /// sufficiently complete source had coverage capable of proving that absence." Complete
    /// coverage over the whole window with no gap in it is that condition and nothing less is.
    #[must_use]
    pub fn can_prove_absence(&self, capability: &str) -> bool {
        self.completeness_of(capability) == Some(TemporalCompleteness::Complete)
            && !self.gaps.iter().any(|gap| &*gap.capability == capability)
    }

    /// Every gap the composition found (§7.5).
    #[must_use]
    pub fn gaps(&self) -> &[TemporalGap] {
        &self.gaps
    }

    /// Every source that contributed, so `inspect` can show source-level detail (§8.5).
    pub fn sources(&self) -> impl Iterator<Item = &EvidenceSource> {
        self.sources.iter()
    }

    /// The window the composition covers.
    #[must_use]
    pub fn window(&self) -> &TimeRange {
        &self.window
    }

    /// Whether anything at all was composed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }
}

/// The window a composition actually answers over.
fn effective_window(
    intervals: &[TemporalCoverage],
    window: &TimeRange,
) -> Option<(Timestamp, Timestamp)> {
    let from = window
        .from
        .or_else(|| intervals.iter().map(|interval| interval.from).min())?;
    let until = window
        .until
        .or_else(|| intervals.iter().map(|interval| interval.until).max())?;
    (from <= until).then_some((from, until))
}

/// Composes one capability's intervals over `[from, until]`.
fn compose_one(
    capability: &Arc<str>,
    intervals: &[&TemporalCoverage],
    from: Timestamp,
    until: Timestamp,
) -> (TemporalCompleteness, Vec<TemporalGap>) {
    let covering = merged(intervals, from, until, TemporalCompleteness::is_covering);
    let complete = merged(intervals, from, until, |state| {
        state == TemporalCompleteness::Complete
    });

    let holes = holes_in(&complete, from, until);
    let state = if holes.is_empty() && !complete.is_empty() {
        TemporalCompleteness::Complete
    } else if !covering.is_empty() {
        TemporalCompleteness::Partial
    } else {
        weakest_state(intervals)
    };

    // Gaps are the parts no covering source reached: a partial source still saw something there,
    // and reporting that as a hole would be as dishonest as hiding a real one (§11.7).
    let gaps = holes_in(&covering, from, until)
        .into_iter()
        .map(|(gap_from, gap_until)| {
            let (reason, source) = explain(intervals, gap_from, gap_until);
            let detail = gap_detail(&source, reason);
            TemporalGap {
                scope: intervals
                    .first()
                    .map_or_else(fallback_scope, |interval| interval.scope.clone()),
                from: gap_from,
                until: gap_until,
                capability: Arc::clone(capability),
                reason,
                source,
                detail,
            }
        })
        .collect();
    (state, gaps)
}

/// The scope a gap belongs to when every interval vanished, which the caller prevents.
fn fallback_scope() -> SpatialScope {
    SpatialScope::host(
        "localhost",
        ono_spatial_core::BootIdentity::unknown_boot("localhost"),
    )
}

/// The union of the intervals matching `select`, clipped to `[from, until]`.
fn merged(
    intervals: &[&TemporalCoverage],
    from: Timestamp,
    until: Timestamp,
    select: impl Fn(TemporalCompleteness) -> bool,
) -> Vec<(Timestamp, Timestamp)> {
    let mut spans: Vec<(Timestamp, Timestamp)> = intervals
        .iter()
        .filter(|interval| select(interval.completeness))
        .map(|interval| (interval.from.max(from), interval.until.min(until)))
        .filter(|(start, end)| start < end || (from == until && start == end))
        .collect();
    spans.sort_unstable();
    let mut union: Vec<(Timestamp, Timestamp)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match union.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => union.push((start, end)),
        }
    }
    union
}

/// The parts of `[from, until]` no span in `union` reaches.
fn holes_in(
    union: &[(Timestamp, Timestamp)],
    from: Timestamp,
    until: Timestamp,
) -> Vec<(Timestamp, Timestamp)> {
    if from == until {
        return if union.is_empty() {
            vec![(from, until)]
        } else {
            Vec::new()
        };
    }
    let mut holes = Vec::new();
    let mut cursor = from;
    for (start, end) in union {
        if *start > cursor {
            holes.push((cursor, *start));
        }
        cursor = cursor.max(*end);
    }
    if cursor < until {
        holes.push((cursor, until));
    }
    holes
}

/// Why an uncovered span is uncovered, and whose gap it is.
fn explain(
    intervals: &[&TemporalCoverage],
    from: Timestamp,
    until: Timestamp,
) -> (GapReason, EvidenceSource) {
    for interval in intervals {
        if !interval.overlaps(from, until) {
            continue;
        }
        match interval.completeness {
            TemporalCompleteness::PermissionDenied => {
                return (GapReason::PermissionDenied, interval.source.clone());
            }
            TemporalCompleteness::Unavailable => {
                return (GapReason::ProviderUnavailable, interval.source.clone());
            }
            _ => {}
        }
    }
    let source = intervals
        .first()
        .map_or_else(EvidenceSource::session, |interval| interval.source.clone());
    (GapReason::NotRecorded, source)
}

/// What a capability composes to when no source covered any of the window.
fn weakest_state(intervals: &[&TemporalCoverage]) -> TemporalCompleteness {
    for state in [
        TemporalCompleteness::PointSample,
        TemporalCompleteness::PermissionDenied,
        TemporalCompleteness::Unavailable,
    ] {
        if intervals
            .iter()
            .any(|interval| interval.completeness == state)
        {
            return state;
        }
    }
    TemporalCompleteness::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(text: &str) -> Timestamp {
        text.parse().unwrap_or(Timestamp::UNIX_EPOCH)
    }

    #[test]
    fn should_find_the_hole_between_two_spans_when_they_do_not_meet() {
        let union = vec![
            (
                instant("2026-01-01T00:00:00Z"),
                instant("2026-01-01T01:00:00Z"),
            ),
            (
                instant("2026-01-01T02:00:00Z"),
                instant("2026-01-01T03:00:00Z"),
            ),
        ];
        assert_eq!(
            holes_in(
                &union,
                instant("2026-01-01T00:00:00Z"),
                instant("2026-01-01T03:00:00Z")
            ),
            vec![(
                instant("2026-01-01T01:00:00Z"),
                instant("2026-01-01T02:00:00Z")
            )]
        );
    }

    #[test]
    fn should_read_every_vocabulary_word_back_when_it_is_rendered() {
        for state in TemporalCompleteness::ALL {
            assert_eq!(
                TemporalCompleteness::from_name(state.as_str()),
                Some(*state)
            );
        }
        for reason in GapReason::ALL {
            assert_eq!(GapReason::from_name(reason.as_str()), Some(*reason));
        }
    }
}
