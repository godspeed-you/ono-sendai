//! Outcome tests for the `ono` binary.
//!
//! These assert what a user observes: printed text and exit status. They must survive any
//! restructuring of the implementation behind them (AGENTS.md section 11).

use ono_testkit::{run_ono, stdout_of};

#[test]
fn should_print_name_and_version_when_asked_for_the_version() {
    let output = run_ono(&["--version"]);

    assert!(output.status.success(), "--version must exit successfully");
    let text = stdout_of(&output);
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
    let output = run_ono(&["--help"]);

    assert!(output.status.success(), "--help must exit successfully");
    let text = stdout_of(&output);
    assert!(
        text.contains("usage: ono"),
        "help must show a usage line, got {text:?}"
    );
}

#[test]
fn should_fail_with_a_usage_error_when_given_unknown_arguments() {
    let output = run_ono(&["--definitely-not-a-flag"]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "an unusable command line must exit with the usage status"
    );
}
