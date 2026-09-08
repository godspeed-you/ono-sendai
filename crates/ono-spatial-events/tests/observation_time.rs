//! When a spatial change was observed (spec v0.5 §3.3, §9.2, §18.1, §39.1, §39.2).
//!
//! v0.5 §18.1 forbids a live diff model and a historical event model with subtly incompatible
//! semantics, and §39.1 keeps this crate's job while requiring its canonical output to be
//! ingestible into the temporal event path. A change with no time cannot be: whoever bridged it
//! would have to invent a timestamp, which §3.3 forbids by keeping source time, observed time and
//! ingestion time separate.
//!
//! The distinction these tests are about is §9.2's. A change a provider announced happened at an
//! instant the provider stated. A change found by comparing two observations happened somewhere
//! between them, and Ono MUST NOT claim the state at a moment in between — so the change carries
//! the interval and refuses to name a point inside it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_events::{
    ChangeKind, ChangeSet, ChangeSource, EventMerge, Freshness, MapSnapshot, ObservationWindow,
    ObservedAt, SpatialChange, compare,
};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

mod common;

use common::{id, node, nodes_at as map};

fn instant(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).expect("a representable instant")
}

/// One `ono.socket-event/1` as the v0.2 watch runtime writes it, observed at `at`.
fn watch_event(kind: &str, at: Timestamp) -> Value {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.socket-event", 1))
        .expect("the workspace carries the socket event contract");
    let event = RecordValue::builder(
        schema,
        Provenance::local("ono.runtime", SchemaId::new("ono.socket-event", 1)),
    )
    .set("kind", Value::string(kind))
    .expect("`kind` is a declared field")
    .set("at", Value::Timestamp(at))
    .expect("`at` is a declared field")
    .set("source", Value::string("subscription"))
    .expect("`source` is a declared field")
    .build();
    Value::Record(Arc::new(event))
}

#[test]
fn should_carry_the_instant_the_provider_stated_when_the_change_came_from_an_event() {
    // v0.5 §3.3: `observed_at` is when the observing component detected the change. A provider
    // announced this one and said when, so the change says the same and nothing else.
    let at = instant(1_756_000_000);
    let mut merge = EventMerge::new();
    let observed = merge
        .absorb(&watch_event("added", at))
        .expect("an `ono.socket-event/1` record is an event this merge understands");

    let change = SpatialChange::to_node(
        ChangeKind::NodeAppeared,
        id("listener"),
        "127.0.0.1:8080",
        observed.observed(),
    );

    assert_eq!(
        change.observed(),
        ObservedAt::At(at),
        "spec v0.5 §3.3: the change happened when the source says it happened"
    );
    assert_eq!(
        change.observed().instant(),
        Some(at),
        "an announced change has an instant, and it is the announced one"
    );
}

#[test]
fn should_report_no_instant_at_all_when_the_event_that_announced_the_change_stated_no_time() {
    // v0.5 §3.3: an absent observation time is a fact and a substituted one is a fiction.
    let change = SpatialChange::to_node(
        ChangeKind::NodeAppeared,
        id("listener"),
        "127.0.0.1:8080",
        ObservedAt::Unknown,
    );

    assert_eq!(change.observed(), ObservedAt::Unknown);
    assert_eq!(change.observed().instant(), None);
    assert_eq!(change.observed().earliest(), None);
}

#[test]
fn should_carry_the_interval_between_the_two_observations_when_the_change_came_from_comparing_them()
{
    // v0.5 §9.2: between two observations Ono MUST NOT claim the state at a moment in between.
    // A comparison knows the change happened somewhere inside the interval and knows no more,
    // so it carries both ends and refuses to name a point.
    let earlier = instant(1_756_000_000);
    let later = instant(1_756_000_010);
    let before = MapSnapshot::of(&map(Vec::new(), earlier));
    let after = MapSnapshot::of(&map(vec![node("listener", "127.0.0.1:8080")], later));

    let changes = compare(&before, &after, Freshness::Polled);
    let change = changes
        .changes()
        .next()
        .expect("the node that appeared is a change");

    assert_eq!(
        change.observed(),
        ObservedAt::Between {
            from: earlier,
            until: later
        },
        "spec v0.5 §9.2: the honest answer is the interval, not a point inside it"
    );
    assert_eq!(
        change.observed().instant(),
        None,
        "spec v0.5 §9.2: a comparison names no instant, because it observed none"
    );
    assert_eq!(change.observed().earliest(), Some(earlier));
    assert_eq!(change.observed().latest(), Some(later));
}

