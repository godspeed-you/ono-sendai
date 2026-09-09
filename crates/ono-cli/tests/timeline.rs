//! `timeline`: the chronological projection, its scope, its rendering and its references
//! (v0.5 §11.2, §11.3, §11.4, §11.5, §11.6, §11.7, §5.2, §6.1, §8.5, §35.1).
//!
//! §11.1 says what `timeline` is for, and §55.1 says what it must not become: "a pretty log
//! viewer". The difference between the two is entirely in what a test can observe. A log viewer
//! prints lines; `timeline` answers with typed values, scoped to where the session stands, each
//! one carrying a reference the next command accepts, and states the window and the coverage the
//! answer rests on instead of implying completeness.
//!
//! Every event on the timelines below was caused by the test that reads it: §17.1's action
//! lifecycle is what a session records about itself, and stopping a `sleep` the test started is a
//! real Ono mutation against a real Linux process. Recording is on, so the events survive the
//! invocation that made them — which is what lets a later shell be the one that asks.
//!
//! Ono has no `$(…)` command substitution (ADR-0019), so a fixture pid is interpolated into the
//! script from Rust rather than read inside it.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_testkit::Run;
use serde_yaml_ng::Value;

use support::{recording_shell, text};

/// The events a `timeline … | to json` answered with.
///
/// §11.4's value is a stream of `ono.temporal-event/1`, so the rows *are* the events.
fn events(run: &Run) -> Vec<Value> {
    support::last_json_rows(run)
}

/// The `@e…` references the default rendering printed, in the order it printed them (§11.6).
fn references(run: &Run) -> Vec<String> {
    run.stdout()
        .split_whitespace()
        .filter(|word| word.starts_with("@e") && word.len() > 2)
        .map(str::to_owned)
        .collect()
}

#[test]
fn should_answer_with_typed_events_when_the_shell_has_changed_something() {
    // §11.4: the value is `ono.temporal-event/1`, and §55.1's pretty log viewer is exactly what a
    // stream of lines would be. §3.3 keeps three instants apart on every event — when the source
    // says it happened, when Ono saw it, when Ono stored it — and a renderer that had flattened
    // them into one printed time would have lost the distinction the whole evidence model rests on.
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(&home, "timeline --since 5m | to json");
    let shown = events(&run);
    assert!(
        !shown.is_empty(),
        "v0.5 §11.1: the timeline shows what happened, and an Ono mutation happened. Got {:?}",
        run.output()
    );
    for event in &shown {
        assert_eq!(
            support::field(event, "provenance.schema").as_str(),
            Some("ono.temporal-event/1"),
            "v0.5 §11.4, §35.1: every value on the timeline is an `ono.temporal-event/1`. Got \
             {event:?}"
        );
        for field in ["source_time", "observed_at", "ingested_at"] {
            assert!(
                event[field].as_str().is_some(),
                "v0.5 §3.3: `{field}` is a field of its own on the event, not a rendered time. \
                 Got {event:?}"
            );
        }
        assert!(
            !text(event, "kind").is_empty(),
            "v0.5 §6.1: the event names one of the canonical kinds, so a pipeline can filter on \
             it. Got {event:?}"
        );
    }
}

#[test]
fn should_identify_the_process_it_acted_on_by_its_lifetime_rather_than_by_its_pid() {
    // §5.2: an object bound to a lifetime is identified by that lifetime, not by the number the
    // kernel lends it. The identity Ono acts on therefore carries the pid *and* the instant the
    // process started, which is the whole reason a pid reused after this process exits is a
    // different object rather than the same one seen again (§5.2, §5.4).
    let home = ono_testkit::scratch();
    let mut victim = support::fixture_process();
    let pid = victim.id();
    let run = recording_shell(&home, &format!("stop process {pid} | to json"));
    let _ = victim.kill();
    let _ = victim.wait();
    let target = text(&support::single_result(&run), "target");
    assert!(
        target.contains(&pid.to_string()),
        "v0.5 §5.1: spatial identity stays authoritative, so the object acted on is the process \
         asked for. Got {target:?}"
    );
    assert!(
        target.contains('T') && target.contains('Z') && target.contains('-'),
        "v0.5 §5.2: the identity is a lifetime — the pid together with the instant the process \
         started — so a later process holding the same pid is a different identity and not this \
         one again. Got {target:?}"
    );
}

