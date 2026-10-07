//! A refusal shown to a user carries the fields that name the boundary that decided it (issue
//! #180, v0.4.1 §54.1, §54.2).
//!
//! `docs/contracts/hardening/refusals.yaml` declares, per hardening refusal, the metadata keys
//! that explain it (`explains`). Those keys reach the screen as `key: value` lines under the
//! message — at a terminal and with standard error redirected alike — in the order the census
//! lists them. §54.2: "Users must not need `RUST_LOG=debug` to understand why a security policy
//! denied them", and a field that exists only on the structured value is one only a script sees.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::time::Duration;

use ono_testkit::{Scratch, scratch};

use support::isolated;

/// A directory holding three files, so a sort over them is three values on every host.
fn three_files() -> Scratch {
    let dir = scratch();
    for name in ["one", "two", "three"] {
        dir.write(format!("population/{name}"), name);
    }
    dir
}

/// The explanation lines a refusal printed, in order: `  key: value` lines under its message.
fn explanation(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .filter(|line| {
            line.split_once(": ").is_some_and(|(key, _)| {
                !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            })
        })
        .map(str::to_owned)
        .collect()
}

#[test]
fn should_show_the_limit_and_the_consumption_when_a_materialization_budget_refuses() {
    let dir = three_files();
    let run = isolated(&dir)
        .args([
            "-c",
            &format!(
                "set config limits.materialize_items = 2\nget file {}/population/* | sort name | count",
                dir.path().display()
            ),
        ])
        .run();
    assert!(
        !run.status().is_success() && run.stderr().contains("Ono-Sendai-E1101"),
        "the item budget refuses the sort, got {:?}",
        run.output()
    );
    let lines = explanation(run.stderr());
    let keys: Vec<&str> = lines
        .iter()
        .filter_map(|line| line.split_once(": ").map(|(key, _)| key))
        .collect();
    assert_eq!(
        keys,
        ["stage", "ceiling", "limit", "consumed", "setting"],
        "refusals.yaml `resource.item_limit` explains [stage, ceiling, limit, consumed, setting], \
         and the refusal shows each, in that order, under its message; got {:?}",
        run.stderr()
    );
    for expected in [
        "stage: sort",
        "limit: 2",
        "consumed: 3",
        "setting: limits.materialize_items",
    ] {
        assert!(
            lines.iter().any(|line| line == expected),
            "v0.4.1 §54.1: the refusal shows `{expected}`, got {lines:?}"
        );
    }
    assert!(
        !run.stderr().contains("population/one"),
        "§21.4: a resource refusal shows the figures and never the values it held, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_show_no_explanation_lines_for_an_error_the_census_does_not_explain() {
    // Only the declared keys of a declared refusal are shown: an ordinary error keeps the shape
    // it had, and its machine detail stays on the value.
    let dir = scratch();
    let run = isolated(&dir)
        .args(["-c", "no-such-command-for-this-test"])
        .run();
    assert!(
        run.stderr().contains("Ono-Sendai-E0101"),
        "got {:?}",
        run.stderr()
    );
    assert!(
        explanation(run.stderr()).is_empty(),
        "`resolve.command_not_found` is not in the census, so nothing is added, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_show_the_deciding_fields_at_a_terminal_as_well() {
    let dir = three_files();
    let mut shell = support::interactive_shell_in(&dir);
    let _ = support::read_until(&mut shell, ">", Duration::from_secs(10));
    shell
        .write_all(b"set config limits.materialize_items = 2\n")
        .expect("the terminal accepts a command");
    shell
        .write_all(b"get file population/* | sort name | count\n")
        .expect("the terminal accepts a command");
    let seen = support::read_until(&mut shell, "setting: ", Duration::from_secs(10));
    for expected in ["Ono-Sendai-E1101", "limit: 2", "consumed: 3", "setting: "] {
        assert!(
            seen.contains(expected),
            "v0.4.1 §54.2: at a terminal the refusal shows `{expected}` too; saw:\n{seen}"
        );
    }
    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}
