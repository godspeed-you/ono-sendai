//! Historical collections: v0.5 §9.6 — the best supported set for `T`, with collection-level
//! coverage, and no implication that the rows are the whole list where enumeration is unproven.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{LedgerWrite, SessionLedger, TemporalCompleteness};
use ono_temporal_reconstruct::{ReconstructionRequest, Reconstructor, capability};

use common::{
    checkpoint, coverage, instant, object_state, point_sample, process_record, procfs, recorder,
    scope,
};

fn ledger_with(completeness: TemporalCompleteness) -> SessionLedger {
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            vec![
                object_state(
                    &process_record(1842, "nginx", "2026-08-31T11:00:00Z", "running"),
                    SpatialType::Process,
                    "nginx",
                    "2026-08-31T12:00:00Z",
                    procfs(),
                ),
                object_state(
                    &process_record(1843, "worker", "2026-08-31T11:00:00Z", "sleeping"),
                    SpatialType::Process,
                    "worker",
                    "2026-08-31T12:00:00Z",
                    procfs(),
                ),
            ],
            Vec::new(),
            vec![coverage(
                &capability::existence(SpatialType::Process),
                "2026-08-31T12:00:00Z",
                "2026-08-31T12:00:00Z",
                completeness,
                procfs(),
            )],
        ))
        .expect("the checkpoint is written");
    ledger
}

#[test]
fn should_refuse_to_imply_a_complete_list_when_enumeration_cannot_be_proven() {
    let ledger = ledger_with(TemporalCompleteness::PointSample);
    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");

    let collection = world.collection(SpatialType::Process);
    assert_eq!(collection.len(), 2, "§9.6: the best supported set for `T`");
    assert!(
        !collection.is_enumeration_proven(),
        "§9.6: the result must not imply the rows are the complete process list",
    );
    assert_eq!(collection.completeness(), TemporalCompleteness::PointSample);
    assert_eq!(
        collection.capability().as_ref(),
        capability::existence(SpatialType::Process).as_ref(),
    );
}

#[test]
fn should_state_that_the_list_is_complete_when_the_source_proved_the_enumeration() {
    let ledger = ledger_with(TemporalCompleteness::Complete);
    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");

    let collection = world.collection(SpatialType::Process);
    assert!(collection.is_enumeration_proven());
    assert_eq!(collection.completeness(), TemporalCompleteness::Complete);
}

#[test]
fn should_report_an_empty_collection_as_unknown_when_nothing_covered_the_class() {
    let ledger = SessionLedger::new();
    ledger
        .record_coverage(&[point_sample(
            &capability::existence(SpatialType::Service),
            "2026-08-31T12:00:00Z",
            recorder(),
        )])
        .expect("the coverage is recorded");

    let world = Reconstructor::new(&ledger)
        .reconstruct(&ReconstructionRequest::new(
            scope(),
            instant("2026-08-31T12:00:00Z"),
        ))
        .expect("the reconstruction answers");

    let collection = world.collection(SpatialType::Process);
    assert_eq!(collection.len(), 0);
    assert!(!collection.is_enumeration_proven());
    assert_eq!(collection.completeness(), TemporalCompleteness::Unknown);
}
