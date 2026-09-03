//! Redirection forms of spec §12.5, opened in the parent so failures are structured errors.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]
mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use ono_core::ErrorCode;
use ono_process::{Command, Executor, Fd, ForegroundOutcome, Output, PipelineOutcome, Redirect};
use ono_testkit::SkipReason;
use support::{DEADLINE, sh, text, within};

fn run(command: Command) -> PipelineOutcome {
    let outcome = within(DEADLINE, move || {
        let mut executor = Executor::detached();
        executor.run_foreground(&command.into())
    })
    .expect("the pipeline must run");
    match outcome {
        ForegroundOutcome::Completed(outcome) => outcome,
        ForegroundOutcome::Stopped { signal, .. } => panic!("unexpectedly stopped by {signal}"),
    }
}

#[test]
fn should_truncate_the_target_when_stdout_is_redirected_with_write() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let target = dir.path().join("out");
    fs::write(&target, "stale content that must disappear").expect("seed the file");

    let outcome = run(sh("echo fresh").redirect(Redirect::write(&target)));

    assert_eq!(outcome.status().code(), 0);
    assert_eq!(fs::read_to_string(&target).expect("read back"), "fresh\n");
}

#[test]
fn should_append_to_the_target_when_stdout_is_redirected_with_append() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let target = dir.path().join("out");
    fs::write(&target, "first\n").expect("seed the file");

    let outcome = run(sh("echo second").redirect(Redirect::append(&target)));

    assert_eq!(outcome.status().code(), 0);
    assert_eq!(
        fs::read_to_string(&target).expect("read back"),
        "first\nsecond\n"
    );
}

#[test]
fn should_read_stdin_from_the_target_when_input_is_redirected() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let source = dir.path().join("in");
    fs::write(&source, "from-a-file\n").expect("seed the file");

    let outcome = run(Command::new("cat")
        .redirect(Redirect::read(&source))
        .stdout(Output::Capture));

    assert_eq!(text(outcome.stdout()), "from-a-file\n");
}

#[test]
fn should_redirect_a_numbered_descriptor_to_a_file() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let target = dir.path().join("err");

    let outcome = run(sh("echo problem >&2").redirect(Redirect::write_to(Fd::STDERR, &target)));

    assert_eq!(outcome.status().code(), 0);
    assert_eq!(fs::read_to_string(&target).expect("read back"), "problem\n");
}

#[test]
fn should_redirect_a_descriptor_above_two_to_a_file() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let target = dir.path().join("three");

    let outcome = run(sh("echo on-three >&3").redirect(Redirect::write_to(Fd::new(3), &target)));

    assert_eq!(outcome.status().code(), 0);
    assert_eq!(
        fs::read_to_string(&target).expect("read back"),
        "on-three\n"
    );
}

#[test]
fn should_append_to_a_numbered_descriptor_when_asked() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let target = dir.path().join("err");
    fs::write(&target, "kept\n").expect("seed the file");

    let outcome = run(sh("echo added >&2").redirect(Redirect::append_to(Fd::STDERR, &target)));

    assert_eq!(outcome.status().code(), 0);
    assert_eq!(
        fs::read_to_string(&target).expect("read back"),
        "kept\nadded\n"
    );
}

#[test]
fn should_send_stderr_to_stdout_when_duplicated() {
    let outcome = run(sh("echo out; echo err >&2")
        .stdout(Output::Capture)
        .stderr(Output::Capture)
        .redirect(Redirect::duplicate(Fd::STDERR, Fd::STDOUT)));

    assert_eq!(text(outcome.stdout()), "out\nerr\n");
    assert_eq!(text(outcome.stderr()), "");
}

#[test]
fn should_send_stdout_to_stderr_when_duplicated() {
    let outcome = run(sh("echo out")
        .stdout(Output::Capture)
        .stderr(Output::Capture)
        .redirect(Redirect::duplicate(Fd::STDOUT, Fd::STDERR)));

    assert_eq!(text(outcome.stdout()), "");
    assert_eq!(text(outcome.stderr()), "out\n");
}

#[test]
fn should_resolve_a_duplication_against_the_state_at_that_point_in_the_sequence() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let target = dir.path().join("both");

    // `> file 2>&1`: stderr follows stdout into the file, not to the inherited stdout.
    let outcome = run(sh("echo out; echo err >&2")
        .redirect(Redirect::write(&target))
        .redirect(Redirect::duplicate(Fd::STDERR, Fd::STDOUT)));

    assert_eq!(outcome.status().code(), 0);
    let written = fs::read_to_string(&target).expect("read back");
    assert!(
        written.contains("out\n"),
        "stdout went to the file: {written:?}"
    );
    assert!(written.contains("err\n"), "stderr followed it: {written:?}");
}

#[test]
fn should_duplicate_an_input_descriptor_onto_stdin() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let source = dir.path().join("in");
    fs::write(&source, "through-fd-three\n").expect("seed the file");

    let outcome = run(Command::new("cat")
        .stdout(Output::Capture)
        .redirect(Redirect::read_from(Fd::new(3), &source))
        .redirect(Redirect::duplicate(Fd::STDIN, Fd::new(3))));

    assert_eq!(text(outcome.stdout()), "through-fd-three\n");
}

#[test]
fn should_report_a_structured_error_when_the_input_file_does_not_exist() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let missing = dir.path().join("absent");

    let outcome = run(Command::new("cat").redirect(Redirect::read(&missing)));

    let failure = outcome.stages()[0]
        .failure
        .as_ref()
        .expect("a failed redirection must be reported as a structured error");
    assert_eq!(failure.code(), ErrorCode::IoNotFound);
    assert!(!outcome.status().is_success());
}

#[test]
fn should_report_not_a_directory_when_a_path_component_is_a_file() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let file = dir.path().join("plain");
    fs::write(&file, "").expect("seed the file");
    let target = file.join("below");

    let outcome = run(sh("echo x").redirect(Redirect::write(&target)));

    let failure = outcome.stages()[0]
        .failure
        .as_ref()
        .expect("a failed redirection must be reported as a structured error");
    assert_eq!(failure.code(), ErrorCode::IoNotDirectory);
}

#[test]
fn should_report_permission_denied_when_the_target_cannot_be_opened() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let target = dir.path().join("locked");
    fs::write(&target, "").expect("seed the file");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o000)).expect("lock the file");
    if fs::OpenOptions::new().write(true).open(&target).is_ok() {
        // A privileged user bypasses the permission bits, so there is nothing to observe.
        ono_testkit::skipped(
            SkipReason::MissingPrivilege,
            "this user bypasses the permission bits, so an unopenable target cannot be made",
        );
        return;
    }

    let outcome = run(sh("echo x").redirect(Redirect::write(&target)));

    let failure = outcome.stages()[0]
        .failure
        .as_ref()
        .expect("a failed redirection must be reported as a structured error");
    assert_eq!(failure.code(), ErrorCode::IoPermissionDenied);
}

#[test]
fn should_not_run_the_command_at_all_when_a_redirection_fails() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let marker = dir.path().join("marker");
    let missing = dir.path().join("absent");

    let script = format!("echo ran > {}", marker.display());
    let outcome = run(sh(&script).redirect(Redirect::read(&missing)));

    assert!(outcome.stages()[0].failure.is_some());
    assert!(
        !marker.exists(),
        "the command must not run when its redirection could not be opened"
    );
}
