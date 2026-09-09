//! The recorder as a user meets it: off, opt-in, and removable (v0.5 §10.2, §30.8, §2.16).
//!
//! §10.2 is a MUST with a filesystem consequence: "Persistent recording MUST be disabled by
//! default", and §32.1 adds that storage initialization is lazy while it is off. Together they
//! make the strongest observation in this suite a *negative* one — after a fresh installation has
//! run, looked at processes and asked what it remembers, there is no store on disk. §2's
//! sixteenth invariant states the same rule as an invariant of the whole tranche.
//!
//! §30.8 is the other end of the same principle: what was retained can be destroyed, through the
//! existing destructive-operation policy rather than a temporal-specific one, so `remove
//! temporal-history` refuses without confirmation, says what it would destroy when asked to
//! rehearse, and leaves nothing behind when confirmed.
//!
//! Every test drives the real binary and asserts on what it printed and what is on disk
//! afterwards (AGENTS.md §11). Nothing here reaches into the ledger's internals: a store that
//! exists is a file that exists.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::path::PathBuf;
use std::time::Duration;

use ono_testkit::{Scratch, Shell, scratch};
use serde_yaml_ng::Value;

/// A shell whose whole world is `home` and which is told nothing: the state §10.2 describes.
///
/// It is [`support::recording_shell`]'s opposite number, and it exists for the same reason that
/// one does — a test about the default must not inherit the environment of the person running
/// it, or "the recorder is off" would be a statement about that machine rather than about Ono.
fn fresh_shell(home: &Scratch, script: &str) -> ono_testkit::Run {
    let root = home.path().display().to_string();
    Shell::new()
        .env("NO_COLOR", "1")
        .env("HOME", root.clone())
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .env_remove("ONO_CONFIG")
        .env_remove("ONO_CONFIG_DIR")
        .env_remove("ONO_TEMPORAL_RECORDING_ENABLED")
        .args(["-c", script])
        .timeout(Duration::from_secs(60))
        .run()
}

/// The canonical store of §31.1, resolved inside `home` the way the shell resolves it: under
/// `$XDG_DATA_HOME/ono/temporal/`, which both shell helpers point at `home/data`.
fn store(home: &Scratch) -> PathBuf {
    home.path().join("data/ono/temporal/ledger.sqlite3")
}

/// Every file the ledger and SQLite's write-ahead log would leave in `home`.
///
/// §31.5 keeps a `-wal` and a `-shm` beside the database, so "the store is gone" is a claim about
/// three files rather than one, and a test that looked only at the database would pass over a
/// retained write-ahead log holding the very events §30.8 was asked to destroy.
fn store_files(home: &Scratch) -> Vec<PathBuf> {
    let directory = store(home)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.starts_with("ledger.sqlite3"))
        })
        .collect();
    found.sort();
    found
}

/// The one `ono.recorder-status/1` record `get recorder` answers with (§10.3).
fn status(run: &ono_testkit::Run) -> Value {
    run.assert_success();
    support::single_result(run)
}

/// A store this home holds, made the only way a user can make one: by switching recording on.
fn recorded_store(home: &Scratch) -> PathBuf {
    let run = support::recording_shell(home, "get recorder | to json");
    let record = status(&run);
    assert_eq!(
        record["running"].as_bool(),
        Some(true),
        "v0.5 §10.2, §33: `temporal.recording.enabled` is what starts persistent recording, and \
         this fixture needs a store to exist before it can be destroyed, got {record:?}"
    );
    let path = store(home);
    assert!(
        path.is_file(),
        "v0.5 §31.1: a running recorder keeps its ledger at `{}`",
        path.display()
    );
    path
}

#[test]
fn should_report_a_stopped_recorder_and_create_no_store_when_a_fresh_shell_runs() {
    // §10.2: "Persistent recording MUST be disabled by default." §32.1: "Temporal storage
    // initialization MUST be lazy when recording is disabled." A fresh installation therefore
    // answers completely about a recorder that is not running, and has touched no file doing it.
    let home = scratch();
    let run = fresh_shell(&home, "get recorder | to json");
    let record = status(&run);

    assert_eq!(
        record["running"].as_bool(),
        Some(false),
        "v0.5 §10.2, §2.16: a fresh installation is not recording, got {record:?}"
    );
    assert_eq!(
        record["enabled"].as_bool(),
        Some(false),
        "v0.5 §10.2: nothing enabled persistent recording, got {record:?}"
    );
    assert_eq!(
        record["store"],
        Value::Null,
        "v0.5 §10.2, §32.1: a recorder that is not running names no store, because there is \
         none, got {record:?}"
    );
    assert_eq!(
        support::text(&record, "health"),
        "stopped",
        "v0.5 §43.4: the health of a recorder nobody started is `stopped`, not a failure, got \
         {record:?}"
    );
    assert_eq!(
        store_files(&home),
        Vec::<PathBuf>::new(),
        "v0.5 §10.2, §32.1: a fresh installation creates no store; `{}` must not exist",
        store(&home).display()
    );
}

