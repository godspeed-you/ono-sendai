//! The two walks through the history at a real prompt (issue #122, ADR-0564): the bare arrow is
//! anchored on what has been typed, `Ctrl-Up` steps to the previous entry whatever was typed.
//!
//! The test drives a real pseudo-terminal, because the question is whether the terminal's
//! `Ctrl-Up` reaches the editor as one, and asserts on what appears on the screen.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_testkit::scratch;

mod support;
use support::{interactive_shell_in, read_until};

/// What an xterm sends for `Ctrl-Up`.
const CTRL_UP: &[u8] = b"\x1b[1;5A";

#[test]
fn should_reach_an_entry_the_anchor_excludes_when_ctrl_up_is_pressed() {
    let directory = scratch();
    let mut shell = interactive_shell_in(&directory);
    let _ = read_until(&mut shell, "local://", Duration::from_secs(10));

    shell.write_all(b"echo first\n").expect("input");
    let _ = read_until(&mut shell, "first\r\n", Duration::from_secs(10));
    shell.write_all(b"echo second\n").expect("input");
    let _ = read_until(&mut shell, "second\r\n", Duration::from_secs(10));
    let _ = read_until(&mut shell, "local://", Duration::from_secs(10));

    shell.write_all(b"get ").expect("input");
    let _ = read_until(&mut shell, "get \x1b[", Duration::from_secs(10));

    shell.write_all(CTRL_UP).expect("input");
    let seen = read_until(&mut shell, "> echo second", Duration::from_secs(10));
    assert!(
        seen.contains("> echo second"),
        "issue #122: Ctrl-Up with `get ` typed reaches `echo second`, which the bare arrow's \
         anchor excludes; saw:\n{seen:?}"
    );

    shell.write_all(b"\x03exit\n").expect("input");
    let _ = shell.wait();
}
