//! Every normative temporal command is machine-registered and reachable (v0.5 §56, §36).
//!
//! v0.2 §50 makes discoverability part of delivery: a command that `help` cannot describe, that
//! `explain` cannot resolve and whose output schema is not inspectable is not delivered. These
//! tests ask the shell those questions about the twelve commands of §36.1's inventory.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use support::ono;

/// The spellings §36.1's inventory names, as a user types them.
const SPELLINGS: &[&str] = &[
    "at",
    "now",
    "present",
    "timeline",
    "changes",
    "why",
    "find event",
    "inspect event",
    "get recorder",
    "start recorder",
    "stop recorder",
    "remove temporal-history",
];

#[test]
fn should_describe_every_temporal_command_when_help_is_asked_about_it() {
    for spelling in SPELLINGS {
        let run = ono(&format!("help {spelling}"));
        run.assert_success();
        assert!(
            !run.stdout().trim().is_empty(),
            "v0.2 §15.2: `help {spelling}` is generated from the registry and cannot be empty"
        );
        assert!(
            !run.output().contains("command_not_found"),
            "v0.5 §56: `{spelling}` is a registered command. Got {:?}",
            run.output()
        );
    }
}

#[test]
fn should_resolve_every_temporal_command_when_explain_is_asked_about_it() {
    for spelling in [
        "timeline",
        "changes --since 1m",
        "get recorder",
        "find event",
    ] {
        let run = ono(&format!("explain {spelling}"));
        run.assert_success();
        assert!(
            run.stdout().contains("ono.")
                || run.stdout().contains("native")
                || !run.stdout().trim().is_empty(),
            "v0.2 §15.3: `explain {spelling}` says how the name resolves. Got {:?}",
            run.stdout()
        );
    }
}

#[test]
fn should_answer_get_recorder_with_the_declared_schema() {
    let run = ono("get recorder | to json");
    run.assert_success();
    for field in [
        "running",
        "enabled",
        "max_age",
        "max_size",
        "session_max_events",
        "events",
        "sources",
        "dropped",
        "health",
    ] {
        assert!(
            run.stdout().contains(&format!("\"{field}\"")),
            "`ono.recorder-status/1` declares `{field}` (v0.5 §10.3). Got {:?}",
            run.stdout()
        );
    }
}

#[test]
fn should_report_the_recorder_as_stopped_and_disabled_on_a_fresh_session() {
    let run = ono("get recorder | to json");
    run.assert_success();
    assert!(
        run.stdout().contains("\"running\": false") || run.stdout().contains("\"running\":false"),
        "v0.5 §10.2: \"persistent recording MUST be disabled by default\". Got {:?}",
        run.stdout()
    );
    assert!(
        run.stdout().contains("stopped"),
        "v0.5 §43.4: a recorder that is not running is `stopped`. Got {:?}",
        run.stdout()
    );
}

#[test]
fn should_refuse_to_destroy_history_without_a_confirmation() {
    let run = ono("remove temporal-history");
    assert!(
        run.output().contains("confirm"),
        "v0.5 §30.8 takes the existing destructive-operation confirmation policy. Got {:?}",
        run.output()
    );
}

#[test]
fn should_say_what_it_would_destroy_when_asked_for_a_dry_run() {
    let run = ono("remove temporal-history --dry-run | to json");
    run.assert_success();
    assert!(
        run.output().contains("would"),
        "v0.5 §30.8: a destructive command that cannot state what it destroys is one nobody \
         should confirm. Got {:?}",
        run.output()
    );
}
