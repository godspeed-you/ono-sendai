//! `find event`: discovering an event without knowing when it happened (v0.5 §20.1, §20.2,
//! §20.3, §11.4, §28.1).
//!
//! §20.1 states the rule the whole suite is written against: "A user must not need to know the
//! exact timestamp before being able to discover an event." §20.3 gives it a spelling —
//! `find event <predicate>` — and one constraint on that spelling: it "reuses the existing `find`
//! verb and Ono expression semantics rather than inventing a new search language". So the tests
//! below never name an instant, and they check that the predicate is *evaluated* rather than
//! matched as text: a second conjunct no event satisfies must empty the answer, and a predicate
//! that is not an expression must be refused where a search language would have shrugged.
//!
//! Every event searched for is one the test itself caused. §17.1's action lifecycle is what a
//! session records about itself, and a mutation Ono made against a process the test owns is the
//! one kind of event a one-shot shell is guaranteed to have. Recording is on, so the events
//! outlive the invocation that made them and the invocation that searches has nothing but the
//! retained ledger to answer from.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::time::Duration;

use serde_yaml_ng::Value;

use support::{recording_shell, rows, text};

/// The `kind` of every row, which is what a predicate over `kind` is judged by.
fn kinds(rows: &[Value]) -> Vec<String> {
    rows.iter().map(|row| text(row, "kind")).collect()
}

/// The schema each row declares it is, read from the provenance every value carries.
fn schemas(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            support::field(row, "provenance.schema")
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

#[test]
fn should_find_an_event_when_a_predicate_names_it_and_no_timestamp_is_known() {
    // §20.1: the user must not need to know when it happened. The script below names no instant
    // at all — no `--since`, no `--until`, no coordinate — and the event is still reachable.
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(&home, "find event 'kind == \"action.completed\"' | to json");
    let found = rows(&run);
    assert!(
        !found.is_empty(),
        "v0.5 §20.1, §20.3: a predicate finds an event with no timestamp named anywhere, and the \
         event was recorded by an earlier session. Got {:?}",
        run.output()
    );
}

#[test]
fn should_answer_only_the_events_the_predicate_selects() {
    // §20.3: the predicate is the query. An answer that also carried the three other lifecycle
    // events would be a listing with a highlight rather than a search.
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(&home, "find event 'kind == \"action.completed\"' | to json");
    let found = kinds(&rows(&run));
    assert!(
        !found.is_empty() && found.iter().all(|kind| kind == "action.completed"),
        "v0.5 §20.3: `find event` answers the predicate and nothing else; the same action left \
         `action.requested`, `action.authorized` and `action.executed` behind it and none of them \
         satisfies this predicate. Got {found:?}"
    );
}

#[test]
fn should_read_a_compound_predicate_as_an_ono_expression() {
    // §20.3: "This reuses the existing `find` verb and Ono expression semantics rather than
    // inventing a new search language." Two runs distinguish an evaluated expression from a text
    // match: they differ only in a second conjunct, and the one no event satisfies must empty the
    // answer even though its first conjunct still matches every executed action.
    let home = support::home_with_a_recorded_action();
    let matched = recording_shell(
        &home,
        "find event 'kind == \"action.executed\" and source == \"ono.session\"' | to json",
    );
    let excluded = recording_shell(
        &home,
        "find event 'kind == \"action.executed\" and source == \"ono.recorder\"' | to json",
    );
    assert!(
        !rows(&matched).is_empty(),
        "v0.5 §20.3: a conjunction both terms satisfy answers. Got {:?}",
        matched.output()
    );
    assert!(
        rows(&excluded).is_empty(),
        "v0.5 §20.3: both conjuncts are evaluated, so a second term no event satisfies empties \
         the answer — the predicate is an Ono expression, not a search string. Got {:?}",
        excluded.output()
    );
}

#[test]
fn should_refuse_a_predicate_that_is_not_an_expression() {
    // §20.3 again, from the other side: a search language would accept `kind ===` as a phrase to
    // look for. Ono expression semantics cannot parse it, and the refusal says so rather than
    // silently answering with everything or with nothing.
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(&home, "find event 'kind ==='");
    assert!(
        !run.status().is_success() && run.output().contains("Ono-Sendai-E0001"),
        "v0.5 §20.3: a predicate that is not an Ono expression is a parse refusal, never an \
         ignored filter. Got {:?}",
        run.output()
    );
}

#[test]
fn should_return_a_stream_of_events_a_pipeline_continues() {
    // §20.3: "It returns `Stream<TemporalEvent>`", and §28.1 makes that an ordinary value: the
    // stage after it is `take`, with no special form and no second syntax for events.
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(
        &home,
        "find event 'kind == \"action.executed\"' | take 1 | to json",
    );
    let found = rows(&run);
    assert_eq!(
        found.len(),
        1,
        "v0.5 §28.1: `take 1` is an ordinary pipeline stage over the events. Got {:?}",
        run.output()
    );
    assert_eq!(
        schemas(&found),
        vec!["ono.temporal-event/1".to_owned()],
        "v0.5 §20.3, §35.1: what survives the pipeline is still an `ono.temporal-event/1`. Got \
         {:?}",
        run.output()
    );
    for field in ["source_time", "observed_at", "ingested_at"] {
        assert!(
            found[0][field].as_str().is_some(),
            "v0.5 §3.3: the event keeps its three instants apart, so `{field}` is on the value a \
             pipeline reads. Got {:?}",
            run.output()
        );
    }
}

#[test]
fn should_answer_the_bounded_window_when_no_predicate_is_given() {
    // §20.3's degenerate case, and it is the one that proves the answer is a stream of events
    // rather than a report about them: with nothing to select on, every retained event in the
    // window is a value, and every value declares itself an `ono.temporal-event/1`.
    let home = support::home_with_a_recorded_action();
    let run = recording_shell(&home, "find event | to json");
    let found = rows(&run);
    assert!(
        !found.is_empty(),
        "v0.5 §20.3: `find event` without a predicate answers the window. Got {:?}",
        run.output()
    );
    assert!(
        schemas(&found)
            .iter()
            .all(|schema| schema == "ono.temporal-event/1"),
        "v0.5 §20.3, §28.1: the answer is a stream of `ono.temporal-event/1` values. Got {:?}",
        schemas(&found)
    );
}

#[test]
fn should_narrow_the_answer_when_the_window_excludes_what_was_recorded() {
    // §20.2: the window bounds the search. The action is made, two seconds pass, and a
    // one-second window must then exclude what an hour still reaches — otherwise `--since` is
    // decoration and every answer is the whole ledger.
    //
    // What the narrow window excludes is the *action*, not everything: a shell with the recorder
    // running writes §8.1's coverage markers as it starts, so the second in which the question is
    // asked is never empty. Asserting emptiness would be asserting that the recorder does not
    // record (ADR-0777).
    let home = support::home_with_a_recorded_action();
    std::thread::sleep(Duration::from_secs(2));
    let hour = recording_shell(&home, "find event --since 1h | to json");
    let second = recording_shell(&home, "find event --since 1s | to json");
    let acted = |run: &ono_testkit::Run| {
        kinds(&rows(run))
            .iter()
            .any(|kind| kind.starts_with("action."))
    };
    assert!(
        acted(&hour),
        "v0.5 §20.2: an hour-wide window reaches an action recorded seconds ago. Got {:?}",
        hour.output()
    );
    assert!(
        !acted(&second),
        "v0.5 §20.2: `--since` narrows rather than decorates — the action is two seconds old, so \
         a one-second window does not reach it. Got {:?}",
        second.output()
    );
}
