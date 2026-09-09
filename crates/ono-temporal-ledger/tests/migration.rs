//! Versioned, forward-only migration (v0.5 §31.6, §44.3).
//!
//! §31.6: "store schema migrations MUST be versioned and tested against fixtures from every shipped
//! v0.5 store version", and "migration MUST preserve `EventId`, `EvidenceId`, `ActionId` and causal
//! references". The fixture is committed as bytes — `tests/fixtures/ledger-v1.sqlite3` — because a
//! fixture the test regenerates proves only that the code agrees with itself.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ono_temporal_core::{EventKind, EventQuery, LedgerRead, LedgerWrite, TimeRange};
use ono_temporal_ledger::{LedgerStore, STORE_VERSION};

use common::{event, path_in, store_in};

/// The committed version 1 store, copied where a test may open and migrate it.
fn v1_fixture(home: &Path) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("ledger-v1.sqlite3");
    let target = path_in(home);
    std::fs::create_dir_all(target.parent().expect("the store has a directory"))
        .expect("the directory is created");
    std::fs::copy(&source, &target).expect("the committed fixture is readable");
    target
}

/// Everything the fixture holds that a migration must not change, read with plain SQL so the
/// assertion does not depend on the code under test to answer it.
fn identities(path: &Path) -> (u32, BTreeSet<String>) {
    let connection = rusqlite::Connection::open(path).expect("the fixture opens");
    let version: String = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'store_version'",
            [],
            |row| row.get(0),
        )
        .expect("the fixture states its version");
    let mut ids = BTreeSet::new();
    for (sql, prefix) in [
        ("SELECT event_id FROM events", "event"),
        ("SELECT evidence_id FROM evidence", "evidence"),
        ("SELECT action_id FROM actions", "action"),
        ("SELECT link_id FROM causal_links", "link"),
        ("SELECT cause FROM causal_links", "cause"),
        ("SELECT effect FROM causal_links", "effect"),
        (
            "SELECT evidence_id FROM causal_link_evidence",
            "link_evidence",
        ),
        ("SELECT evidence_id FROM event_evidence", "event_evidence"),
        ("SELECT checkpoint_id FROM checkpoints", "checkpoint"),
        ("SELECT spatial_id FROM checkpoint_objects", "object"),
    ] {
        let mut statement = connection.prepare(sql).expect("the fixture holds the set");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("the set reads");
        for id in rows {
            ids.insert(format!("{prefix}:{}", id.expect("an identity")));
        }
    }
    (version.parse().expect("a numeric version"), ids)
}

#[test]
fn should_preserve_every_identity_when_a_version_one_store_is_migrated() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = v1_fixture(home.path());

    let (before_version, before) = identities(&path);
    assert_eq!(before_version, 1, "the committed fixture is a v1 store");
    assert!(
        !before.is_empty(),
        "a fixture with no identities proves nothing"
    );

    let store = LedgerStore::open(&path).expect("a v1 store migrates on open");
    assert_eq!(store.store_version(), STORE_VERSION);
    drop(store);

    let (after_version, after) = identities(&path);
    assert_eq!(after_version, STORE_VERSION);
    assert_eq!(
        after, before,
        "§31.6: migration preserves EventId, EvidenceId, ActionId and every causal reference"
    );
}

#[test]
fn should_answer_the_migrated_history_when_a_version_one_store_is_opened() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = v1_fixture(home.path());
    let store = LedgerStore::open(&path).expect("a v1 store migrates on open");

    let events = store
        .events(&EventQuery::in_range(TimeRange::all()))
        .expect("events answer");
    assert_eq!(events.len(), 3, "the fixture's history survived intact");

    let effect = events
        .iter()
        .find(|event| event.kind == EventKind::ObjectChanged)
        .expect("the fixture holds a changed event");
    let links = store.causal_links(&effect.event_id).expect("links answer");
    assert_eq!(links.len(), 1, "§31.6: causal references survive");
    assert_eq!(&links[0].effect, &effect.event_id);
    assert_eq!(
        store
            .evidence(&links[0].evidence)
            .expect("evidence answers")
            .len(),
        1,
        "the evidence a link cites is still reachable through it"
    );

    let actions = store.actions(TimeRange::all()).expect("actions answer");
    assert_eq!(actions.len(), 1);
    assert!(
        actions[0].command.is_redacted(),
        "§17.5: what was redacted before the migration is redacted after it"
    );
}

#[test]
fn should_keep_writing_when_a_migrated_store_is_appended_to() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = v1_fixture(home.path());
    let store = LedgerStore::open(&path).expect("a v1 store migrates on open");
    let fresh = event(
        EventKind::ObjectObserved,
        "2026-08-31T15:00:00Z",
        "linux.procfs",
    );
    let appended = store
        .append(std::slice::from_ref(&fresh), &[])
        .expect("§44.3: the store is migrated before new-version events are written");
    assert_eq!(appended.stored, 1);
    assert_eq!(
        store.event(&fresh.event_id).expect("the lookup answers"),
        Some(fresh)
    );
}

#[test]
fn should_create_a_store_at_the_current_version_when_there_was_none() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    assert_eq!(store.store_version(), STORE_VERSION);
}

#[test]
fn should_refuse_the_store_and_keep_the_shell_working_when_it_was_written_by_a_newer_ono() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = {
        let store = store_in(home.path());
        store.path().to_path_buf()
    };
    let connection = rusqlite::Connection::open(&path).expect("the store opens");
    connection
        .execute(
            "UPDATE metadata SET value = '99' WHERE key = 'store_version'",
            [],
        )
        .expect("the version is written");
    drop(connection);

    let refusal = LedgerStore::open(&path).expect_err("§44.3: a newer store is refused");
    assert_eq!(
        refusal.code(),
        ono_core::ErrorCode::TemporalStoreUnavailable
    );
    assert!(
        refusal
            .help()
            .is_some_and(|help| help.contains("without persistent history")),
        "§44.3: the diagnostic says the shell keeps working; it said {:?}",
        refusal.help()
    );
}
