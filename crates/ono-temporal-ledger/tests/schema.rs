//! The ten logical sets of v0.5 §31.3, and the indexes §32.3's query budgets need.
//!
//! §31.3 permits a different physical normalisation and permits no missing set, so the test asks
//! the store what it holds rather than how it spells it: every logical set must be answerable.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_temporal_core::{CoverageQuery, EventKind, EventQuery, LedgerRead, LedgerWrite, TimeRange};
use ono_temporal_ledger::LOGICAL_SETS;

use common::{action, checkpoint, coverage, event, evidence_for, instant, link, scope, store_in};

#[test]
fn should_hold_every_logical_set_of_the_specification_when_a_store_is_created() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let held = store
        .logical_sets()
        .expect("a fresh store reports its sets");
    for set in LOGICAL_SETS {
        assert!(
            held.contains(*set),
            "§31.3 requires the logical set `{set}`; the store holds {held:?}"
        );
    }
}

#[test]
fn should_answer_every_logical_set_when_one_of_each_has_been_written() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());

    let cause = event(
        EventKind::ActionExecuted,
        "2026-08-31T14:03:11Z",
        "ono.session",
    );
    let effect = event(
        EventKind::ObjectChanged,
        "2026-08-31T14:03:13Z",
        "linux.procfs",
    );
    let evidence = evidence_for("2026-08-31T14:03:13Z", 1827, "active");

    let appended = store
        .append(
            &[cause.clone(), effect.clone()],
            std::slice::from_ref(&evidence),
        )
        .expect("an append succeeds");
    assert_eq!(appended.stored, 2);

    store
        .append_links(&[link(&cause, &effect, &evidence)])
        .expect("links append");
    store
        .record_coverage(&[coverage("2026-08-31T14:00:00Z", "2026-08-31T14:10:00Z")])
        .expect("coverage records");
    store
        .record_action(&action(
            "2026-08-31T14:03:11Z",
            ono_temporal_core::RedactedCommandSummary::of("restart", Some("service"), &[]),
        ))
        .expect("an action records");
    store
        .write_checkpoint(&checkpoint("2026-08-31T14:02:00Z"))
        .expect("a checkpoint writes");
    store.flush().expect("a flush succeeds");

    assert_eq!(
        store
            .events(&EventQuery::in_range(TimeRange::all()))
            .expect("events answer")
            .len(),
        2
    );
    assert_eq!(
        store
            .evidence(std::slice::from_ref(&evidence.evidence_id))
            .expect("evidence answers")
            .len(),
        1
    );
    assert_eq!(
        store
            .causal_links(&effect.event_id)
            .expect("links answer")
            .len(),
        1
    );
    assert_eq!(
        store
            .coverage(&CoverageQuery::default())
            .expect("coverage answers")
            .len(),
        1
    );
    assert_eq!(
        store
            .actions(TimeRange::all())
            .expect("actions answer")
            .len(),
        1
    );
    assert!(
        store
            .checkpoint_before(&scope(), instant("2026-08-31T14:03:00Z"))
            .expect("checkpoints answer")
            .is_some()
    );
    assert!(
        store.retention().latest.is_some(),
        "the metadata set answers how much history there is"
    );
}

#[test]
fn should_plan_a_timeline_query_on_an_index_when_one_place_and_one_window_are_named() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    let plan = store
        .explain_timeline_plan()
        .expect("the store can explain its own plan");
    assert!(
        plan.iter().any(|step| step.contains("USING INDEX")),
        "§32.3 budgets 100 ms for a 15-minute timeline of one place; the plan was {plan:?}"
    );
    assert!(
        !plan.iter().any(|step| step.contains("SCAN events")
            && !step.contains("USING INDEX")
            && !step.contains("USING COVERING INDEX")),
        "a timeline must not table-scan the events: {plan:?}"
    );
}

#[test]
fn should_return_a_checkpoint_and_a_link_lookup_from_an_index_when_the_planner_is_asked() {
    let home = tempfile::tempdir().expect("a temporary home");
    let store = store_in(home.path());
    for plan in [
        store.explain_checkpoint_plan().expect("a checkpoint plan"),
        store.explain_causal_plan().expect("a causal plan"),
        store.explain_retention_plan().expect("a retention plan"),
        store.explain_event_plan().expect("an event lookup plan"),
        store.explain_changes_plan().expect("a changes plan"),
    ] {
        assert!(
            plan.iter()
                .any(|step| step.contains("INDEX") || step.contains("PRIMARY KEY")),
            "§32.3 budgets each of these; the plan was {plan:?}"
        );
    }
}