#[test]
fn should_scope_the_timeline_to_the_current_place_when_no_selector_is_given() {
    // §11.3: "Without a selector, `timeline` is scoped to the current spatial place and its
    // directly relevant events. At the root system place, it shows high-significance events and
    // current-session actions rather than dumping every event from every object." The control is
    // in the same test on purpose: an assertion that a place shows nothing proves nothing unless
    // the same ledger shows something somewhere else.
    let home = support::home_with_a_recorded_action();
    let at_root = recording_shell(&home, "timeline --since 5m | to json");
    let elsewhere = recording_shell(&home, "enter identity\ntimeline --since 5m | to json");
    assert!(
        !events(&at_root).is_empty(),
        "v0.5 §11.3: at the root place the session's own actions are on the timeline. Got {:?}",
        at_root.output()
    );
    assert!(
        events(&elsewhere).is_empty(),
        "v0.5 §11.3, §55.1: a place with no events of its own does not inherit the root's — the \
         default timeline is a place and a window, never the firehose. Got {:?}",
        elsewhere.output()
    );
}

#[test]
fn should_widen_the_timeline_to_the_visible_scope_when_all_is_given() {
    // §11.3: "`--all` requests the full visible scope subject to retention and permissions." The
    // same place that answers empty by default must answer with the ledger's events under
    // `--all`, or the narrowing above would be a loss of reach rather than a choice of scope.
    let home = support::home_with_a_recorded_action();
    let narrowed = recording_shell(&home, "enter identity\ntimeline --since 5m | to json");
    let widened = recording_shell(&home, "enter identity\ntimeline --all --since 5m | to json");
    assert!(
        events(&narrowed).is_empty() && !events(&widened).is_empty(),
        "v0.5 §11.3: `--all` reaches from the same place what the default scope left out. \
         Default gave {:?}, `--all` gave {:?}",
        narrowed.output(),
        widened.output()
    );
}

#[test]
fn should_restrict_the_timeline_to_the_kind_it_was_asked_for() {
    // §11.2's `--kind`, over §6.1's closed list. One action leaves four kinds behind it, so a
    // filter that narrowed nothing would still look plausible on a single row — this asks for the
    // one kind and checks the other three are gone.
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(
        &home,
        "timeline --since 5m --kind action.executed | to json",
    );
    let shown: Vec<String> = events(&run)
        .iter()
        .map(|event| text(event, "kind"))
        .collect();
    assert!(
        !shown.is_empty() && shown.iter().all(|kind| kind == "action.executed"),
        "v0.5 §11.2, §6.1: `--kind` restricts to the canonical kind named, and the same action's \
         `requested`, `authorized` and `completed` events are not it. Got {shown:?}"
    );
}

#[test]
fn should_refuse_a_kind_that_is_not_one_of_the_canonical_ones() {
    // §6.1 is a closed list, so an unknown kind is a refusal that names the list rather than a
    // filter that quietly matches nothing — which would read as "this never happened".
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(&home, "timeline --since 5m --kind nonsense.kind");
    assert!(
        !run.status().is_success() && run.output().contains("object.changed"),
        "v0.5 §6.1: a kind outside the canonical list is refused, and the refusal names the \
         kinds that exist. Got {:?}",
        run.output()
    );
}

#[test]
fn should_render_a_row_per_event_carrying_its_source_and_its_reference() {
    // §11.5 fixes the default rendering: a row per event, with an abbreviated but inspectable
    // source tag, and §11.6 requires the row to expose a reference usable in the next command.
    // §45.2 is checked beside them because a meaning that lived in an escape sequence would be
    // invisible to everything that is not a colour terminal.
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(&home, "timeline --since 5m");
    let printed = run.stdout().to_owned();
    assert!(
        !references(&run).is_empty(),
        "v0.5 §11.6: a rendered row exposes an `@e…` reference. Got {printed:?}"
    );
    assert!(
        printed.contains("[ono]"),
        "v0.5 §11.5: the row carries an abbreviated source tag, so a reader can tell who saw it. \
         Got {printed:?}"
    );
    assert!(
        !printed.contains('\u{1b}'),
        "v0.5 §45.2: no escape sequence carries the meaning of a row. Got {printed:?}"
    );
}

