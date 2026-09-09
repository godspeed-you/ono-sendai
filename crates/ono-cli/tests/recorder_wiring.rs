//! The shell actually runs the recorder (spec v0.5 §10.6, §10.8, §17.2, §17.3, §44.1, §55.5).
//!
//! `crates/ono-cli/tests/recorder.rs` proves the recorder is opt-in and removable — that a shell
//! nobody asked creates nothing. This suite proves the other half, which is what makes the first
//! half worth having: once it *is* asked, something collects.
//!
//! Four claims, each of which was false while `start recorder` merely swapped a ledger:
//!
//! - **§44.1 step 3.** "Mark any unobserved downtime as a coverage gap." Between two runs of a
//!   recorder nothing was watching, and §55.5 names leaving that implicit as the failure that
//!   destroys operator trust: an unrepresented downtime reads as a quiet morning.
//! - **§10.6.** What the recorder collects is visible — and every source it names is a source
//!   some event in the ledger actually carries.
//! - **§6.1.** An object appearing while the session was watching is a typed `object.appeared`
//!   event, with the observation window §9.2 forbids collapsing into a point.
//! - **§17.2, §17.4.** `action.completed` and `action.failed` are different kinds, and the
//!   `ActionResult` is what tells them apart. An action whose every target failed did not succeed.
//!
//! Every test drives the real binary and reads what a pipeline can read (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::time::Duration;

use ono_testkit::{Scratch, scratch};
use serde_yaml_ng::Value;

/// The events `find event <predicate>` answered with (§20.3).
fn found(home: &Scratch, predicate: &str) -> Vec<Value> {
    let run = support::recording_shell(home, &format!("find event '{predicate}' | to json"));
    run.assert_success();
    array(&run)
}

/// The one JSON array a `to json` stage printed, whatever a stage before it drew.
///
/// A script that has to *make* something happen before it can query it prints what those commands
/// print, so the document is the last line rather than the whole of stdout.
fn array(run: &ono_testkit::Run) -> Vec<Value> {
    let line = support::last_line(run);
    let document: Value = serde_yaml_ng::from_str(&line).unwrap_or_else(|error| {
        panic!(
            "spec §33.5: `to json` emits a JSON document, got {line:?} ({error}); output {:?}",
            run.output()
        )
    });
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!("spec §33.5: `to json` emits the stream as an array, got {line:?}")
        })
        .clone()
}

/// A recording shell whose `temporal.session.max_events` is `ceiling` (§10.7, §33).
///
/// `ONO_TEMPORAL_SESSION_MAX_EVENTS` is the environment spelling of the setting, which is how a
/// test states a configuration without writing a file the shell then has to be told about.
fn bounded_shell(home: &Scratch, ceiling: &str, script: &str) -> ono_testkit::Run {
    let root = home.path().to_string_lossy().into_owned();
    ono_testkit::Shell::new()
        .env("NO_COLOR", "1")
        .env("HOME", root.clone())
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .env("ONO_TEMPORAL_SESSION_MAX_EVENTS", ceiling)
        .args(["-c", script])
        .timeout(Duration::from_secs(60))
        .run()
}

/// The `kind` of every event in `rows`, for a panic that says what was there instead.
fn kinds(rows: &[Value]) -> Vec<String> {
    rows.iter().map(|row| support::text(row, "kind")).collect()
}

