//! Historical relations: v0.5 §9.5 (a relation exists at `T` only where reconstruction supports
//! it, and unknown is distinguishable from absent), §14.3 (a current-only exit does not leak
//! into a historical neighbourhood) and §5.3 (one service across many process lifetimes).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{EventKind, LedgerWrite, SessionLedger, TemporalCompleteness};
use ono_temporal_reconstruct::{Presence, ReconstructionRequest, Reconstructor, capability};

use common::{
    coverage, instant, process_record, recorder, relation_event, resolved, scope, service_record,
};

const RUNS: &str = "service.runs_process";

#[test]
fn should_leave_a_relation_out_of_the_map_when_it_was_added_after_the_requested_time() {
    let service = service_record("nginx.service", "active", Some(1842));
    let process = process_record(1842, "nginx", "2026-08-31T12:30:00Z", "running");
    let service_id = common::identity_of(&service, SpatialType::Service, "2026-08-31T12:00:00Z");
    let process_id = common::identity_of(&process, SpatialType::Process, "2026-08-31T12:30:00Z");

    let ledger = SessionLedger::new();
    ledger
        .append(
            &[relation_event(
                EventKind::RelationAdded,
                "2026-08-31T12:30:00Z",
                resolved(&service_id, SpatialType::Service, "nginx.service"),
                resolved(&process_id, SpatialType::Process, "nginx"),
                RUNS,
            )],
            &[],
        )
        .expect("the event is appended");
    ledger
        .record_coverage(&[coverage(
            &capability::relation(RUNS),
            "2026-08-31T12:00:00Z",
            "2026-08-31T13:00:00Z",
            TemporalCompleteness::Complete,
            recorder(),
        )])
        .expect("the coverage is recorded");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:10:00Z"),
        ))
        .expect("the reconstruction answers");

    assert_eq!(
        world.relation(&service_id, &process_id, RUNS),
        Presence::Absent,
        "§14.3: a current-only exit does not leak into a historical neighbourhood",
    );
    assert!(
        world.relations().next().is_none(),
        "nothing supports the edge at 12:10, so no edge is drawn",
    );
}

#[test]
fn should_distinguish_an_unknown_relation_from_an_absent_one_when_coverage_cannot_decide() {
    let service = service_record("nginx.service", "active", Some(1842));
    let process = process_record(1842, "nginx", "2026-08-31T12:30:00Z", "running");
    let service_id = common::identity_of(&service, SpatialType::Service, "2026-08-31T12:00:00Z");
    let process_id = common::identity_of(&process, SpatialType::Process, "2026-08-31T12:30:00Z");

    let ledger = SessionLedger::new();
    ledger
        .append(
            &[relation_event(
                EventKind::RelationAdded,
                "2026-08-31T12:30:00Z",
                resolved(&service_id, SpatialType::Service, "nginx.service"),
                resolved(&process_id, SpatialType::Process, "nginx"),
                RUNS,
            )],
            &[],
        )
        .expect("the event is appended");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:10:00Z"),
        ))
        .expect("the reconstruction answers");

    assert_eq!(
        world.relation(&service_id, &process_id, RUNS),
        Presence::Unknown,
        "§9.5: with no coverage, `not observed` is not `not there`",
    );
    assert_ne!(Presence::Unknown, Presence::Absent);
}

#[test]
fn should_show_the_edge_when_it_was_added_before_the_requested_time_and_never_removed() {
    let service = service_record("nginx.service", "active", Some(1842));
    let process = process_record(1842, "nginx", "2026-08-31T12:00:00Z", "running");
    let service_id = common::identity_of(&service, SpatialType::Service, "2026-08-31T12:00:00Z");
    let process_id = common::identity_of(&process, SpatialType::Process, "2026-08-31T12:00:00Z");

    let ledger = SessionLedger::new();
    ledger
        .append(
            &[relation_event(
                EventKind::RelationAdded,
                "2026-08-31T12:00:00Z",
                resolved(&service_id, SpatialType::Service, "nginx.service"),
                resolved(&process_id, SpatialType::Process, "nginx"),
                RUNS,
            )],
            &[],
        )
        .expect("the event is appended");
    ledger
        .record_coverage(&[coverage(
            &capability::relation(RUNS),
            "2026-08-31T11:00:00Z",
            "2026-08-31T13:00:00Z",
            TemporalCompleteness::Complete,
            recorder(),
        )])
        .expect("the coverage is recorded");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:20:00Z"),
        ))
        .expect("the reconstruction answers");

    assert_eq!(
        world.relation(&service_id, &process_id, RUNS),
        Presence::Present,
    );
    let edge = world.relations().next().expect("the edge is reconstructed");
    assert_eq!(edge.valid_from(), Some(instant("2026-08-31T12:00:00Z")));
    assert_eq!(edge.valid_until(), None);
}

#[test]
fn should_show_one_service_joined_to_different_process_lifetimes_at_different_times() {
    let service = service_record("nginx.service", "active", None);
    let first = process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running");
    let second = process_record(2198, "nginx", "2026-08-31T12:00:00Z", "running");
    let service_id = common::identity_of(&service, SpatialType::Service, "2026-08-31T11:00:00Z");
    let first_id = common::identity_of(&first, SpatialType::Process, "2026-08-31T11:00:00Z");
    let second_id = common::identity_of(&second, SpatialType::Process, "2026-08-31T12:00:00Z");

    let ledger = SessionLedger::new();
    ledger
        .append(
            &[
                relation_event(
                    EventKind::RelationAdded,
                    "2026-08-31T11:00:00Z",
                    resolved(&service_id, SpatialType::Service, "nginx.service"),
                    resolved(&first_id, SpatialType::Process, "nginx"),
                    RUNS,
                ),
                relation_event(
                    EventKind::RelationRemoved,
                    "2026-08-31T11:50:00Z",
                    resolved(&service_id, SpatialType::Service, "nginx.service"),
                    resolved(&first_id, SpatialType::Process, "nginx"),
                    RUNS,
                ),
                relation_event(
                    EventKind::RelationAdded,
                    "2026-08-31T12:00:00Z",
                    resolved(&service_id, SpatialType::Service, "nginx.service"),
                    resolved(&second_id, SpatialType::Process, "nginx"),
                    RUNS,
                ),
            ],
            &[],
        )
        .expect("the events are appended");
    ledger
        .record_coverage(&[coverage(
            &capability::relation(RUNS),
            "2026-08-31T10:00:00Z",
            "2026-08-31T13:00:00Z",
            TemporalCompleteness::Complete,
            recorder(),
        )])
        .expect("the coverage is recorded");

    let early = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T11:30:00Z"),
        ))
        .expect("the reconstruction answers");
    let late = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:30:00Z"),
        ))
        .expect("the reconstruction answers");

    assert_eq!(
        early.relation(&service_id, &first_id, RUNS),
        Presence::Present,
    );
    assert_eq!(
        early.relation(&service_id, &second_id, RUNS),
        Presence::Absent,
    );
    assert_eq!(
        late.relation(&service_id, &first_id, RUNS),
        Presence::Absent,
        "§5.3: the same conceptual service, a different process lifetime",
    );
    assert_eq!(
        late.relation(&service_id, &second_id, RUNS),
        Presence::Present,
    );
}
