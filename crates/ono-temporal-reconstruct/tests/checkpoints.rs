//! Checkpoints: v0.5 §42.1 (partitioned by host and scope), §42.2 (what a default local
//! checkpoint holds, and what it must not), §42.4 (a checkpoint inherits the coverage quality of
//! its sources) and §9.1's nearest-trusted selection.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    GapReason, HeadlineCoverage, LedgerWrite, SessionLedger, TemporalCompleteness,
};
use ono_temporal_reconstruct::{
    CheckpointPolicy, CheckpointRequest, ReconstructionRequest, Reconstructor, capability,
    nearest_trusted, project_checkpoint,
};

use common::{
    checkpoint, coverage, file_record, instant, object_state, other_scope, point_sample,
    process_record, procfs, provenance, recorder, scope, service_record, systemd,
};

fn service_state(name: &str, at: &str) -> ono_temporal_core::ObjectState {
    object_state(
        &service_record(name, "active", None),
        SpatialType::Service,
        name,
        at,
        systemd(),
    )
}

#[test]
fn should_hold_the_canonical_state_classes_when_a_default_local_checkpoint_is_projected() {
    let request = CheckpointRequest::new(scope(), instant("2026-08-31T12:00:00Z"))
        .with_objects(vec![
            service_state("nginx.service", "2026-08-31T12:00:00Z"),
            object_state(
                &process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running"),
                SpatialType::Process,
                "nginx",
                "2026-08-31T12:00:00Z",
                procfs(),
            ),
        ])
        .with_coverage(vec![point_sample(
            &capability::existence(SpatialType::Process),
            "2026-08-31T12:00:00Z",
            procfs(),
        )])
        .with_provenance(provenance("ono.recorder"));

    let projection = project_checkpoint(&request, &CheckpointPolicy::default_local());

    assert_eq!(projection.checkpoint().objects.len(), 2);
    assert_eq!(projection.checkpoint().scope, scope());
    assert_eq!(
        projection.checkpoint().captured_at,
        instant("2026-08-31T12:00:00Z"),
    );
    assert!(projection.excluded().is_empty());
}

#[test]
fn should_leave_the_unbounded_set_out_when_a_default_checkpoint_is_projected() {
    let request = CheckpointRequest::new(scope(), instant("2026-08-31T12:00:00Z"))
        .with_objects(vec![
            service_state("nginx.service", "2026-08-31T12:00:00Z"),
            object_state(
                &file_record("/etc/nginx/nginx.conf"),
                SpatialType::File,
                "/etc/nginx/nginx.conf",
                "2026-08-31T12:00:00Z",
                recorder(),
            ),
        ])
        .with_provenance(provenance("ono.recorder"));

    let projection = project_checkpoint(&request, &CheckpointPolicy::default_local());

    assert_eq!(
        projection.checkpoint().objects.len(),
        1,
        "§42.2: unbounded sets such as all files under `/` are not checkpointed by default",
    );
    let excluded = projection.excluded();
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded[0].object_type, SpatialType::File);
    assert_eq!(excluded[0].count, 1);
    assert_eq!(excluded[0].reason, GapReason::Unsupported);
    assert!(
        projection
            .checkpoint()
            .coverage
            .iter()
            .any(
                |interval| interval.completeness == TemporalCompleteness::Unavailable
                    && &*interval.capability == capability::existence(SpatialType::File).as_ref()
            ),
        "what was left out is stated as coverage, so a reconstruction reports unknown not absent",
    );
}

#[test]
fn should_leave_a_whole_class_out_when_it_exceeds_the_bound_the_policy_sets() {
    let objects: Vec<_> = (0..5)
        .map(|index| {
            object_state(
                &process_record(1000 + index, "worker", "2026-08-31T11:00:00Z", "running"),
                SpatialType::Process,
                "worker",
                "2026-08-31T12:00:00Z",
                procfs(),
            )
        })
        .collect();
    let request = CheckpointRequest::new(scope(), instant("2026-08-31T12:00:00Z"))
        .with_objects(objects)
        .with_provenance(provenance("ono.recorder"));

    let projection = project_checkpoint(
        &request,
        &CheckpointPolicy::default_local().with_max_per_class(3),
    );

    assert!(
        projection.checkpoint().objects.is_empty(),
        "a truncated set implies a completeness it does not have, so the class goes entirely",
    );
    assert_eq!(projection.excluded()[0].count, 5);
}

