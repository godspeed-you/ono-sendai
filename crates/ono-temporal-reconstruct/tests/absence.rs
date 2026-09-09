//! Negative claims: v0.5 §7.4 (absence needs coverage capable of proving it), §8.4 (a point
//! sample explains no intermediate change) and §14.5 (historical filesystem structure is a
//! refusal unless one of four kinds of evidence supports it).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    EventKind, GapReason, LedgerWrite, SessionLedger, TemporalCapabilities, TemporalCompleteness,
};
use ono_temporal_reconstruct::{
    Presence, ReconstructionRequest, Reconstructor, SourceMatrix, StructureSupport, capability,
};

use common::{
    checkpoint, coverage, event, file_record, instant, object_state, point_sample, process_record,
    procfs, recorder, resolved, scope, times,
};

#[test]
fn should_refuse_to_prove_absence_when_only_a_point_sample_covers_the_window() {
    let ledger = SessionLedger::new();
    ledger
        .record_coverage(&[point_sample(
            &capability::existence(SpatialType::Process),
            "2026-08-31T12:00:00Z",
            procfs(),
        )])
        .expect("the coverage is recorded");

    let world = Reconstructor::new(&ledger)
        .reconstruct(
            &ReconstructionRequest::new(scope(), instant("2026-08-31T12:10:00Z"))
                .since(instant("2026-08-31T11:50:00Z")),
        )
        .expect("the reconstruction answers");

    assert!(
        !world.can_prove_absence(&capability::existence(SpatialType::Process)),
        "§7.4: a snapshot at 12:00 cannot prove a process did not exist from 11:50 to 12:10",
    );
    let missing = common::identity_of(
        &process_record(9999, "ghost", "2026-08-31T11:00:00Z", "running"),
        SpatialType::Process,
        "2026-08-31T12:00:00Z",
    );
    assert_eq!(
        world.presence_of(&missing, SpatialType::Process),
        Presence::Unknown,
        "a missing observation is not an absence",
    );
}

#[test]
fn should_prove_absence_when_a_complete_source_covered_the_whole_window() {
    let ledger = SessionLedger::new();
    ledger
        .record_coverage(&[coverage(
            &capability::existence(SpatialType::Process),
            "2026-08-31T11:00:00Z",
            "2026-08-31T13:00:00Z",
            TemporalCompleteness::Complete,
            recorder(),
        )])
        .expect("the coverage is recorded");

    let world = Reconstructor::new(&ledger)
        .reconstruct(
            &ReconstructionRequest::new(scope(), instant("2026-08-31T12:10:00Z"))
                .since(instant("2026-08-31T11:50:00Z")),
        )
        .expect("the reconstruction answers");

    assert!(world.can_prove_absence(&capability::existence(SpatialType::Process)));
    let missing = common::identity_of(
        &process_record(9999, "ghost", "2026-08-31T11:00:00Z", "running"),
        SpatialType::Process,
        "2026-08-31T12:00:00Z",
    );
    assert_eq!(
        world.presence_of(&missing, SpatialType::Process),
        Presence::Absent,
    );
}

#[test]
fn should_report_the_gap_when_a_stretch_of_the_window_was_not_recorded() {
    let ledger = SessionLedger::new();
    ledger
        .record_coverage(&[coverage(
            &capability::existence(SpatialType::Process),
            "2026-08-31T11:00:00Z",
            "2026-08-31T12:00:00Z",
            TemporalCompleteness::Complete,
            recorder(),
        )])
        .expect("the coverage is recorded");

    let world = Reconstructor::new(&ledger)
        .reconstruct(
            &ReconstructionRequest::new(scope(), instant("2026-08-31T12:30:00Z"))
                .since(instant("2026-08-31T11:30:00Z")),
        )
        .expect("the reconstruction answers");

    let gap = world
        .gaps()
        .iter()
        .find(|gap| &*gap.capability == capability::existence(SpatialType::Process).as_ref())
        .expect("§7.5: the uncovered stretch is a typed object, not a silence");
    assert_eq!(gap.from, instant("2026-08-31T12:00:00Z"));
    assert_eq!(gap.until, instant("2026-08-31T12:30:00Z"));
    assert_eq!(gap.reason, GapReason::NotRecorded);
    assert!(!world.can_prove_absence(&capability::existence(SpatialType::Process)));
}

