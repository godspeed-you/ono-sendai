//! A bounded question stays bounded (v0.5 §32.3, §19.4).
//!
//! §32.3 budgets a fifteen-minute timeline of one place at 100 ms p95 "on a ledger within default
//! retention", and the way that budget is met is that the window, the place and the limit are in
//! the statement: the store reads the rows it answers with and no others. The size here is chosen
//! so the test runs inside the gate; the million-event fixture belongs to the performance package,
//! and this is the API it will call.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::time::Instant;

use ono_temporal_core::{EventKind, EventQuery, LedgerRead, LedgerWrite, QueryOrder, TimeRange};

use common::{instant, nested_scope, scope, seed, store_in, subject, times};

/// How many events the gate-sized fixture holds.
const EVENTS: i64 = 20_000;

/// Appends `EVENTS` events one second apart, half of them inside a nested scope.
fn fill(store: &ono_temporal_ledger::LedgerStore) {
    let start = instant("2026-08-31T00:00:00Z");
    let mut batch = Vec::with_capacity(500);
    for index in 0..EVENTS {
        let at = start + jiff::Span::new().seconds(index);
        let mut event = seed(
            if index % 3 == 0 {
                EventKind::ObjectChanged
            } else {
                EventKind::ObjectObserved
            },
            times("2026-08-31T00:00:00Z"),
            "linux.procfs",
        );
        event.times.observed_at = at;
        event.times.ingested_at = at;
        event.subject = Some(ono_temporal_core::SpatialRef::Resolved {
            id: subject(index % 50),
            object_type: ono_spatial_core::SpatialType::Process,
            label: std::sync::Arc::from(format!("pid {}", index % 50)),
        });
        if index % 2 == 0 {
            event.scope = nested_scope();
        }
        batch.push(event.seal());
        if batch.len() == 500 {
            store.append(&batch, &[]).expect("an append succeeds");
            batch.clear();
        }
    }
    if !batch.is_empty() {
        store.append(&batch, &[]).expect("an append succeeds");
    }
}

#[test]
fn should_answer_a_bounded_question_without_reading_the_whole_ledger() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    fill(&store);
    store.flush().expect("a flush succeeds");
    assert_eq!(store.retention().events, u64::try_from(EVENTS).unwrap_or(0));

    // §32.3's timeline: fifteen minutes of one place, newest first.
    let window_start = instant("2026-08-31T01:00:00Z");
    let started = Instant::now();
    let timeline = store
        .events(&EventQuery {
            scope: Some(nested_scope()),
            range: TimeRange::between(window_start, window_start + jiff::Span::new().minutes(15)),
            order: QueryOrder::Descending,
            limit: Some(200),
            ..EventQuery::default()
        })
        .expect("the timeline answers");
    let elapsed = started.elapsed();

    assert_eq!(
        timeline.len(),
        200,
        "the window holds 450 events in that place and the limit takes 200"
    );
    assert!(
        timeline.windows(2).all(
            |pair| pair[0].times.presentation_instant() >= pair[1].times.presentation_instant()
        ),
        "§11.2: descending means descending"
    );
    assert!(
        timeline
            .iter()
            .all(|event| nested_scope().contains(&event.scope)),
        "§3.2: the place filter is a place filter"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "§32.3 budgets 100 ms p95 for this on release hardware; a debug build taking {elapsed:?} \
         means the query is reading rows it does not answer with"
    );
}

#[test]
fn should_answer_one_event_by_identity_without_reading_the_whole_ledger() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    fill(&store);

    let last = store
        .events(&EventQuery {
            order: QueryOrder::Descending,
            limit: Some(1),
            ..EventQuery::in_range(TimeRange::all())
        })
        .expect("events answer");
    let wanted = last.first().expect("the ledger holds events").clone();

    let started = Instant::now();
    let found = store.event(&wanted.event_id).expect("the lookup answers");
    let elapsed = started.elapsed();
    assert_eq!(found, Some(wanted));
    assert!(
        elapsed < std::time::Duration::from_millis(100),
        "§32.3: an event by id is a primary-key lookup; it took {elapsed:?}"
    );
}

#[test]
fn should_answer_a_one_hour_changes_query_over_a_large_ledger() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    fill(&store);

    let from = instant("2026-08-31T02:00:00Z");
    let started = Instant::now();
    let changes = store
        .events(&EventQuery {
            scope: Some(scope()),
            kinds: vec![EventKind::ObjectChanged],
            range: TimeRange::between(from, from + jiff::Span::new().hours(1)),
            ..EventQuery::default()
        })
        .expect("the changes answer");
    let elapsed = started.elapsed();

    assert_eq!(changes.len(), 1_200, "one in three of 3600 seconds changed");
    assert!(
        changes
            .iter()
            .all(|event| event.kind == EventKind::ObjectChanged)
    );
    assert!(
        elapsed < std::time::Duration::from_millis(1_000),
        "§32.3 budgets 150 ms p95 for this on release hardware; it took {elapsed:?}"
    );
}

#[test]
fn should_find_the_nearest_checkpoint_before_an_instant_over_a_large_ledger() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    fill(&store);
    for hour in 0..5 {
        let at = format!("2026-08-31T{hour:02}:00:00Z");
        store
            .write_checkpoint(&common::checkpoint(&at))
            .expect("a checkpoint writes");
    }

    let started = Instant::now();
    let found = store
        .checkpoint_before(&scope(), instant("2026-08-31T03:30:00Z"))
        .expect("checkpoints answer");
    let elapsed = started.elapsed();

    assert_eq!(
        found.map(|checkpoint| checkpoint.captured_at),
        Some(instant("2026-08-31T03:00:00Z")),
        "§9.1: the nearest checkpoint at or before the instant"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(100),
        "§32.3 budgets 150 ms p95 for `at`; the lookup took {elapsed:?}"
    );
}
