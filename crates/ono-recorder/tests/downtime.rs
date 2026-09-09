//! Restart, reboot and the intervals nobody watched (v0.5 §44.1, §44.2, §43.2, §55.5).

mod common;

use common::{BOOT, HOST, instant, options, procfs_profile, scope, settings};
use ono_recorder::{Recorder, downtime};
use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    ClockDomain, EventKind, EventSeed, EventTimes, EvidenceSource, GapReason, LedgerWrite as _,
    SpatialRef,
};
use ono_temporal_ledger::{LedgerStore, StoreOptions};
use ono_value::Provenance;

fn observation(at: &str, sequence: Option<u64>, boot: &str) -> ono_temporal_core::TemporalEvent {
    let when = instant(at);
    EventSeed {
        kind: EventKind::ObjectObserved,
        subtype: None,
        scope: scope(),
        subject: Some(SpatialRef::Unresolved {
            source: EvidenceSource::builtin("linux.procfs")
                .unwrap_or_else(EvidenceSource::recorder),
            described: "pid 1842".into(),
        }),
        related: Vec::new(),
        times: ono_temporal_core::EventTimes {
            source_sequence: sequence,
            ..EventTimes::observed(when, when, ClockDomain::new(HOST, Some(boot)))
        },
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: Provenance::local(
            "linux.procfs",
            ono_value::SchemaId::new("ono.temporal-event", 1),
        ),
    }
    .seal()
}

#[test]
fn should_mark_the_interval_between_two_runs_as_a_gap_when_the_recorder_restarts() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let path = directory.path().join("ledger.sqlite3");
    let store = LedgerStore::open_with(&StoreOptions::at(&path)).expect("a store");
    store
        .append(&[observation("2026-08-31T12:00:00Z", Some(1), BOOT)], &[])
        .expect("one observed event");
    drop(store);

    let store = LedgerStore::open_with(&StoreOptions::at(&path)).expect("the store again");
    let plan = downtime::restart(
        &store,
        &scope(),
        &ClockDomain::new(HOST, Some(BOOT)),
        &[procfs_profile()],
        instant("2026-08-31T12:30:00Z"),
    )
    .expect("§44.1's restart procedure");

    let gap = plan
        .gaps
        .iter()
        .find(|gap| gap.reason == GapReason::NotRecorded)
        .expect("§44.1: unobserved downtime is a coverage gap");
    assert_eq!(gap.from, instant("2026-08-31T12:00:00Z"));
    assert_eq!(gap.until, instant("2026-08-31T12:30:00Z"));
    assert_eq!(gap.source, EvidenceSource::recorder());
    assert_eq!(
        gap.detail.as_deref(),
        Some("recorder not running"),
        "§44.1's fifth clause: continue without pretending the gap was covered"
    );
    assert!(
        plan.gaps
            .iter()
            .any(|gap| &*gap.capability == "process.existence"),
        "the gap is filed under the capability a reconstruction looks it up by"
    );
}

#[test]
fn should_restore_the_source_sequence_when_the_recorder_restarts() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let path = directory.path().join("ledger.sqlite3");
    let store = LedgerStore::open_with(&StoreOptions::at(&path)).expect("a store");
    store
        .append(
            &[
                observation("2026-08-31T12:00:00Z", Some(7), BOOT),
                observation("2026-08-31T12:00:01Z", Some(8), BOOT),
            ],
            &[],
        )
        .expect("two numbered events");
    drop(store);

    let store = LedgerStore::open_with(&StoreOptions::at(&path)).expect("the store again");
    let plan = downtime::restart(
        &store,
        &scope(),
        &ClockDomain::new(HOST, Some(BOOT)),
        &[procfs_profile()],
        instant("2026-08-31T12:30:00Z"),
    )
    .expect("§44.1's restart procedure");

    let resumed = plan
        .sequences
        .iter()
        .find(|sequence| sequence.domain.boot_id.as_deref() == Some(BOOT))
        .expect("§44.1's second step restores the source sequence checkpoint");
    assert_eq!(resumed.highest, 8);
    assert!(!plan.boot_changed);
}

#[test]
fn should_not_carry_a_sequence_across_a_boot_when_the_host_rebooted() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let path = directory.path().join("ledger.sqlite3");
    let store = LedgerStore::open_with(&StoreOptions::at(&path)).expect("a store");
    store
        .append(&[observation("2026-08-31T12:00:00Z", Some(41), BOOT)], &[])
        .expect("an event from the earlier boot");
    drop(store);

    let store = LedgerStore::open_with(&StoreOptions::at(&path)).expect("the store again");
    let plan = downtime::restart(
        &store,
        &scope(),
        &ClockDomain::new(HOST, Some("boot-b")),
        &[procfs_profile()],
        instant("2026-08-31T12:30:00Z"),
    )
    .expect("§44.1's restart procedure");

    assert!(
        plan.boot_changed,
        "§44.2: a host reboot creates a new boot clock domain"
    );
    assert_eq!(
        plan.resumable_sequence(&procfs_profile().source),
        None,
        "§44.2: sequence semantics do not cross the boot boundary"
    );
}

#[test]
fn should_ask_for_a_fresh_checkpoint_when_the_recorder_restarts() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let store = LedgerStore::open_with(&StoreOptions::at(&directory.path().join("ledger.sqlite3")))
        .expect("a store");

    let plan = downtime::restart(
        &store,
        &scope(),
        &ClockDomain::new(HOST, Some(BOOT)),
        &[procfs_profile()],
        instant("2026-08-31T12:30:00Z"),
    )
    .expect("§44.1's restart procedure");

    assert!(
        plan.checkpoint_due,
        "§44.1's fourth step takes a fresh checkpoint as appropriate"
    );
    assert!(
        plan.integrity.is_healthy(),
        "§44.1's first step validated the metadata"
    );
}

#[test]
fn should_declare_continuity_only_for_a_source_that_promised_it_when_the_recorder_starts() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(
        options(directory.path()).with_sources(vec![procfs_profile(), common::netlink_profile()]),
    );

    recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("a start");

    let declared = recorder.declared_contiguous().expect("the store answers");
    assert!(
        declared.contains(&common::netlink_profile().source),
        "§21.5, §43.2: a source claiming exhaustive events promises continuity"
    );
    assert!(
        !declared.contains(&procfs_profile().source),
        "§22.1: a polled snapshot source promises nothing of the kind"
    );
}

#[test]
fn should_report_the_recorder_as_the_gap_source_when_no_provider_was_watching() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("a start");
    recorder
        .stop(instant("2026-08-31T12:10:00Z"))
        .expect("a stop");

    let restarted = recorder
        .start(&settings(), instant("2026-08-31T12:40:00Z"))
        .expect("a restart");
    let plan = restarted.plan.expect("a restart carries §44.1's plan");

    let existence = ono_temporal_reconstruct::capability::existence(SpatialType::Process);
    assert!(
        plan.gaps
            .iter()
            .any(|gap| gap.reason == GapReason::NotRecorded && gap.capability == existence),
        "the downtime is a gap in the capability object presence is gated on"
    );
    let stored = recorder.gaps().expect("the store answers for its gaps");
    assert!(
        stored
            .iter()
            .any(|gap| gap.reason == GapReason::NotRecorded),
        "§55.5: the gap is in the ledger, not only in the plan"
    );
}
