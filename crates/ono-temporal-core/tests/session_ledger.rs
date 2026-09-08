//! The bounded session ledger of v0.5 §10.7 — "the interactive session SHOULD maintain a bounded
//! in-memory ledger", default 100000 events — and the ledger contract of §39 it implements
//! without naming a store.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_core::ErrorCode;
use ono_temporal_core::{
    DEFAULT_SESSION_MAX_EVENTS, EventKind, EventQuery, GapReason, LedgerRead, LedgerWrite,
    QueryOrder, SessionLedger, TimeRange,
};

use common::{event, instant, seed, times};

fn numbered(sequence: u64) -> ono_temporal_core::TemporalEvent {
    let mut seed = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T12:00:00Z"),
        "linux.netlink",
    );
    seed.times.source_sequence = Some(sequence);
    seed.times.observed_at = instant("2026-08-31T12:00:00Z")
        .checked_add(jiff::Span::new().seconds(i64::try_from(sequence).unwrap_or(0)))
        .expect("the fixture instant stays in range");
    seed.seal()
}

#[test]
fn should_default_to_the_ceiling_of_section_ten_seven_when_a_session_ledger_is_created() {
    assert_eq!(DEFAULT_SESSION_MAX_EVENTS, 100_000);
    assert_eq!(SessionLedger::new().capacity(), 100_000);
}

#[test]
fn should_store_and_return_an_event_when_it_is_appended() {
    let ledger = SessionLedger::new();
    let event = event(
        EventKind::ObjectChanged,
        "2026-08-31T12:00:00Z",
        "ono.recorder",
    );
    let appended = ledger
        .append(std::slice::from_ref(&event), &[])
        .expect("the append succeeds");
    assert_eq!((appended.stored, appended.duplicates), (1, 0));
    assert_eq!(
        ledger
            .event(&event.event_id)
            .expect("the lookup succeeds")
            .map(|found| found.event_id),
        Some(event.event_id)
    );
}

#[test]
fn should_count_a_second_report_as_a_duplicate_when_the_observation_is_the_same() {
    let ledger = SessionLedger::new();
    let event = event(
        EventKind::ObjectObserved,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
    );
    ledger
        .append(std::slice::from_ref(&event), &[])
        .expect("the first append succeeds");
    let second = ledger
        .append(&[event], &[])
        .expect("the second append succeeds");
    assert_eq!(
        (second.stored, second.duplicates),
        (0, 1),
        "§6.8: deduplication falls out of identity"
    );
    assert_eq!(
        ledger
            .events(&EventQuery::default())
            .expect("the query succeeds")
            .len(),
        1
    );
}

#[test]
fn should_evict_the_oldest_and_report_the_boundary_when_the_ceiling_is_passed() {
    let ledger = SessionLedger::with_capacity(4);
    let events: Vec<_> = (0..6).map(numbered).collect();
    ledger.append(&events, &[]).expect("the append succeeds");

    let held = ledger
        .events(&EventQuery::default())
        .expect("the query succeeds");
    assert_eq!(held.len(), 4, "the ceiling is a ceiling");
    assert_eq!(
        held.first().map(|event| event.event_id.clone()),
        Some(events[2].event_id.clone()),
        "the oldest goes first"
    );
    assert_eq!(
        ledger
            .event(&events[0].event_id)
            .expect("the lookup succeeds"),
        None
    );

    let boundary = ledger
        .eviction_boundary()
        .expect("§10.7's ceiling reports what it dropped rather than dropping it silently");
    assert_eq!(boundary.reason, GapReason::RetentionExpired);
    assert_eq!(boundary.from, events[0].times.presentation_instant());
    assert_eq!(boundary.until, events[1].times.presentation_instant());
    assert_eq!(ledger.retention().evicted, 2);
}