#[test]
fn should_carry_a_rendered_reference_into_inspect_at_and_why() {
    // §11.6: "Rendered events MUST expose stable references usable in subsequent commands", with
    // `inspect event`, `at event` and `why event` named as the three. Stability is what the two
    // invocations prove: the reference is read out of one shell's rendering and handed to a
    // different shell, which resolves it against the retained ledger rather than a session table.
    let home = support::home_with_a_recorded_action();
    let rendered = recording_shell(&home, "timeline --since 5m");
    let reference = references(&rendered).pop().unwrap_or_else(|| {
        panic!(
            "v0.5 §11.6: a rendered row prints a reference. Got {:?}",
            rendered.output()
        )
    });

    let inspected = recording_shell(&home, &format!("inspect event {reference} | to json"));
    assert_eq!(
        support::rows(&inspected).len(),
        1,
        "v0.5 §11.6: `inspect event {reference}` resolves the reference a later session read off \
         the rendering. Got {:?}",
        inspected.output()
    );

    let stood_at = recording_shell(&home, &format!("at event {reference}"));
    assert!(
        stood_at.status().is_success()
            && stood_at
                .output()
                .contains(reference.trim_start_matches('@')),
        "v0.5 §11.6, §12.2: `at event {reference}` moves the coordinate to that event and says \
         which event it stood at. Got {:?}",
        stood_at.output()
    );

    let explained = recording_shell(&home, &format!("why event {reference}"));
    assert!(
        explained.status().is_success() && explained.output().contains("cause"),
        "v0.5 §11.6, §16.2: `why event {reference}` explains that event, and §15.7 makes an \
         unknown cause a stated one rather than a silence. Got {:?}",
        explained.output()
    );
}

#[test]
fn should_refuse_an_event_reference_naming_nothing_retained() {
    // The other half of §11.6, and §55.3's: a reference nobody can resolve is refused by name.
    // Answering the nearest event, or an empty result, would let a typo read as an explanation.
    let home = support::home_with_a_recorded_action();
    for command in ["inspect event", "at event", "why event"] {
        let run = recording_shell(&home, &format!("{command} @edeadbeefdeadbeef"));
        assert!(
            !run.status().is_success() && run.output().contains("temporal.invalid_time"),
            "v0.5 §11.6, §34: `{command}` refuses a reference no retained event answers to, \
             rather than choosing a neighbour. Got {:?}",
            run.output()
        );
    }
}

#[test]
fn should_state_the_window_and_the_coverage_the_answer_rests_on() {
    // §11.2 makes the timeline a statement about an interval, and §8.5 makes the coverage behind
    // it part of what is said. §11.4 fixes where each of those lives: the *value* is a stream of
    // events, and "the timeline renderer is only a presentation" — so the interval and the
    // coverage are said by the rendering, not carried in the stream a pipeline filters
    // (ADR-0778). A reader who sees rows and no interval has been told the rows are everything.
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(&home, "timeline --since 5m");
    let rendered = run.output();
    assert!(
        rendered.contains("window:"),
        "v0.5 §11.2: the rendering declares the window the rows are a statement about. Got \
         {rendered:?}"
    );
    assert!(
        rendered.contains("coverage:"),
        "v0.5 §8.5: the rendering carries the coverage that backs the rows rather than implying \
         completeness. Got {rendered:?}"
    );
}

#[test]
fn should_stay_a_stream_a_later_stage_can_filter_when_the_timeline_is_piped() {
    // §11.4 fixes the output type as `Stream<TemporalEvent>` and writes the pipeline out as a
    // MUST. It is the one rule that decides the shape of everything above: a wrapper record would
    // make `where kind == …` a type error, which is what it used to be.
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(
        &home,
        "timeline --since 5m | where kind == \"action.executed\" | to json",
    );
    let rows = support::last_json_rows(&run);
    assert!(
        !rows.is_empty(),
        "v0.5 §11.4: `timeline --since 1h | where kind == …` MUST work. Got {:?}",
        run.output()
    );
    assert!(
        rows.iter()
            .all(|row| text(row, "kind") == "action.executed"),
        "v0.5 §11.4: the stage filters the events it was handed. Got {:?}",
        run.output()
    );
}
