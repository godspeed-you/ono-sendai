//! Property tests of v0.5 §47.2: "serialization round-trips of the canonical schemas",
//! deduplication idempotence, and ordering stability under a wall-clock jump where a source
//! sequence exists (§25.4).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_temporal_core::{
    EventId, EventKind, EventQuery, LedgerRead, LedgerWrite, OrderEvidence, Ordering,
    SessionLedger, happens_before, presentation_order, value,
};
use ono_testkit::Rng;

use common::{instant, seed, times};

fn kind_of(index: u64) -> EventKind {
    EventKind::ALL[usize::try_from(index).unwrap_or(0) % EventKind::ALL.len()]
}

#[test]
fn should_round_trip_through_its_schema_when_any_event_becomes_a_record() {
    let mut rng = Rng::seeded(0x0005_5e11_0da1_u64);
    for _ in 0..256 {
        let sequence = rng.next_u64();
        let mut seed = seed(
            kind_of(sequence),
            times("2026-08-31T12:00:00Z"),
            "linux.procfs",
        );
        seed.times.source_sequence = Some(sequence);
        seed.times.monotonic_nanos = Some(sequence);
        let event = seed.seal();
        let record = value::event_record(&event).expect("an event becomes a record");
        record
            .validate()
            .unwrap_or_else(|error| panic!("the record must validate: {}", error.message()));
        assert_eq!(
            record
                .get("event_id")
                .and_then(|value| match value {
                    ono_value::Value::String(text) => EventId::parse(text),
                    _ => None,
                })
                .as_ref(),
            Some(&event.event_id),
            "the identity survives the round trip through the schema"
        );
    }
}

#[test]
fn should_stay_at_one_stored_event_when_the_same_observation_is_appended_again_and_again() {
    let ledger = SessionLedger::new();
    let event = common::event(
        EventKind::ObjectObserved,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
    );
    let mut duplicates = 0;
    for _ in 0..64 {
        duplicates += ledger
            .append(std::slice::from_ref(&event), &[])
            .expect("the append succeeds")
            .duplicates;
    }
    assert_eq!(
        duplicates, 63,
        "every append after the first is a duplicate"
    );
    assert_eq!(
        ledger
            .events(&EventQuery::default())
            .expect("the query succeeds")
            .len(),
        1,
        "§6.8: deduplication is idempotent"
    );
}

#[test]
fn should_keep_the_sequence_order_when_the_wall_clock_jumps_backwards() {
    let mut rng = Rng::seeded(0x5eed_1eaf_u64);
    for _ in 0..128 {
        let jump = i64::try_from(rng.below(7_200)).unwrap_or(0) - 3_600;
        let mut first = seed(
            EventKind::ProviderEvent,
            times("2026-08-31T12:00:00Z"),
            "linux.netlink",
        );
        first.times.source_sequence = Some(41);
        let mut second = seed(
            EventKind::ProviderEvent,
            times("2026-08-31T12:00:00Z"),
            "linux.netlink",
        );
        second.times.source_sequence = Some(42);
        second.times.observed_at = instant("2026-08-31T12:00:00Z")
            .checked_add(jiff::Span::new().seconds(jump))
            .expect("the fixture instant stays in range");
        assert_eq!(
            happens_before(&first.seal(), &second.seal()),
            (Ordering::Before, Some(OrderEvidence::SourceSequence)),
            "§25.4: an NTP correction MUST NOT reorder events inside a source sequence"
        );
    }
}

#[test]
fn should_produce_the_same_display_order_whatever_order_the_events_arrived_in() {
    let mut rng = Rng::seeded(0xf00d_cafe_u64);
    let events: Vec<_> = (0..32)
        .map(|index| {
            let mut seed = seed(
                kind_of(index),
                times("2026-08-31T12:00:00Z"),
                "linux.netlink",
            );
            seed.times.observed_at = instant("2026-08-31T12:00:00Z")
                .checked_add(jiff::Span::new().seconds(i64::try_from(rng.below(16)).unwrap_or(0)))
                .expect("the fixture instant stays in range");
            seed.times.source_sequence = Some(index);
            seed.seal()
        })
        .collect();

    let mut ordered = events.clone();
    presentation_order(&mut ordered);
    let expected: Vec<_> = ordered.iter().map(|event| event.event_id.clone()).collect();

    for _ in 0..16 {
        let mut shuffled = events.clone();
        for index in (1..shuffled.len()).rev() {
            let other = rng.below(index + 1);
            shuffled.swap(index, other);
        }
        presentation_order(&mut shuffled);
        assert_eq!(
            shuffled
                .iter()
                .map(|event| event.event_id.clone())
                .collect::<Vec<_>>(),
            expected,
            "§26.3: a stable display order is stable"
        );
    }
}

#[test]
fn should_never_claim_an_order_from_wall_time_alone_whatever_the_instants_are() {
    let mut rng = Rng::seeded(0x1234_5678_u64);
    for _ in 0..256 {
        let apart = i64::try_from(rng.below(86_400)).unwrap_or(0);
        let mut first = seed(
            EventKind::ObjectObserved,
            times("2026-08-31T00:00:00Z"),
            "linux.procfs",
        );
        first.times.domain = common::domain("web01", Some("boot-a"));
        let mut second = seed(
            EventKind::ObjectObserved,
            times("2026-08-31T00:00:00Z"),
            "linux.procfs",
        );
        second.times.domain = common::domain("db01", Some("boot-b"));
        second.times.observed_at = instant("2026-08-31T00:00:00Z")
            .checked_add(jiff::Span::new().seconds(apart))
            .expect("the fixture instant stays in range");
        assert_eq!(
            happens_before(&first.seal(), &second.seal()),
            (Ordering::Concurrent, None),
            "§26.1: two clock domains with no shared ordering evidence are concurrent"
        );
    }
}
