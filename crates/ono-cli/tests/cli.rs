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

#[test]
fn should_stop_quietly_when_the_reader_of_its_output_goes_away() {
    // `ono -c 'get process | to json' | head -c 200`: the reader closes the pipe after its two
    // hundred bytes. Every other shell stops there without a word; reporting the closed pipe as
    // an I/O failure turns an ordinary `| head` into a diagnostic on the user's terminal.
    let ono = env!("CARGO_BIN_EXE_ono");
    let scratch = ono_testkit::scratch();
    let errors = scratch.path().join("stderr.txt");
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{ono} -c 'get process | to json' 2>{} | head -c 200 >/dev/null",
            errors.display()
        ))
        .status()
        .expect("the shell runs");
    assert!(status.success(), "`head` finished normally");
    let written = std::fs::read_to_string(&errors).expect("the stderr file");
    assert!(
        written.trim().is_empty(),
        "a closed reader is not an error to report: {written:?}"
    );
}
