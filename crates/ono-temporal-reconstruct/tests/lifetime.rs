//! Lifetime identity replay: v0.5 §5.2 (a PID is not enough across time), §5.3 (one service
//! across many process lifetimes), §5.4 (a tombstone stays in its own lifetime) and §5.5 (an
//! unresolved subject is never attached to a guessed object).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::sync::Arc;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    EventKind, EventSeed, EvidenceSource, LedgerWrite, SessionLedger, SpatialRef,
    TemporalCompleteness,
};
use ono_temporal_reconstruct::{Presence, ReconstructionRequest, Reconstructor, capability};

use common::{
    coverage, event, instant, process_record, procfs, provenance, recorder, resolved, scope, times,
};

/// Two processes that reused pid 1842, told apart by the start time their identity carries.
fn two_lifetimes() -> (
    SessionLedger,
    ono_spatial_core::SpatialId,
    ono_spatial_core::SpatialId,
) {
    let first = process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running");
    let second = process_record(1842, "curl", "2026-08-31T12:30:00Z", "running");
    let first_id = common::identity_of(&first, SpatialType::Process, "2026-08-31T11:00:00Z");
    let second_id = common::identity_of(&second, SpatialType::Process, "2026-08-31T12:30:00Z");
    assert_ne!(
        first_id, second_id,
        "§5.2: a pid alone cannot be an identity across time",
    );

    let ledger = SessionLedger::new();
    ledger
        .append(
            &[
                event(
                    EventKind::ObjectAppeared,
                    times("2026-08-31T11:00:00Z"),
                    resolved(&first_id, SpatialType::Process, "nginx"),
                ),
                event(
                    EventKind::ObjectDisappeared,
                    times("2026-08-31T11:30:00Z"),
                    resolved(&first_id, SpatialType::Process, "nginx"),
                ),
                event(
                    EventKind::ObjectAppeared,
                    times("2026-08-31T12:30:00Z"),
                    resolved(&second_id, SpatialType::Process, "curl"),
                ),
            ],
            &[],
        )
        .expect("the events are appended");
    ledger
        .record_coverage(&[coverage(
            &capability::existence(SpatialType::Process),
            "2026-08-31T10:00:00Z",
            "2026-08-31T13:00:00Z",
            TemporalCompleteness::Complete,
            recorder(),
        )])
        .expect("the coverage is recorded");
    (ledger, first_id, second_id)
}

#[test]
fn should_resolve_the_lifetime_that_existed_when_a_later_process_reused_the_pid() {
    let (ledger, first_id, second_id) = two_lifetimes();

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T11:15:00Z"),
        ))
        .expect("the reconstruction answers");

    assert_eq!(
        world
            .object(&first_id)
            .map(ono_temporal_reconstruct::ReconstructedObject::presence),
        Some(Presence::Present),
    );
    assert!(
        world.object(&second_id).is_none(),
        "§5.2: a process that starts at 12:30 is not the process running at 11:15",
    );
    assert_eq!(
        world.presence_of(&second_id, SpatialType::Process),
        Presence::Absent,
        "complete existence coverage over the window proves the later lifetime was not there",
    );
}

#[test]
fn should_keep_a_dead_process_dead_when_the_requested_time_is_after_its_lifetime() {
    let (ledger, first_id, _) = two_lifetimes();

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:45:00Z"),
        ))
        .expect("the reconstruction answers");

    assert_eq!(
        world.presence_of(&first_id, SpatialType::Process),
        Presence::Absent,
        "§5.4: `now` never revives a tombstoned place",
    );
}

#[test]
fn should_enter_a_tombstoned_place_when_the_requested_time_is_inside_its_own_lifetime() {
    let (ledger, first_id, _) = two_lifetimes();

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T11:10:00Z"),
        ))
        .expect("the reconstruction answers");

    let object = world
        .object(&first_id)
        .expect("§5.4: the place is enterable inside its own lifetime");
    assert_eq!(object.presence(), Presence::Present);
    assert_eq!(
        object.lifetime_from(),
        Some(instant("2026-08-31T11:00:00Z"))
    );
    assert_eq!(object.lifetime_until(), None);
}

#[test]
fn should_report_the_identity_it_had_when_it_was_live_when_an_object_is_projected() {
    let record = process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running");
    let id = common::identity_of(&record, SpatialType::Process, "2026-08-31T11:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&common::checkpoint(
            scope(),
            "2026-08-31T11:05:00Z",
            vec![common::object_state(
                &record,
                SpatialType::Process,
                "nginx",
                "2026-08-31T11:05:00Z",
                procfs(),
            )],
            Vec::new(),
            vec![common::point_sample(
                &capability::existence(SpatialType::Process),
                "2026-08-31T11:05:00Z",
                procfs(),
            )],
        ))
        .expect("the checkpoint is written");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T11:05:00Z"),
        ))
        .expect("the reconstruction answers");
    let projected = world
        .object(&id)
        .expect("the process is reconstructed")
        .project()
        .expect("the archived record projects");

    assert_eq!(projected.spatial_id(), &id);
    assert_eq!(projected.object_type(), SpatialType::Process);
}

#[test]
fn should_keep_an_unreconciled_subject_unresolved_when_a_source_named_something_it_could_not_map() {
    let ledger = SessionLedger::new();
    let unresolved = EventSeed {
        kind: EventKind::ProviderEvent,
        subtype: Some(Arc::from("linux.journald.line")),
        scope: scope(),
        subject: Some(SpatialRef::Unresolved {
            source: EvidenceSource::parse("linux.journald").expect("a §7.1 source class"),
            described: Arc::from("pid 1842"),
        }),
        related: Vec::new(),
        times: times("2026-08-31T12:00:00Z"),
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: provenance("linux.journald"),
    }
    .seal();
    ledger
        .append(std::slice::from_ref(&unresolved), &[])
        .expect("the event is appended");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:05:00Z"),
        ))
        .expect("the reconstruction answers");

    assert!(
        world.objects().next().is_none(),
        "§5.5: an unresolved subject is never attached to a guessed object",
    );
    let subjects: Vec<_> = world.unresolved().collect();
    assert_eq!(subjects.len(), 1);
    assert_eq!(subjects[0].described(), "pid 1842");
    assert_eq!(
        subjects[0].events(),
        std::slice::from_ref(&unresolved.event_id)
    );
}
