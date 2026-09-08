//! Coverage: the per-capability composition of v0.5 §8.5, the gaps of §7.5, the negative
//! evidence rule of §7.4 and the prompt rule of §8.6 — "a prompt MUST NOT display `[PAST]`
//! merely because at least one event exists near that time".

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_spatial_core::PermissionState;
use ono_temporal_core::{
    CoverageSummary, EvidenceSource, GapReason, HeadlineCoverage, TemporalCompleteness,
    TemporalCoverage,
};

use common::{instant, procfs, scope, systemd, window};

fn interval(
    capability: &str,
    from: &str,
    until: &str,
    completeness: TemporalCompleteness,
    source: EvidenceSource,
) -> TemporalCoverage {
    TemporalCoverage {
        scope: scope(),
        capability: capability.into(),
        from: instant(from),
        until: instant(until),
        completeness,
        sampling_interval: None,
        source,
        permission: PermissionState::Available,
    }
}

#[test]
fn should_compose_per_capability_when_two_sources_cover_different_classes() {
    let summary = CoverageSummary::compose(
        &[
            interval(
                "service.state",
                "2026-08-31T12:00:00Z",
                "2026-08-31T13:00:00Z",
                TemporalCompleteness::Complete,
                systemd(),
            ),
            interval(
                "process.existence",
                "2026-08-31T12:30:00Z",
                "2026-08-31T12:30:00Z",
                TemporalCompleteness::PointSample,
                procfs(),
            ),
        ],
        window(),
    );
    let composed: Vec<(String, TemporalCompleteness)> = summary
        .per_capability()
        .map(|(name, state)| (name.to_owned(), state))
        .collect();
    assert_eq!(
        composed,
        vec![
            (
                "process.existence".to_owned(),
                TemporalCompleteness::PointSample
            ),
            ("service.state".to_owned(), TemporalCompleteness::Complete),
        ],
        "§8.5 composes per capability rather than picking one global label"
    );
}

#[test]
fn should_report_partial_when_one_source_covers_only_part_of_the_window() {
    let summary = CoverageSummary::compose(
        &[interval(
            "service.state",
            "2026-08-31T12:00:00Z",
            "2026-08-31T12:20:00Z",
            TemporalCompleteness::Complete,
            systemd(),
        )],
        window(),
    );
    assert_eq!(
        summary.per_capability().collect::<Vec<_>>(),
        vec![("service.state", TemporalCompleteness::Partial)]
    );
    assert_eq!(summary.headline(), HeadlineCoverage::Partial);
}

#[test]
fn should_join_two_sources_when_together_they_cover_the_whole_window() {
    let summary = CoverageSummary::compose(
        &[
            interval(
                "service.state",
                "2026-08-31T12:00:00Z",
                "2026-08-31T12:40:00Z",
                TemporalCompleteness::Complete,
                systemd(),
            ),
            interval(
                "service.state",
                "2026-08-31T12:30:00Z",
                "2026-08-31T13:00:00Z",
                TemporalCompleteness::Complete,
                procfs(),
            ),
        ],
        window(),
    );
    assert_eq!(
        summary.per_capability().collect::<Vec<_>>(),
        vec![("service.state", TemporalCompleteness::Complete)]
    );
    assert!(
        summary.gaps().is_empty(),
        "the sources overlap, so nothing is missing"
    );
    assert_eq!(summary.headline(), HeadlineCoverage::Complete);
}

#[test]
fn should_propagate_a_gap_when_no_source_covers_part_of_the_window() {
    let summary = CoverageSummary::compose(
        &[
            interval(
                "service.state",
                "2026-08-31T12:00:00Z",
                "2026-08-31T12:20:00Z",
                TemporalCompleteness::Complete,
                systemd(),
            ),
            interval(
                "service.state",
                "2026-08-31T12:24:12Z",
                "2026-08-31T13:00:00Z",
                TemporalCompleteness::Complete,
                systemd(),
            ),
        ],
        window(),
    );
    let gaps = summary.gaps();
    assert_eq!(gaps.len(), 1, "one hole, one gap: {gaps:?}");
    assert_eq!(gaps[0].from, instant("2026-08-31T12:20:00Z"));
    assert_eq!(gaps[0].until, instant("2026-08-31T12:24:12Z"));
    assert_eq!(gaps[0].reason, GapReason::NotRecorded);
    assert_eq!(&*gaps[0].capability, "service.state");
}

