//! External pipelines of spec §11 and §12.5: real pipes, one process group, no deadlocks.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]
mod support;

use std::fs;
use std::time::Duration;

use ono_process::{
    Command, Executor, Fd, ForegroundOutcome, Output, Pipeline, PipelineOutcome, Redirect,
};
use support::{DEADLINE, text, within};

fn sh(script: &str) -> Command {
    Command::new("/bin/sh").arg("-c").arg(script)
}

fn run(pipeline: Pipeline) -> PipelineOutcome {
    run_within(DEADLINE, pipeline)
}

fn run_within(timeout: Duration, pipeline: Pipeline) -> PipelineOutcome {
    let outcome = within(timeout, move || {
        let mut executor = Executor::detached();
        executor.run_foreground(&pipeline)
    })
    .expect("the pipeline must run");
    match outcome {
        ForegroundOutcome::Completed(outcome) => outcome,
        ForegroundOutcome::Stopped { signal, .. } => panic!("unexpectedly stopped by {signal}"),
    }
}

#[test]
fn should_connect_two_stages_when_a_pipeline_runs() {
    let outcome = run(Pipeline::new()
        .stage(Command::new("echo").arg("piped"))
        .stage(Command::new("cat").stdout(Output::Capture)));

    assert_eq!(text(outcome.stdout()), "piped\n");
    assert_eq!(outcome.status().code(), 0);
}

#[test]
fn should_connect_every_stage_when_the_pipeline_is_long() {
    let outcome = run(Pipeline::new()
        .stage(sh("printf 'b\\na\\nc\\n'"))
        .stage(Command::new("sort"))
        .stage(
            Command::new("grep")
                .arg("-c")
                .arg("")
                .stdout(Output::Capture),
        ));

    assert_eq!(text(outcome.stdout()).trim(), "3");
}

#[test]
fn should_report_the_status_of_every_stage_and_take_the_last_as_the_pipeline_status() {
    let outcome = run(Pipeline::new()
        .stage(sh("exit 1"))
        .stage(sh("exit 2"))
        .stage(sh("exit 3")));

    let statuses: Vec<u8> = outcome.statuses().iter().map(|s| s.code()).collect();
    assert_eq!(statuses, vec![1, 2, 3], "every stage status is retained");
    assert_eq!(
        outcome.status().code(),
        3,
        "the pipeline status is the last stage's (ADR-0008)"
    );
}

#[test]
fn should_place_every_stage_in_one_process_group() {
    let outcome = run(Pipeline::new()
        .stage(sh("exit 0"))
        .stage(sh("exit 0"))
        .stage(sh("exit 0")));

    let group = outcome.process_group();
    assert_eq!(
        group,
        outcome.stages()[0].pid,
        "the first stage leads the group"
    );
    for stage in outcome.stages() {
        assert!(stage.pid > 1, "each stage reports its pid");
    }
}

#[test]
fn should_leave_stderr_of_every_stage_alone_unless_it_is_redirected() {
    let outcome = run(Pipeline::new()
        .stage(sh("echo out; echo err >&2").stderr(Output::Capture))
        .stage(Command::new("cat").stdout(Output::Capture)));

    assert_eq!(
        text(outcome.stages()[0].stderr.as_slice()),
        "err\n",
        "stage stderr is not folded into the pipe"
    );
    assert_eq!(text(outcome.stdout()), "out\n");
}

#[test]
fn should_honour_a_redirection_that_overrides_the_pipe() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let target = dir.path().join("diverted");

    let outcome = run(Pipeline::new()
        .stage(sh("echo diverted").redirect(Redirect::write_to(Fd::STDOUT, &target)))
        .stage(Command::new("cat").stdout(Output::Capture)));

    assert_eq!(
        fs::read_to_string(&target).expect("read back"),
        "diverted\n"
    );
    assert_eq!(
        text(outcome.stdout()),
        "",
        "the second stage saw an immediate end of input"
    );
}

#[test]
fn should_terminate_promptly_when_a_downstream_stage_exits_early() {
    let outcome = run_within(
        Duration::from_secs(15),
        Pipeline::new()
            .stage(Command::new("yes"))
            .stage(Command::new("head").arg("-1").stdout(Output::Capture)),
    );

    assert_eq!(text(outcome.stdout()), "y\n");
    assert_eq!(outcome.status().code(), 0, "head exits successfully");
    assert_eq!(
        outcome.statuses()[0].signal(),
        Some(13),
        "the upstream stage is killed by SIGPIPE rather than hanging"
    );
}

#[test]
fn should_move_a_large_payload_through_a_pipeline_without_deadlocking() {
    let outcome = run_within(
        Duration::from_secs(30),
        Pipeline::new()
            .stage(
                Command::new("head")
                    .arg("-c")
                    .arg("1048576")
                    .redirect(Redirect::read("/dev/zero")),
            )
            .stage(Command::new("wc").arg("-c").stdout(Output::Capture)),
    );

    assert_eq!(text(outcome.stdout()).trim(), "1048576");
}

#[test]
fn should_feed_the_first_stage_and_capture_the_last_when_both_are_asked_for() {
    let outcome = run(Pipeline::new()
        .stage(Command::new("cat").stdin(ono_process::Input::Bytes(b"c\na\nb\n".to_vec())))
        .stage(Command::new("sort").stdout(Output::Capture)));

    assert_eq!(text(outcome.stdout()), "a\nb\nc\n");
}

#[test]
fn should_report_success_for_an_empty_pipeline() {
    let outcome = run(Pipeline::new());
    assert_eq!(outcome.status().code(), 0);
    assert!(outcome.stages().is_empty());
}

#[test]
fn should_keep_running_the_remaining_stages_when_one_cannot_be_resolved() {
    let outcome = run(Pipeline::new()
        .stage(sh("echo upstream"))
        .stage(Command::new("ono-no-such-program-42"))
        .stage(Command::new("cat").stdout(Output::Capture)));

    assert_eq!(outcome.statuses()[1].code(), 127);
    assert!(outcome.stages()[1].failure.is_some());
    assert_eq!(outcome.status().code(), 0, "the last stage still ran");
    assert_eq!(
        text(outcome.stdout()),
        "",
        "the broken stage passed nothing on, and the last stage saw end of input"
    );
}
