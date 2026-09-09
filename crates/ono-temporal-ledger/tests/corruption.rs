//! Corruption (v0.5 §31.7), and the shell that keeps working through it (§10.8, §44.3).
//!
//! §31.7 lists five obligations, and the tests here are one per obligation: refuse to present the
//! affected history as valid, identify the affected store or segment, preserve current shell
//! functionality, offer diagnostic guidance, and mark the discarded interval as a coverage gap.
//!
//! `PRAGMA integrity_check` is the cheap half. The interesting half is a file SQLite still opens:
//! a payload someone has scribbled on decodes to nothing, and that has to fail its row rather than
//! the process.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::io::{Seek as _, SeekFrom, Write as _};

use ono_core::ErrorCode;
use ono_temporal_core::{EventKind, EventQuery, GapReason, LedgerRead, LedgerWrite, TimeRange};
use ono_temporal_ledger::LedgerStore;

use common::{event, event_about, path_in, store_in};

/// Writes three events and returns the store's path, closed.
fn history(home: &std::path::Path) -> std::path::PathBuf {
    let store = store_in(home);
    store
        .append(
            &[
                event_about(EventKind::ObjectAppeared, "2026-08-31T14:00:00Z", 1827),
                event_about(EventKind::ObjectChanged, "2026-08-31T14:00:01Z", 1827),
                event_about(EventKind::ObjectDisappeared, "2026-08-31T14:00:02Z", 1827),
            ],
            &[],
        )
        .expect("an append succeeds");
    store.flush().expect("a flush succeeds");
    store.path().to_path_buf()
}

#[test]
fn should_refuse_the_store_and_name_it_when_the_file_is_scribbled_on_beyond_repair() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = history(home.path());
    {
        // Overwrite the page after the header with rubbish. SQLite's own integrity check finds it.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("the store is writable");
        file.seek(SeekFrom::Start(4_096))
            .expect("the second page is reachable");
        file.write_all(&[0x5a; 8_192]).expect("the scribble lands");
        file.sync_all().expect("the scribble is durable");
    }

    let refusal = LedgerStore::open(&path).expect_err("§31.7: damaged history is refused");
    assert_eq!(refusal.code(), ErrorCode::TemporalStoreCorrupt);
    assert!(
        refusal
            .field("metadata")
            .is_some_and(|metadata| format!("{metadata:?}").contains("ledger.sqlite3")),
        "§31.7's second obligation: the affected store is identified"
    );
    assert!(
        refusal
            .help()
            .is_some_and(|help| help.contains("remove temporal-history")),
        "§31.7's fourth obligation: guidance is offered; it said {:?}",
        refusal.help()
    );
}

#[test]
fn should_keep_answering_and_report_a_gap_when_one_stored_payload_will_not_decode() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = history(home.path());
    {
        // Replace one event's payload with bytes that are not CBOR at all. SQLite is content;
        // the decoder is not.
        let connection = rusqlite::Connection::open(&path).expect("the store opens");
        let victim: String = connection
            .query_row(
                "SELECT event_id FROM events ORDER BY presentation_nanos ASC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .expect("the store holds an event");
        connection
            .execute(
                "UPDATE events SET body = ?1 WHERE event_id = ?2",
                rusqlite::params![vec![0xff_u8; 32], victim],
            )
            .expect("the scribble lands");
    }

    let store = LedgerStore::open(&path).expect("§31.7's third obligation: the store still opens");
    let events = store
        .events(&EventQuery::in_range(TimeRange::all()))
        .expect("the query still answers");
    assert_eq!(
        events.len(),
        2,
        "§31.7's first obligation: the damaged row is refused rather than rendered"
    );

    let report = store.integrity();
    assert!(!report.is_healthy());
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.segment.starts_with("events:")),
        "§31.7's second obligation: the affected segment is identified; findings were {:?}",
        report.findings
    );
    assert!(!report.is_fatal(), "one bad row is not a dead store");
    assert!(report.guidance().contains("temporal ledger"));

    let gaps = store.gaps().expect("gaps answer");
    assert_eq!(
        gaps.len(),
        1,
        "§31.7's fifth obligation: the discarded interval is a coverage gap"
    );
    assert_eq!(gaps[0].reason, GapReason::CorruptSegment);
    assert_eq!(
        gaps[0].from,
        common::instant("2026-08-31T14:00:00Z"),
        "the gap covers the interval the refused row covered"
    );
}

#[test]
fn should_keep_the_store_writable_when_one_row_has_been_refused() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = history(home.path());
    {
        let connection = rusqlite::Connection::open(&path).expect("the store opens");
        connection
            .execute(
                "UPDATE events SET body = ?1 WHERE presentation_nanos = \
                    (SELECT MIN(presentation_nanos) FROM events)",
                rusqlite::params![vec![0x00_u8; 4]],
            )
            .expect("the scribble lands");
    }
    let store = LedgerStore::open(&path).expect("the store opens");
    let fresh = event(
        EventKind::ObjectObserved,
        "2026-08-31T15:00:00Z",
        "linux.procfs",
    );
    assert_eq!(
        store
            .append(std::slice::from_ref(&fresh), &[])
            .expect("§10.8: a crash must not leave the ledger unrecoverable")
            .stored,
        1
    );
    assert_eq!(
        store.event(&fresh.event_id).expect("the lookup answers"),
        Some(fresh)
    );
}

#[test]
fn should_report_a_healthy_store_when_nothing_is_wrong_with_it() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = history(home.path());
    let store = LedgerStore::open(&path).expect("the store opens");
    store
        .events(&EventQuery::in_range(TimeRange::all()))
        .expect("events answer");
    let report = store.integrity();
    assert!(report.is_healthy());
    assert!(report.gaps.is_empty());
    assert!(report.guidance().contains("passed its integrity check"));
}

#[test]
fn should_refuse_a_truncated_file_rather_than_answer_from_half_of_it() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = history(home.path());
    let full = std::fs::metadata(&path).expect("the store exists").len();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("the store is writable");
    file.set_len(full / 3).expect("the truncation lands");
    file.sync_all().expect("the truncation is durable");
    drop(file);

    match LedgerStore::open(&path) {
        Err(refusal) => assert!(
            matches!(
                refusal.code(),
                ErrorCode::TemporalStoreCorrupt | ErrorCode::TemporalStoreUnavailable
            ),
            "§31.7: a truncated store is refused, not half-answered; it said {refusal:?}"
        ),
        Ok(store) => {
            let held = store
                .events(&EventQuery::in_range(TimeRange::all()))
                .map(|events| events.len())
                .unwrap_or_default();
            assert!(
                held < 3 || !store.integrity().is_healthy(),
                "a store missing two thirds of its bytes may not report a whole history"
            );
        }
    }
}

#[test]
fn should_open_a_fresh_store_when_the_damaged_one_has_been_removed() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = history(home.path());
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("the store is writable");
        file.seek(SeekFrom::Start(4_096))
            .expect("the second page is reachable");
        file.write_all(&[0x5a; 8_192]).expect("the scribble lands");
    }
    assert!(LedgerStore::open(&path).is_err());

    // §31.7's guidance, carried out: the damaged store goes and recording starts again.
    std::fs::remove_file(&path).expect("the damaged store is removable");
    let store = LedgerStore::open(&path_in(home.path())).expect("a fresh store opens");
    assert!(store.integrity().is_healthy());
    assert_eq!(store.retention().events, 0);
}