#[test]
fn should_refuse_historical_directory_contents_when_only_a_current_observation_supports_them() {
    let directory = file_record("/etc/nginx");
    let id = common::identity_of(&directory, SpatialType::Directory, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .append(
            &[event(
                EventKind::ObjectObserved,
                times("2026-08-31T12:00:00Z"),
                resolved(&id, SpatialType::Directory, "/etc/nginx"),
            )],
            &[],
        )
        .expect("the event is appended");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");

    assert!(
        world.object(&id).is_none(),
        "§14.5: current directory contents are never the past",
    );
    let gap = world
        .gaps()
        .iter()
        .find(|gap| gap.reason == GapReason::Unsupported)
        .expect("the refusal is stated as a gap rather than as silence");
    assert_eq!(
        &*gap.capability,
        capability::existence(SpatialType::Directory).as_ref(),
    );
}

#[test]
fn should_show_historical_directory_contents_when_a_checkpoint_holds_them() {
    let directory = file_record("/etc/nginx");
    let id = common::identity_of(&directory, SpatialType::Directory, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            vec![object_state(
                &directory,
                SpatialType::Directory,
                "/etc/nginx",
                "2026-08-31T12:00:00Z",
                recorder(),
            )],
            Vec::new(),
            vec![point_sample(
                &capability::existence(SpatialType::Directory),
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

    assert!(
        world.object(&id).is_some(),
        "§14.5: a recorder checkpoint is one of the four supports",
    );
}

#[test]
fn should_show_historical_directory_contents_when_the_source_streams_exhaustive_events() {
    let directory = file_record("/etc/nginx");
    let id = common::identity_of(&directory, SpatialType::Directory, "2026-08-31T12:00:00Z");
    let ledger = SessionLedger::new();
    ledger
        .append(
            &[event(
                EventKind::ObjectObserved,
                times("2026-08-31T12:00:00Z"),
                resolved(&id, SpatialType::Directory, "/etc/nginx"),
            )],
            &[],
        )
        .expect("the event is appended");

    let inotify = SourceMatrix::new().with(
        common::recorder(),
        TemporalCapabilities {
            live_events: true,
            exhaustive_events: true,
            ..TemporalCapabilities::none()
        },
    );
    let world = Reconstructor::new(&ledger)
        .with_sources(&inotify)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");

    assert!(world.object(&id).is_some());
    assert_eq!(
        inotify.structure_support(&common::recorder(), false),
        Some(StructureSupport::AuditEvidence),
    );
}

#[test]
fn should_name_the_four_supports_the_specification_admits_when_asked_what_carries_structure() {
    let matrix = SourceMatrix::new()
        .with(
            procfs(),
            TemporalCapabilities {
                current_snapshot: true,
                ..TemporalCapabilities::none()
            },
        )
        .with(
            common::systemd(),
            TemporalCapabilities {
                historical_query: true,
                ..TemporalCapabilities::none()
            },
        );

    assert_eq!(
        matrix.structure_support(&procfs(), true),
        Some(StructureSupport::Checkpoint),
    );
    assert_eq!(
        matrix.structure_support(&procfs(), false),
        None,
        "a current snapshot is not evidence about the past",
    );
    assert_eq!(
        matrix.structure_support(&common::systemd(), false),
        Some(StructureSupport::SnapshotProvider),
    );
    let kuang = ono_temporal_core::EvidenceSource::kuang("dev.example.zfs", "snapshots")
        .expect("a §7.1 source class");
    assert_eq!(
        matrix.structure_support(&kuang, false),
        Some(StructureSupport::KuangProvider),
    );
}
