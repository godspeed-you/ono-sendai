//! The reconstruction algorithm of v0.5 §9.1 and the interpolation refusal of §9.2.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    EventKind, EvidenceStrength, LedgerWrite, SessionLedger, TemporalCompleteness,
    value::temporal_metadata,
};
use ono_temporal_reconstruct::{
    FieldKnowledge, Presence, ReconstructionRequest, Reconstructor, capability,
};
use ono_value::Value;

use common::{
    changed, checkpoint, coverage, event, instant, object_state, point_sample, process_record,
    recorder, resolved, scope, service_record, systemd, times,
};

#[test]
fn should_report_the_state_the_events_left_when_a_checkpoint_precedes_the_requested_time() {
    let at_capture = service_record("nginx.service", "active", Some(1842));
    let id = common::identity_of(&at_capture, SpatialType::Service, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            vec![object_state(
                &at_capture,
                SpatialType::Service,
                "nginx.service",
                "2026-08-31T12:00:00Z",
                systemd(),
            )],
            Vec::new(),
            vec![
                coverage(
                    &capability::field(SpatialType::Service, "state"),
                    "2026-08-31T12:00:00Z",
                    "2026-08-31T13:00:00Z",
                    TemporalCompleteness::Complete,
                    systemd(),
                ),
                coverage(
                    &capability::existence(SpatialType::Service),
                    "2026-08-31T12:00:00Z",
                    "2026-08-31T13:00:00Z",
                    TemporalCompleteness::Complete,
                    systemd(),
                ),
            ],
        ))
        .expect("the checkpoint is written");
    ledger
        .append(
            &[changed(
                "2026-08-31T12:04:00Z",
                resolved(&id, SpatialType::Service, "nginx.service"),
                "state",
                Value::string("active"),
                Value::string("failed"),
            )],
            &[],
        )
        .expect("the event is appended");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:07:00Z"),
        ))
        .expect("the reconstruction answers");

    let object = world.object(&id).expect("the service is reconstructed");
    assert_eq!(object.presence(), Presence::Present);
    assert_eq!(
        object.field("state").and_then(|field| field.value()),
        Some(&Value::string("failed")),
        "§9.1: the checkpoint state plus the ordered events through T",
    );
    assert!(world.is_reconstructed());
}

#[test]
fn should_report_the_checkpoint_state_when_no_event_changed_it_before_the_requested_time() {
    let at_capture = service_record("nginx.service", "active", Some(1842));
    let id = common::identity_of(&at_capture, SpatialType::Service, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            vec![object_state(
                &at_capture,
                SpatialType::Service,
                "nginx.service",
                "2026-08-31T12:00:00Z",
                systemd(),
            )],
            Vec::new(),
            vec![coverage(
                &capability::field(SpatialType::Service, "state"),
                "2026-08-31T12:00:00Z",
                "2026-08-31T13:00:00Z",
                TemporalCompleteness::Complete,
                systemd(),
            )],
        ))
        .expect("the checkpoint is written");
    ledger
        .append(
            &[changed(
                "2026-08-31T12:09:00Z",
                resolved(&id, SpatialType::Service, "nginx.service"),
                "state",
                Value::string("active"),
                Value::string("failed"),
            )],
            &[],
        )
        .expect("the event is appended");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:05:00Z"),
        ))
        .expect("the reconstruction answers");

    assert_eq!(
        world
            .object(&id)
            .and_then(|object| object.field("state"))
            .and_then(|field| field.value()),
        Some(&Value::string("active")),
        "an event after T never reaches back before it",
    );
}

#[test]
fn should_report_unknown_in_the_interval_when_nothing_proves_what_happened_between_observations() {
    let running = service_record("nginx.service", "active", Some(1842));
    let id = common::identity_of(&running, SpatialType::Service, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .append(
            &[
                changed(
                    "2026-08-31T12:00:00Z",
                    resolved(&id, SpatialType::Service, "nginx.service"),
                    "state",
                    Value::Null,
                    Value::string("running"),
                ),
                changed(
                    "2026-08-31T12:10:00Z",
                    resolved(&id, SpatialType::Service, "nginx.service"),
                    "state",
                    Value::string("running"),
                    Value::string("failed"),
                ),
            ],
            &[],
        )
        .expect("the events are appended");
    ledger
        .record_coverage(&[
            point_sample(
                &capability::field(SpatialType::Service, "state"),
                "2026-08-31T12:00:00Z",
                systemd(),
            ),
            point_sample(
                &capability::field(SpatialType::Service, "state"),
                "2026-08-31T12:10:00Z",
                systemd(),
            ),
        ])
        .expect("the coverage is recorded");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:05:00Z"),
        ))
        .expect("the reconstruction answers");

    let field = world
        .object(&id)
        .and_then(|object| object.field("state"))
        .expect("the field is reconstructed");
    match field.knowledge() {
        FieldKnowledge::UnknownInInterval { last, next } => {
            assert_eq!(
                last.as_ref().map(|seen| seen.at),
                Some(instant("2026-08-31T12:00:00Z")),
            );
            assert_eq!(
                last.as_ref().map(|seen| seen.value.clone()),
                Some(Value::string("running")),
            );
            assert_eq!(
                next.as_ref().map(|seen| seen.at),
                Some(instant("2026-08-31T12:10:00Z")),
            );
            assert_eq!(
                next.as_ref().map(|seen| seen.value.clone()),
                Some(Value::string("failed")),
            );
        }
        other => panic!("§9.2 forbids claiming the state at 12:05: {other:?}"),
    }
    assert_eq!(
        field.to_value(),
        Value::Null,
        "an unknown field reads back as unknown, never as a fabricated value",
    );
}

