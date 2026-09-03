//! Behaviour of a single external command: resolution, status, environment and stdio.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]
mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use ono_core::ErrorCode;
use ono_process::{Command, Executor, ForegroundOutcome, Input, Output, PipelineOutcome};
use support::{DEADLINE, text, within};

fn run(command: Command) -> PipelineOutcome {
    let outcome = within(DEADLINE, move || {
        let mut executor = Executor::detached();
        executor.run_foreground(&command.into())
    })
    .expect("the pipeline must run");
    match outcome {
        ForegroundOutcome::Completed(outcome) => outcome,
        ForegroundOutcome::Stopped { signal, .. } => {
            panic!("the command was unexpectedly stopped by signal {signal}")
        }
    }
}

fn captured(command: Command) -> PipelineOutcome {
    run(command.stdout(Output::Capture).stderr(Output::Capture))
}

fn write_script(dir: &Path, name: &str, body: &str, mode: u32) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).expect("the script must be writable");
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("mode must be settable");
    path
}

/// Runs a script this suite has just written, waiting out a thread that is still holding it open.
///
/// A thread that forks between this thread's `open` and `close` of the script inherits the write
/// descriptor, and until that child execs, `execve` answers ETXTBSY — which ADR-0008 maps to 126,
/// "found and not executable", about a file that is executable. Issue #27 is one sighting of that
/// under a `cargo test --workspace` with a container build beside it. Every other failure is
/// answered on the first attempt (ADR-0520).
fn captured_script(script: &Path) -> PipelineOutcome {
    ono_testkit::while_text_file_busy(
        |outcome: &PipelineOutcome| {
            outcome.stages()[0]
                .failure
                .as_ref()
                .is_some_and(|failure| failure.message().contains("Text file busy"))
        },
        || captured(Command::new(script)),
    )
}

#[test]
fn should_report_success_when_the_command_exits_zero() {
    let outcome = run(Command::new("/bin/true"));
    assert_eq!(outcome.status().code(), 0, "/bin/true must succeed");
}

#[test]
fn should_pass_the_child_status_through_unchanged_when_it_is_non_zero() {
    let outcome = run(Command::new("/bin/sh").arg("-c").arg("exit 42"));
    assert_eq!(
        outcome.status().code(),
        42,
        "the child's own status must not be translated"
    );
}

#[test]
fn should_pass_a_child_chosen_127_through_unchanged() {
    let outcome = run(Command::new("/bin/sh").arg("-c").arg("exit 127"));
    assert_eq!(outcome.status().code(), 127);
    assert!(
        outcome.stages()[0].failure.is_none(),
        "a status the child chose itself is not a resolution failure"
    );
}

#[test]
fn should_report_127_when_the_program_cannot_be_resolved() {
    let outcome = run(Command::new("ono-no-such-program-42"));
    assert_eq!(outcome.status().code(), 127);
    let failure = outcome.stages()[0]
        .failure
        .as_ref()
        .expect("an unresolvable program must carry a structured error");
    assert_eq!(failure.code(), ErrorCode::ResolveCommandNotFound);
}

#[test]
fn should_report_126_when_the_file_is_found_but_not_executable() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let script = write_script(dir.path(), "not-executable", "#!/bin/sh\nexit 0\n", 0o644);
    let outcome = run(Command::new(&script));
    assert_eq!(outcome.status().code(), 126);
    let failure = outcome.stages()[0]
        .failure
        .as_ref()
        .expect("a non-executable file must carry a structured error");
    assert_eq!(failure.code(), ErrorCode::IoPermissionDenied);
}

#[test]
fn should_report_126_when_the_program_is_a_directory() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let outcome = run(Command::new(dir.path()));
    assert_eq!(outcome.status().code(), 126);
}

#[test]
fn should_report_126_when_the_file_is_executable_but_not_a_program() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let script = write_script(dir.path(), "garbage", "\u{0}not a program\n", 0o755);
    let outcome = run(Command::new(&script));
    assert_eq!(
        outcome.status().code(),
        126,
        "a file the machine cannot execute was found but could not be run"
    );
    let failure = outcome.stages()[0]
        .failure
        .as_ref()
        .expect("the stage must carry a structured error");
    assert_eq!(failure.code(), ErrorCode::IoPermissionDenied);
}

#[test]
fn should_run_a_text_script_without_a_shebang_through_the_shell() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let script = write_script(dir.path(), "plain", "echo from-plain-text\n", 0o755);
    let outcome = captured_script(&script);
    assert_eq!(outcome.status().code(), 0);
    assert_eq!(text(outcome.stdout()), "from-plain-text\n");
}

#[test]
fn should_run_a_shebang_script_when_it_is_executable() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let script = write_script(dir.path(), "greet", "#!/bin/sh\necho from-shebang\n", 0o755);
    let outcome = captured_script(&script);
    assert_eq!(outcome.status().code(), 0);
    assert_eq!(text(outcome.stdout()), "from-shebang\n");
}

#[test]
fn should_search_the_path_when_the_program_has_no_slash() {
    let outcome = captured(Command::new("echo").arg("found-on-path"));
    assert_eq!(outcome.status().code(), 0);
    assert_eq!(text(outcome.stdout()), "found-on-path\n");
}

