//! History records semantics, not only strings (spec §20.1), survives a restart (phase A11),
//! and never becomes the place a secret leaks from (spec §17.5, ADR-0015 T8).

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does. clippy's allow-*-in-tests only covers `#[test]` functions."
)]

use std::path::Path;
use std::time::Duration;

use ono_core::ExitStatus;
use ono_history::{Direction, History, Outcome, Policy};
use ono_testkit::scratch;

fn history_at(path: &Path) -> History {
    History::open(path, Policy::default()).expect("a fresh history file must open")
}

fn record(history: &mut History, text: &str) {
    history.record(
        text,
        Path::new("/home/case"),
        Outcome::new(ExitStatus::SUCCESS, Duration::from_millis(3)),
    );
}

#[test]
fn should_recall_what_was_run_in_the_order_it_was_run_when_asked() {
    let dir = scratch();
    let mut history = history_at(&dir.path().join("history.jsonl"));
    for text in ["get process", "cd /etc", "ls -la"] {
        record(&mut history, text);
    }

    let texts: Vec<&str> = history.entries().iter().map(|e| e.command_text()).collect();
    assert_eq!(texts, vec!["get process", "cd /etc", "ls -la"]);
}

#[test]
fn should_still_know_what_was_run_after_the_shell_restarts() {
    // Phase A11's whole point. Exit test for acceptance case 026.
    let dir = scratch();
    let path = dir.path().join("history.jsonl");
    {
        let mut history = history_at(&path);
        record(&mut history, "get process | where cpu > 20");
        history.flush().expect("history must be written");
    }
    let reopened = history_at(&path);
    assert_eq!(reopened.entries().len(), 1);
    assert_eq!(
        reopened.entries()[0].command_text(),
        "get process | where cpu > 20"
    );
}

#[test]
fn should_record_where_and_how_a_command_ran_rather_than_only_its_text() {
    // Spec §20.1: an entry carries cwd, exit status and duration, not just the string.
    let dir = scratch();
    let mut history = history_at(&dir.path().join("history.jsonl"));
    history.record(
        "false",
        Path::new("/tmp/work"),
        Outcome::new(ExitStatus::FAILURE, Duration::from_millis(120)),
    );

    let entry = &history.entries()[0];
    assert_eq!(entry.cwd(), Path::new("/tmp/work"));
    assert_eq!(entry.exit_status(), Some(ExitStatus::FAILURE));
    assert_eq!(entry.duration(), Some(Duration::from_millis(120)));
    assert!(
        !entry.id().is_empty(),
        "an entry needs an identity to be referred to"
    );
}

#[test]
fn should_give_every_entry_a_distinct_identity_when_many_are_recorded() {
    let dir = scratch();
    let mut history = history_at(&dir.path().join("history.jsonl"));
    for index in 0..500 {
        record(&mut history, &format!("echo {index}"));
    }
    let mut ids: Vec<&str> = history.entries().iter().map(|e| e.id()).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "entry identities must be unique");
}

#[test]
fn should_survive_a_corrupted_line_without_losing_the_rest_of_the_file() {
    // One entry per line exists precisely so a torn write costs one entry, not the history.
    let dir = scratch();
    let path = dir.path().join("history.jsonl");
    {
        let mut history = history_at(&path);
        record(&mut history, "first");
        record(&mut history, "second");
        history.flush().expect("write");
    }
    let mut text = std::fs::read_to_string(&path).expect("read");
    text.push_str("{\"this is not\": ");
    std::fs::write(&path, text).expect("write");

    let reopened = history_at(&path);
    let texts: Vec<&str> = reopened
        .entries()
        .iter()
        .map(|e| e.command_text())
        .collect();
    assert_eq!(texts, vec!["first", "second"]);
}

#[test]
fn should_not_record_a_command_the_user_hid_with_a_leading_space() {
    // The convention every shell user already knows, and the cheapest secret-aware policy there
    // is (spec §17.5, ADR-0015 T8).
    let dir = scratch();
    let mut history = history_at(&dir.path().join("history.jsonl"));
    record(&mut history, " export TOKEN=hunter2");
    record(&mut history, "get process");

    let texts: Vec<&str> = history.entries().iter().map(|e| e.command_text()).collect();
    assert_eq!(texts, vec!["get process"]);
}

#[test]
fn should_keep_a_hidden_command_out_of_the_file_and_not_merely_out_of_the_listing() {
    let dir = scratch();
    let path = dir.path().join("history.jsonl");
    let mut history = history_at(&path);
    record(&mut history, " export TOKEN=hunter2");
    history.flush().expect("write");

    let written = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        !written.contains("hunter2"),
        "a secret must never reach the file: {written:?}"
    );
}

