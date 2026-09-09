//! Retention by age, by size and by both (v0.5 §10.4, §31.8, §53), and §47.2's property that it
//! "never leaves dangling causal/evidence references".

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use jiff::Timestamp;
use ono_temporal_core::{
    EventKind, EventQuery, LedgerRead, LedgerWrite, RedactedCommandSummary, TimeRange,
};
use ono_temporal_ledger::{LedgerStore, RetentionPolicy, StoreOptions};
use ono_value::{ByteSize, Duration};

use common::{
    action, checkpoint, coverage, event, evidence_for, instant, link, path_in, scope, seed, times,
};

fn store_with(home: &std::path::Path, policy: RetentionPolicy) -> LedgerStore {
    let path = path_in(home);
    LedgerStore::open_with(&StoreOptions::at(&path).with_retention(policy))
        .expect("a fresh store opens")
}

fn hours(count: i64) -> Duration {
    Duration::from_nanoseconds(i128::from(count) * 3_600 * 1_000_000_000)
}

/// Fills the store with `count` events an hour apart, oldest first.
fn fill(store: &LedgerStore, count: i64, from: Timestamp) -> Vec<Timestamp> {
    let mut instants = Vec::new();
    for index in 0..count {
        let at = from + jiff::Span::new().hours(index);
        let mut event = seed(
            EventKind::ObjectObserved,
            times("2026-01-01T00:00:00Z"),
            "linux.procfs",
        );
        event.times.observed_at = at;
        event.times.ingested_at = at;
        store
            .append(&[event.seal()], &[])
            .expect("an append succeeds");
        instants.push(at);
    }
    instants
}

#[test]
fn should_remove_only_what_is_older_than_the_age_bound_when_age_is_the_binding_limit() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_with(
        home.path(),
        RetentionPolicy::unlimited().with_max_age(Some(hours(24))),
    );
    let start = instant("2026-08-30T00:00:00Z");
    fill(&store, 48, start);
    let now = start + jiff::Span::new().hours(47);

    let swept = store.sweep(now).expect("a sweep succeeds");
    assert!(
        swept.complete,
        "§31.8: the sweep finishes inside its budget"
    );
    assert_eq!(swept.events, 23, "24 hours before 47h in is 23h in");

    let retention = store.retention();
    assert_eq!(retention.events, 25);
    assert_eq!(
        retention.earliest,
        Some(start + jiff::Span::new().hours(23)),
        "§12.3: the retained boundary is what `at` refuses beyond"
    );
    assert_eq!(retention.evicted, 23);
}

#[test]
fn should_remove_the_oldest_first_when_size_is_the_binding_limit() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_with(
        home.path(),
        RetentionPolicy::unlimited()
            .with_max_size(Some(ByteSize::from_bytes(384 * 1024)))
            .with_batch(4_096),
    );
    let start = instant("2026-08-30T00:00:00Z");
    fill(&store, 600, start);
    store.flush().expect("a flush succeeds");
    let before = store.retention();
    assert!(
        before
            .stored_size
            .is_some_and(|size| size.bytes() > 384 * 1024),
        "the fixture has to exceed the bound for the bound to be testable; it was {:?}",
        before.stored_size
    );

    let mut rounds = 0;
    loop {
        let swept = store.sweep(start).expect("a sweep succeeds");
        rounds += 1;
        assert!(rounds < 64, "a bounded sweep must converge");
        if swept.complete {
            break;
        }
    }

    let after = store.retention();
    assert!(
        after.events < before.events,
        "the size bound removed events"
    );
    assert!(
        after.earliest.is_some_and(|earliest| earliest > start),
        "§31.8: removal is oldest-first"
    );
}

#[test]
fn should_apply_whichever_bound_removes_data_first_when_both_are_in_force() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_with(
        home.path(),
        RetentionPolicy::default()
            .with_max_age(Some(hours(10)))
            .with_max_size(Some(ByteSize::from_bytes(384 * 1024)))
            .with_batch(4_096),
    );
    let start = instant("2026-08-30T00:00:00Z");
    fill(&store, 600, start);
    let now = start + jiff::Span::new().hours(599);

    let mut rounds = 0;
    loop {
        let swept = store.sweep(now).expect("a sweep succeeds");
        rounds += 1;
        assert!(rounds < 64, "a bounded sweep must converge");
        if swept.complete {
            break;
        }
    }

    let after = store.retention();
    assert!(
        after.events <= 11,
        "§53: 10 hours or 384 KiB, whichever removes data first; {} survived",
        after.events
    );
    assert_eq!(after.max_age, Some(hours(10)));
    assert_eq!(after.max_size, Some(ByteSize::from_bytes(384 * 1024)));
}