#[test]
fn should_not_search_the_path_when_the_program_contains_a_slash() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    write_script(dir.path(), "echo", "#!/bin/sh\necho shadowed\n", 0o755);
    let outcome = captured(
        Command::new("./echo")
            .arg("ignored")
            .current_dir(dir.path()),
    );
    assert_eq!(outcome.status().code(), 0);
    assert_eq!(
        text(outcome.stdout()),
        "shadowed\n",
        "a program name containing a slash must not be looked up on PATH"
    );
}

#[test]
fn should_apply_environment_changes_to_the_child() {
    let outcome = captured(
        Command::new("/bin/sh")
            .arg("-c")
            .arg("echo \"[${ONO_TEST_VAR-unset}]\"")
            .env("ONO_TEST_VAR", "present"),
    );
    assert_eq!(text(outcome.stdout()), "[present]\n");
}

#[test]
fn should_unset_an_inherited_variable_when_asked() {
    let outcome = captured(Command::new("env").env_remove("PATH"));
    assert_eq!(outcome.status().code(), 0);
    assert!(
        !text(outcome.stdout())
            .lines()
            .any(|line| line.starts_with("PATH=")),
        "an unset variable must not reach the child at all, saw {:?}",
        text(outcome.stdout())
    );
}

#[test]
fn should_still_find_the_program_when_the_command_removes_its_own_path() {
    let outcome = captured(Command::new("env").env_remove("PATH"));
    assert_eq!(
        outcome.status().code(),
        0,
        "resolution falls back to the POSIX default search path"
    );
}

#[test]
fn should_let_a_removal_win_over_an_earlier_assignment() {
    let outcome = captured(
        Command::new("/bin/sh")
            .arg("-c")
            .arg("echo \"[${ONO_TEST_VAR-unset}]\"")
            .env("ONO_TEST_VAR", "present")
            .env_remove("ONO_TEST_VAR"),
    );
    assert_eq!(text(outcome.stdout()), "[unset]\n");
}

#[test]
fn should_start_the_child_in_the_requested_directory() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let expected = fs::canonicalize(dir.path()).expect("the directory must be canonicalizable");
    let outcome = captured(
        Command::new("/bin/sh")
            .arg("-c")
            .arg("pwd -P")
            .current_dir(dir.path()),
    );
    assert_eq!(
        text(outcome.stdout()).trim_end(),
        expected.to_string_lossy()
    );
}

#[test]
fn should_report_128_plus_the_signal_when_the_child_is_killed() {
    let outcome = run(Command::new("/bin/sh").arg("-c").arg("kill -TERM $$"));
    assert_eq!(outcome.status().code(), 143, "SIGTERM is 15, so 128 + 15");
    assert_eq!(outcome.status().signal(), Some(15));
}

#[test]
fn should_keep_stdout_and_stderr_separate_when_both_are_captured() {
    let outcome = captured(
        Command::new("/bin/sh")
            .arg("-c")
            .arg("echo to-stdout; echo to-stderr >&2"),
    );
    assert_eq!(text(outcome.stdout()), "to-stdout\n");
    assert_eq!(text(outcome.stderr()), "to-stderr\n");
}

#[test]
fn should_feed_given_bytes_to_the_child_stdin() {
    let outcome = captured(Command::new("cat").stdin(Input::Bytes(b"fed-in\n".to_vec())));
    assert_eq!(text(outcome.stdout()), "fed-in\n");
}

#[test]
fn should_feed_more_bytes_than_a_pipe_buffer_holds() {
    let payload = vec![b'x'; 512 * 1024];
    let outcome = captured(Command::new("wc").arg("-c").stdin(Input::Bytes(payload)));
    assert_eq!(
        text(outcome.stdout()).trim().parse::<usize>().ok(),
        Some(512 * 1024)
    );
}

#[test]
fn should_give_the_child_an_empty_stdin_when_it_is_null() {
    let outcome = captured(Command::new("cat").stdin(Input::Null));
    assert_eq!(text(outcome.stdout()), "");
}

#[test]
fn should_discard_output_when_the_stream_is_null() {
    let outcome = run(Command::new("/bin/sh")
        .arg("-c")
        .arg("echo noise; echo noise >&2")
        .stdout(Output::Null)
        .stderr(Output::Null));
    assert_eq!(outcome.status().code(), 0);
    assert!(outcome.stdout().is_empty());
}

#[test]
fn should_report_the_child_process_group_and_pid() {
    let outcome = run(Command::new("/bin/true"));
    let stage = &outcome.stages()[0];
    assert!(stage.pid > 1, "the stage must report the pid it ran as");
    assert_eq!(
        outcome.process_group(),
        stage.pid,
        "a single foreground command leads its own process group"
    );
}

#[test]
fn should_finish_promptly_when_the_child_is_trivial() {
    within(Duration::from_secs(10), || {
        let mut executor = Executor::detached();
        for _ in 0..20 {
            let outcome = executor
                .run_foreground(&Command::new("/bin/true").into())
                .expect("each run must succeed");
            assert_eq!(outcome.status().code(), 0);
        }
    });
}
