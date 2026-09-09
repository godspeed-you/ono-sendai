//! The property tests v0.5 §47.2 asks for: applying an event sequence to a checkpoint is
//! deterministic, replaying persisted events yields the same reconstruction as before a restart,
//! and ordering is stable under wall-clock jumps where a source sequence exists.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    EventQuery, LedgerRead, LedgerWrite, SessionLedger, TemporalCompleteness, TemporalEvent,
    TimeRange,
};
use ono_temporal_reconstruct::{ReconstructionRequest, Reconstructor, capability, replay_order};
use ono_testkit::Rng;
use ono_value::Value;

use common::{
    changed, checkpoint, coverage, instant, object_state, resolved, scope, sequenced,
    service_record, systemd,
};

fn transitions() -> (ono_spatial_core::SpatialId, Vec<TemporalEvent>) {
    let record = service_record("nginx.service", "active", None);
    let id = common::identity_of(&record, SpatialType::Service, "2026-08-31T12:00:00Z");
    let states = [
        ("2026-08-31T12:01:00Z", "active", "reloading"),
        ("2026-08-31T12:02:00Z", "reloading", "active"),
        ("2026-08-31T12:03:00Z", "active", "deactivating"),
        ("2026-08-31T12:04:00Z", "deactivating", "inactive"),
        ("2026-08-31T12:05:00Z", "inactive", "activating"),
        ("2026-08-31T12:06:00Z", "activating", "failed"),
    ];
    let events = states
        .iter()
        .map(|(at, before, after)| {
            changed(
                at,
                resolved(&id, SpatialType::Service, "nginx.service"),
                "state",
                Value::string(before),
                Value::string(after),
            )
        })
        .collect();
    (id, events)
}