#[test]
fn should_redact_a_value_matching_a_configured_secret_pattern_before_writing() {
    let dir = scratch();
    let path = dir.path().join("history.jsonl");
    let policy = Policy::default().redacting(["(?i)(token|password|secret)=(\\S+)"]);
    let mut history = History::open(&path, policy).expect("open");
    history.record(
        "deploy --password=hunter2 --host prod",
        Path::new("/"),
        Outcome::new(ExitStatus::SUCCESS, Duration::ZERO),
    );
    history.flush().expect("write");

    let entry = &history.entries()[0];
    assert!(
        !entry.command_text().contains("hunter2"),
        "{}",
        entry.command_text()
    );
    assert!(
        entry.command_text().contains("prod"),
        "the rest must survive"
    );
    let written = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(!written.contains("hunter2"), "{written:?}");
}

#[test]
fn should_collapse_a_command_repeated_immediately_when_the_policy_says_so() {
    let dir = scratch();
    let mut history = History::open(
        &dir.path().join("history.jsonl"),
        Policy::default().collapse_repeats(true),
    )
    .expect("open");
    for text in ["ls", "ls", "ls", "pwd", "ls"] {
        record(&mut history, text);
    }
    let texts: Vec<&str> = history.entries().iter().map(|e| e.command_text()).collect();
    assert_eq!(texts, vec!["ls", "pwd", "ls"]);
}

#[test]
fn should_keep_the_file_bounded_and_keep_the_newest_when_it_reaches_its_limit() {
    let dir = scratch();
    let path = dir.path().join("history.jsonl");
    let mut history = History::open(&path, Policy::default().max_entries(10)).expect("open");
    for index in 0..100 {
        record(&mut history, &format!("echo {index}"));
    }
    history.flush().expect("write");

    let reopened = History::open(&path, Policy::default().max_entries(10)).expect("reopen");
    assert_eq!(reopened.entries().len(), 10);
    assert_eq!(reopened.entries()[9].command_text(), "echo 99");
    assert_eq!(reopened.entries()[0].command_text(), "echo 90");
}

#[test]
fn should_walk_backwards_and_forwards_through_what_was_run_when_recalled() {
    let dir = scratch();
    let mut history = history_at(&dir.path().join("history.jsonl"));
    for text in ["one", "two", "three"] {
        record(&mut history, text);
    }
    let mut cursor = history.cursor();
    assert_eq!(cursor.step(Direction::Older), Some("three"));
    assert_eq!(cursor.step(Direction::Older), Some("two"));
    assert_eq!(cursor.step(Direction::Older), Some("one"));
    assert_eq!(
        cursor.step(Direction::Older),
        None,
        "the start is not a wrap"
    );
    assert_eq!(cursor.step(Direction::Newer), Some("two"));
    assert_eq!(cursor.step(Direction::Newer), Some("three"));
    assert_eq!(
        cursor.step(Direction::Newer),
        None,
        "past the newest is the live line"
    );
}

#[test]
fn should_recall_only_what_starts_with_the_typed_prefix_when_one_is_given() {
    let dir = scratch();
    let mut history = history_at(&dir.path().join("history.jsonl"));
    for text in ["get process", "cd /etc", "get service nginx", "ls"] {
        record(&mut history, text);
    }
    let mut cursor = history.cursor().with_prefix("get ");
    assert_eq!(cursor.step(Direction::Older), Some("get service nginx"));
    assert_eq!(cursor.step(Direction::Older), Some("get process"));
    assert_eq!(cursor.step(Direction::Older), None);
}

#[test]
fn should_find_the_most_recent_match_anywhere_in_the_line_when_searched() {
    let dir = scratch();
    let mut history = history_at(&dir.path().join("history.jsonl"));
    for text in ["get process", "systemctl status nginx", "get service nginx"] {
        record(&mut history, text);
    }
    assert_eq!(history.search_before("nginx", None), Some(2));
    assert_eq!(history.search_before("nginx", Some(2)), Some(1));
    assert_eq!(history.search_before("nginx", Some(1)), None);
    assert_eq!(history.search_before("no-such-thing", None), None);
}

#[test]
fn should_open_a_history_whose_directory_does_not_exist_yet_when_the_shell_first_runs() {
    let dir = scratch();
    let path = dir.path().join("deep/state/ono/history.jsonl");
    let mut history = history_at(&path);
    record(&mut history, "get process");
    history.flush().expect("write");
    assert!(
        path.is_file(),
        "the shell must create its own state directory"
    );
}

