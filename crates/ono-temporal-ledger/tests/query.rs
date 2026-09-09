//! What `EventQuery` means against a persistent store (v0.5 §11.2, §20.3, §32.3).
//!
//! The scope, the subjects, the kinds, the window, the limit and the order are all in the
//! statement rather than in a filter afterwards, so a bounded question over a large ledger reads
//! the rows it answers with. The tests here assert the answers; `scale.rs` asserts that a bounded
//! question stays bounded.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_core::ErrorCode;
use ono_temporal_core::{
    EventId, EventKind, EventQuery, LedgerRead, LedgerWrite, QueryOrder, TimeRange,
};

use common::{event, event_about, instant, nested_scope, scope, store_in, subject};

#[test]
fn should_answer_only_the_window_when_a_range_is_given() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    store
        .append(
            &[
                event(
                    EventKind::ObjectObserved,
                    "2026-08-31T13:00:00Z",
                    "linux.procfs",
                ),
                event(
                    EventKind::ObjectObserved,
                    "2026-08-31T14:00:00Z",
                    "linux.procfs",
                ),
                event(
                    EventKind::ObjectObserved,
                    "2026-08-31T15:00:00Z",
                    "linux.procfs",
                ),
            ],
            &[],
        )
        .expect("an append succeeds");

    let found = store
        .events(&EventQuery::in_range(TimeRange::between(
            instant("2026-08-31T13:30:00Z"),
            instant("2026-08-31T15:00:00Z"),
        )))
        .expect("events answer");
    assert_eq!(
        found.len(),
        1,
        "the range is half-open, so 15:00 is outside"
    );
    assert_eq!(
        found[0].times.presentation_instant(),
        instant("2026-08-31T14:00:00Z")
    );
}

#[test]
fn should_answer_only_the_place_when_a_scope_is_given() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let mut inner = common::seed(
        EventKind::ObjectObserved,
        common::times("2026-08-31T14:00:00Z"),
        "linux.procfs",
    );
    inner.scope = nested_scope();
    let inner = inner.seal();
    let outer = event(
        EventKind::ObjectObserved,
        "2026-08-31T14:00:01Z",
        "linux.procfs",
    );
    store
        .append(&[inner.clone(), outer.clone()], &[])
        .expect("an append succeeds");

    let in_container = store
        .events(&EventQuery {
            scope: Some(nested_scope()),
            ..EventQuery::in_range(TimeRange::all())
        })
        .expect("events answer");
    assert_eq!(in_container, vec![inner.clone()]);

    let on_host = store
        .events(&EventQuery {
            scope: Some(scope()),
            ..EventQuery::in_range(TimeRange::all())
        })
        .expect("events answer");
    assert_eq!(
        on_host.len(),
        2,
        "§3.2: a host scope contains everything nested inside it"
    );
}

#[test]
fn should_answer_only_the_named_kinds_and_subjects_when_a_filter_is_given() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    store
        .append(
            &[
                event_about(EventKind::ObjectAppeared, "2026-08-31T14:00:00Z", 1827),
                event_about(EventKind::ObjectChanged, "2026-08-31T14:00:01Z", 1827),
                event_about(EventKind::ObjectChanged, "2026-08-31T14:00:02Z", 1828),
            ],
            &[],
        )
        .expect("an append succeeds");

    let changed = store
        .events(&EventQuery {
            kinds: vec![EventKind::ObjectChanged],
            ..EventQuery::in_range(TimeRange::all())
        })
        .expect("events answer");
    assert_eq!(changed.len(), 2);

    let about = store
        .events(&EventQuery {
            subjects: vec![subject(1827)],
            ..EventQuery::in_range(TimeRange::all())
        })
        .expect("events answer");
    assert_eq!(about.len(), 2);

    let both = store
        .events(&EventQuery {
            kinds: vec![EventKind::ObjectChanged],
            subjects: vec![subject(1827)],
            ..EventQuery::in_range(TimeRange::all())
        })
        .expect("events answer");
    assert_eq!(both.len(), 1);
}

#[test]
fn should_answer_newest_first_and_stop_at_the_limit_when_the_query_asks_for_it() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    store
        .append(
            &[
                event(
                    EventKind::ObjectObserved,
                    "2026-08-31T14:00:00Z",
                    "linux.procfs",
                ),
                event(
                    EventKind::ObjectObserved,
                    "2026-08-31T14:00:01Z",
                    "linux.procfs",
                ),
                event(
                    EventKind::ObjectObserved,
                    "2026-08-31T14:00:02Z",
                    "linux.procfs",
                ),
            ],
            &[],
        )
        .expect("an append succeeds");

    let newest = store
        .events(&EventQuery {
            order: QueryOrder::Descending,
            limit: Some(2),
            ..EventQuery::in_range(TimeRange::all())
        })
        .expect("events answer");
    assert_eq!(newest.len(), 2);
    assert_eq!(
        newest[0].times.presentation_instant(),
        instant("2026-08-31T14:00:02Z")
    );
    assert_eq!(
        newest[1].times.presentation_instant(),
        instant("2026-08-31T14:00:01Z")
    );
}

