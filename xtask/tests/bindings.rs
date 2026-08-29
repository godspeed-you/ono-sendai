//! Spec §27.2: "every stable command is bound to an implementation" — the check that runs.
//!
//! `ono_command::unbound_stable_commands` has existed since phase D as an API with a unit test
//! behind it, and nothing ever pointed it at the real registry inside the gate. These tests hold
//! the armed version: the list of commands this build deliberately leaves unbound is finite and
//! written down, and anything else losing its implementation is a gate failure.

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::BTreeSet;

use ono_command::{CommandRegistry, builtin_commands};
use xtask::bindings::check_bindings;

fn registry() -> &'static CommandRegistry {
    CommandRegistry::embedded().expect("the embedded command contracts parse")
}

#[test]
fn should_find_every_stable_command_of_this_build_bound_to_an_implementation() {
    let table = builtin_commands(registry());
    let problems = check_bindings(registry(), |id| table.contains(id));

    assert!(
        problems.is_empty(),
        "spec §27.2: every stable command is bound, or is written down as deliberately unbound \
         with its reason:\n{}",
        problems
            .iter()
            .map(|problem| format!("  {} — {}", problem.location, problem.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn should_report_a_stable_command_that_lost_its_implementation() {
    // The guard's own guard. A check that cannot fail is not a check: with nothing bound at all,
    // every stable command that is not on the deliberate list must be reported by name.
    let problems = check_bindings(registry(), |_| false);
    let reported: BTreeSet<&str> = problems
        .iter()
        .map(|problem| problem.location.as_str())
        .collect();

    for id in [
        "ono.data.where",
        "ono.process.get",
        "ono.meta.help",
        "ono.process.watch",
        "ono.process.trace",
    ] {
        assert!(
            reported.contains(id),
            "`{id}` is a stable command with an implementation; losing it must be reported, \
             got {reported:?}"
        );
    }
    for problem in &problems {
        assert!(
            problem.detail.contains("§27.2"),
            "the report cites the rule it enforces, got {problem:?}"
        );
    }
}

#[test]
fn should_report_a_deliberately_unbound_command_that_is_bound_after_all() {
    // The list is a claim about this build, so it has to be wrong in both directions. A command
    // the list excuses, bound after all, means the excuse has expired and the entry must go.
    let table = builtin_commands(registry());
    let problems = check_bindings(registry(), |id| {
        table.contains(id) || id == "ono.config.get"
    });

    assert_eq!(
        problems.len(),
        1,
        "exactly the stale excuse is reported, got {problems:?}"
    );
    assert_eq!(problems[0].location, "ono.config.get");
    assert!(
        problems[0].detail.contains("is bound"),
        "the report says the entry has expired, got {problems:?}"
    );
}
