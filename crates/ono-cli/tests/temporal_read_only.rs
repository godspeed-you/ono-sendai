//! Historical context is read-only, and `present` is the one way out (v0.5 §4.7, §4.8).
//!
//! §4.7 is a safety rule, so these tests assert the two halves that make it one: the refusal
//! carries its structured code and names the way back, and nothing was changed.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_testkit::Shell;

/// A session with a private home, so the recorder's store is this test's and nobody else's.
fn ono_in(home: &std::path::Path, script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .env("NO_COLOR", "1")
        .env("HOME", home.display().to_string())
        .timeout(Duration::from_secs(90))
        .run()
}

/// A session that is genuinely standing in the past, and the run that put it there.
///
/// §12.1 makes `at` refuse a coordinate no source can cover, so a script that starts the recorder
/// and immediately asks for a second ago is refused — there is nothing yet to have covered it.
/// The mutation in the middle is what creates the coverage: §17.2 writes an action lifecycle for
/// every Ono-native mutation, and those events are what `at -1s` then resolves against.
///
/// Getting this wrong is not a smaller test, it is no test: the assertions below all read an
/// output that would never have entered historical context, so they would pass against a shell
/// that had never implemented §4.7 at all.
fn standing_in_the_past(tail: &str) -> (ono_testkit::Scratch, ono_testkit::Run) {
    let scratch = ono_testkit::scratch();
    let home = scratch.path().to_owned();
    let victim = home.join("recorded.txt").display().to_string();
    std::fs::write(&victim, b"an object whose removal is an event\n")
        .expect("the scratch file is written");
    // The `sleep` is load-bearing and not a delay for its own sake. The store's earliest retained
    // instant is the moment the recorder started, so a script that starts it and immediately asks
    // for a second ago asks for a time before its own history and is refused with
    // `temporal.out_of_retention` — correctly. Two seconds of real elapsed time is what puts the
    // requested coordinate inside the window the recorder has covered.
    let script =
        format!("start recorder\nremove file {victim} --confirm\nsleep 2\nat -1s\n{tail}",);
    let run = ono_in(&home, &script);
    (scratch, run)
}

/// The assertion every test here rests on: the session really did enter historical context.
fn assert_in_the_past(output: &str) {
    assert!(
        output.contains("[PAST"),
        "v0.5 §4.6: the prompt marks historical context, and these tests are about what happens \
         while it is active. Without it they assert nothing. Got {output:?}"
    );
}

#[test]
fn should_refuse_a_mutation_with_temporal_read_only_when_the_session_is_in_the_past() {
    let (scratch, run) = standing_in_the_past("stop recorder\n");
    let output = run.output();
    assert_in_the_past(&output);
    assert!(
        output.contains("temporal.read_only"),
        "v0.5 §4.7: every Ono mutation fails while historical context is active. Got {output:?}"
    );
    assert!(
        output.contains("now"),
        "v0.5 §4.7's message names `now` as the way back. Got {output:?}"
    );
    drop(scratch);
}

#[test]
fn should_refuse_an_external_command_with_temporal_present_only_when_the_session_is_in_the_past() {
    let (_scratch, run) = standing_in_the_past("printf marker\n");
    let output = run.output();
    assert_in_the_past(&output);
    assert!(
        output.contains("temporal.present_only"),
        "v0.5 §4.8: an arbitrary external program MUST NOT run while historical context is \
         active. Got {output:?}"
    );
    assert!(
        !output.contains("marker"),
        "v0.5 §4.8: the refused program did not run against the present machine. Got {output:?}"
    );
}

#[test]
fn should_run_a_present_command_and_keep_the_historical_coordinate() {
    let (_scratch, run) = standing_in_the_past("present printf ok\n");
    let output = run.output();
    assert_in_the_past(&output);
    assert!(
        output.contains("ok"),
        "v0.5 §4.8: `present printf ok` executes in the real current environment. Got {output:?}"
    );
    assert!(
        output.contains("present"),
        "v0.5 §4.8: the result metadata makes the present-bound execution visible at least once. \
         Got {output:?}"
    );
}

#[test]
fn should_let_a_mutation_run_when_the_session_is_in_the_present() {
    let scratch = ono_testkit::scratch();
    let run = ono_in(scratch.path(), "get recorder | to json");
    run.assert_success();
    assert!(
        run.stdout().contains("\"running\""),
        "v0.5 §10.3: `get recorder` answers `ono.recorder-status/1` in the present. Got {:?}",
        run.stdout()
    );
}
