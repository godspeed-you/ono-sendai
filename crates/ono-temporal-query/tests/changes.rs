//! Outcome tests for the changes engine and the historical provider merge
//! (spec v0.5 §13, §9.1, §18.7, §21.4).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::sync::Arc;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    ChangeCertainty, ChangeClass, EventKind, EvidenceSource, EvidenceStrength, LedgerWrite,
    SessionLedger, TemporalCompleteness,
};
use ono_temporal_query::changes::{ChangesRequest, changes, summarise};
use ono_temporal_query::merge::{FieldAnswer, merge_fields};
use ono_value::Value;
use support::{changed, coverage, event, id, instant, ledger, relation, subject};

fn request() -> ChangesRequest {
    ChangesRequest::new(support::scope(), instant("2026-08-31T12:00:00Z"))
}

#[test]
fn should_report_the_five_classes_when_objects_and_relations_moved_in_the_window() {
    let held = ledger(&[
        event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:05:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "backup")),
        ),
        event(
            EventKind::ObjectDisappeared,
            "2026-08-31T12:06:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "old-backup")),
        ),
        changed(
            "2026-08-31T12:07:00Z",
            "linux.systemd-dbus",
            subject(SpatialType::Service, "backup"),
            "active_state",
            Some("running"),
            Some("failed"),
        ),
        relation(
            EventKind::RelationAdded,
            "2026-08-31T12:08:00Z",
            "linux.netlink",
            subject(SpatialType::Connection, "98133"),
            subject(SpatialType::Process, "backup"),
            "connection.owned_by",
        ),
        relation(
            EventKind::RelationRemoved,
            "2026-08-31T12:09:00Z",
            "linux.netlink",
            subject(SpatialType::Connection, "97001"),
            subject(SpatialType::Process, "old-backup"),
            "connection.owned_by",
        ),
    ]);
    let found = changes(&held, &request(), instant("2026-08-31T12:10:00Z"))
        .expect("the changes are computed");
    let classes: Vec<ChangeClass> = found.iter().map(|change| change.class).collect();
    assert!(classes.contains(&ChangeClass::Added));
    assert!(classes.contains(&ChangeClass::Removed));
    assert!(classes.contains(&ChangeClass::Changed));
    assert!(classes.contains(&ChangeClass::RelationAdded));
    assert!(classes.contains(&ChangeClass::RelationRemoved));
    assert_eq!(found.len(), 5, "§13.2 defines five classes and no sixth");
}

#[test]
fn should_end_the_comparison_at_the_temporal_coordinate_when_until_is_omitted() {
    let held = ledger(&[changed(
        "2026-08-31T12:20:00Z",
        "linux.procfs",
        subject(SpatialType::Filesystem, "data"),
        "used",
        Some("81"),
        Some("94"),
    )]);
    let found = changes(&held, &request(), instant("2026-08-31T12:30:00Z"))
        .expect("the changes are computed");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].from_time, instant("2026-08-31T12:00:00Z"));
    assert_eq!(
        found[0].to_time,
        instant("2026-08-31T12:30:00Z"),
        "§13.1: `--until` omitted means the active temporal coordinate"
    );
}

#[test]
fn should_report_the_earlier_side_as_unknown_when_nothing_observed_it() {
    let held = SessionLedger::new();
    held.append(
        &[changed(
            "2026-08-31T12:20:00Z",
            "linux.procfs",
            subject(SpatialType::Filesystem, "data"),
            "used",
            None,
            Some("94.1%"),
        )],
        &[],
    )
    .expect("the ledger appends");
    held.record_coverage(&[coverage(
        "filesystem.usage",
        "2026-08-31T12:10:00Z",
        "2026-08-31T12:30:00Z",
        TemporalCompleteness::Partial,
    )])
    .expect("the ledger records coverage");

    let found = changes(&held, &request(), instant("2026-08-31T12:30:00Z"))
        .expect("the changes are computed");
    assert_eq!(found.len(), 1);
    let field = &found[0].field_changes[0];
    assert_eq!(
        field.before, None,
        "§13.4: unknown, never zero and never empty"
    );
    assert_ne!(field.before, Some(Value::Int(0)));
    assert_ne!(field.before, Some(Value::string("")));
    assert_eq!(field.certainty, ChangeCertainty::Unknown);
    assert_eq!(
        found[0].coverage.completeness_of("filesystem.usage"),
        Some(TemporalCompleteness::Partial),
        "§13.4: the change carries the coverage that explains the unknown"
    );
    assert!(
        !found[0].coverage.gaps().is_empty(),
        "the uncovered stretch before the first observation is a gap"
    );
}