#[test]
fn should_name_the_denial_when_the_uncovered_interval_was_refused() {
    let summary = CoverageSummary::compose(
        &[
            interval(
                "service.state",
                "2026-08-31T12:00:00Z",
                "2026-08-31T12:20:00Z",
                TemporalCompleteness::Complete,
                systemd(),
            ),
            interval(
                "service.state",
                "2026-08-31T12:20:00Z",
                "2026-08-31T13:00:00Z",
                TemporalCompleteness::PermissionDenied,
                systemd(),
            ),
        ],
        window(),
    );
    let gaps = summary.gaps();
    assert_eq!(gaps.len(), 1);
    assert_eq!(
        gaps[0].reason,
        GapReason::PermissionDenied,
        "history that exists but cannot be read is denied, never absent (§34)"
    );
}

#[test]
fn should_refuse_to_prove_absence_when_the_only_evidence_is_a_point_sample() {
    let summary = CoverageSummary::compose(
        &[interval(
            "process.existence",
            "2026-08-31T12:30:00Z",
            "2026-08-31T12:30:00Z",
            TemporalCompleteness::PointSample,
            procfs(),
        )],
        window(),
    );
    assert!(
        !summary.can_prove_absence("process.existence"),
        "§7.4: a snapshot at 12:30 cannot prove nothing existed from 12:00 to 13:00"
    );
}

#[test]
fn should_prove_absence_when_a_complete_source_covered_the_whole_window() {
    let summary = CoverageSummary::compose(
        &[interval(
            "service.state",
            "2026-08-31T11:00:00Z",
            "2026-08-31T14:00:00Z",
            TemporalCompleteness::Complete,
            systemd(),
        )],
        window(),
    );
    assert!(summary.can_prove_absence("service.state"));
    assert!(
        !summary.can_prove_absence("process.existence"),
        "a capability nobody covered proves nothing"
    );
}

#[test]
fn should_report_uncertain_when_nothing_covered_the_window() {
    let summary = CoverageSummary::compose(&[], window());
    assert_eq!(summary.headline(), HeadlineCoverage::Uncertain);
    assert_eq!(summary.per_capability().count(), 0);
    assert!(!summary.can_prove_absence("anything"));
}

#[test]
fn should_report_uncertain_when_a_capability_is_only_denied() {
    let summary = CoverageSummary::compose(
        &[interval(
            "service.state",
            "2026-08-31T12:00:00Z",
            "2026-08-31T13:00:00Z",
            TemporalCompleteness::PermissionDenied,
            systemd(),
        )],
        window(),
    );
    assert_eq!(
        summary.per_capability().collect::<Vec<_>>(),
        vec![("service.state", TemporalCompleteness::PermissionDenied)]
    );
    assert_eq!(summary.headline(), HeadlineCoverage::Uncertain);
}

#[test]
fn should_list_every_source_that_contributed_when_the_summary_is_inspected() {
    let summary = CoverageSummary::compose(
        &[
            interval(
                "service.state",
                "2026-08-31T12:00:00Z",
                "2026-08-31T13:00:00Z",
                TemporalCompleteness::Complete,
                systemd(),
            ),
            interval(
                "process.existence",
                "2026-08-31T12:00:00Z",
                "2026-08-31T13:00:00Z",
                TemporalCompleteness::Partial,
                procfs(),
            ),
        ],
        window(),
    );
    let sources: Vec<&str> = summary.sources().map(EvidenceSource::as_str).collect();
    assert_eq!(
        sources,
        vec!["linux.procfs", "linux.systemd-dbus"],
        "§8.5: `inspect` MUST expose source-level detail"
    );
}
