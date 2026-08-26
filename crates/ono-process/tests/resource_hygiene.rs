//! Repeated execution must not leak descriptors or leave zombies behind.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

mod support;

use std::time::Duration;

use ono_process::{Command, Executor, Fd, Input, Output, Pipeline, Redirect};
use support::{exclusive, within};

fn open_descriptors() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .expect("/proc must be mounted")
        .count()
}

#[test]
fn should_not_leak_descriptors_when_many_commands_run() {
    // The descriptor table belongs to the whole process, so this test measures its siblings too
    // unless it runs alone.
    let _exclusive = exclusive();
    within(Duration::from_secs(60), || {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let target = dir.path().join("out");
        let mut executor = Executor::detached();

        let run = |executor: &mut Executor| {
            let pipeline = Pipeline::new()
                .stage(
                    Command::new("echo")
                        .arg("leak-check")
                        .stdin(Input::Bytes(b"in\n".to_vec()))
                        .stderr(Output::Capture),
                )
                .stage(
                    Command::new("cat")
                        .stdout(Output::Capture)
                        .redirect(Redirect::write_to(Fd::new(3), &target)),
                );
            let outcome = executor
                .run_foreground(&pipeline)
                .expect("the pipeline must run");
            assert_eq!(outcome.status().code(), 0);
        };

        // Warm up first: the first run may open long-lived things such as /proc handles.
        for _ in 0..5 {
            run(&mut executor);
        }
        let before = open_descriptors();
        for _ in 0..60 {
            run(&mut executor);
        }
        let after = open_descriptors();
        assert_eq!(
            after, before,
            "running commands must leave the descriptor table where it found it"
        );
    });
}

#[test]
fn should_leave_no_child_behind_when_a_pipeline_finishes() {
    let _exclusive = exclusive();
    within(Duration::from_secs(60), || {
        let mut executor = Executor::detached();
        for _ in 0..20 {
            let outcome = executor
                .run_foreground(
                    &Pipeline::new()
                        .stage(Command::new("yes"))
                        .stage(Command::new("head").arg("-2").stdout(Output::Null)),
                )
                .expect("the pipeline must run");
            assert_eq!(outcome.status().code(), 0);
        }
        assert!(
            executor.jobs().is_empty(),
            "a finished foreground pipeline never becomes a job"
        );
    });
}
