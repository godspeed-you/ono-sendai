//! Deduplication (v0.5 §6.8) and its idempotence (§47.2).
//!
//! §6.8: "the ledger MUST support deduplication where sources provide stable sequence IDs or event
//! IDs", and "deduplication MUST NOT collapse two distinct source events merely because their
//! rendered text is equal". Identity is a content digest, so the first property is the primary key
//! and the second is a consequence of what the digest covers: two events that render identically
//! and differ in a source sequence, an instant or a clock domain are two identities.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_temporal_core::{EventKind, EventQuery, LedgerRead, LedgerWrite, TimeRange};

use common::{event, seed, sequenced, store_in, times};

#[test]
fn should_store_a_batch_once_when_the_same_batch_is_appended_twice() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let batch: Vec<_> = (0..25)
        .map(|second| {
            event(
                EventKind::ObjectObserved,
                &format!("2026-08-31T14:00:{second:02}Z"),
                "linux.procfs",
            )
        })
        .collect();

    let first = store.append(&batch, &[]).expect("an append succeeds");
    let second = store.append(&batch, &[]).expect("a second append succeeds");

    assert_eq!(first.stored, 25);
    assert_eq!(first.duplicates, 0);
    assert_eq!(second.stored, 0, "§6.8: the same observation is one event");
    assert_eq!(second.duplicates, 25);
    assert_eq!(
        store
            .events(&EventQuery::in_range(TimeRange::all()))
            .expect("events answer")
            .len(),
        25
    );
}

/// §47.2: "event deduplication is idempotent." The property is over batches of every shape a
/// caller can hand in, including a batch that repeats an event inside itself.
#[test]
fn should_reach_the_same_ledger_whatever_order_and_repetition_a_batch_arrives_in() {
    for repeats in 1..=4_usize {
        for chunk in [1_usize, 3, 7, 25] {
            let home = tempfile::tempdir().expect("a temporary home");
            let store = store_in(home.path());
            let batch: Vec<_> = (0..25)
                .map(|second| {
                    event(
                        EventKind::ObjectObserved,
                        &format!("2026-08-31T14:00:{second:02}Z"),
                        "linux.procfs",
                    )
                })
                .collect();

            let mut stored = 0;
            let mut duplicates = 0;
            for _ in 0..repeats {
                for window in batch.chunks(chunk) {
                    let appended = store.append(window, &[]).expect("an append succeeds");
                    stored += appended.stored;
                    duplicates += appended.duplicates;
                }
            }
            let held = store
                .events(&EventQuery::in_range(TimeRange::all()))
                .expect("events answer");
            assert_eq!(
                held.len(),
                25,
                "repeating a batch {repeats} times in chunks of {chunk} must store it once"
            );
            assert_eq!(stored, 25, "only the first pass stores anything");
            assert_eq!(duplicates, 25 * (repeats - 1));
            assert_eq!(held, batch, "the ledger holds exactly what was appended");
        }
    }
}

#[test]
fn should_report_a_repetition_inside_one_batch_as_a_duplicate() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let once = event(
        EventKind::ObjectChanged,
        "2026-08-31T14:00:00Z",
        "linux.procfs",
    );
    let appended = store
        .append(&[once.clone(), once.clone(), once], &[])
        .expect("an append succeeds");
    assert_eq!(appended.stored, 1);
    assert_eq!(appended.duplicates, 2);
}

/// §6.8's prohibition, stated as an outcome: two events a renderer would draw identically stay two
/// events, because the source distinguished them and the identity carries what the source said.
#[test]
fn should_keep_two_events_apart_when_only_their_source_sequence_differs() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let first = seed(
        EventKind::ProviderEvent,
        sequenced("2026-08-31T14:00:00Z", 41),
        "linux.netlink",
    )
    .seal();
    let second = seed(
        EventKind::ProviderEvent,
        sequenced("2026-08-31T14:00:00Z", 42),
        "linux.netlink",
    )
    .seal();
    assert_ne!(
        first.event_id, second.event_id,
        "§6.8: a distinct source event is a distinct identity"
    );

    let appended = store
        .append(&[first, second], &[])
        .expect("an append succeeds");
    assert_eq!(appended.stored, 2);
    assert_eq!(appended.duplicates, 0);
}

#[test]
fn should_keep_two_events_apart_when_only_their_clock_domain_differs() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let mut here = seed(
        EventKind::ObjectChanged,
        times("2026-08-31T14:00:00Z"),
        "linux.procfs",
    );
    here.times.domain = common::domain("testbox", Some("boot-a"));
    let mut there = seed(
        EventKind::ObjectChanged,
        times("2026-08-31T14:00:00Z"),
        "linux.procfs",
    );
    there.times.domain = common::domain("testbox", Some("boot-b"));

    let appended = store
        .append(&[here.seal(), there.seal()], &[])
        .expect("an append succeeds");
    assert_eq!(
        appended.stored, 2,
        "§25.5: a reboot is a new clock domain, and the same reading in two of them is two facts"
    );
}

#[test]
fn should_store_evidence_once_when_two_events_cite_the_same_observation() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let evidence = common::evidence_for("2026-08-31T14:00:00Z", 1827, "active");
    store
        .append(
            &[event(
                EventKind::ObjectChanged,
                "2026-08-31T14:00:00Z",
                "linux.procfs",
            )],
            &[evidence.clone(), evidence.clone()],
        )
        .expect("an append succeeds");
    store
        .append(&[], std::slice::from_ref(&evidence))
        .expect("a second append succeeds");
    assert_eq!(
        store
            .evidence(std::slice::from_ref(&evidence.evidence_id))
            .expect("evidence answers")
            .len(),
        1
    );
}
