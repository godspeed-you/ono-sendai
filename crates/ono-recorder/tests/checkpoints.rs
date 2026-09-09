//! The checkpoint cadence, and the prompt it must not hold (v0.5 §31.9, §42, §8.1).

mod common;

use common::{HOST, instant, options, procfs_profile, scope, settings};
use ono_recorder::CheckpointCapture;
use ono_recorder::{CheckpointReason, CheckpointSchedule, Recorder, is_significant};
use ono_spatial_core::{PermissionState, SpatialIdentity, SpatialType};
use ono_temporal_core::{
    ClockDomain, EventKind, EventSeed, EventTimes, EvidenceSource, LedgerRead as _, ObjectState,
    SpatialRef,
};
use ono_temporal_reconstruct::CheckpointPolicy;
use ono_value::{Duration, Provenance, SchemaId};

fn schedule() -> CheckpointSchedule {
    CheckpointSchedule::every(Duration::from_nanoseconds(5 * 60 * 1_000_000_000))
}

fn event(kind: EventKind, at: &str) -> ono_temporal_core::TemporalEvent {
    let when = instant(at);
    EventSeed {
        kind,
        subtype: None,
        scope: scope(),
        subject: Some(SpatialRef::Unresolved {
            source: EvidenceSource::recorder(),
            described: "pid 1842".into(),
        }),
        related: Vec::new(),
        times: EventTimes::observed(when, when, ClockDomain::new(HOST, Some(common::BOOT))),
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: Provenance::local("ono.recorder", SchemaId::new("ono.temporal-event", 1)),
    }
    .seal()
}

#[test]
fn should_come_due_after_five_minutes_when_the_schedule_is_asked() {
    let mut schedule = schedule();
    let start = instant("2026-08-31T12:00:00Z");
    schedule.taken(start);

    assert_eq!(schedule.due(instant("2026-08-31T12:04:59Z")), None);
    assert_eq!(
        schedule.due(instant("2026-08-31T12:05:00Z")),
        Some(CheckpointReason::Scheduled),
        "§31.9: the default checkpoint interval is 5 minutes"
    );
}

#[test]
fn should_come_due_at_once_when_nothing_has_been_checkpointed_yet() {
    let schedule = schedule();

    assert_eq!(
        schedule.due(instant("2026-08-31T12:00:00Z")),
        Some(CheckpointReason::Scheduled),
        "a recorder with no checkpoint has nothing to reconstruct from"
    );
}

#[test]
fn should_take_an_extra_checkpoint_when_a_high_significance_change_arrives() {
    let mut schedule = schedule();
    schedule.taken(instant("2026-08-31T12:00:00Z"));

    let reason = schedule.on_change(
        &event(EventKind::ObjectDisappeared, "2026-08-31T12:00:30Z"),
        instant("2026-08-31T12:00:30Z"),
    );

    assert_eq!(
        reason,
        Some(CheckpointReason::Significant),
        "§31.9: providers MAY trigger additional checkpoints around high-significance changes"
    );
    assert!(is_significant(&event(
        EventKind::RelationRemoved,
        "2026-08-31T12:00:30Z"
    )));
    assert!(!is_significant(&event(
        EventKind::ObjectObserved,
        "2026-08-31T12:00:30Z"
    )));
}

#[test]
fn should_bound_the_extra_checkpoints_when_significant_changes_storm() {
    let mut schedule = schedule();
    schedule.taken(instant("2026-08-31T12:00:00Z"));

    let mut extra = 0;
    for index in 0..200_i64 {
        let at = instant("2026-08-31T12:00:00Z") + jiff::Span::new().milliseconds(index);
        if schedule
            .on_change(
                &event(EventKind::ObjectAppeared, "2026-08-31T12:00:00Z"),
                at,
            )
            .is_some()
        {
            extra += 1;
            schedule.taken(at);
        }
    }

    assert_eq!(
        extra,
        CheckpointSchedule::MAX_SIGNIFICANT_PER_INTERVAL,
        "§31.9: additional checkpoints only where doing so is cheap and bounded"
    );
}

#[test]
fn should_record_existence_coverage_when_a_checkpoint_is_written() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    let at = instant("2026-08-31T12:00:00Z");
    recorder.start(&settings(), at).expect("a start");

    let outcome = recorder
        .checkpoint(
            CheckpointCapture::new(scope(), at).with_objects(vec![object_state(at)]),
            &CheckpointPolicy::default_local(),
        )
        .expect("a checkpoint is written");

    let existence = ono_temporal_reconstruct::capability::existence(SpatialType::Process);
    assert!(
        outcome
            .coverage
            .iter()
            .any(|interval| interval.capability == existence),
        "object presence is gated on `<type>.existence` coverage; a checkpoint without it \
         reconstructs to `Presence::Unknown`"
    );
    let stored = recorder
        .ledger()
        .checkpoint_before(&scope(), instant("2026-08-31T12:01:00Z"))
        .expect("the store answers")
        .expect("the checkpoint is there");
    assert_eq!(stored.objects.len(), 1);
    assert!(
        stored
            .coverage
            .iter()
            .any(|interval| interval.capability == existence),
        "the coverage travels with the checkpoint into the ledger"
    );
}

#[test]
fn should_answer_the_status_while_a_checkpoint_is_being_taken() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    let at = instant("2026-08-31T12:00:00Z");
    recorder.start(&settings(), at).expect("a start");

    let objects: Vec<ObjectState> = (0..2_000).map(|_| object_state(at)).collect();
    let pending = recorder.begin_checkpoint(
        CheckpointCapture::new(scope(), at).with_objects(objects),
        CheckpointPolicy::default_local(),
    );

    let status = recorder.status(instant("2026-08-31T12:00:01Z"));
    assert!(
        status.running,
        "§31.9: checkpoints MUST not block the interactive prompt"
    );
    assert_eq!(status.checkpoints_in_flight, 1);

    let outcome = pending.join().expect("the checkpoint finishes");
    assert!(!outcome.coverage.is_empty());
    assert_eq!(
        recorder
            .status(instant("2026-08-31T12:00:02Z"))
            .checkpoints_in_flight,
        0
    );
}

#[test]
fn should_declare_the_polling_interval_as_the_sampling_interval_when_a_source_is_polled() {
    let profile = procfs_profile();
    let coverage = profile.coverage(
        &scope(),
        instant("2026-08-31T12:00:00Z"),
        instant("2026-08-31T12:05:00Z"),
        PermissionState::Available,
    );

    assert_eq!(
        coverage.sampling_interval,
        Some(Duration::from_nanoseconds(5_000_000_000)),
        "§22.1: coverage MUST reflect polling limitations"
    );
    assert_eq!(
        coverage.completeness,
        ono_temporal_core::TemporalCompleteness::Partial,
        "§21.5: a polled source does not get to claim exhaustive events"
    );
}

fn object_state(at: jiff::Timestamp) -> ObjectState {
    let identity = SpatialIdentity::lifetime(
        SpatialType::Process,
        [("pid", "1842"), ("started", "1756638000")],
    );
    ObjectState {
        id: identity.spatial_id(),
        object_type: SpatialType::Process,
        label: "nginx".into(),
        record: common::process_record(1842, "nginx", &["nginx"]),
        observed_at: at,
        source: EvidenceSource::builtin("linux.procfs").unwrap_or_else(EvidenceSource::recorder),
    }
}
