#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! The real runner: a program, an argument vector and no shell (§12.3, §2.17, §43.6).
//!
//! The programs here are written by the test into its own scratch directory, so nothing depends
//! on what happens to be installed on the machine running the suite.

use std::time::{Duration, Instant};

use ono_change_core::ToolRunner;
use ono_recovery_btrfs::{METADATA_COVERAGE, ProcessRunner};
use ono_testkit::{executable_script, scratch, while_text_file_busy};

/// Whether this answer is the machine reporting a busy file rather than the provider answering.
///
/// `cargo test` runs a crate's tests in threads of one process, so a sibling thread that forks
/// between this one's `open` and `close` inherits the write descriptor and `execve` answers
/// `ETXTBSY` until the child execs. It is not a finding about the runner, and issue #27 is the
/// same race one crate over.
fn text_file_busy<T>(outcome: &Result<T, ono_value::ErrorValue>) -> bool {
    outcome.as_ref().err().is_some_and(|error| {
        error
            .help()
            .is_some_and(|help| help.contains("Text file busy"))
    })
}

#[test]
fn should_pass_a_hostile_argument_through_as_one_argument() {
    let scratch = scratch();
    executable_script(scratch.path(), "repeat", "#!/bin/sh\nprintf '%s' \"$1\"\n");
    let runner = ProcessRunner::searching(scratch.path().to_string_lossy().into_owned());
    let hostile = "@var; rm -rf /";
    let output = while_text_file_busy(text_file_busy, || runner.run("repeat", &[hostile]))
        .expect("the program runs");
    assert_eq!(
        output.stdout(),
        hostile,
        "§12.3 and §43.6: a subvolume name containing shell syntax arrives as one argument, \
         because there is no shell between the provider and the program"
    );
    assert!(output.succeeded());
}

#[test]
fn should_report_a_program_that_ran_and_failed_as_output_rather_than_an_error() {
    let scratch = scratch();
    executable_script(
        scratch.path(),
        "refuse",
        "#!/bin/sh\necho 'ERROR: Not a Btrfs subvolume: Invalid argument' >&2\nexit 1\n",
    );
    let runner = ProcessRunner::searching(scratch.path().to_string_lossy().into_owned());
    let output = while_text_file_busy(text_file_busy, || {
        runner.run("refuse", &["subvolume", "show", "/tmp"])
    })
    .expect(
        "a program that ran and failed is an answer, not a runner failure: `Not a Btrfs \
         subvolume` is what Appendix B.9's question looks like when the answer is no",
    );
    assert!(!output.succeeded());
    assert!(output.stderr().contains("Not a Btrfs subvolume"));
}

#[test]
fn should_refuse_a_program_that_is_not_on_the_search_path() {
    let scratch = scratch();
    let runner = ProcessRunner::searching(scratch.path().to_string_lossy().into_owned());
    assert!(
        !runner.is_available("btrfs"),
        "§54.4: a provider whose tool is absent degrades rather than pretending"
    );
    let error = runner
        .run("btrfs", &["--version"])
        .expect_err("a program that is not there cannot have run");
    assert_eq!(error.code().name(), "recovery.provider_unavailable");
}

#[test]
fn should_run_a_program_with_a_predictable_environment() {
    let scratch = scratch();
    executable_script(
        scratch.path(),
        "environment",
        "#!/bin/sh\nprintf '%s' \"$LC_ALL\"\n",
    );
    let runner = ProcessRunner::searching(scratch.path().to_string_lossy().into_owned());
    let output = while_text_file_busy(text_file_busy, || runner.run("environment", &[]))
        .expect("the program runs");
    assert_eq!(
        output.stdout(),
        "C",
        "every parser in this crate reads English field names, and a localised `btrfs` would \
         rename `Subvolume ID` underneath a provider that never asked (Appendix G.4)"
    );
}

#[test]
fn should_say_which_file_metadata_a_selective_restore_does_not_put_back() {
    let coverage = METADATA_COVERAGE;
    assert!(
        coverage.content && coverage.mode,
        "a copy inside one filesystem carries the bytes and the permission bits"
    );
    assert_eq!(
        coverage.gaps(),
        vec![
            "owner/group",
            "ACLs",
            "extended attributes",
            "file capabilities",
            "SELinux labels",
            "hard-link relationships"
        ],
        "Appendix C.7: missing metadata support reduces recovery coverage and MUST be visible. A \
         configuration file returned without its SELinux label has not been returned"
    );
}

#[test]
fn should_stop_waiting_for_a_program_that_does_not_finish_within_its_bound() {
    let runner = ProcessRunner::new().within(Duration::from_millis(300));
    let started = Instant::now();
    let error = runner
        .run("sleep", &["30"])
        .expect_err("a program still running at the bound is refused rather than waited for");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "ADR-0805: the wait is bounded — a btrfs command blocked in the kernel must not hang the \
         shell, and this one returned after {:?}",
        started.elapsed()
    );
    assert_eq!(error.code().name(), "recovery.provider_unavailable");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("did not finish")),
        "the refusal says why the program stopped"
    );
}

#[test]
fn should_bound_the_wait_by_default() {
    assert!(
        ProcessRunner::new().timeout() <= Duration::from_secs(60),
        "ADR-0805: the real runner waits a bounded time unless told otherwise"
    );
}