#[test]
fn should_file_a_coverage_gap_when_the_recorder_starts_after_an_earlier_run() {
    // §44.1 runs five steps at every recorder start, and step 3 is the one everything else exists
    // for: "mark any unobserved downtime as a coverage gap". §55.5: an unrepresented downtime is
    // a silent gap, and a silent gap reads as a quiet morning. The gap reaches the ledger as the
    // `coverage.ended` marker §11.7 draws, so a pipeline can find it.
    let home = scratch();

    let first = found(&home, r#"kind == "coverage.ended""#);
    assert!(
        first.is_empty(),
        "v0.5 §44.1: the first run of a recorder has no downtime behind it, so it declares no \
         gap. Got {first:?}"
    );

    // Long enough that the interval between the two runs is a real one rather than a rounding
    // artefact, and short enough that the suite stays quick.
    std::thread::sleep(Duration::from_secs(2));

    let second = found(&home, r#"kind == "coverage.ended""#);
    assert!(
        !second.is_empty(),
        "v0.5 §44.1 step 3, §55.5: nothing was watching between the two runs, and a recorder that \
         joined the two sides of that interval would report a quiet morning. Got {:?}",
        kinds(&second)
    );
}

#[test]
fn should_produce_events_from_the_recorder_it_names_as_a_source_when_it_is_running() {
    // §10.6 makes what the recorder collects visible. A status that lists `ono.recorder` while
    // nothing under that name ever reaches the ledger is a claim about a collector that does not
    // exist — the same fabrication §55.1 warns about, made about Ono itself.
    let home = scratch();
    let run = support::recording_shell(&home, "get recorder | to json");
    run.assert_success();
    let status = support::single_result(&run);
    let sources: Vec<String> = status["sources"]
        .as_sequence()
        .unwrap_or_else(|| panic!("v0.5 §10.3: `sources` is a list, got {status:?}"))
        .iter()
        .map(|source| source.as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        !sources.is_empty(),
        "v0.5 §10.3, §10.6: a running recorder says what it collects from. Got {status:?}"
    );

    let markers = found(&home, r#"kind == "coverage.started""#);
    assert!(
        !markers.is_empty(),
        "v0.5 §8.1, §10.6: the recorder declares the coverage it opened, so the stretch it \
         watched is distinguishable from one nobody watched. Got {:?}",
        kinds(&markers)
    );
    let recorded: Vec<String> = found(&home, r#"kind != "nothing""#)
        .iter()
        .map(|event| support::text(event, "source"))
        .collect();
    for source in &sources {
        assert!(
            recorded.contains(source),
            "v0.5 §10.6: `get recorder` names `{source}` among what it collects from, so \
             something under that name has to have reached the ledger. Got {recorded:?}"
        );
    }
}

#[test]
fn should_record_an_appearance_as_a_typed_event_when_the_session_observes_one() {
    // §6.1 declares `object.appeared`, and §48.2's seventh scenario is "a process appearing is a
    // typed event". The shell sweeps the providers on every orientation; a second sweep that
    // finds an object the first one did not is an appearance the session observed, and §39.1
    // makes the bridge from that observation to a canonical event the shell's own job.
    //
    // The fixture is a real connection to a real listener this test owns, opened *after* the
    // first sweep, so the appearance cannot be a startup artefact (v0.4 §43.6).
    let home = scratch();
    let (listener, port) = support::listener();
    let accepted = std::thread::spawn(move || {
        listener
            .set_nonblocking(false)
            .expect("a blocking fixture listener");
        listener.accept().ok()
    });
    let connecting = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(4));
        std::net::TcpStream::connect(("127.0.0.1", port)).ok()
    });

    let run = support::recording_shell(
        &home,
        &format!(
            "enter socket {port}\n\
             look\n\
             sleep 8\n\
             look\n\
             find event 'kind == \"object.appeared\"' | to json"
        ),
    );
    let held = connecting.join().expect("the fixture client thread joins");
    let _ = accepted.join();
    assert!(
        held.is_some(),
        "the fixture connection has to be made, or there is no appearance to record"
    );
    run.assert_success();
    let appearances = array(&run);

    assert!(
        !appearances.is_empty(),
        "v0.5 §6.1, §48.2 scenario 7: a connection appeared while the session was watching, and \
         an appearance the shell observed is an `object.appeared` event. Got {:?}",
        run.output()
    );
    let first = &appearances[0];
    assert_eq!(
        support::text(first, "kind"),
        "object.appeared",
        "v0.5 §6.1: the predicate asked for one kind. Got {first:?}"
    );
    assert!(
        !first["evidence"]
            .as_sequence()
            .unwrap_or(&Vec::new())
            .is_empty(),
        "v0.5 §6.7, §7.2: an event carries the evidence it rests on. Got {first:?}"
    );
    assert!(
        !support::text(first, "observed_at").is_empty(),
        "v0.5 §3.3: the observation instant travels with the event. Got {first:?}"
    );
}

#[test]
fn should_record_the_action_as_failed_when_every_one_of_its_targets_failed() {
    // §17.2 gives `action.completed` and `action.failed` as separate kinds, and §17.4's
    // `ActionResult` is what tells them apart. A ledger that files a mutation whose every target
    // failed under `action.completed` with `result: "succeeded"` states that something succeeded
    // which did not — the worst thing an evidence ledger can do (§55.1, §55.5).
    let home = scratch();
    let doomed = 4_294_967_294_u32;

    let run = support::recording_shell(
        &home,
        &format!(
            "stop process {doomed}\n\
             find event 'kind == \"action.completed\" or kind == \"action.failed\"' | to json"
        ),
    );
    let events = array(&run);
    assert_eq!(
        events.len(),
        1,
        "v0.5 §17.2: one mutation closes with exactly one terminal lifecycle event. Got {:?}",
        run.output()
    );
    let event = &events[0];
    assert_eq!(
        support::text(event, "kind"),
        "action.failed",
        "v0.5 §17.2, §17.4: every target of the mutation failed, so the action failed. Got \
         {event:?}"
    );
    assert_eq!(
        support::text(&event["payload"], "result"),
        "failed",
        "v0.5 §17.4: the recorded `ActionResult` says what became of the action. Got {event:?}"
    );
}

#[test]
fn should_report_when_the_running_recorder_started_rather_than_null() {
    // §10.3 lists `since` among what `get recorder` states, and a running recorder that reports
    // `null` for it has answered the question "how long has this been collecting" with silence.
    // §30.1's intent is what makes that a defect rather than a cosmetic gap: persistent history
    // must be impossible to confuse with hidden surveillance, and a start nobody can date is
    // exactly the shape of a thing that has been on longer than the user thinks.
    let home = scratch();
    let run = support::recording_shell(&home, "get recorder | to json");
    run.assert_success();
    let status = support::single_result(&run);
    assert_eq!(
        status["running"].as_bool(),
        Some(true),
        "the fixture needs a running recorder before `since` means anything. Got {status:?}"
    );
    assert!(
        !status["since"].is_null(),
        "v0.5 §10.3, §30.1: a running recorder says when it started. Got {status:?}"
    );
}

#[test]
fn should_apply_the_configured_session_ceiling_when_more_events_are_recorded_than_it_allows() {
    // §10.7 bounds the in-memory session ledger and §33 makes `temporal.session.max_events` the
    // number. A ceiling `get recorder` reports and nothing enforces is worse than no ceiling at
    // all: the status states it as a fact about what this shell will keep.
    let home = scratch();
    let run = bounded_shell(
        &home,
        "4",
        "stop process 4294967294\n\
         stop process 4294967293\n\
         stop process 4294967292\n\
         get recorder | to json",
    );
    run.assert_success();
    let status = array(&run)
        .first()
        .cloned()
        .unwrap_or_else(|| panic!("v0.5 §10.3: `get recorder` answers with one record"));
    assert_eq!(
        status["session_max_events"].as_u64(),
        Some(4),
        "v0.5 §33: the status reports the configured ceiling, not the built-in default. Got \
         {status:?}"
    );
    let events = status["events"]
        .as_u64()
        .unwrap_or_else(|| panic!("v0.5 §10.3: `events` is a count. Got {status:?}"));
    assert!(
        events <= 4,
        "v0.5 §10.7: three mutations wrote twelve lifecycle events into a ledger bounded at four, \
         and the bound is what the ledger keeps. Got {events} in {status:?}"
    );
}
