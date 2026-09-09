//! Privacy (v0.5 §30.3, §17.5): a secret never reaches the ledger's bytes.
//!
//! §30.3: "values typed as `Secret` MUST be redacted before persistence." §17.5 makes the redaction
//! semantic and puts it in front of the ledger type rather than in front of the renderer, so the
//! test is a byte scan of the file rather than an inspection of what a command prints. What is not
//! in the file cannot leak from the file.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::sync::Arc;

use ono_temporal_core::LedgerRead as _;
use ono_temporal_core::{
    EventKind, LedgerWrite, REDACTED, Redactable, RedactedCommandSummary, TimeRange,
};

use common::{action, seed, store_in, times};

/// A password no other part of the fixture could produce by accident.
const SECRET: &str = "correct-horse-battery-staple-8f2a1c";

/// Whether `needle` appears anywhere in the store's bytes, including its write-ahead log.
fn appears_in_store(directory: &std::path::Path, needle: &str) -> bool {
    let needle = needle.as_bytes();
    let mut found = false;
    for entry in std::fs::read_dir(directory).expect("the directory reads") {
        let entry = entry.expect("an entry");
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        if bytes.windows(needle.len()).any(|window| window == needle) {
            found = true;
        }
    }
    found
}

#[test]
fn should_hold_no_plaintext_secret_when_a_redacted_command_has_been_recorded() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let summary = RedactedCommandSummary::of(
        "set",
        Some("credential"),
        &[
            Redactable::plain("registry"),
            Redactable::secret("--token", SECRET),
            Redactable::option("--password", SECRET),
        ],
    );
    assert!(
        !summary.as_str().contains(SECRET),
        "§17.5 redacts before the ledger type exists"
    );

    store
        .record_action(&action("2026-08-31T14:03:11Z", summary))
        .expect("an action records");
    store.flush().expect("a flush succeeds");

    let directory = store.path().parent().expect("a directory");
    assert!(
        !appears_in_store(directory, SECRET),
        "§30.3: a secret MUST NOT reach persistence"
    );

    let recorded = store.actions(TimeRange::all()).expect("actions answer");
    assert_eq!(recorded.len(), 1);
    assert!(recorded[0].command.is_redacted());
    assert!(
        recorded[0].command.as_str().contains(REDACTED),
        "a reader is told a hole is there rather than shown a shortened command"
    );
}

#[test]
fn should_hold_no_plaintext_secret_when_an_event_payload_carried_a_redacted_field() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let mut carried = seed(
        EventKind::ProviderEvent,
        times("2026-08-31T14:03:11Z"),
        "linux.systemd-dbus",
    );
    // The recorder redacts on the way in; what reaches the ledger is already the placeholder.
    let mut body = ono_value::MapValue::new();
    body.insert(
        Arc::from("unit"),
        ono_value::Value::string("registry.service"),
    );
    body.insert(Arc::from("password"), ono_value::Value::string(REDACTED));
    carried.payload = Some(ono_value::Value::Map(Arc::new(body)));

    store
        .append(&[carried.seal()], &[])
        .expect("an append succeeds");
    store.flush().expect("a flush succeeds");

    let directory = store.path().parent().expect("a directory");
    assert!(!appears_in_store(directory, SECRET));
    assert!(
        appears_in_store(directory, "registry.service"),
        "the fixture has to reach the file for its absence to mean anything"
    );
}

#[test]
fn should_hold_the_secret_only_where_a_caller_puts_it_unredacted() {
    // The store persists what it is given; §30.3's boundary is the recorder's, and this test says
    // so out loud so nobody mistakes the ledger for a redaction engine it is not.
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let unredacted = RedactedCommandSummary::of("set", None, &[Redactable::plain("harmless")]);
    store
        .record_action(&action("2026-08-31T14:03:11Z", unredacted))
        .expect("an action records");
    store.flush().expect("a flush succeeds");
    assert!(appears_in_store(
        store.path().parent().expect("a directory"),
        "harmless"
    ));
}