#[test]
fn should_report_a_history_it_cannot_write_rather_than_losing_entries_silently() {
    let dir = scratch();
    let path = dir.path().join("history.jsonl");
    std::fs::create_dir_all(&path).expect("a directory where the file should be");
    let opened = History::open(&path, Policy::default());
    assert!(opened.is_err(), "an unusable history path must be reported");
}

#[test]
fn should_redact_the_obvious_secrets_before_anyone_configures_it_to() {
    // Spec §17.5 requires that secrets not reach history. An empty default pattern list would
    // mean the mechanism worked and the product did not: nobody configures redaction before the
    // first time it would have mattered.
    let dir = scratch();
    let path = dir.path().join("history.jsonl");
    let mut history = history_at(&path);

    for command in [
        "deploy --password=hunter2 --host prod",
        "curl --token abc123xyz https://example",
        "AWS_SECRET_ACCESS_KEY=s3kr1t terraform apply",
        "run --api-key=deadbeef",
    ] {
        history.record(
            command,
            Path::new("/"),
            Outcome::new(ExitStatus::SUCCESS, Duration::ZERO),
        );
    }
    history.flush().expect("write");

    let written = std::fs::read_to_string(&path).expect("read back");
    for secret in ["hunter2", "abc123xyz", "s3kr1t", "deadbeef"] {
        assert!(
            !written.contains(secret),
            "`{secret}` reached the history file:\n{written}"
        );
    }
    // The rest of each command survives, or the entry would be useless for recall.
    for kept in ["deploy", "prod", "curl", "terraform apply", "run"] {
        assert!(written.contains(kept), "`{kept}` was lost:\n{written}");
    }
}

#[test]
fn should_not_redact_ordinary_text_that_merely_mentions_a_secret() {
    // A pattern that fires on ordinary text teaches people to turn redaction off, which is worse
    // than not having it.
    let dir = scratch();
    let path = dir.path().join("history.jsonl");
    let mut history = history_at(&path);
    for command in [
        "grep password /etc/config",
        "cat secrets.md",
        "echo my token is safe",
    ] {
        history.record(
            command,
            Path::new("/"),
            Outcome::new(ExitStatus::SUCCESS, Duration::ZERO),
        );
    }
    history.flush().expect("write");

    let texts: Vec<&str> = history.entries().iter().map(|e| e.command_text()).collect();
    assert_eq!(
        texts,
        vec![
            "grep password /etc/config",
            "cat secrets.md",
            "echo my token is safe"
        ]
    );
}

#[cfg(unix)]
#[test]
fn should_keep_the_history_readable_only_by_its_owner() {
    // A shell's history is a record of what someone did on the machine, and a person's habits are
    // often more revealing than any single command. At the usual umask it would land as 0644 —
    // readable by every account on a shared host.
    use std::os::unix::fs::PermissionsExt;

    let dir = scratch();
    let path = dir.path().join("state/ono/history.jsonl");
    let mut history = history_at(&path);
    record(&mut history, "get process");
    history.flush().expect("write");

    let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the history file is 0o{mode:o}, not 0o600");

    let directory = std::fs::metadata(path.parent().expect("a parent"))
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        directory, 0o700,
        "the state directory is 0o{directory:o}, not 0o700"
    );
}

#[test]
fn should_remember_that_a_command_was_adapted_and_explain_it() {
    // Spec v0.3 §1.58: an entry records the adapter and plan used, so the same line behaving
    // raw on another machine is explicable.
    let dir = ono_testkit::scratch();
    let mut history = history_at(&dir.path().join("history.jsonl"));
    let outcome = ono_history::Outcome::new(
        ono_core::ExitStatus::SUCCESS,
        std::time::Duration::from_millis(12),
    )
    .adapted_by(
        ["org.ono.compat.util-linux.lsblk".to_owned()],
        ["lsblk --json --list".to_owned()],
    );
    let id = history
        .record(
            "lsblk | where type == \"disk\"",
            std::path::Path::new("/"),
            outcome,
        )
        .expect("recorded");
    history.flush().expect("flushed");
    let reopened = history_at(&dir.path().join("history.jsonl"));
    let entry = reopened
        .entries()
        .iter()
        .find(|entry| entry.id() == id)
        .expect("persisted");
    assert_eq!(entry.adapters(), ["org.ono.compat.util-linux.lsblk"]);
    let explained = entry.explain();
    assert!(
        explained.contains("org.ono.compat.util-linux.lsblk")
            && explained.contains("lsblk --json --list"),
        "the explanation names the adapter and the plan, got {explained:?}"
    );
}
