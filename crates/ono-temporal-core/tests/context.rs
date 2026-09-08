//! The session temporal context of v0.5 §3.9 and §4, and the prompt markers of §4.6 and §8.6 —
//! "a prompt MUST NOT display `[PAST]` merely because at least one event exists near that time".

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_spatial_core::PermissionState;
use ono_temporal_core::{
    CoverageSummary, EvidenceSource, TemporalCompleteness, TemporalContext, TemporalCoverage,
    TimeRange, TimeSelector,
};

use common::{instant, scope};

fn reconstruction_window() -> TimeRange {
    TimeRange::between(
        instant("2026-08-31T10:00:00Z"),
        instant("2026-08-31T10:10:00Z"),
    )
}

fn coverage(completeness: TemporalCompleteness, from: &str, until: &str) -> CoverageSummary {
    CoverageSummary::compose(
        &[TemporalCoverage {
            scope: scope(),
            capability: "service.state".into(),
            from: instant(from),
            until: instant(until),
            completeness,
            sampling_interval: None,
            source: EvidenceSource::parse("linux.systemd-dbus").expect("a §7.1 class"),
            permission: PermissionState::Available,
        }],
        reconstruction_window(),
    )
}

fn historical(coverage: CoverageSummary) -> TemporalContext {
    TemporalContext::Historical {
        requested: TimeSelector::parse("-10m").expect("a selector"),
        requested_text: "-10m".into(),
        resolved_at: instant("2026-08-31T10:07:14Z"),
        coverage,
        anchor_event: None,
    }
}

#[test]
fn should_show_no_marker_when_the_session_is_in_the_present() {
    let present = TemporalContext::Present;
    assert!(!present.is_historical());
    assert_eq!(
        present.instant(),
        None,
        "§4.1: the present means read the clock, which this crate never does"
    );
    assert_eq!(present.prompt_marker(), None);
}

#[test]
fn should_show_the_certain_marker_when_coverage_supports_the_place_summary() {
    let context = historical(coverage(
        TemporalCompleteness::Complete,
        "2026-08-31T09:00:00Z",
        "2026-08-31T11:00:00Z",
    ));
    assert!(context.is_historical());
    assert_eq!(context.instant(), Some(instant("2026-08-31T10:07:14Z")));
    assert_eq!(
        context.prompt_marker(),
        Some("[PAST]"),
        "§8.6: `[PAST]` means supported coverage sufficient for the current place summary"
    );
}

#[test]
fn should_show_the_uncertain_marker_when_a_single_event_is_all_there_is() {
    let context = historical(coverage(
        TemporalCompleteness::PointSample,
        "2026-08-31T10:05:00Z",
        "2026-08-31T10:05:00Z",
    ));
    assert_eq!(
        context.prompt_marker(),
        Some("[PAST?]"),
        "§8.6 forbids `[PAST]` merely because one event exists near that time"
    );
}

#[test]
fn should_show_the_uncertain_marker_when_part_of_the_window_is_a_gap() {
    let context = historical(coverage(
        TemporalCompleteness::Complete,
        "2026-08-31T10:00:00Z",
        "2026-08-31T10:04:00Z",
    ));
    assert_eq!(context.prompt_marker(), Some("[PAST?]"));
}

#[test]
fn should_show_the_uncertain_marker_when_nothing_covered_the_requested_time() {
    let context = historical(CoverageSummary::compose(&[], reconstruction_window()));
    assert_eq!(
        context.prompt_marker(),
        Some("[PAST?]"),
        "no coverage is uncertain, never complete"
    );
}

#[test]
fn should_keep_what_was_asked_for_when_a_historical_context_is_reported() {
    let context = historical(coverage(
        TemporalCompleteness::Complete,
        "2026-08-31T09:00:00Z",
        "2026-08-31T11:00:00Z",
    ));
    let TemporalContext::Historical { requested_text, .. } = &context else {
        panic!("the fixture is historical");
    };
    assert_eq!(
        &**requested_text, "-10m",
        "§4.2 reports the request beside what it resolved to"
    );
}