#[test]
fn should_resolve_a_shortened_reference_when_it_names_one_event() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let held = event(
        EventKind::ObjectChanged,
        "2026-08-31T14:00:00Z",
        "linux.procfs",
    );
    store
        .append(std::slice::from_ref(&held), &[])
        .expect("an append succeeds");

    let short = EventId::parse(&held.event_id.as_str()[..6]).expect("a prefix is a reference");
    assert_eq!(
        store.event(&short).expect("the lookup answers"),
        Some(held.clone())
    );
    assert_eq!(
        store.event(&held.event_id).expect("the lookup answers"),
        Some(held)
    );
}

#[test]
fn should_refuse_an_ambiguous_reference_when_it_names_more_than_one_event() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    for minute in 0..40 {
        store
            .append(
                &[event(
                    EventKind::ObjectObserved,
                    &format!("2026-08-31T14:{minute:02}:00Z"),
                    "linux.procfs",
                )],
                &[],
            )
            .expect("an append succeeds");
    }
    // "@e" is the shortest reference there is, and it names every event in the store.
    let everything = EventId::parse("@e0").or_else(|| EventId::parse("@e1"));
    let Some(everything) = everything else {
        panic!("one of the two shortest references parses");
    };
    let mut ambiguous = false;
    for prefix in [
        "@e0", "@e1", "@e2", "@e3", "@e4", "@e5", "@e6", "@e7", "@e8", "@e9",
    ] {
        let Some(reference) = EventId::parse(prefix) else {
            continue;
        };
        if let Err(refusal) = store.event(&reference) {
            assert_eq!(refusal.code(), ErrorCode::TemporalAmbiguousEvent);
            ambiguous = true;
            break;
        }
    }
    assert!(
        ambiguous,
        "§11.6: forty events share a one-digit prefix, and the reference must refuse rather than \
         pick one; the widest reference tried was {everything}"
    );
}

#[test]
fn should_answer_nothing_when_a_reference_names_no_event() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let missing = EventId::parse("@eaaaaaaaaaaaaaaaaaaaaaaa").expect("a well-formed reference");
    assert_eq!(store.event(&missing).expect("the lookup answers"), None);
}

#[test]
fn should_shorten_an_identity_only_as_far_as_the_store_can_still_tell_it_apart() {
    // §11.6's references are prefixes of the identity, so whoever prints one has to know where
    // this store starts calling a prefix ambiguous. The answer is the inverse of `event`: a
    // prefix of the length returned resolves to exactly one event, and a shorter one does not.
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let held: Vec<_> = (0..400)
        .map(|pid| event_about(EventKind::ObjectObserved, "2026-08-31T13:00:00Z", pid))
        .collect();
    store.append(&held, &[]).expect("an append succeeds");

    let ids: Vec<EventId> = held.iter().map(|event| event.event_id.clone()).collect();
    let lengths = store
        .shortest_unique_prefixes(&ids, 3)
        .expect("the store answers");
    assert_eq!(
        lengths.len(),
        ids.len(),
        "one answer per identity asked for"
    );

    let mut shortened = 0_usize;
    for (id, length) in ids.iter().zip(lengths) {
        assert!(length >= 3, "no answer is shorter than §11.6's own `@e42`");
        let prefix = EventId::parse(&id.as_str()[..length]).expect("a prefix is a reference");
        assert_eq!(
            store
                .event(&prefix)
                .expect("a unique prefix is not ambiguous")
                .map(|event| event.event_id),
            Some(id.clone()),
            "a prefix of the length the store named resolves to exactly one event"
        );
        if length > 3 {
            shortened += 1;
            let shorter = EventId::parse(&id.as_str()[..length - 1]).expect("a shorter prefix");
            assert_eq!(
                store.event(&shorter).map(|_| ()).unwrap_err().code(),
                ErrorCode::TemporalAmbiguousEvent,
                "the answer is the shortest such prefix, so one character less is ambiguous"
            );
        }
    }
    assert!(
        shortened > 0,
        "400 events do not fit in 256 two-digit prefixes, so some are spelled longer"
    );
}

#[test]
fn should_spell_a_lonely_identity_at_the_shortest_length_a_caller_would_print() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let only = event(
        EventKind::ObjectAppeared,
        "2026-08-31T13:00:00Z",
        "linux.procfs",
    );
    store
        .append(std::slice::from_ref(&only), &[])
        .expect("an append succeeds");

    assert_eq!(
        store
            .shortest_unique_prefixes(std::slice::from_ref(&only.event_id), 3)
            .expect("the store answers"),
        vec![3],
        "§11.6 keeps `@e42` where nothing else in the ledger answers to it"
    );
    assert!(
        store
            .shortest_unique_prefixes(&[], 3)
            .expect("the store answers")
            .is_empty(),
        "asking about nothing reads nothing"
    );
}
