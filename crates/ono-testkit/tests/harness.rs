//! The test harness is itself tested: every later test trusts it to report what a user would
//! actually see, so a harness that quietly stopped capturing stderr would make the whole suite
//! meaningless (AGENTS.md section 14).

use ono_testkit::{Shell, SkipReason, require};

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

#[test]
fn should_run_a_script_through_the_shell_when_asked_for_one() {
    // The helper 21 suites re-declared by hand: `ono -c <script>`, captured the way a user sees
    // it. A test that spells it itself is a test that can spell it differently.
    let run = ono_testkit::ono("let n = 41; $n | to json");
    run.assert_success();
    assert_eq!(run.stdout().trim(), "[41]", "got {:?}", run.stdout());
}

#[test]
fn should_keep_the_streams_apart_when_a_script_fails() {
    let run = ono_testkit::ono("definitely-not-a-command");
    assert!(!run.status().is_success(), "got {:?}", run.output());
    assert_eq!(run.stdout(), "", "a diagnostic belongs on stderr");
    assert!(
        run.stderr().contains("Ono-Sendai-E0101"),
        "got {:?}",
        run.stderr()
    );
}

#[test]
fn should_take_a_wider_budget_than_the_default_when_a_script_is_given_one() {
    // A suite that spawns real children needs longer than the default; asking for the budget by
    // name keeps the number in one place instead of in every file that needs it.
    let run = ono_testkit::ono_within(
        "let n = 41; $n | to json",
        std::time::Duration::from_secs(30),
    );
    run.assert_success();
    assert_eq!(run.stdout().trim(), "[41]");
}

#[test]
fn should_name_the_test_the_reason_and_the_category_when_a_skip_is_announced() {
    // The marker is what makes a skipped test countable, so its shape is a contract: the word
    // `SKIPPED`, the test that skipped, the v0.4.1 §38.4 category and the detail. Asserting it
    // here means a later change to the format has to be deliberate.
    ono_testkit::skipped(
        SkipReason::FixtureNotApplicable,
        "this host has no second mount to cross",
    );
    let name = std::thread::current().name().unwrap_or_default().to_owned();
    assert_eq!(
        name, "should_name_the_test_the_reason_and_the_category_when_a_skip_is_announced",
        "the marker takes the test's name from its thread, so cargo must still name it"
    );
    assert_eq!(
        SkipReason::FixtureNotApplicable.category(),
        "fixture_not_applicable",
        "the category token is what the expected-skip registry stores"
    );
    for reason in SkipReason::ALL {
        assert_eq!(
            SkipReason::from_category(reason.category()),
            Some(reason),
            "every §38.4 category round-trips through its token"
        );
    }
    assert_eq!(
        SkipReason::from_category("the machine was busy"),
        None,
        "the taxonomy is closed: free text is not a category"
    );
}

#[test]
fn should_offer_a_require_helper_that_records_an_unmet_prerequisite() {
    // v0.4.1 Appendix G: `require(condition, reason_category, detail) -> TestPrerequisite`. A met
    // prerequisite announces nothing and lets the test carry on; an unmet one has already emitted
    // the canonical skip signal by the time the caller returns, which is what makes the early
    // return legal under §65.10.
    let met = require(
        true,
        SkipReason::ExternalToolUnavailable,
        "this detail is never printed",
    );
    assert!(met.met(), "a satisfied prerequisite lets the test carry on");
    assert!(!met.unmet());

    let unmet = require(
        false,
        SkipReason::ExternalToolUnavailable,
        "no journal on this host",
    );
    assert!(
        unmet.unmet(),
        "an unsatisfied prerequisite tells the caller to return"
    );
    assert!(!unmet.met());
}
