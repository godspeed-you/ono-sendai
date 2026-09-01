//! Backgrounded native pipelines (spec §18.4, ADR-0024): a live view the user cannot see, list
//! or stop would be the worst kind of background work — so `&` makes a job, the same table as
//! an external command.

use ono_testkit::ono;

#[test]
fn should_background_a_watch_as_a_job_the_shell_lists() {
    let run = ono("watch process --every 200ms &; jobs");
    run.assert_success();
    let text = run.output();
    assert!(
        text.contains("[%1]") && text.contains("watch process"),
        "the watch appears in the job table under a number (spec §18.4), got {text:?}"
    );
    assert!(
        text.contains("running"),
        "a live stream is running until it is stopped, got {text:?}"
    );
}

#[test]
fn should_share_one_number_space_between_native_and_external_jobs() {
    // `fg 2` must never be ambiguous: whichever kind the job is, its number names it alone.
    let run = ono("watch process --every 200ms &; sleep 5 &; jobs");
    run.assert_success();
    let text = run.output();
    assert!(
        text.contains("[%1]") && text.contains("[%2]"),
        "two jobs, two distinct numbers from one sequence, got {text:?}"
    );
}

#[test]
fn should_finish_a_bounded_background_pipeline_and_say_so() {
    let run = ono("get process | count &; sleep 0.4; jobs");
    run.assert_success();
    let text = run.output();
    assert!(
        text.contains("done") && text.contains("count"),
        "a bounded pipeline finishes and the table says so, got {text:?}"
    );
}
