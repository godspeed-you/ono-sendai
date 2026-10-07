//! `get process --tree` at a terminal draws the hierarchy; anywhere else the stream and its
//! serialisation are unchanged (issue #223, ADR-0948).

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_testkit::{ono, scratch};

mod support;
use support::{interactive_shell_in, read_until};

#[test]
fn should_draw_the_process_tree_with_guides_at_a_terminal() {
    // This test's own shell is a process with a parent among the processes it can see, so the
    // tree has at least one nested row wherever the suite runs.
    let directory = scratch();
    let mut shell = interactive_shell_in(&directory);
    let _ = read_until(&mut shell, "> ", Duration::from_secs(10));

    shell.write_all(b"get process --tree\n").expect("input");
    let seen = read_until(&mut shell, "+-- ", Duration::from_secs(20));
    assert!(
        seen.contains("+-- "),
        "a terminal shows the hierarchy with tree guides; saw:\n{seen}"
    );

    // The tree is long; what is still on its way is read to the end, so the shell is never
    // blocked writing into a terminal nobody reads while it is asked to leave.
    shell.write_all(b"exit\n").expect("input");
    let _ = read_until(&mut shell, "\u{0}never", Duration::from_secs(20));
    let _ = shell.wait();
}

#[test]
fn should_keep_the_table_of_roots_when_stdout_is_not_a_terminal() {
    let run = ono("get process --tree");
    run.assert_success();
    assert!(
        !run.stdout().contains("+-- "),
        "redirected output stays the deterministic table of the stream's records, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_serialise_the_nested_records_unchanged() {
    let run = ono("get process --tree | to json");
    run.assert_success();
    assert!(
        run.stdout().contains(r#""children":["#) && !run.stdout().contains("+-- "),
        "the pipeline carries ono.process/1 records with `children`, and `to json` writes them, \
         got {:?}",
        run.stdout().chars().take(400).collect::<String>()
    );
}
