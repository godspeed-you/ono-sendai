//! Bounded ingestion, overflow and aggregation (v0.5 §43.1, §43.2, §43.3, §43.4).

mod common;

use common::{HOST, instant, netlink_profile, options, procfs_profile, scope, settings};
use ono_recorder::{Admission, AggregationRules, Intake, Recorder, RecorderHealth};
use ono_spatial_core::{Confidence, SpatialType};
use ono_temporal_core::{
    ClockDomain, EventKind, EventSeed, EventTimes, EvidenceSource, FieldChange, GapReason,
    SpatialRef, TemporalEvent, relation_payload,
};
use ono_value::{Provenance, SchemaId, Value};

fn event(kind: EventKind, at: &str, fields: &[&str]) -> TemporalEvent {
    let when = instant(at);
    EventSeed {
        kind,
        subtype: None,
        scope: scope(),
        subject: Some(SpatialRef::Unresolved {
            source: EvidenceSource::builtin("linux.procfs")
                .unwrap_or_else(EvidenceSource::recorder),
            described: "pid 1842".into(),
        }),
        related: Vec::new(),
        times: EventTimes::observed(when, when, ClockDomain::new(HOST, Some(common::BOOT))),
        before: None,
        after: None,
        changed_fields: fields
            .iter()
            .map(|name| FieldChange {
                field: (*name).into(),
                before: Some(Value::Int(1)),
                after: Some(Value::Int(2)),
                certainty: ono_temporal_core::ChangeCertainty::Observed,
            })
            .collect(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: (kind == EventKind::RelationAdded)
            .then(|| relation_payload("process.owns_socket", Confidence::Exact)),
        provenance: Provenance::local("linux.procfs", SchemaId::new("ono.temporal-event", 1)),
    }
    .seal()
}

#[test]
fn should_drop_rather_than_grow_when_a_source_exceeds_its_capacity() {
    let (intake, queue) = Intake::bounded(&procfs_profile(), &scope(), 4);

    let mut accepted: usize = 0;
    let mut dropped: u64 = 0;
    for index in 0..64 {
        let at = instant("2026-08-31T12:00:00Z") + jiff::Span::new().seconds(index);
        match intake.offer(
            event(EventKind::ObjectObserved, "2026-08-31T12:00:00Z", &[]),
            at,
        ) {
            Admission::Accepted => accepted += 1,
            Admission::Dropped { .. } => dropped += 1,
        }
    }

    assert_eq!(
        accepted,
        intake.capacity(),
        "§43.1: every ingestion path is bounded"
    );
    assert_eq!(dropped, 64 - intake.capacity() as u64);
    assert_eq!(intake.dropped(), dropped);
    drop(queue);
}

#[test]
fn should_produce_an_explicit_coverage_gap_when_events_were_dropped() {
    let (intake, _queue) = Intake::bounded(&netlink_profile(), &scope(), 2);
    let first = instant("2026-08-31T14:03:12.100Z");
    let last = instant("2026-08-31T14:03:13.411Z");
    for at in [first, first, first, last] {
        let _ = intake.offer(
            event(EventKind::ObjectChanged, "2026-08-31T14:03:12Z", &["state"]),
            at,
        );
    }

    let gap = intake
        .overflow_gap()
        .expect("§43.2: an explicit coverage gap rather than pretended continuity");

    assert_eq!(gap.source, netlink_profile().source);
    assert_eq!(gap.reason, GapReason::SourceDisconnected);
    assert_eq!(gap.detail.as_deref(), Some("dropped events"));
    assert_eq!(gap.from, first);
    assert_eq!(gap.until, last);
    assert_eq!(
        &*gap.capability,
        &*ono_temporal_reconstruct::capability::existence(SpatialType::Interface),
        "the gap is filed under the capability the loss actually costs"
    );
}

#[test]
fn should_carry_the_loss_into_the_ledger_when_the_recorder_notes_an_overflow() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()).with_sources(vec![netlink_profile()]));
    recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("a start");
    let (intake, _queue) = Intake::bounded(&netlink_profile(), &scope(), 1);
    for _ in 0..8 {
        let _ = intake.offer(
            event(EventKind::ObjectChanged, "2026-08-31T14:03:12Z", &["state"]),
            instant("2026-08-31T14:03:12.100Z"),
        );
    }

    recorder
        .note_overflow(&intake, instant("2026-08-31T14:03:13.411Z"))
        .expect("the loss is written down");

    let status = recorder.status(instant("2026-08-31T14:03:14Z"));
    assert_eq!(status.dropped, 7);
    assert_eq!(
        status.health,
        RecorderHealth::Degraded,
        "§43.4: a recorder falling behind is visible"
    );
    let gaps = recorder.gaps().expect("the store answers for its gaps");
    assert!(
        gaps.iter()
            .any(|gap| gap.detail.as_deref() == Some("dropped events")),
        "§43.2's gap reaches the ledger, so a timeline draws it"
    );
}

#[test]
fn should_fold_a_metric_change_and_keep_a_lifecycle_change_when_aggregation_is_declared() {
    let rules = AggregationRules::default();
    let mut aggregator = rules.aggregator();

    let first = aggregator.admit(event(
        EventKind::ObjectChanged,
        "2026-08-31T12:00:00Z",
        &["cpu"],
    ));
    let second = aggregator.admit(event(
        EventKind::ObjectChanged,
        "2026-08-31T12:00:01Z",
        &["cpu"],
    ));
    let appeared = aggregator.admit(event(
        EventKind::ObjectAppeared,
        "2026-08-31T12:00:02Z",
        &[],
    ));
    let relation = aggregator.admit(event(EventKind::RelationAdded, "2026-08-31T12:00:03Z", &[]));
    let state = aggregator.admit(event(
        EventKind::ObjectChanged,
        "2026-08-31T12:00:04Z",
        &["state"],
    ));

    assert!(
        first.is_some(),
        "the first sample of a folded field is kept"
    );
    assert!(
        second.is_none(),
        "§43.3: high-frequency changes that are not semantically relevant may be aggregated"
    );
    assert!(
        appeared.is_some(),
        "§43.3: aggregation never hides an object lifecycle change"
    );
    assert!(relation.is_some(), "§43.3: nor a relation lifecycle change");
    assert!(
        state.is_some(),
        "a field nothing declared aggregatable is never folded"
    );
    assert!(
        rules.declared().any(|field| field == "cpu"),
        "§43.3: aggregation rules MUST be declared"
    );
}

#[test]
fn should_reopen_coverage_when_the_queue_has_drained() {
    let (intake, mut queue) = Intake::bounded(&netlink_profile(), &scope(), 1);
    for _ in 0..4 {
        let _ = intake.offer(
            event(EventKind::ObjectChanged, "2026-08-31T12:00:00Z", &["state"]),
            instant("2026-08-31T12:00:00Z"),
        );
    }
    assert!(intake.overflow_gap().is_some());

    let _drained = queue.try_recv();
    intake.clear_overflow();

    assert!(
        intake.overflow_gap().is_none(),
        "the gap covers the interval that was lost, not every interval after it"
    );
    assert_eq!(
        intake.dropped(),
        3,
        "the count of what was lost survives the gap"
    );
}