#[test]
fn should_inherit_the_coverage_of_its_sources_when_a_checkpoint_is_reconstructed_from() {
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            vec![service_state("nginx.service", "2026-08-31T12:00:00Z")],
            Vec::new(),
            vec![coverage(
                &capability::existence(SpatialType::Service),
                "2026-08-31T12:00:00Z",
                "2026-08-31T12:00:00Z",
                TemporalCompleteness::Partial,
                systemd(),
            )],
        ))
        .expect("the checkpoint is written");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");

    assert_eq!(
        world.coverage().headline(),
        HeadlineCoverage::Partial,
        "§42.4: a checkpoint is not globally authoritative",
    );
    assert!(!world.can_prove_absence(&capability::existence(SpatialType::Service)));
}

#[test]
fn should_pick_the_nearest_checkpoint_at_or_before_the_requested_time() {
    let ledger = SessionLedger::new();
    for at in [
        "2026-08-31T11:00:00Z",
        "2026-08-31T12:00:00Z",
        "2026-08-31T13:00:00Z",
    ] {
        ledger
            .write_checkpoint(&checkpoint(
                scope(),
                at,
                vec![service_state("nginx.service", at)],
                Vec::new(),
                vec![point_sample(
                    &capability::existence(SpatialType::Service),
                    at,
                    systemd(),
                )],
            ))
            .expect("the checkpoint is written");
    }

    let chosen = nearest_trusted(&ledger, &scope(), instant("2026-08-31T12:30:00Z"))
        .expect("the ledger answers")
        .expect("a checkpoint at or before the requested time");
    assert_eq!(chosen.captured_at, instant("2026-08-31T12:00:00Z"));
}

#[test]
fn should_step_back_to_the_next_checkpoint_when_the_nearest_one_could_see_nothing() {
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T11:00:00Z",
            vec![service_state("nginx.service", "2026-08-31T11:00:00Z")],
            Vec::new(),
            vec![point_sample(
                &capability::existence(SpatialType::Service),
                "2026-08-31T11:00:00Z",
                systemd(),
            )],
        ))
        .expect("the checkpoint is written");
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            Vec::new(),
            Vec::new(),
            vec![coverage(
                &capability::existence(SpatialType::Service),
                "2026-08-31T12:00:00Z",
                "2026-08-31T12:00:00Z",
                TemporalCompleteness::PermissionDenied,
                systemd(),
            )],
        ))
        .expect("the denied checkpoint is written");

    let chosen = nearest_trusted(&ledger, &scope(), instant("2026-08-31T12:30:00Z"))
        .expect("the ledger answers")
        .expect("a trusted checkpoint further back");
    assert_eq!(chosen.captured_at, instant("2026-08-31T11:00:00Z"));
}

#[test]
fn should_ignore_another_host_when_a_checkpoint_is_selected_for_this_scope() {
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            other_scope(),
            "2026-08-31T12:00:00Z",
            Vec::new(),
            Vec::new(),
            vec![point_sample(
                &capability::existence(SpatialType::Service),
                "2026-08-31T12:00:00Z",
                systemd(),
            )],
        ))
        .expect("the checkpoint is written");

    assert!(
        nearest_trusted(&ledger, &scope(), instant("2026-08-31T12:30:00Z"))
            .expect("the ledger answers")
            .is_none(),
        "§42.1: reconstructing one host does not deserialize a federated environment",
    );
}

#[test]
fn should_reconstruct_from_events_alone_when_there_is_no_checkpoint() {
    let record = process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running");
    let id = common::identity_of(&record, SpatialType::Process, "2026-08-31T11:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .append(
            &[common::event(
                ono_temporal_core::EventKind::ObjectAppeared,
                common::times("2026-08-31T11:00:00Z"),
                common::resolved(&id, SpatialType::Process, "nginx"),
            )],
            &[],
        )
        .expect("the event is appended");
    ledger
        .record_coverage(&[coverage(
            &capability::existence(SpatialType::Process),
            "2026-08-31T10:00:00Z",
            "2026-08-31T13:00:00Z",
            TemporalCompleteness::Complete,
            recorder(),
        )])
        .expect("the coverage is recorded");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");

    assert!(world.checkpoint().is_none());
    assert!(
        world.object(&id).is_some(),
        "§9.1: events alone, where coverage supports it"
    );
}