fn seeded_ledger(events: &[TemporalEvent]) -> SessionLedger {
    let record = service_record("nginx.service", "active", None);
    let ledger = SessionLedger::new();
    ledger
        .write_checkpoint(&checkpoint(
            scope(),
            "2026-08-31T12:00:00Z",
            vec![object_state(
                &record,
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
    ledger.append(events, &[]).expect("the events are appended");
    ledger
}

#[test]
fn should_produce_the_same_state_whatever_order_the_events_arrived_in() {
    let (id, events) = transitions();
    let request = ReconstructionRequest::new(scope(), instant("2026-08-31T12:07:00Z"));
    let expected = {
        let ledger = seeded_ledger(&events);
        Reconstructor::new(&ledger)
            .reconstruct(&request)
            .expect("the reconstruction answers")
    };

    let mut rng = Rng::seeded(0x5EED_0F17);
    for round in 0..32 {
        let mut shuffled = events.clone();
        for index in (1..shuffled.len()).rev() {
            shuffled.swap(index, rng.below(index + 1));
        }
        let ledger = seeded_ledger(&shuffled);
        let actual = Reconstructor::new(&ledger)
            .reconstruct(&request)
            .expect("the reconstruction answers");
        assert_eq!(
            actual, expected,
            "§47.2: applying an event sequence to a checkpoint is deterministic (round {round})",
        );
        assert_eq!(
            actual
                .object(&id)
                .and_then(|object| object.field("state"))
                .and_then(|field| field.value()),
            Some(&Value::string("failed")),
        );
    }
}

#[test]
fn should_answer_the_same_after_a_restart_when_the_persisted_events_are_reloaded() {
    let (_, events) = transitions();
    let request = ReconstructionRequest::new(scope(), instant("2026-08-31T12:07:00Z"));

    let before = seeded_ledger(&events);
    let first = Reconstructor::new(&before)
        .reconstruct(&request)
        .expect("the reconstruction answers");

    // A restart: everything the ledger held is read out and given to a fresh one.
    let reloaded = before
        .events(&EventQuery::in_range(TimeRange::all()))
        .expect("the ledger answers");
    let checkpoint_again = before
        .checkpoint_before(&scope(), instant("2026-08-31T12:07:00Z"))
        .expect("the ledger answers")
        .expect("the checkpoint survives");
    let after = SessionLedger::new();
    after
        .write_checkpoint(&checkpoint_again)
        .expect("the checkpoint is written");
    after
        .append(&reloaded, &[])
        .expect("the events are appended");
    after
        .record_coverage(
            &before
                .coverage(&ono_temporal_core::CoverageQuery::default())
                .expect("the ledger answers"),
        )
        .expect("the coverage is recorded");

    let second = Reconstructor::new(&after)
        .reconstruct(&request)
        .expect("the reconstruction answers");
    assert_eq!(
        first, second,
        "§47.2: replaying persisted events yields the same reconstruction as before restart",
    );
}

#[test]
fn should_keep_the_sources_own_order_when_the_wall_clock_jumped_backwards() {
    let record = service_record("nginx.service", "active", None);
    let id = common::identity_of(&record, SpatialType::Service, "2026-08-31T12:00:00Z");
    let subject = resolved(&id, SpatialType::Service, "nginx.service");

    let mut first = changed(
        "2026-08-31T12:05:00Z",
        subject.clone(),
        "state",
        Value::string("active"),
        Value::string("reloading"),
    );
    first.times = sequenced("2026-08-31T12:05:00Z", 1);
    let mut second = changed(
        "2026-08-31T12:01:00Z",
        subject,
        "state",
        Value::string("reloading"),
        Value::string("failed"),
    );
    second.times = sequenced("2026-08-31T12:01:00Z", 2);

    let ordered = replay_order(vec![second.clone(), first.clone()]);
    assert_eq!(
        ordered
            .iter()
            .map(|event| event.times.source_sequence)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2)],
        "§47.2: ordering is stable under wall-clock jumps when a source sequence exists",
    );
}

#[test]
fn should_place_concurrent_events_in_a_defined_order_without_claiming_one() {
    let record = service_record("nginx.service", "active", None);
    let id = common::identity_of(&record, SpatialType::Service, "2026-08-31T12:00:00Z");
    let subject = resolved(&id, SpatialType::Service, "nginx.service");
    let left = changed(
        "2026-08-31T12:01:00Z",
        subject.clone(),
        "state",
        Value::string("active"),
        Value::string("reloading"),
    );
    let right = changed(
        "2026-08-31T12:01:00Z",
        subject,
        "substate",
        Value::string("running"),
        Value::string("reload"),
    );

    let one = replay_order(vec![left.clone(), right.clone()]);
    let other = replay_order(vec![right, left]);
    assert_eq!(
        one.iter().map(|event| &event.event_id).collect::<Vec<_>>(),
        other
            .iter()
            .map(|event| &event.event_id)
            .collect::<Vec<_>>(),
        "§26.3: a defined display order, taken from the identity rather than from a clock",
    );
    assert_eq!(
        ono_temporal_core::happens_before(&one[0], &one[1]).0,
        ono_temporal_core::Ordering::Concurrent,
        "and no ordering is claimed for them",
    );
}

#[test]
fn should_say_what_placed_every_step_when_a_replay_plan_is_asked_for() {
    let record = service_record("nginx.service", "active", None);
    let id = common::identity_of(&record, SpatialType::Service, "2026-08-31T12:00:00Z");
    let subject = resolved(&id, SpatialType::Service, "nginx.service");

    let mut first = changed(
        "2026-08-31T12:01:00Z",
        subject.clone(),
        "state",
        Value::string("active"),
        Value::string("reloading"),
    );
    first.times = sequenced("2026-08-31T12:01:00Z", 1);
    let mut second = changed(
        "2026-08-31T12:02:00Z",
        subject.clone(),
        "state",
        Value::string("reloading"),
        Value::string("failed"),
    );
    second.times = sequenced("2026-08-31T12:02:00Z", 2);
    let unordered = changed(
        "2026-08-31T12:03:00Z",
        subject,
        "substate",
        Value::string("running"),
        Value::string("dead"),
    );

    let ordered = replay_order(vec![unordered, second, first]);
    let plan = ono_temporal_reconstruct::replay_plan(&ordered);

    assert_eq!(plan.len(), 3);
    assert_eq!(plan[0].evidence, None, "nothing precedes the first step");
    assert_eq!(
        plan[1].evidence,
        Some(ono_temporal_core::OrderEvidence::SourceSequence),
        "§26.1: one source's own stream is what orders these two",
    );
    assert_eq!(
        plan[2].evidence, None,
        "§26.3: an event with no ordering evidence is placed, and no order is claimed for it",
    );
}
