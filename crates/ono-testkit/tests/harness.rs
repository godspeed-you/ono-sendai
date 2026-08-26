//! The test harness is itself tested: every later test trusts it to report what a user would
//! actually see, so a harness that quietly stopped capturing stderr would make the whole suite
//! meaningless (AGENTS.md section 14).

use ono_testkit::Shell;

#[test]
fn should_report_the_version_on_standard_output_when_asked() {
    let run = Shell::new().args(["--version"]).run();
    run.assert_success();
    assert!(run.stdout().starts_with("ono "), "got {:?}", run.stdout());
    assert_eq!(run.stderr(), "");
}

#[test]
fn should_report_a_failing_status_and_keep_the_streams_apart_when_the_command_line_is_wrong() {
    let run = Shell::new().args(["--definitely-not-a-flag"]).run();
    assert_eq!(run.status().code(), 2);
    assert!(!run.status().is_success());
    assert!(
        run.stderr().contains("unrecognised"),
        "the complaint belongs on stderr, got stdout={:?} stderr={:?}",
        run.stdout(),
        run.stderr()
    );
    assert_eq!(
        run.stdout(),
        "",
        "nothing should reach stdout on a usage error"
    );
}

#[test]
fn should_run_in_a_scratch_directory_that_does_not_outlive_the_test_when_one_is_requested() {
    let scratch = ono_testkit::scratch();
    scratch.write("a.txt", "alpha\n");
    assert_eq!(scratch.read("a.txt"), "alpha\n");
    let path = scratch.path().to_path_buf();
    assert!(path.is_dir());
    drop(scratch);
    assert!(!path.exists(), "the scratch directory must be removed");
}

#[test]
fn should_pass_environment_and_working_directory_to_the_shell_when_configured() {
    // Proven against a program every system has, so the harness is verified before the shell
    // it will be used to verify exists.
    let scratch = ono_testkit::scratch();
    let run = Shell::program("/bin/sh")
        .args(["-c", "pwd; printf '%s\\n' \"$ONO_HARNESS_PROBE\""])
        .cwd(scratch.path())
        .env("ONO_HARNESS_PROBE", "alive")
        .run();
    run.assert_success();
    assert!(run.stdout().contains("alive"), "got {:?}", run.stdout());
    let reported = run.stdout().lines().next().unwrap_or_default().to_owned();
    assert_eq!(
        std::fs::canonicalize(reported).ok(),
        std::fs::canonicalize(scratch.path()).ok()
    );
}

#[test]
fn should_feed_standard_input_to_the_shell_when_given() {
    let run = Shell::program("/bin/cat").stdin("hello\nworld\n").run();
    run.assert_success();
    assert_eq!(run.stdout(), "hello\nworld\n");
}

#[test]
fn should_fail_the_test_rather_than_hang_when_a_run_exceeds_its_budget() {
    // A shell test that hangs is worse than one that fails: it stops the whole suite.
    let outcome = Shell::program("/bin/sh")
        .args(["-c", "sleep 30"])
        .timeout(std::time::Duration::from_millis(300))
        .try_run();
    assert!(
        outcome.is_err(),
        "an overrunning run must be reported, not awaited"
    );
}

#[test]
fn should_report_the_signal_that_killed_the_program_as_128_plus_it_when_it_is_signalled() {
    let run = Shell::program("/bin/sh")
        .args(["-c", "kill -TERM $$"])
        .run();
    assert_eq!(run.status().code(), 143);
    assert_eq!(run.status().signal(), Some(15));
}