#[test]
fn should_expose_the_eviction_as_coverage_when_the_ledger_is_asked_what_it_covers() {
    let ledger = SessionLedger::with_capacity(2);
    ledger
        .append(&(0..4).map(numbered).collect::<Vec<_>>(), &[])
        .expect("the append succeeds");
    let coverage = ledger
        .coverage(&ono_temporal_core::CoverageQuery::default())
        .expect("the query succeeds");
    assert!(
        coverage.iter().any(|interval| interval.completeness
            == ono_temporal_core::TemporalCompleteness::Unavailable),
        "an evicted interval composes into a gap rather than into silence: {coverage:?}"
    );
}

#[test]
fn should_return_the_events_in_the_requested_order_when_a_query_names_one() {
    let ledger = SessionLedger::new();
    ledger
        .append(&(0..3).map(numbered).collect::<Vec<_>>(), &[])
        .expect("the append succeeds");
    let descending = ledger
        .events(&EventQuery {
            order: QueryOrder::Descending,
            ..EventQuery::default()
        })
        .expect("the query succeeds");
    let instants: Vec<_> = descending
        .iter()
        .map(|event| event.times.presentation_instant())
        .collect();
    assert_eq!(
        instants,
        vec![
            instant("2026-08-31T12:00:02Z"),
            instant("2026-08-31T12:00:01Z"),
            instant("2026-08-31T12:00:00Z"),
        ]
    );
}

#[test]
fn should_narrow_to_the_window_when_a_query_names_a_range() {
    let ledger = SessionLedger::new();
    ledger
        .append(&(0..5).map(numbered).collect::<Vec<_>>(), &[])
        .expect("the append succeeds");
    let inside = ledger
        .events(&EventQuery {
            range: TimeRange::between(
                instant("2026-08-31T12:00:01Z"),
                instant("2026-08-31T12:00:03Z"),
            ),
            ..EventQuery::default()
        })
        .expect("the query succeeds");
    assert_eq!(inside.len(), 2, "the range is half-open: {inside:?}");
}

#[test]
fn should_resolve_a_shortened_reference_when_it_names_exactly_one_event() {
    let ledger = SessionLedger::new();
    let event = event(
        EventKind::ObjectChanged,
        "2026-08-31T12:00:00Z",
        "ono.recorder",
    );
    ledger
        .append(std::slice::from_ref(&event), &[])
        .expect("the append succeeds");
    let short = ono_temporal_core::EventId::parse(&event.event_id.as_str()[..5])
        .expect("a shortened reference parses");
    assert_eq!(
        ledger
            .event(&short)
            .expect("the lookup succeeds")
            .map(|found| found.event_id),
        Some(event.event_id),
        "§11.6's `@e42` is a reference a later command can use"
    );
}

#[test]
fn should_refuse_a_shortened_reference_when_it_names_more_than_one_event() {
    let ledger = SessionLedger::new();
    let events: Vec<_> = (0..64).map(numbered).collect();
    ledger.append(&events, &[]).expect("the append succeeds");

    // Sixty-four ids over sixteen first digits: some digit is shared, by pigeonhole.
    let mut shared: Vec<&str> = Vec::new();
    for event in &events {
        let head = &event.event_id.as_str()[..2];
        if events
            .iter()
            .filter(|other| other.event_id.as_str().starts_with(head))
            .count()
            > 1
        {
            shared.push(head);
        }
    }
    let head = shared.first().expect("some first digit is shared");
    let ambiguous = ono_temporal_core::EventId::parse(head).expect("a one-digit reference parses");
    let refused = ledger
        .event(&ambiguous)
        .expect_err("§34 E1306: a reference resolving to many events is ambiguous");
    assert_eq!(refused.code(), ErrorCode::TemporalAmbiguousEvent);
}

#[test]
fn should_report_what_it_holds_when_the_retention_state_is_read() {
    let ledger = SessionLedger::new();
    ledger
        .append(&(0..3).map(numbered).collect::<Vec<_>>(), &[])
        .expect("the append succeeds");
    let retention = ledger.retention();
    assert_eq!(retention.events, 3);
    assert_eq!(retention.earliest, Some(instant("2026-08-31T12:00:00Z")));
    assert_eq!(retention.latest, Some(instant("2026-08-31T12:00:02Z")));
    assert_eq!(retention.evicted, 0);
}
