//! Every v0.6 command is machine-registered and reachable (spec v0.6 §5, §36, v0.2 §50).
//!
//! v0.2 §50 makes discoverability part of delivery: a command that `help` cannot describe, that
//! `explain` cannot resolve, that the registry does not list for completion and whose record
//! `inspect` cannot open is not delivered. These tests ask the shell those questions about the
//! thirteen commands of `docs/contracts/commands/change.yaml`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{home, ono_at};

/// The thirteen commands, as a user types them and as the registry names them.
const COMMANDS: &[(&str, &str)] = &[
    ("plan", "ono.change.plan"),
    ("get plan", "ono.change-plan.get"),
    ("inspect plan", "ono.change-plan.inspect"),
    ("rebase plan", "ono.change-plan.rebase"),
    ("resume plan", "ono.change-plan.resume"),
    ("impact", "ono.change.impact"),
    ("protect", "ono.change.protect"),
    ("apply", "ono.change.apply"),
    ("verify", "ono.change.verify"),
    ("recover", "ono.change.recover"),
    ("get recovery", "ono.recovery.get"),
    ("inspect recovery", "ono.recovery.inspect"),
    ("remove recovery", "ono.recovery.remove"),
];

#[test]
fn should_describe_every_change_command_when_help_is_asked_about_it() {
    let home = home();
    for (spelling, id) in COMMANDS {
        let run = ono_at(home.path(), &format!("help {spelling}"));
        run.assert_success();
        assert!(
            !run.stdout().trim().is_empty() && !run.output().contains("command_not_found"),
            "v0.2 §15.2: `help {spelling}` ({id}) is generated from the registry. Got {:?}",
            run.output()
        );
    }
}

#[test]
fn should_list_every_change_command_in_the_registry_completion_reads() {
    let home = home();
    for (spelling, id) in COMMANDS {
        let run = ono_at(
            home.path(),
            &format!("get command | where id == \"{id}\" | count | to json"),
        );
        run.assert_success();
        assert_eq!(
            run.stdout().split_whitespace().collect::<String>(),
            "[1]",
            "v0.2 §50: `{spelling}` is one registry entry, which is what completion and \
             `get command` read. Got {:?}",
            run.output()
        );
    }
}

#[test]
fn should_resolve_every_change_command_to_its_registry_id_when_explained() {
    let home = home();
    for (spelling, id) in COMMANDS {
        let run = ono_at(home.path(), &format!("explain {spelling}"));
        run.assert_success();
        assert!(
            run.stdout().contains(id),
            "v0.2 §15.3: `explain {spelling}` names the command it resolves to, {id}. Got {:?}",
            run.stdout()
        );
    }
}

#[test]
fn should_open_every_change_commands_record_when_inspected() {
    let home = home();
    for (spelling, id) in COMMANDS {
        let run = ono_at(
            home.path(),
            &format!("get command | where id == \"{id}\" | inspect"),
        );
        run.assert_success();
        assert!(
            run.stdout().contains("ono.command/1"),
            "v0.2 §15.4: `inspect` opens the registry record of `{spelling}` ({id}). Got {:?}",
            run.stdout()
        );
    }
}
