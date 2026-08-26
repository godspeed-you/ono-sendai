//! Process-wide signal setup. Kept in its own test binary because it changes global state.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]
mod support;

use std::time::Duration;

use ono_process::{Command, Executor, Output};
use support::{DEADLINE, exclusive, poll_until, text, within};

fn sh(script: &str) -> Command {
    Command::new("/bin/sh").arg("-c").arg(script)
}

#[test]
fn should_reset_the_child_signal_dispositions_even_when_the_shell_ignores_them() {
    // Signal dispositions and the child-transition flag belong to the whole
    // process, so a sibling test spawning a child would otherwise set the flag this
    // one is asserting about.
    let _exclusive = exclusive();
    within(DEADLINE, || {
        ono_process::install_shell_signals().expect("the shell signal setup must succeed");
        ono_process::install_shell_signals()
            .expect("installing the shell signal setup twice is harmless");

        let mut executor = Executor::detached();
        let outcome = executor
            .run_foreground(
                &sh("kill -INT $$; echo NOT-REACHED")
                    .stdout(Output::Capture)
                    .into(),
            )
            .expect("the run must not fail");

        assert_eq!(
            outcome.status().code(),
            130,
            "the child must die of SIGINT, so its disposition was reset before exec"
        );
    });
}

#[test]
fn should_keep_the_shell_alive_when_a_signal_the_shell_ignores_is_raised_in_the_child_group() {
    // Signal dispositions and the child-transition flag belong to the whole
    // process, so a sibling test spawning a child would otherwise set the flag this
    // one is asserting about.
    let _exclusive = exclusive();
    within(DEADLINE, || {
        ono_process::install_shell_signals().expect("the shell signal setup must succeed");
        let mut executor = Executor::detached();
        let outcome = executor
            .run_foreground(&sh("kill -QUIT $$").into())
            .expect("the run must not fail");
        assert_eq!(outcome.status().code(), 128 + 3, "SIGQUIT is 3");
    });
}

#[test]
fn should_flag_a_child_transition_when_the_child_watch_is_installed() {
    // Signal dispositions and the child-transition flag belong to the whole
    // process, so a sibling test spawning a child would otherwise set the flag this
    // one is asserting about.
    let _exclusive = exclusive();
    within(DEADLINE, || {
        ono_process::install_child_watch().expect("the child watch must install");
        let _ = ono_process::take_child_transition();

        let mut executor = Executor::detached();
        let id = executor
            .run_background(&sh("exit 0").into())
            .expect("the job must start");

        poll_until(Duration::from_secs(20), || {
            ono_process::take_child_transition().then_some(())
        });

        executor
            .wait_job(id, Some(DEADLINE))
            .expect("waiting must not fail")
            .expect("the job must finish");
        assert!(
            !ono_process::take_child_transition(),
            "taking the flag clears it"
        );
    });
}

#[test]
fn should_still_capture_output_while_the_child_watch_is_installed() {
    // Signal dispositions and the child-transition flag belong to the whole
    // process, so a sibling test spawning a child would otherwise set the flag this
    // one is asserting about.
    let _exclusive = exclusive();
    within(DEADLINE, || {
        ono_process::install_child_watch().expect("the child watch must install");
        let mut executor = Executor::detached();
        let outcome = executor
            .run_foreground(&sh("echo still-works").stdout(Output::Capture).into())
            .expect("the run must not fail");
        let completed = outcome.completed().expect("echo does not stop");
        assert_eq!(text(completed.stdout()), "still-works\n");
    });
}
