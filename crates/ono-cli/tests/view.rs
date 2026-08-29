//! `view` (spec §13.5, ADR-0050): the interactive browser, and the selection it leaves behind.

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::{Duration, Instant};

use ono_process::{Command, Executor, PtySession, WindowSize};

fn interactive_shell() -> PtySession {
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    executor
        .run_pty(&command, WindowSize::new(30, 100))
        .expect("a pseudo-terminal")
}

#[test]
fn should_pick_a_row_and_leave_it_addressable_as_the_current_value() {
    let mut shell = interactive_shell();
    let mut seen = String::new();
    let mut buffer = [0u8; 8192];
    let mut wait_for = |shell: &mut PtySession, needle: &str, budget: Duration| {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if let Ok(Some(count)) = shell.read_timeout(&mut buffer, Duration::from_millis(150)) {
                seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
            }
            if seen.contains(needle) {
                return true;
            }
        }
        false
    };

    assert!(
        wait_for(&mut shell, ">", Duration::from_secs(10)),
        "a prompt"
    );
    shell
        .write_all(b"let xs = [\"alpha\", \"beta\", \"gamma\"]; $xs | view table\n")
        .expect("the terminal accepts the view");
    assert!(
        wait_for(
            &mut shell,
            "keep selection and leave",
            Duration::from_secs(8)
        ),
        "the view opens with its key line (ADR-0050); saw:\n{seen}"
    );

    // Down once — the cursor sits on `beta` — then Enter opens the pane, then q leaves.
    shell.write_all(b"\x1b[B").expect("arrow down");
    shell.write_all(b"\r").expect("enter");
    assert!(
        wait_for(&mut shell, "--- inspect", Duration::from_secs(8)),
        "the inspect pane opens beside the collection; saw:\n{seen}"
    );
    shell.write_all(b"q").expect("leave");

    shell
        .write_all(b"@ | to json\n")
        .expect("act on the selection");
    assert!(
        wait_for(&mut shell, "[\"beta\"]", Duration::from_secs(8)),
        "bare `@` names the row the view left selected (spec §6.4, ADR-0050); saw:\n{seen}"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

#[test]
fn should_fall_back_to_plain_rendering_when_nobody_is_watching() {
    // §31.55 and §17.4: a view in a pipeline is the same values, rendered deterministically.
    let run = ono_testkit::Shell::new()
        .args([
            "-c",
            "get process | where pid == 1 | select pid | view table",
        ])
        .run();
    run.assert_success();
    run.assert_stdout_contains("PID");
    run.assert_stdout_contains("1");
}

#[test]
fn should_open_the_tree_view_over_a_graph_and_leave_the_pick_behind() {
    // Spec §13.6: a graph never renders as a table, so `view tree` is the shape it takes in the
    // browser. The rendering is proven in `ono-render`; what nothing exercised is the navigation
    // over it — `view tree` appeared in no test and in no case. This drives the real thing: the
    // view opens on a real trace, answers a key inside it, and leaves the selection addressable
    // (spec §6.4, ADR-0033).
    let mut shell = interactive_shell();
    let mut seen = String::new();
    let mut buffer = [0u8; 8192];
    let mut wait_for = |shell: &mut PtySession, needle: &str, budget: Duration| {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if let Ok(Some(count)) = shell.read_timeout(&mut buffer, Duration::from_millis(150)) {
                seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
            }
            if seen.contains(needle) {
                return true;
            }
        }
        false
    };

    assert!(
        wait_for(&mut shell, ">", Duration::from_secs(10)),
        "a prompt"
    );
    shell
        .write_all(b"trace process 1 | view tree\n")
        .expect("the terminal accepts the view");
    assert!(
        wait_for(
            &mut shell,
            "keep selection and leave",
            Duration::from_secs(15)
        ),
        "the tree view opens with its key line (ADR-0050); saw:\n{seen}"
    );
    assert!(
        wait_for(&mut shell, "ono.graph/1", Duration::from_secs(5)),
        "spec §13.6: what is drawn is the graph as a tree, not a table; saw:\n{seen}"
    );

    shell.write_all(b"\r").expect("enter");
    assert!(
        wait_for(&mut shell, "--- inspect", Duration::from_secs(10)),
        "a key pressed inside the tree view is answered; saw:\n{seen}"
    );
    shell.write_all(b"q").expect("leave");

    shell
        .write_all(b"@ | count | to json\n")
        .expect("act on the selection");
    assert!(
        wait_for(&mut shell, "[1]", Duration::from_secs(10)),
        "leaving the tree view keeps the graph it was showing as `@`; saw:\n{seen}"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}
