//! `Ctrl-L` at the prompt: the screen clears, the prompt comes back at the top, and the line the
//! user was typing is still there (issue #121).
//!
//! The test drives a real pseudo-terminal, because the effect is on the screen and nowhere else,
//! and asserts on the bytes the terminal receives, never on the editor's outcome (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_testkit::scratch;

mod support;
use support::{interactive_shell_in, read_until};

const CLEAR_SCREEN: &str = "\x1b[2J";
const HOME_CURSOR: &str = "\x1b[1;1H";

#[test]
fn should_clear_the_screen_and_keep_the_typed_line_when_ctrl_l_is_pressed() {
    let directory = scratch();
    let mut shell = interactive_shell_in(&directory);

    // The banner says `local` too; the prompt is the first thing that says `local://`. Typing
    // before it is drawn lands in the cooked terminal, which echoes it, and proves nothing.
    let before = read_until(&mut shell, "local://", Duration::from_secs(10));
    assert!(
        before.contains("local://") && !before.contains(CLEAR_SCREEN),
        "a fresh prompt is drawn without clearing the screen; saw:\n{before:?}"
    );

    shell.write_all(b"get pro").expect("input");
    let typed = read_until(&mut shell, "get pro\x1b[", Duration::from_secs(10));
    assert!(
        typed.contains("get pro\x1b["),
        "the editor paints the typed line; saw:\n{typed:?}"
    );

    shell.write_all(b"\x0c").expect("input");
    let seen = read_until(&mut shell, CLEAR_SCREEN, Duration::from_secs(10));

    let cleared = seen
        .find(CLEAR_SCREEN)
        .unwrap_or_else(|| panic!("Ctrl-L clears the whole screen; the terminal saw:\n{seen:?}"));
    let homed = seen[cleared..]
        .find(HOME_CURSOR)
        .map(|at| cleared + at)
        .expect("the cursor is homed after the clear");
    assert!(
        seen[homed..].contains("get pro"),
        "the line being typed is painted again at the top; after the clear the terminal saw:\n{:?}",
        &seen[homed..]
    );

    shell.write_all(b"\x03exit\n").expect("input");
    let _ = shell.wait();
}
