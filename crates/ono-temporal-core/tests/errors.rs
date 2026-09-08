//! The temporal error family of v0.5 §34, renumbered by ADR-0610. Every refusal carries the
//! metadata the spec's example messages show, because "no evidence source covers that time" is
//! only useful when it says which sources were asked and how far back each reaches.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_core::ErrorCode;
use ono_spatial_core::PermissionState;
use ono_temporal_core::{EvidenceSource, GapReason, SourceAvailability, TemporalGap, error};

use common::{instant, scope};

#[test]
fn should_name_the_sources_and_their_reach_when_no_evidence_covers_the_requested_time() {
    let refused = error::not_recorded(
        &scope(),
        instant("2026-08-30T12:00:00Z"),
        &[
            SourceAvailability {
                source: EvidenceSource::recorder(),
                earliest: None,
                available: false,
                detail: Some("recording is disabled".into()),
            },
            SourceAvailability {
                source: EvidenceSource::parse("linux.journald").expect("a §7.1 class"),
                earliest: Some(instant("2026-08-31T00:00:00Z")),
                available: true,
                detail: None,
            },
        ],
    );
    assert_eq!(refused.code(), ErrorCode::TemporalNotRecorded);
    let rendered = refused.render_full();
    for expected in ["ono.recorder", "linux.journald", "2026-08-31T00:00:00Z"] {
        assert!(
            rendered.contains(expected),
            "§12.3 asks the refusal to say what was asked and how far back it reaches: {rendered}"
        );
    }
}

#[test]
fn should_name_the_boundary_when_the_requested_time_is_out_of_retention() {
    let refused = error::out_of_retention(
        instant("2026-08-20T12:00:00Z"),
        instant("2026-08-30T12:00:00Z"),
    );
    assert_eq!(refused.code(), ErrorCode::TemporalOutOfRetention);
    assert!(refused.render_full().contains("2026-08-30T12:00:00Z"));
}

#[test]
fn should_name_the_command_when_a_mutation_is_attempted_in_historical_context() {
    let refused = error::read_only("restart service");
    assert_eq!(refused.code(), ErrorCode::TemporalReadOnly);
    assert!(
        refused.message().contains("restart service"),
        "§4.7: the refusal says what was refused: {}",
        refused.message()
    );
    assert_eq!(
        error::present_only("ssh").code(),
        ErrorCode::TemporalPresentOnly
    );
}

#[test]
fn should_list_the_candidates_when_an_event_reference_is_ambiguous() {
    let first = common::event(
        ono_temporal_core::EventKind::ObjectChanged,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
    );
    let second = common::event(
        ono_temporal_core::EventKind::ObjectChanged,
        "2026-08-31T12:00:01Z",
        "linux.procfs",
    );
    let refused = error::ambiguous_event(&[first.event_id.clone(), second.event_id.clone()]);
    assert_eq!(refused.code(), ErrorCode::TemporalAmbiguousEvent);
    let rendered = refused.render_full();
    assert!(rendered.contains(first.event_id.as_str()));
    assert!(rendered.contains(second.event_id.as_str()));
}

#[test]
fn should_name_the_capability_when_a_source_cannot_answer_it() {
    let refused = error::unsupported_source(
        &EvidenceSource::parse("linux.procfs").expect("a §7.1 class"),
        "historical_query",
    );
    assert_eq!(refused.code(), ErrorCode::TemporalUnsupportedSource);
    assert!(refused.render_full().contains("historical_query"));
    assert!(refused.render_full().contains("linux.procfs"));
}

#[test]
fn should_describe_the_hole_when_an_operation_requires_an_interval_it_cannot_have() {
    let refused = error::coverage_gap(&TemporalGap {
        scope: scope(),
        from: instant("2026-08-31T12:20:00Z"),
        until: instant("2026-08-31T12:24:12Z"),
        capability: "service.state".into(),
        reason: GapReason::ProviderUnavailable,
        source: EvidenceSource::recorder(),
    });
    assert_eq!(refused.code(), ErrorCode::TemporalCoverageGap);
    let rendered = refused.render_full();
    assert!(rendered.contains("service.state"));
    assert!(rendered.contains("provider_unavailable"));
}

#[test]
fn should_name_both_events_when_strict_ordering_cannot_be_established() {
    let a = common::event(
        ono_temporal_core::EventKind::ObjectChanged,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
    );
    let b = common::event(
        ono_temporal_core::EventKind::ObjectChanged,
        "2026-08-31T12:00:01Z",
        "remote",
    );
    let refused = error::clock_uncertain(&a.event_id, &b.event_id);
    assert_eq!(refused.code(), ErrorCode::TemporalClockUncertain);
    assert!(refused.render_full().contains(a.event_id.as_str()));
    assert!(refused.render_full().contains(b.event_id.as_str()));
}

#[test]
fn should_carry_the_right_code_for_every_member_of_the_family() {
    assert_eq!(
        error::invalid_time("wobble", "not a time").code(),
        ErrorCode::TemporalInvalidTime
    );
    assert_eq!(
        error::store_unavailable("the store is locked").code(),
        ErrorCode::TemporalStoreUnavailable
    );
    assert_eq!(
        error::store_corrupt("segment-14", "checksum mismatch").code(),
        ErrorCode::TemporalStoreCorrupt
    );
    assert_eq!(
        error::permission_denied(&scope(), "the journal is not readable").code(),
        ErrorCode::TemporalPermissionDenied
    );
    assert_eq!(
        error::recorder_not_running().code(),
        ErrorCode::TemporalRecorderNotRunning
    );
    assert_eq!(
        error::recorder_already_running().code(),
        ErrorCode::TemporalRecorderAlreadyRunning
    );
}

#[test]
fn should_keep_a_denial_apart_from_an_absence_when_history_exists_but_cannot_be_read() {
    let refused = error::permission_denied(&scope(), "the journal is not readable");
    assert_ne!(
        refused.code(),
        ErrorCode::TemporalNotRecorded,
        "§34: history that exists and is not accessible is denied, never not recorded"
    );
    assert_eq!(
        PermissionState::PermissionDenied.as_str(),
        "permission_denied"
    );
}
