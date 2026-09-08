//! The session temporal context of v0.5 §3.9 and §4.
//!
//! §8.6 fixes what the prompt may claim: `[PAST]` means "supported coverage sufficient for
//! current place summary", `[PAST?]` means the reconstruction is materially partial or
//! uncertain, and "a prompt MUST NOT display `[PAST]` merely because at least one event exists
//! near that time". [`TemporalContext::prompt_marker`] therefore reads the composed coverage
//! summary rather than counting events.

use std::sync::Arc;

use jiff::Timestamp;

use crate::coverage::{CoverageSummary, HeadlineCoverage};
use crate::id::EventId;
use crate::time::TimeSelector;

/// Where in time the session is evaluating (§3.9).
///
/// The context is orthogonal to spatial place: §4.2's `at` "MUST NOT change spatial place", so
/// nothing about a place appears here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemporalContext {
    /// The present. Current behaviour from v0.2–v0.4 is unchanged (§4.1).
    Present,
    /// A resolved historical instant, with the coverage that backs it.
    Historical {
        /// The selector that was resolved.
        requested: TimeSelector,
        /// What the user typed, verbatim, because the instant alone cannot say (§4.2).
        requested_text: Arc<str>,
        /// The instant a query evaluates at.
        resolved_at: Timestamp,
        /// The composed coverage behind the reconstruction (§8.5).
        coverage: CoverageSummary,
        /// The event `at event @e42` resolved through (§12.2).
        anchor_event: Option<EventId>,
    },
}

impl TemporalContext {
    /// Whether the session is evaluating in the past, which §4.7 makes read-only.
    #[must_use]
    pub const fn is_historical(&self) -> bool {
        matches!(self, TemporalContext::Historical { .. })
    }

    /// The instant a query evaluates at.
    ///
    /// `None` in the present, which means "read the clock" — done by the caller, never here
    /// (§39.2).
    #[must_use]
    pub const fn instant(&self) -> Option<Timestamp> {
        match self {
            TemporalContext::Present => None,
            TemporalContext::Historical { resolved_at, .. } => Some(*resolved_at),
        }
    }

    /// The composed coverage behind the reconstruction, or `None` in the present.
    #[must_use]
    pub const fn coverage(&self) -> Option<&CoverageSummary> {
        match self {
            TemporalContext::Present => None,
            TemporalContext::Historical { coverage, .. } => Some(coverage),
        }
    }

    /// What the user asked for, verbatim, or `None` in the present (§4.2).
    #[must_use]
    pub fn requested_text(&self) -> Option<&str> {
        match self {
            TemporalContext::Present => None,
            TemporalContext::Historical { requested_text, .. } => Some(requested_text),
        }
    }

    /// The prompt marker of §4.6, or `None` in the present.
    ///
    /// `[PAST]` only where every capability composed to complete coverage over the window;
    /// `[PAST?]` for everything else, including no coverage at all. §45.2: the word carries the
    /// meaning without colour.
    #[must_use]
    pub fn prompt_marker(&self) -> Option<&'static str> {
        match self {
            TemporalContext::Present => None,
            TemporalContext::Historical { coverage, .. } => Some(match coverage.headline() {
                HeadlineCoverage::Complete => "[PAST]",
                HeadlineCoverage::Partial | HeadlineCoverage::Uncertain => "[PAST?]",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_answer_nothing_about_time_when_the_session_is_in_the_present() {
        let present = TemporalContext::Present;
        assert!(!present.is_historical());
        assert_eq!(present.instant(), None);
        assert_eq!(present.coverage(), None);
        assert_eq!(present.requested_text(), None);
        assert_eq!(present.prompt_marker(), None);
    }
}
