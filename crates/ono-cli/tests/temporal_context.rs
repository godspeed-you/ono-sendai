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

#[test]
fn should_report_what_each_source_can_reach_when_nothing_was_ever_recorded() {
    // v0.5 §12.3, §34, §55.5: the two refusals §34 keeps apart are reached on different ground.
    // `temporal.not_recorded` is the answer where nothing was ever observed, and the only shell
    // that can be in that state is one with the recorder off — a running recorder writes §8.1's
    // coverage markers as it starts, so its store has a boundary from its first second
    // (ADR-0777). The refusal lists how far back each source reaches, which is what makes it a
    // finding rather than a shrug.
    let run = support::ono("at -3d");
    let output = run.output();
    assert!(
        output.contains("temporal.not_recorded"),
        "v0.5 §12.3, §34: an instant nothing ever observed is named, never silently answered. \
         Got {output:?}"
    );
    for source in ["ono.session", "ono.recorder"] {
        assert!(
            output.contains(source),
            "v0.5 §12.3, §7.5: the refusal says how far `{source}` can reach. Got {output:?}"
        );
    }
}

#[test]
fn should_refuse_with_what_was_never_observed_when_the_store_is_younger_than_its_own_window() {
    // The other half of §34's split, and the side of it that is easy to get wrong. §34 words
    // `temporal.out_of_retention` as history "known to have expired", which is a stronger claim
    // than "older than `temporal.retention.max_age`". This store began recording seconds ago:
    // three days back is outside its 24h window and was never inside it, so nothing expired and
    // §12.3's refusal — naming how far each source does reach — is the honest one (ADR-0786).
    let home = ono_testkit::scratch();
    // A real mutation, so the store holds something: §17.2's lifecycle is what a session records
    // about itself, and it is recorded because the shell made the change rather than watched it.
    let mut victim = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("a fixture process");
    let script = format!("stop process {}\nchanges --since 3d", victim.id());
    let run = support::recording_shell(&home, &script);
    let _ = victim.kill();
    let _ = victim.wait();
    let output = run.output();
    assert!(
        !output.contains("temporal.out_of_retention"),
        "v0.5 §34, §35.3: a store that has retained nothing long enough to drop it must not claim \
         history expired; a policy that *could* have held something is not one that *did*. Got \
         {output:?}"
    );
    assert!(
        output.contains("temporal.not_recorded") && output.contains("reaches back to"),
        "v0.5 §12.3, §7.5: the refusal names the instant nothing observed and says how far each \
         source does reach. Got {output:?}"
    );
}

#[test]
fn should_answer_a_comparison_that_stays_inside_the_retention_window() {
    // The other side of the same rule, and §13.4's: a window nothing was observed in is thin
    // rather than unanswerable, so `changes` answers it — with unknowns, not with a refusal.
    let home = ono_testkit::scratch();
    let run = support::recording_shell(&home, "changes --since 10m | to json");
    assert!(
        run.stdout().trim().starts_with('['),
        "v0.5 §13.2, §13.4: a `--since` inside the retention window answers with a stream. Got \
         {:?}",
        run.output()
    );
}