#[test]
fn should_keep_the_earlier_side_when_the_source_reported_the_transition() {
    let held = ledger(&[changed(
        "2026-08-31T12:20:00Z",
        "linux.systemd-dbus",
        subject(SpatialType::Service, "nginx"),
        "active_state",
        Some("active"),
        Some("failed"),
    )]);
    let found = changes(&held, &request(), instant("2026-08-31T12:30:00Z"))
        .expect("the changes are computed");
    let field = &found[0].field_changes[0];
    assert_eq!(field.before, Some(Value::string("active")));
    assert_eq!(field.after, Some(Value::string("failed")));
    assert_eq!(field.certainty, ChangeCertainty::Observed);
}

#[test]
fn should_fold_repeated_changes_to_one_field_into_the_span_of_the_window() {
    let held = ledger(&[
        changed(
            "2026-08-31T12:05:00Z",
            "linux.systemd-dbus",
            subject(SpatialType::Service, "nginx"),
            "active_state",
            Some("active"),
            Some("reloading"),
        ),
        changed(
            "2026-08-31T12:06:00Z",
            "linux.systemd-dbus",
            subject(SpatialType::Service, "nginx"),
            "active_state",
            Some("reloading"),
            Some("failed"),
        ),
    ]);
    let found = changes(&held, &request(), instant("2026-08-31T12:30:00Z"))
        .expect("the changes are computed");
    assert_eq!(found.len(), 1, "one subject and one field is one change");
    let field = &found[0].field_changes[0];
    assert_eq!(
        (field.before.clone(), field.after.clone()),
        (Some(Value::string("active")), Some(Value::string("failed"))),
        "§13.1 compares two instants rather than replaying every step"
    );
}

#[test]
fn should_produce_a_stable_identity_when_the_same_change_is_computed_twice() {
    let held = ledger(&[changed(
        "2026-08-31T12:20:00Z",
        "linux.systemd-dbus",
        subject(SpatialType::Service, "nginx"),
        "active_state",
        Some("active"),
        Some("failed"),
    )]);
    let once = changes(&held, &request(), instant("2026-08-31T12:30:00Z"))
        .expect("the changes are computed");
    let twice = changes(&held, &request(), instant("2026-08-31T12:30:00Z"))
        .expect("the changes are computed");
    assert_eq!(once[0].change_id, twice[0].change_id);
    assert!(once[0].change_id.starts_with('c'));
}

#[test]
fn should_render_the_change_as_its_own_schema_when_a_pipeline_consumes_it() {
    let held = ledger(&[event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:05:00Z",
        "linux.procfs",
        Some(subject(SpatialType::Process, "backup")),
    )]);
    let found = changes(&held, &request(), instant("2026-08-31T12:30:00Z"))
        .expect("the changes are computed");
    let record = found[0].to_record().expect("the change record is built");
    assert_eq!(record.schema_id().to_string(), "ono.temporal-change/1");
    assert_eq!(
        record.get("kind").and_then(|value| value.as_str().ok()),
        Some("added")
    );
    assert_eq!(
        record
            .get("field_changes")
            .and_then(|value| value.as_list().ok().map(<[Value]>::len)),
        Some(0),
        "the whole object is the change when it was added"
    );
}

#[test]
fn should_restrict_the_answer_to_the_selector_when_one_was_given() {
    let held = ledger(&[
        changed(
            "2026-08-31T12:05:00Z",
            "linux.systemd-dbus",
            subject(SpatialType::Service, "nginx"),
            "active_state",
            Some("active"),
            Some("failed"),
        ),
        changed(
            "2026-08-31T12:06:00Z",
            "linux.procfs",
            subject(SpatialType::Process, "cron"),
            "rss",
            Some("10"),
            Some("11"),
        ),
    ]);
    let mut selected = request();
    selected.subjects = vec![id(SpatialType::Service, "nginx")];
    let found = changes(&held, &selected, instant("2026-08-31T12:30:00Z"))
        .expect("the changes are computed");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].subject.label(), "nginx");
}

#[test]
fn should_count_the_net_change_per_type_when_a_session_returns_to_now() {
    let held = ledger(&[
        event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:05:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "a")),
        ),
        event(
            EventKind::ObjectAppeared,
            "2026-08-31T12:06:00Z",
            "linux.procfs",
            Some(subject(SpatialType::Process, "b")),
        ),
        event(
            EventKind::ObjectDisappeared,
            "2026-08-31T12:07:00Z",
            "linux.netlink",
            Some(subject(SpatialType::Connection, "c")),
        ),
        changed(
            "2026-08-31T12:08:00Z",
            "linux.systemd-dbus",
            subject(SpatialType::Service, "nginx"),
            "active_state",
            Some("active"),
            Some("failed"),
        ),
    ]);
    let found = changes(&held, &request(), instant("2026-08-31T12:30:00Z"))
        .expect("the changes are computed");
    let summary = summarise(&found);
    assert_eq!(summary.added, 2);
    assert_eq!(summary.removed, 1);
    assert_eq!(summary.changed, 1);
    assert_eq!(
        summary.net_by_type,
        vec![(Arc::from("Connection"), -1), (Arc::from("Process"), 2)],
        "§18.7 summarises the accumulated change by type"
    );
    assert_eq!(summary.highlights.len(), 1);
    assert_eq!(&*summary.highlights[0].field, "active_state");
    assert_eq!(&*summary.highlights[0].subject, "nginx");
}