#[test]
fn should_retain_nothing_across_sessions_when_recording_is_off() {
    // §10.2's second half: an installation without recording still answers temporal queries from
    // §10.7's in-memory session ledger, "discarded at session end". So a session that watched a
    // real process appear and vanish leaves the next session knowing nothing about it, and
    // leaves nothing on disk to know it from.
    let home = scratch();
    let mut fixture = support::fixture_process();
    let pid = fixture.id();

    let observed = fresh_shell(
        &home,
        &format!("get process | where pid == {pid} | select pid | to json"),
    );
    observed.assert_success();
    let seen = support::rows(&observed);
    assert_eq!(
        seen.len(),
        1,
        "the fixture process must be visible to the session that is meant to forget it, got {:?}",
        observed.output()
    );

    let _ = fixture.kill();
    let _ = fixture.wait();

    let later = fresh_shell(&home, "timeline --since 24h | to json");
    later.assert_success();
    let events = support::rows(&later);
    assert!(
        events.is_empty(),
        "v0.5 §10.2, §10.7, §11.4: the previous session's ledger was in memory and is gone with \
         it, so the stream a new session's `timeline` answers with carries no event from it, got \
         {events:?}"
    );

    let record = status(&fresh_shell(&home, "get recorder | to json"));
    assert_eq!(
        record["events"].as_u64(),
        Some(0),
        "v0.5 §10.2: nothing was retained, got {record:?}"
    );
    assert_eq!(
        store_files(&home),
        Vec::<PathBuf>::new(),
        "v0.5 §10.2: a process appeared and vanished while Ono watched, and with recording off \
         none of it reached the disk; `{}` must not exist",
        store(&home).display()
    );
}

#[test]
fn should_create_the_canonical_store_only_after_recording_is_switched_on() {
    // §2's sixteenth invariant — recording is opt-in — is a statement about a transition, so the
    // proof is one home observed twice: the same installation that held no store holds one once,
    // and only once, `temporal.recording.enabled` says so (§33). §31.1 fixes where it appears.
    let home = scratch();
    let before = fresh_shell(&home, "get recorder | to json");
    assert_eq!(
        status(&before)["running"].as_bool(),
        Some(false),
        "v0.5 §2.16: recording is off until it is switched on"
    );
    assert_eq!(
        store_files(&home),
        Vec::<PathBuf>::new(),
        "v0.5 §10.2: nothing has been switched on, so nothing has been written"
    );

    let after = support::recording_shell(&home, "get recorder | to json");
    let record = status(&after);
    assert_eq!(
        record["running"].as_bool(),
        Some(true),
        "v0.5 §33: `temporal.recording.enabled` is the switch, and a session that finds it on \
         records, got {record:?}"
    );
    assert_eq!(
        support::text(&record, "store"),
        store(&home).display().to_string(),
        "v0.5 §31.1: the ledger is `$XDG_DATA_HOME/ono/temporal/ledger.sqlite3`, and the status \
         says so rather than leaving the user to guess, got {record:?}"
    );
    assert!(
        store(&home).is_file(),
        "v0.5 §31.1: the store the status names exists at `{}`",
        store(&home).display()
    );
}

#[test]
fn should_refuse_to_destroy_the_local_store_when_the_removal_is_not_confirmed() {
    // §30.8: "This operation is destructive and MUST use existing destructive-operation
    // confirmation/policy semantics." The existing policy answers `safety.confirmation_required`
    // and changes nothing, which is exactly what a user who typed the command by accident needs.
    let home = scratch();
    let path = recorded_store(&home);

    let run = support::recording_shell(&home, "remove temporal-history");
    assert!(
        !run.status().is_success(),
        "v0.5 §30.8: an unconfirmed destruction does not report success, got {:?}",
        run.output()
    );
    assert!(
        run.output().contains("safety.confirmation_required"),
        "v0.5 §30.8: the refusal is the shell's existing destructive-operation policy, not a \
         temporal-specific one, got {:?}",
        run.output()
    );
    assert!(
        path.is_file(),
        "v0.5 §30.8: a refused destruction destroys nothing; `{}` is still there",
        path.display()
    );
}

#[test]
fn should_say_what_it_would_destroy_when_the_removal_is_a_dry_run() {
    // The rehearsal half of the destructive-operation policy: the user learns which store is at
    // stake before deciding, and the run changes nothing (spec §11.5's `changed` field).
    let home = scratch();
    let path = recorded_store(&home);

    let run = support::recording_shell(&home, "remove temporal-history --dry-run | to json");
    run.assert_success();
    let result = support::single_result(&run);

    assert_eq!(
        support::text(&result, "operation"),
        "ono.temporal-history.remove",
        "v0.5 §30.8: `temporal-history` is the canonical target, got {result:?}"
    );
    assert_eq!(
        result["changed"].as_bool(),
        Some(false),
        "v0.5 §30.8: a rehearsal changes nothing, got {result:?}"
    );
    assert!(
        support::text(&result, "message").contains(&path.display().to_string()),
        "v0.5 §30.8: the rehearsal names the store it would destroy, got {result:?}"
    );
    assert!(
        path.is_file(),
        "v0.5 §30.8: `--dry-run` leaves `{}` where it was",
        path.display()
    );
}

#[test]
fn should_clear_the_local_store_when_the_removal_is_confirmed() {
    // §30.8: "The user MUST be able to clear local retained history explicitly." Confirmed, the
    // command reports a successful change and the retained history is gone from the disk —
    // database, write-ahead log and shared-memory index alike (§31.5).
    let home = scratch();
    let path = recorded_store(&home);

    let run = support::recording_shell(&home, "remove temporal-history --confirm | to json");
    run.assert_success();
    let result = support::single_result(&run);

    assert_eq!(
        support::text(&result, "status"),
        "success",
        "v0.5 §30.8: the confirmed destruction succeeds, got {result:?}"
    );
    assert_eq!(
        result["changed"].as_bool(),
        Some(true),
        "v0.5 §30.8: destroying retained history is a change, got {result:?}"
    );
    assert!(
        support::text(&result, "message").ends_with("and its retained history are gone"),
        "v0.5 §30.8: the result says what happened to the history, got {result:?}"
    );
    assert_eq!(
        store_files(&home),
        Vec::<PathBuf>::new(),
        "v0.5 §30.8, §31.5: nothing of `{}` survives the confirmed removal — not the database, \
         not its write-ahead log",
        path.display()
    );
}
