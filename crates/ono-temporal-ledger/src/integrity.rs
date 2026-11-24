//! Integrity and corruption (v0.5 §31.7, §10.8, §44.1).
//!
//! §31.7 lists five obligations, and they are five because each one is separately breakable:
//!
//! 1. refuse to present affected history as valid — a row that does not decode is refused, never
//!    rendered with its broken fields blanked;
//! 2. identify the affected store or segment — [`IntegrityFinding::segment`] names it;
//! 3. preserve current shell functionality — every finding is data, and opening a damaged store
//!    answers with a report rather than a failure, so `look` still works;
//! 4. offer diagnostic guidance — [`IntegrityFinding::detail`] carries what SQLite said;
//! 5. mark temporal coverage gaps resulting from discarded corrupt data — [`IntegrityReport::gaps`]
//!    carries a `corrupt_segment` gap for every interval that was dropped.
//!
//! Opening the store checks what a damaged file breaks first — the root and the first and last
//! leaf of every table and index — at a cost that does not grow with the store (ADR-1068); the
//! whole-file `PRAGMA integrity_check` runs where a migration is about to rewrite the store. Damage
//! elsewhere in the file is met by the read that reaches it and refused as corruption naming the
//! store. The damage SQLite cannot see is a row whose payload no longer decodes, and that is found
//! on read too: the row fails, the query answers, and the affected interval becomes a gap.

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::SpatialScope;
use ono_temporal_core::{EvidenceSource, GapReason, TemporalGap};

/// The capability a corrupt segment stops covering.
pub(crate) const LEDGER_CAPABILITY: &str = "temporal.events";

/// One thing wrong with the store (§31.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityFinding {
    /// The store or segment the damage is in — a table, a row identity, or the file itself.
    pub segment: Arc<str>,
    /// What a reader needs in order to act on it (§31.7's fourth obligation).
    pub detail: Arc<str>,
    /// Whether the damage stops the store from being written to at all.
    pub fatal: bool,
}

impl IntegrityFinding {
    /// A finding about `segment`.
    #[must_use]
    pub fn new(segment: &str, detail: &str, fatal: bool) -> Self {
        Self {
            segment: Arc::from(segment),
            detail: Arc::from(detail),
            fatal,
        }
    }
}

/// What opening the store found, and what history that costs (§31.7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IntegrityReport {
    /// Everything wrong, in the order it was found.
    pub findings: Vec<IntegrityFinding>,
    /// The intervals discarded because of it (§31.7's fifth obligation).
    pub gaps: Vec<TemporalGap>,
}

impl IntegrityReport {
    /// Whether the store passed every check.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.findings.is_empty()
    }

    /// Whether the damage stops the store being used for history at all.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        self.findings.iter().any(|finding| finding.fatal)
    }

    /// The guidance §31.7 requires an operator to be offered.
    #[must_use]
    pub fn guidance(&self) -> String {
        if self.is_healthy() {
            return "the temporal ledger passed its integrity check".to_owned();
        }
        let segments: Vec<&str> = self
            .findings
            .iter()
            .map(|finding| finding.segment.as_ref())
            .collect();
        format!(
            "the temporal ledger is damaged in {}; the shell keeps working without persistent \
             history, and `remove temporal-history` discards the damaged store so recording can \
             start again",
            segments.join(", ")
        )
    }

    /// Records a finding and the interval it costs.
    pub(crate) fn note(
        &mut self,
        finding: IntegrityFinding,
        scope: &SpatialScope,
        from: Timestamp,
        until: Timestamp,
        source: EvidenceSource,
    ) {
        self.findings.push(finding);
        self.gaps.push(TemporalGap {
            scope: scope.clone(),
            from,
            until,
            capability: Arc::from(LEDGER_CAPABILITY),
            reason: GapReason::CorruptSegment,
            source,
            detail: Some(Arc::from("corrupt ledger segment")),
        });
    }
}
