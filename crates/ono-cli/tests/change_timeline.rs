//! v0.6's history is v0.5's ledger (spec v0.6 §22, §29.2, §55.11 cases 48–50).
//!
//! §22.1 is the whole of this suite's subject: a plan's lifecycle is recorded as events on the
//! v0.5 ledger, under the seventeen canonical kinds v0.5 §17.2 closes, distinguished by an
//! `ono.plan.*` subtype. There is no second audit subsystem, and §22.4 makes the plan identity
//! the causal anchor the two histories join on.
//!
//! Every test drives the real binary over one script, because that is the only way to observe
//! what one shell wrote and then read back: the session ledger is in-memory unless a recorder is
//! running (v0.5 §10.2), so a second process would legitimately see nothing.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{home, ono_at};
use ono_testkit::Run;
use serde_yaml_ng::Value;

/// v0.5 §17.2's seventeen canonical kinds. v0.6 adds none of them.
const CANONICAL: [&str; 17] = [
    "object.appeared",
    "object.disappeared",
    "object.changed",
    "object.observed",
    "state.transition",
    "action.requested",
    "action.authorized",
    "action.executed",
    "action.completed",
    "action.failed",
    "resource.threshold",
    "config.changed",
    "boot.started",
    "boot.completed",
    "clock.stepped",
    "link.established",
    "link.lost",
];

/// Plans a file copy, applies it, and answers with every event the shell recorded.
///
/// One script and therefore one process: `plan` writes `PlanCreated` and `PlanSealed`, `apply`
/// writes the rest, and `find event` reads the ledger both of them wrote to. That they are the
/// same ledger is half of what this suite exists to prove (§29.2).
fn applied_run(home: &std::path::Path) -> Run {
    let source = home.join("source.txt");
    let destination = home.join("destination.txt");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let run = ono_at(
        home,
        &format!(
            "plan copy file {} {} --overwrite | apply\nfind event | to json",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    run
}

/// The events a run printed, read out of the JSON array that follows everything else.
///
/// `plan … | apply` prints its own action results first, so the stream is not the whole of
/// stdout — the array `to json` emitted is, and it starts at the first `[{`.
fn events_of(run: &Run) -> Vec<Value> {
    let out = run.stdout();
    let Some(start) = out.find("[{") else {
        return Vec::new();
    };
    serde_yaml_ng::from_str(&out[start..]).unwrap_or_default()
}

/// The `(kind, subtype)` of every recorded event.
fn recorded(run: &Run) -> Vec<(String, String)> {
    events_of(run)
        .into_iter()
        .filter_map(|event| {
            let kind = event["kind"].as_str()?.to_owned();
            let subtype = event["subtype"].as_str().unwrap_or_default().to_owned();
            Some((kind, subtype))
        })
        .collect()
}

#[test]
fn should_record_the_plan_lifecycle_on_the_v05_ledger() {
    // §55.11 case 48 and §22.1: the plan's own history, from creation to verification.
    let home = home();
    let events = recorded(&applied_run(home.path()));
    for wanted in [
        "ono.plan.created",
        "ono.plan.sealed",
        "ono.plan.action.started",
        "ono.plan.action.completed",
        "ono.plan.verification.observed",
        "ono.plan.verified",
    ] {
        assert!(
            events.iter().any(|(_, subtype)| subtype == wanted),
            "v0.6 §22.1 names `{wanted}` as a plan lifecycle event, and it is not in the ledger. \
             Got {events:?}"
        );
    }
}

#[test]
fn should_record_that_a_plan_protected_itself_exactly_when_it_did() {
    // §22.1's `PlanProtected` is a fact about a recovery asset that exists (§4.6). The scratch
    // home a test runs in may sit on a filesystem no provider can protect — §11.2 refuses a
    // tmpfs target rather than inventing coverage — so the event is required against what the
    // run actually created rather than unconditionally. Neither direction may be silent: an
    // asset with no event is history missing, and an event with no asset is history invented.
    let home = home();
    let run = applied_run(home.path());
    let events = recorded(&run);
    let recorded_protection = events
        .iter()
        .any(|(_, subtype)| subtype == "ono.plan.protected");

    let assets = ono_at(home.path(), "get recovery | to json");
    assets.assert_success();
    let created = assets.stdout().contains("\"state\":");

    assert_eq!(
        recorded_protection,
        created,
        "§22.1 and §4.6: `ono.plan.protected` is written for exactly the runs that created a \
         recovery asset. Events {events:?}, assets {:?}",
        assets.stdout()
    );
}

#[test]
fn should_add_no_event_kind_of_its_own_to_the_closed_list() {
    // §22.1 and v0.5 §17.2: the kinds are a closed list of seventeen, extended by `subtype` and
    // never by a new word. A v0.6 kind would be a second history in the same table.
    let home = home();
    for (kind, subtype) in recorded(&applied_run(home.path())) {
        assert!(
            CANONICAL.contains(&kind.as_str()),
            "v0.5 §17.2 closes the kinds at seventeen, and `{kind}` (subtype `{subtype}`) is not \
             one of them"
        );
    }
}

#[test]
fn should_carry_the_plan_identity_on_every_event_it_records() {
    // §22.4: the plan id is the causal anchor, so an event nobody can join to a plan is an event
    // `timeline --plan` and `why` cannot reach.
    let home = home();
    let run = applied_run(home.path());
    let planned: Vec<Value> = events_of(&run)
        .into_iter()
        .filter(|event| {
            event["subtype"]
                .as_str()
                .is_some_and(|subtype| subtype.starts_with("ono.plan."))
        })
        .collect();
    assert!(!planned.is_empty(), "the run recorded no plan event at all");
    for event in &planned {
        let plan = event["payload"]["plan"].as_str().unwrap_or_default();
        assert!(
            plan.len() == 16,
            "v0.6 §22.4: every plan event carries the plan identity it is anchored to. \
             Got {event:?}"
        );
    }
}

#[test]
fn should_leave_one_creation_event_for_one_creation() {
    // §6.8 makes an event's identity a content digest, so two `PlanCreated` events differing
    // only by their instant are both kept — and a reader counting creations would find two for
    // one plan. `apply` continues a lifecycle rather than starting a second one.
    let home = home();
    let events = recorded(&applied_run(home.path()));
    let created = events
        .iter()
        .filter(|(_, subtype)| subtype == "ono.plan.created")
        .count();
    assert_eq!(created, 1, "one plan was created once. Got {events:?}");
}

#[test]
fn should_record_no_plan_event_when_nothing_was_planned() {
    // §2.1's other half: a shell that planned nothing has no plan history, and an empty ledger
    // is the honest answer rather than a session marker standing in for one.
    let home = home();
    let run = ono_at(home.path(), "find event | to json");
    run.assert_success();
    let events = recorded(&run);
    let _ = &events;
    assert!(
        events
            .iter()
            .all(|(_, subtype)| !subtype.starts_with("ono.plan.")),
        "nothing was planned, so nothing may claim a plan happened. Got {events:?}"
    );
}

#[test]
fn should_reach_the_plans_events_through_the_timeline() {
    // §55.11 case 48: the events are not merely in the ledger, they are in the view an operator
    // reads history through.
    let home = home();
    let source = home.path().join("source.txt");
    let destination = home.path().join("destination.txt");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let run = ono_at(
        home.path(),
        &format!(
            "plan copy file {} {} --overwrite | apply\ntimeline",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    assert!(
        run.stdout().contains("ono.plan."),
        "v0.6 §22.1: a plan's lifecycle is visible in the v0.5 timeline, not only in the store. \
         Got {:?}",
        run.stdout()
    );
}
