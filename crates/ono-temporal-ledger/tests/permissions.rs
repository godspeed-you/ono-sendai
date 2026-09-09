//! v0.5 §30.2: the ledger directory is `0700` and the database `0600`, checked against the real
//! filesystem metadata rather than against the code that set it.
//!
//! §30.1 is the reason the check is on creation rather than afterwards: a file that was briefly
//! world-readable was world-readable, so the modes are what the file is made with.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::os::unix::fs::PermissionsExt;

use ono_temporal_core::LedgerWrite as _;

use common::{path_in, store_in};

#[test]
fn should_create_a_private_directory_and_database_when_the_store_is_opened() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let path = store.path().to_path_buf();

    let directory = path.parent().expect("the database sits in a directory");
    let directory_mode = std::fs::metadata(directory)
        .expect("the directory exists")
        .permissions()
        .mode()
        & 0o7777;
    let file_mode = std::fs::metadata(&path)
        .expect("the database exists")
        .permissions()
        .mode()
        & 0o7777;

    assert_eq!(
        directory_mode, 0o700,
        "§30.2: `~/.local/share/ono/temporal/` is 0700"
    );
    assert_eq!(file_mode, 0o600, "§30.2: `ledger.sqlite3` is 0600");
}

#[test]
fn should_place_the_store_at_the_canonical_path_when_a_home_is_resolved() {
    let home = tempfile::tempdir().expect("a temporary home");
    let path = path_in(home.path());
    assert_eq!(
        path,
        home.path()
            .join(".local")
            .join("share")
            .join("ono")
            .join("temporal")
            .join("ledger.sqlite3"),
        "§31.1 fixes the canonical path"
    );
}

#[test]
fn should_prefer_the_data_home_when_the_environment_names_one() {
    let home = tempfile::tempdir().expect("a temporary home");
    let data = home.path().join("elsewhere");
    let resolved = ono_temporal_ledger::ledger_path(
        |name| (name == "XDG_DATA_HOME").then(|| data.display().to_string()),
        Some(home.path()),
    )
    .expect("an explicit data home resolves");
    assert_eq!(
        resolved,
        data.join("ono").join("temporal").join("ledger.sqlite3")
    );
}

#[test]
fn should_keep_the_write_ahead_log_private_when_events_have_been_appended() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    store
        .append(
            &[common::event(
                ono_temporal_core::EventKind::ObjectObserved,
                "2026-08-31T14:03:13Z",
                "linux.procfs",
            )],
            &[],
        )
        .expect("an append succeeds");

    let directory = store.path().parent().expect("a directory").to_path_buf();
    for entry in std::fs::read_dir(&directory).expect("the directory reads") {
        let entry = entry.expect("an entry");
        let mode = entry.metadata().expect("metadata").permissions().mode() & 0o077;
        assert_eq!(
            mode,
            0,
            "§30.2 makes the ledger user-private; {} was group- or world-readable",
            entry.path().display()
        );
    }
}