#[test]
fn should_let_the_stronger_evidence_win_when_two_sources_answer_one_field() {
    let replayed = vec![FieldAnswer {
        field: Arc::from("active_state"),
        value: Some(Value::string("activating")),
        strength: EvidenceStrength::Derived,
        source: EvidenceSource::recorder(),
        coverage: TemporalCompleteness::Partial,
        evidence: Vec::new(),
        observed_at: Some(instant("2026-08-31T12:20:00Z")),
    }];
    let provider = vec![FieldAnswer {
        field: Arc::from("active_state"),
        value: Some(Value::string("active")),
        strength: EvidenceStrength::Authoritative,
        source: EvidenceSource::parse("linux.systemd-dbus").expect("a source class"),
        coverage: TemporalCompleteness::Complete,
        evidence: Vec::new(),
        observed_at: Some(instant("2026-08-31T12:19:00Z")),
    }];
    let merged = merge_fields(&replayed, &provider);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].value, Some(Value::string("active")));
    assert_eq!(merged[0].strength, EvidenceStrength::Authoritative);
    assert_eq!(
        merged[0].coverage,
        TemporalCompleteness::Complete,
        "coverage survives the merge with the answer it belongs to"
    );
    assert_eq!(merged[0].source.as_str(), "linux.systemd-dbus");
    assert!(merged[0].conflicting, "the two sources disagreed");
    assert_eq!(merged[0].contributors.len(), 2);
}

#[test]
fn should_never_raise_the_strength_when_a_weaker_source_agrees() {
    let replayed = vec![FieldAnswer {
        field: Arc::from("used"),
        value: Some(Value::string("94.1%")),
        strength: EvidenceStrength::Observational,
        source: EvidenceSource::recorder(),
        coverage: TemporalCompleteness::PointSample,
        evidence: Vec::new(),
        observed_at: Some(instant("2026-08-31T12:20:00Z")),
    }];
    let provider = vec![FieldAnswer {
        field: Arc::from("used"),
        value: Some(Value::string("94.1%")),
        strength: EvidenceStrength::Correlated,
        source: EvidenceSource::parse("linux.procfs").expect("a source class"),
        coverage: TemporalCompleteness::Partial,
        evidence: Vec::new(),
        observed_at: Some(instant("2026-08-31T12:21:00Z")),
    }];
    let merged = merge_fields(&replayed, &provider);
    assert_eq!(
        merged[0].strength,
        EvidenceStrength::Correlated,
        "§7.2: the winner keeps its own strength and nothing is upgraded"
    );
    assert!(!merged[0].conflicting);
}

#[test]
fn should_break_a_tie_by_the_closer_observation_rather_than_by_source_order() {
    let older = FieldAnswer {
        field: Arc::from("state"),
        value: Some(Value::string("stale")),
        strength: EvidenceStrength::Asserted,
        source: EvidenceSource::recorder(),
        coverage: TemporalCompleteness::Partial,
        evidence: Vec::new(),
        observed_at: Some(instant("2026-08-31T12:00:00Z")),
    };
    let newer = FieldAnswer {
        field: Arc::from("state"),
        value: Some(Value::string("fresh")),
        strength: EvidenceStrength::Asserted,
        source: EvidenceSource::parse("linux.procfs").expect("a source class"),
        coverage: TemporalCompleteness::Partial,
        evidence: Vec::new(),
        observed_at: Some(instant("2026-08-31T12:20:00Z")),
    };
    let forwards = merge_fields(std::slice::from_ref(&older), std::slice::from_ref(&newer));
    let backwards = merge_fields(&[newer], &[older]);
    assert_eq!(forwards[0].value, Some(Value::string("fresh")));
    assert_eq!(
        backwards[0].value,
        Some(Value::string("fresh")),
        "the ordering in EvidenceStrength decides, and source order never does"
    );
}

#[test]
fn should_keep_a_field_only_one_side_answered_when_the_other_is_silent() {
    let replayed = vec![FieldAnswer {
        field: Arc::from("pid"),
        value: Some(Value::Int(1842)),
        strength: EvidenceStrength::Asserted,
        source: EvidenceSource::recorder(),
        coverage: TemporalCompleteness::Complete,
        evidence: Vec::new(),
        observed_at: Some(instant("2026-08-31T12:20:00Z")),
    }];
    let merged = merge_fields(&replayed, &[]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].value, Some(Value::Int(1842)));
    assert_eq!(merged[0].contributors.len(), 1);
}