/// §31.8 and §47.2: retention "MUST also handle orphaned evidence/checkpoints/causal links without
/// leaving invalid references".
#[test]
fn should_leave_nothing_pointing_at_what_is_gone_when_retention_has_run() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_with(
        home.path(),
        RetentionPolicy::unlimited().with_max_age(Some(hours(1))),
    );
    let cause = event(
        EventKind::ActionExecuted,
        "2026-08-30T00:00:00Z",
        "ono.session",
    );
    let effect = event(
        EventKind::ObjectChanged,
        "2026-08-30T00:00:01Z",
        "linux.procfs",
    );
    let old_evidence = evidence_for("2026-08-30T00:00:01Z", 1827, "active");
    let survivor = event(
        EventKind::ObjectChanged,
        "2026-08-30T10:00:00Z",
        "linux.procfs",
    );
    let new_evidence = evidence_for("2026-08-30T10:00:00Z", 1828, "inactive");

    store
        .append(
            &[cause.clone(), effect.clone(), survivor.clone()],
            &[old_evidence.clone(), new_evidence.clone()],
        )
        .expect("an append succeeds");
    store
        .append_links(&[link(&cause, &effect, &old_evidence)])
        .expect("links append");
    store
        .record_coverage(&[coverage("2026-08-30T00:00:00Z", "2026-08-30T00:30:00Z")])
        .expect("coverage records");
    store
        .record_action(&action(
            "2026-08-30T00:00:00Z",
            RedactedCommandSummary::of("restart", Some("service"), &[]),
        ))
        .expect("an action records");
    store
        .write_checkpoint(&checkpoint("2026-08-30T00:00:00Z"))
        .expect("a checkpoint writes");

    let now = instant("2026-08-30T10:30:00Z");
    let swept = store.sweep(now).expect("a sweep succeeds");
    assert!(swept.complete);
    assert_eq!(swept.events, 2);

    let held = store
        .events(&EventQuery::in_range(TimeRange::all()))
        .expect("events answer");
    assert_eq!(held, vec![survivor.clone()]);

    assert!(
        store
            .causal_links(&effect.event_id)
            .expect("links answer")
            .is_empty(),
        "§31.8: a link whose effect is gone is removed rather than left pointing at nothing"
    );
    assert!(
        store
            .evidence(std::slice::from_ref(&old_evidence.evidence_id))
            .expect("evidence answers")
            .is_empty(),
        "§31.8: evidence nothing cites any more goes with the events it belonged to"
    );
    assert_eq!(
        store
            .evidence(std::slice::from_ref(&new_evidence.evidence_id))
            .expect("evidence answers")
            .len(),
        1,
        "evidence a surviving event cites stays"
    );
    assert!(
        store
            .checkpoint_before(&scope(), instant("2026-08-30T09:00:00Z"))
            .expect("checkpoints answer")
            .is_none(),
        "a checkpoint older than the retained boundary goes"
    );
    assert!(
        store
            .actions(TimeRange::all())
            .expect("actions answer")
            .is_empty()
    );
    assert!(
        store
            .coverage(&ono_temporal_core::CoverageQuery::default())
            .expect("coverage answers")
            .is_empty()
    );
}

#[test]
fn should_remove_nothing_and_finish_when_no_bound_is_in_force() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_with(home.path(), RetentionPolicy::unlimited());
    fill(&store, 5, instant("2026-08-30T00:00:00Z"));
    let swept = store
        .sweep(instant("2030-01-01T00:00:00Z"))
        .expect("a sweep succeeds");
    assert!(swept.complete);
    assert!(!swept.removed_anything());
    assert_eq!(store.retention().events, 5);
}

/// §31.8: "retention cleanup MUST run in bounded background work." One call removes at most one
/// batch and says there is more to do, so a caller is never held for an unbounded time.
#[test]
fn should_stop_at_the_batch_and_report_more_to_do_when_much_has_expired() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_with(
        home.path(),
        RetentionPolicy::unlimited()
            .with_max_age(Some(hours(1)))
            .with_batch(10),
    );
    let start = instant("2026-08-30T00:00:00Z");
    fill(&store, 50, start);
    let now = start + jiff::Span::new().hours(49);

    let first = store.sweep(now).expect("a sweep succeeds");
    assert_eq!(first.events, 10, "the batch bounds the work");
    assert!(!first.complete, "the caller is told to come back");

    let mut rounds = 1;
    while !store.sweep(now).expect("a sweep succeeds").complete {
        rounds += 1;
        assert!(rounds < 20, "a bounded sweep must converge");
    }
    assert_eq!(store.retention().events, 2);
}

#[test]
fn should_hold_nothing_when_the_whole_local_ledger_is_removed() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_with(home.path(), RetentionPolicy::default());
    let held = event(
        EventKind::ObjectChanged,
        "2026-08-30T00:00:00Z",
        "linux.procfs",
    );
    let evidence = evidence_for("2026-08-30T00:00:00Z", 1827, "active");
    store
        .append(&[held], std::slice::from_ref(&evidence))
        .expect("an append succeeds");
    store
        .write_checkpoint(&checkpoint("2026-08-30T00:00:00Z"))
        .expect("a checkpoint writes");

    store.remove_all().expect("§30.8: the user may clear it");

    assert_eq!(store.retention().events, 0);
    assert!(
        store
            .evidence(&[evidence.evidence_id])
            .expect("evidence answers")
            .is_empty()
    );
    assert!(
        store
            .checkpoint_before(&scope(), instant("2030-01-01T00:00:00Z"))
            .expect("checkpoints answer")
            .is_none()
    );
}
