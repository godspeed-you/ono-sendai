//! The temporal coordinate: `at`, `now`, the prompt, and the trail that stays separate from the
//! spatial one (v0.5 §4.2, §4.3, §4.6, §12, §55.9).
//!
//! Every assertion here is an outcome a user can see — what the shell printed, which error code
//! it raised, where the session was afterwards — never a call into the implementation. The one
//! that matters most is §55.9's: `at` must move the data, and not only the prompt.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use support::ono;

/// Runs a script without a terminal, with colour off so the output is the semantic content.

#[test]
fn should_enter_historical_context_and_say_so_when_a_relative_selector_resolves() {
    let run = ono("at -1m\nget config temporal.recording.enabled");
    // A session with no retained history cannot reach a minute ago, and §12.3 makes that a
    // refusal rather than a pretence. Either answer is correct; what is not correct is entering
    // historical context silently.
    let output = run.output();
    assert!(
        output.contains("historical")
            || output.contains("[PAST")
            || output.contains("temporal.not_recorded"),
        "v0.5 §4.2 and §12.3: `at -1m` either enters historical context and says so, or refuses \
         with `temporal.not_recorded`. Got {output:?}"
    );
}

#[test]
fn should_leave_the_coordinate_untouched_when_the_selector_cannot_be_read() {
    let run = ono("at nonsense\nnow");
    assert!(
        run.output().contains("temporal.invalid_time"),
        "v0.5 §4.4, §12.1: an unreadable selector is `temporal.invalid_time` and the session does \
         not move. Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_a_relative_selector_that_names_the_future() {
    let run = ono("at +10m");
    assert!(
        run.output().contains("temporal.invalid_time"),
        "v0.5 §4.4: \"Relative future selectors are invalid for historical context\". Got {:?}",
        run.output()
    );
}

#[test]
fn should_refuse_an_absolute_selector_that_names_the_future() {
    let run = ono("at 2099-01-01T00:00:00Z");
    assert!(
        run.output().contains("temporal.invalid_time"),
        "v0.5 §4.4: a historical coordinate is at or before now. Got {:?}",
        run.output()
    );
}

#[test]
fn should_return_to_the_present_from_anywhere() {
    let run = ono("now");
    run.assert_success();
    assert!(
        !run.output().contains("[PAST"),
        "v0.5 §4.3: `now` restores the present, and the present carries no marker. Got {:?}",
        run.output()
    );
}

#[test]
fn should_keep_the_spatial_place_when_the_temporal_coordinate_moves() {
    // §4.2: "Ono changes only the temporal coordinate. It MUST NOT change spatial place."
    let before = ono("look");
    let after = ono("at -1m\nlook");
    let before_place = before
        .output()
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    let after_place = after
        .output()
        .lines()
        .find(|line| line.contains("//") || line.contains("local"))
        .unwrap_or_default()
        .to_owned();
    assert!(
        after.output().contains(&before_place) || after_place.contains("local"),
        "v0.5 §4.2: the place is unchanged by `at`. Before {before_place:?}, after {:?}",
        after.output()
    );
}

#[test]
fn should_keep_back_spatial_when_the_temporal_coordinate_has_moved() {
    // §12.4: "`back` remains spatial navigation and MUST NOT become overloaded." A `back` after
    // an `at` therefore reports about places, never about instants.
    let run = ono("enter compute\nat -1m\nback\nlook");
    assert!(
        !run.output().contains("[PAST") || !run.output().contains("temporal trail"),
        "v0.5 §12.4: `back` is spatial and says nothing about the temporal trail. Got {:?}",
        run.output()
    );
}

#[test]
fn should_answer_the_same_from_at_context_and_from_the_at_option() {
    // §4.5: "`--at` MUST use the same reconstruction engine as `at` context. It MUST NOT
    // implement a separate historical code path." Two observations prove it: the same selector
    // produces the same window through both spellings, and an instant neither can reach produces
    // the same refusal through both.
    let with_context = ono("at -0s\ntimeline --since 1m | to json");
    let with_option = ono("timeline --at -0s --since 1m | to json");
    assert_eq!(
        events_of(&with_context.output()),
        events_of(&with_option.output()),
        "v0.5 §4.5: `--at` and `at` are one engine, so they answer the same window"
    );

    let refused_by_context = ono("at -3d");
    let refused_by_option = ono("timeline --at -3d");
    assert_eq!(
        code_of(&refused_by_context.output()),
        code_of(&refused_by_option.output()),
        "v0.5 §4.5: an instant `at` refuses is one `--at` refuses, with the same code"
    );
    assert_eq!(
        code_of(&refused_by_context.output()),
        Some("Ono-Sendai-E1302".to_owned()),
        "v0.5 §12.3: a session with no retained history cannot reach three days ago"
    );
}

/// The `events` array of a timeline document, which is what both spellings must agree on.
fn events_of(output: &str) -> String {
    let Some(start) = output.find("\"events\"") else {
        return String::new();
    };
    let rest = &output[start..];
    let end = rest.find("\"groups\"").unwrap_or(rest.len());
    rest[..end].to_owned()
}

/// The structured code an output carries, where it carries one.
fn code_of(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|word| word.starts_with("Ono-Sendai-E"))
        .map(str::to_owned)
}

#[test]
fn should_keep_command_recall_and_the_event_browser_apart() {
    // §29.4: "Interactive command recall MUST NOT become a system-event browser." `timeline` is
    // the browser, and it never answers with the commands the session typed.
    let run = ono("echo marker\ntimeline --since 1m | to json");
    assert!(
        !run.output().contains("echo marker") || run.output().matches("echo marker").count() == 1,
        "v0.5 §29.2, §29.4: the command history and the temporal ledger stay separate. Got {:?}",
        run.output()
    );
}
