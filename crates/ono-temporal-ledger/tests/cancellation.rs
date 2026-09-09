//! Abandoning a long read (v0.5 §32.6), and what the store looks like afterwards.
//!
//! §32.6: "Long historical queries MUST be cancellable using normal Ono cancellation semantics.
//! Ctrl-C MUST not corrupt the ledger or leave locks held." A query over a large ledger is one
//! synchronous scan, and while it runs the shell that asked for it is inside this call: unless the
//! scan itself asks, an interrupt cannot be answered until the answer nobody wants is complete.
//!
//! What is asserted here is what a caller can see: a scan that is told to stop refuses instead of
//! answering, a scan that is not told to stop answers in full, and the store is the same store
//! either way — still readable, still writable, still holding every event it held.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use ono_core::ErrorCode;
use ono_temporal_core::{EventKind, EventQuery, LedgerRead, LedgerWrite, TimeRange};

use common::{event_about, store_in};

/// Whether the watcher installed below is currently saying "stop".
///
/// `watch_for_cancellation` takes the first answer a process offers and keeps it, because a
/// process has one shell asking the question. A test binary has one too, so the switch is here
/// rather than in the watcher, and [`asking`] is what makes a test the only one holding it.
static CANCELLING: AtomicBool = AtomicBool::new(false);

/// Serialises the tests, which share one process-wide watcher.
static TURN: Mutex<()> = Mutex::new(());

/// Installs the watcher, takes the turn, and leaves the switch off for the next test.
struct Asking(
    #[expect(dead_code, reason = "held for the lifetime of the turn")] MutexGuard<'static, ()>,
);

fn asking() -> Asking {
    let turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ono_temporal_ledger::watch_for_cancellation(|| CANCELLING.load(Ordering::SeqCst));
    CANCELLING.store(false, Ordering::SeqCst);
    Asking(turn)
}

impl Drop for Asking {
    fn drop(&mut self) {
        CANCELLING.store(false, Ordering::SeqCst);
    }
}

/// How many events the fixture writes. More than one batch of the scan, so the question is asked
/// more than once and a cancelled scan is one that stopped part way rather than one that never
/// started.
const EVENTS: usize = 700;

/// A store holding [`EVENTS`] events, one per second.
fn history(home: &std::path::Path) -> ono_temporal_ledger::LedgerStore {
    let store = store_in(home);
    let events: Vec<_> = (0..EVENTS)
        .map(|index| {
            event_about(
                EventKind::ObjectChanged,
                &format!("2026-08-31T14:{:02}:{:02}Z", index / 60, index % 60),
                1827,
            )
        })
        .collect();
    store.append(&events, &[]).expect("an append succeeds");
    store.flush().expect("a flush succeeds");
    store
}

#[test]
fn should_refuse_the_answer_rather_than_finish_it_when_a_long_read_is_cancelled() {
    let _turn = asking();
    let home = tempfile::tempdir().expect("a temporary home");
    let store = history(home.path());

    CANCELLING.store(true, Ordering::SeqCst);
    let refused = store
        .events(&EventQuery::in_range(TimeRange::all()))
        .expect_err("v0.5 §32.6: a scan the shell has abandoned does not answer");
    assert_eq!(
        refused.code(),
        ErrorCode::StreamCancelled,
        "v0.5 §32.6, spec §18.5: an abandoned read is a cancellation and not a store failure, \
         got {refused:?}"
    );
}

#[test]
fn should_still_read_write_and_hold_everything_after_a_read_was_cancelled() {
    let _turn = asking();
    let home = tempfile::tempdir().expect("a temporary home");
    let store = history(home.path());

    CANCELLING.store(true, Ordering::SeqCst);
    let _ = store.events(&EventQuery::in_range(TimeRange::all()));
    CANCELLING.store(false, Ordering::SeqCst);

    // §32.6's second sentence, from the one side a caller can observe it: the store answers again,
    // it answers in full, and it still takes a write.
    let read = store
        .events(&EventQuery::in_range(TimeRange::all()))
        .expect("v0.5 §32.6: an interrupted query leaves the ledger readable");
    assert_eq!(
        read.len(),
        EVENTS,
        "v0.5 §32.6: the cancelled scan discarded its own partial answer and nothing else"
    );
    store
        .append(
            &[event_about(
                EventKind::ObjectAppeared,
                "2026-08-31T23:59:59Z",
                4242,
            )],
            &[],
        )
        .expect("v0.5 §32.6: an interrupted query leaves no lock held against the next writer");
    assert_eq!(
        store
            .events(&EventQuery::in_range(TimeRange::all()))
            .expect("the store still answers")
            .len(),
        EVENTS + 1,
        "the write after the cancelled read is in the history the next query sees"
    );
}

#[test]
fn should_read_to_the_end_when_nobody_is_asking_it_to_stop() {
    let _turn = asking();
    let home = tempfile::tempdir().expect("a temporary home");
    let store = history(home.path());

    // The watcher is installed for the whole binary, so "not cancelled" has to be a fact about
    // the answer rather than about whether anything is watching.
    let read = store
        .events(&EventQuery::in_range(TimeRange::all()))
        .expect("an uncancelled scan answers");
    assert_eq!(
        read.len(),
        EVENTS,
        "the cancellation check costs an ordinary query nothing but the question"
    );
}