#[test]
fn should_narrow_the_uncertainty_when_an_exhaustive_stream_proves_no_transition_happened() {
    let id = common::identity_of(
        &service_record("nginx.service", "active", Some(1842)),
        SpatialType::Service,
        "2026-08-31T12:00:00Z",
    );
    let ledger = SessionLedger::new();
    ledger
        .append(
            &[changed(
                "2026-08-31T12:00:00Z",
                resolved(&id, SpatialType::Service, "nginx.service"),
                "state",
                Value::Null,
                Value::string("running"),
            )],
            &[],
        )
        .expect("the event is appended");
    ledger
        .record_coverage(&[coverage(
            &capability::field(SpatialType::Service, "state"),
            "2026-08-31T12:00:00Z",
            "2026-08-31T12:08:00Z",
            TemporalCompleteness::Complete,
            systemd(),
        )])
        .expect("the coverage is recorded");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:05:00Z"),
        ))
        .expect("the reconstruction answers");

    let field = world
        .object(&id)
        .and_then(|object| object.field("state"))
        .expect("the field is reconstructed");
    assert_eq!(field.value(), Some(&Value::string("running")));
    assert_eq!(
        field.valid_from(),
        Some(instant("2026-08-31T12:00:00Z")),
        "§9.3: the interval starts where the observation is, never at a guessed midpoint",
    );
    assert_eq!(
        field.valid_until(),
        Some(instant("2026-08-31T12:08:00Z")),
        "§9.3: and ends where the source's own coverage ends",
    );
}

#[test]
fn should_return_the_same_answer_whenever_it_runs_because_it_reads_no_clock() {
    let record = process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running");
    let id = common::identity_of(&record, SpatialType::Process, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .append(
            &[event(
                EventKind::ObjectObserved,
                times("2026-08-31T12:00:00Z"),
                resolved(&id, SpatialType::Process, "nginx"),
            )],
            &[],
        )
        .expect("the event is appended");

    let request = ReconstructionRequest::new(scope(), instant("2026-08-31T12:00:00Z"));
    let first = Reconstructor::new(&ledger)
        .reconstruct(&request)
        .expect("the reconstruction answers");
    let second = Reconstructor::new(&ledger)
        .reconstruct(&request)
        .expect("the reconstruction answers again");
    assert_eq!(first, second);
    assert_eq!(first.as_of(), instant("2026-08-31T12:00:00Z"));
}

#[test]
fn should_carry_the_temporal_metadata_of_the_specification_when_an_object_is_reconstructed() {
    let record = process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running");
    let id = common::identity_of(&record, SpatialType::Process, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            vec![object_state(
                &record,
                SpatialType::Process,
                "nginx",
                "2026-08-31T12:00:00Z",
                recorder(),
            )],
            Vec::new(),
            vec![point_sample(
                &capability::existence(SpatialType::Process),
                "2026-08-31T12:00:00Z",
                recorder(),
            )],
        ))
        .expect("the checkpoint is written");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");
    let object = world.object(&id).expect("the process is reconstructed");

    let expected = temporal_metadata(
        world.as_of(),
        object.coverage(),
        true,
        &object.sources().cloned().collect::<Vec<_>>(),
        object.gaps(),
    )
    .expect("the metadata builds");
    assert_eq!(object.temporal_metadata().expect("the metadata"), expected);
}

#[test]
fn should_report_the_most_recent_reading_when_an_earlier_one_carries_stronger_evidence() {
    // §16.5's own scenario with §7.2's own strengths: systemd D-Bus is authoritative for a unit's
    // state, a journald-sourced unit result is asserted. If strength decided which reading is
    // current, the authoritative 12:00 `active` would beat the asserted 12:09 `failed` and the
    // reconstruction would report the service running nine minutes after a source watched it
    // fail — a state no evidence supports, which §9.2 forbids.
    //
    // Strength says how far to trust a reading and travels into the field's provenance. Recency
    // says which reading is current. They are different questions.
    let record = service_record("nginx.service", "active", Some(1842));
    let id = common::identity_of(&record, SpatialType::Service, "2026-08-31T12:00:00Z");
    let subject = resolved(&id, SpatialType::Service, "nginx.service");

    let (earlier, earlier_evidence) = common::changed_with_strength(
        "2026-08-31T12:00:00Z",
        subject.clone(),
        "active_state",
        Value::string("activating"),
        Value::string("active"),
        EvidenceStrength::Authoritative,
        "linux.systemd-dbus",
    );
    let (later, later_evidence) = common::changed_with_strength(
        "2026-08-31T12:09:00Z",
        subject.clone(),
        "active_state",
        Value::string("active"),
        Value::string("failed"),
        EvidenceStrength::Asserted,
        "linux.journald",
    );

    let ledger = SessionLedger::new();
    ledger
        .append(&[earlier, later], &[earlier_evidence, later_evidence])
        .expect("the append succeeds");
    // §9.2 lets a reading be carried forward only where a source declared complete coverage over
    // the interval; without it the honest answer is `unknown`, which is a different test. Here
    // the question is which of two readings is current, so the window is covered.
    ledger
        .record_coverage(&[coverage(
            &capability::field(SpatialType::Service, "active_state"),
            "2026-08-31T11:00:00Z",
            "2026-08-31T13:00:00Z",
            TemporalCompleteness::Complete,
            systemd(),
        )])
        .expect("coverage records");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            common::scope(),
            instant("2026-08-31T12:10:00Z"),
        ))
        .expect("the reconstruction answers");

    let service = world.object(&id).expect("the service is reconstructed");
    let state = service
        .field("active_state")
        .expect("the field was observed");
    assert_eq!(
        state.knowledge().value(),
        Some(&Value::string("failed")),
        "§9.2: the most recent observation is what the field is, whatever strength an earlier \
         reading carried"
    );
}