#[test]
fn should_report_the_window_a_change_set_is_a_statement_about() {
    // v0.5 §9.2 and §8: a consumer has to know which period the set covers before it can say
    // what the set proves. The window is the interval the two observations span.
    let earlier = instant(1_756_000_000);
    let later = instant(1_756_000_010);
    let before = MapSnapshot::of(&map(Vec::new(), earlier));
    let after = MapSnapshot::of(&map(vec![node("listener", "127.0.0.1:8080")], later));

    let changes = compare(&before, &after, Freshness::Polled);

    assert_eq!(changes.window(), ObservationWindow::new(earlier, later));
    assert_eq!(changes.window().since(), earlier);
    assert_eq!(changes.window().until(), later);
}

#[test]
fn should_report_a_single_instant_as_the_window_when_only_one_observation_was_made() {
    // The opening value of a live stream compares to nothing (§24.3). It covers the instant its
    // one observation was made, and stating a wider window would claim coverage of a period
    // nothing was watching.
    let at = instant(1_756_000_000);

    let changes = ChangeSet::new(
        ChangeSource::SnapshotComparison,
        Freshness::Cached,
        ObservationWindow::at(at),
    );

    assert!(changes.is_empty());
    assert_eq!(changes.window(), ObservationWindow::new(at, at));
}

#[test]
fn should_report_no_change_at_all_when_two_identical_projections_were_made_at_different_instants() {
    // v0.5 §25.2 of v0.4 and §43.6: motion means change. Time passing is not change, so a second
    // observation of an unchanged system produces nothing however much later it was made.
    let before = MapSnapshot::of(&map(
        vec![node("listener", "127.0.0.1:8080")],
        instant(1_756_000_000),
    ));
    let after = MapSnapshot::of(&map(
        vec![node("listener", "127.0.0.1:8080")],
        instant(1_756_003_600),
    ));

    let changes = compare(&before, &after, Freshness::Polled);

    assert!(
        changes.is_empty(),
        "spec v0.4 §25.2: an unchanged system produces no change, got {:?}",
        changes.changes().collect::<Vec<_>>()
    );
}

#[test]
fn should_answer_the_same_thing_every_time_because_it_reads_no_clock() {
    // v0.5 §39.2: pure comparison logic accepts time as a parameter. Everything below is built
    // from stated instants, so two runs an arbitrary distance apart are identical values —
    // which is what makes a temporal test deterministic at all.
    let earlier = instant(1_756_000_000);
    let later = instant(1_756_000_010);
    let projection = |at| MapSnapshot::of(&map(vec![node("listener", "127.0.0.1:8080")], at));
    let changed = |at| {
        let mut moved = node("listener", "127.0.0.1:8080");
        moved.state = Some("close_wait".to_owned());
        MapSnapshot::of(&map(vec![moved], at))
    };

    let first = compare(&projection(earlier), &changed(later), Freshness::Polled);
    let second = compare(&projection(earlier), &changed(later), Freshness::Polled);

    assert_eq!(
        first, second,
        "spec v0.5 §39.2: nothing here reads a clock, so the same inputs are the same answer"
    );
    assert_eq!(
        first
            .changes()
            .map(SpatialChange::observed)
            .collect::<Vec<_>>(),
        vec![ObservedAt::Between {
            from: earlier,
            until: later
        }],
        "the instants in the answer are the instants that were handed in"
    );
}
