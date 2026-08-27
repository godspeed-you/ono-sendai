//! Outcome tests for the `ono` binary.
//!
//! These assert what a user observes: printed text and exit status. They must survive any
//! restructuring of the implementation behind them (AGENTS.md section 11).

use ono_testkit::Shell;

#[test]
fn should_print_name_and_version_when_asked_for_the_version() {
    let run = Shell::new().args(["--version"]).run();
    run.assert_success();

    let text = run.stdout();
    assert!(
        text.starts_with("ono "),
        "the version line must name the binary, got {text:?}"
    );
    assert_eq!(
        text.trim().split(' ').count(),
        2,
        "the version line must be `ono <version>`, got {text:?}"
    );
}

#[test]
fn should_describe_its_usage_when_asked_for_help() {
    let run = Shell::new().args(["--help"]).run();
    run.assert_success();
    run.assert_stdout_contains("usage: ono");
}

#[test]
fn should_fail_with_a_usage_error_when_given_unknown_arguments() {
    Shell::new()
        .args(["--definitely-not-a-flag"])
        .run()
        .assert_status(2);
}
