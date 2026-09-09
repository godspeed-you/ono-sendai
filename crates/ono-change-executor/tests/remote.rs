//! Per-host fragments and link failure (spec v0.6 §29.1, §29.3, §55.10 cases 44 and 45).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::cell::RefCell;

use common::{PlanSpec, instant};
use ono_change_core::{ActionStatus, PlanAction};
use ono_change_executor::execute::ExecutionOutcome;
use ono_change_executor::remote::{LinkState, decompose, run_fragments};

fn succeeds(_host: &str, _action: &PlanAction) -> ExecutionOutcome {
    ExecutionOutcome::Succeeded
}

// ---- §29.1: per-host truth ---------------------------------------------------------------------

#[test]
fn should_split_a_remote_plan_into_one_fragment_per_host() {
    let now = instant(1_000);
    let plan = PlanSpec::over(3)
        .on_hosts(&["api-04", "api-05", "api-04"])
        .seal(now);

    let fragments = decompose(&plan);

    assert_eq!(
        fragments.len(),
        2,
        "§29.1: remote plans are decomposed into host-local action fragments"
    );
    let first = fragments
        .iter()
        .find(|fragment| fragment.host() == "api-04")
        .expect("api-04 has a fragment");
    assert_eq!(first.actions().len(), 2);
}

#[test]
fn should_keep_a_local_target_in_the_local_fragment() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);

    let fragments = decompose(&plan);

    assert_eq!(fragments.len(), 1);
    assert_eq!(
        fragments[0].host(),
        "",
        "§7.1: an action about a local object is not made remote by its neighbours"
    );
}

#[test]
fn should_compose_no_plan_level_word_over_the_hosts() {
    let now = instant(1_000);
    let plan = PlanSpec::over(2).on_hosts(&["api-04", "api-05"]).seal(now);
    let fragments = decompose(&plan);
    let execute = |host: &str, _action: &PlanAction| {
        if host == "api-05" {
            ExecutionOutcome::Failed(ono_change_core::error::tool_failed(
                "/usr/bin/systemctl",
                "the unit refused",
            ))
        } else {
            ExecutionOutcome::Succeeded
        }
    };

    let run = run_fragments(&fragments, &execute);

    assert!(
        run.host("api-04").is_some_and(|host| host.is_complete()),
        "§29.1: Ono MUST NOT imply global atomicity, so one host's failure is not another's"
    );
    assert!(
        run.host("api-05").is_some_and(|host| !host.is_complete()),
        "and the failing host reports its own failure"
    );
}

// ---- §29.3: link failure -------------------------------------------------------------------------

#[test]
fn should_preserve_the_exact_per_host_action_state_when_a_link_drops() {
    let now = instant(1_000);
    let plan = PlanSpec::over(3)
        .on_hosts(&["api-04", "api-04", "api-04"])
        .seal(now);
    let fragments = decompose(&plan);
    let execute = |_host: &str, action: &PlanAction| {
        if action.target() == Some("svc-2") {
            ExecutionOutcome::Unknown(ono_change_core::error::remote_state_unknown(
                "api-04",
                action.summary(),
            ))
        } else {
            ExecutionOutcome::Succeeded
        }
    };

    let run = run_fragments(&fragments, &execute);
    let host = run.host("api-04").expect("api-04 ran");

    assert_eq!(host.link(), LinkState::Lost);
    assert_eq!(
        host.status_of(plan.actions()[0].id()),
        Some(ActionStatus::Succeeded),
        "§29.3: what the link did report is preserved exactly"
    );
    assert_eq!(
        host.status_of(plan.actions()[1].id()),
        Some(ActionStatus::Unknown),
        "§29.3: an unknown remote action is not marked failed or successful without evidence"
    );
    assert_eq!(
        host.status_of(plan.actions()[2].id()),
        Some(ActionStatus::Pending),
        "nothing ever tried the third, so it is PENDING rather than SKIPPED or FAILED"
    );
}

#[test]
fn should_report_a_disconnected_host_as_unknown_rather_than_failed() {
    let now = instant(1_000);
    let plan = PlanSpec::over(2).on_hosts(&["api-04", "api-05"]).seal(now);
    let fragments = decompose(&plan);
    let execute = |host: &str, action: &PlanAction| {
        if host == "api-05" {
            ExecutionOutcome::Unknown(ono_change_core::error::remote_state_unknown(
                host,
                action.summary(),
            ))
        } else {
            ExecutionOutcome::Succeeded
        }
    };

    let run = run_fragments(&fragments, &execute);

    assert_eq!(
        run.disconnected().len(),
        1,
        "§55.10 case 45: a disconnected host is UNKNOWN, not automatically failed"
    );
    assert!(run.has_unknown());
    assert_eq!(
        run.host("api-05")
            .map(|host| host.uncertainty_boundary().len()),
        Some(1),
        "Appendix F.2: recovery planning treats it as an uncertainty boundary"
    );
}

#[test]
fn should_tell_the_operator_the_link_can_be_queried_again() {
    let now = instant(1_000);
    let plan = PlanSpec::default().on_hosts(&["api-04"]).seal(now);
    let fragments = decompose(&plan);
    let execute = |host: &str, action: &PlanAction| {
        ExecutionOutcome::Unknown(ono_change_core::error::remote_state_unknown(
            host,
            action.summary(),
        ))
    };

    let run = run_fragments(&fragments, &execute);
    let refusal = run
        .host("api-04")
        .and_then(|host| host.error().cloned())
        .expect("the link left something unresolved");

    assert_eq!(refusal.code().name(), "change.remote_state_unknown");
    assert_eq!(
        refusal.retryable(),
        Some(true),
        "§29.3: querying again when the link returns is how it is resolved"
    );
    assert!(
        refusal
            .metadata()
            .get("host")
            .is_some_and(|value| *value == ono_value::Value::string("api-04")),
        "the outcome names the host, because per-host truth is the whole point"
    );
}

#[test]
fn should_not_touch_a_hosts_later_actions_once_its_link_dropped() {
    let now = instant(1_000);
    let plan = PlanSpec::over(3)
        .on_hosts(&["api-04", "api-04", "api-04"])
        .seal(now);
    let fragments = decompose(&plan);
    let attempted: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let execute = |_host: &str, action: &PlanAction| {
        attempted
            .borrow_mut()
            .push(action.target().unwrap_or("").to_owned());
        ExecutionOutcome::Unknown(ono_change_core::error::remote_state_unknown(
            "api-04",
            action.summary(),
        ))
    };

    let _ = run_fragments(&fragments, &execute);

    assert_eq!(
        attempted.into_inner(),
        vec!["svc-1".to_owned()],
        "§29.3: nothing is attempted over a link nobody can account for"
    );
}

#[test]
fn should_run_every_host_when_every_link_holds() {
    let now = instant(1_000);
    let plan = PlanSpec::over(3)
        .on_hosts(&["api-04", "api-05", "api-06"])
        .seal(now);
    let fragments = decompose(&plan);

    let run = run_fragments(&fragments, &succeeds);

    assert_eq!(run.hosts().len(), 3);
    assert!(
        run.hosts()
            .iter()
            .all(ono_change_executor::remote::HostOutcome::is_complete)
    );
    assert!(!run.has_unknown());
    assert!(run.disconnected().is_empty());
}

#[test]
fn should_stop_one_hosts_fragment_at_its_own_failure() {
    let now = instant(1_000);
    let plan = PlanSpec::over(4)
        .on_hosts(&["api-04", "api-04", "api-05", "api-05"])
        .seal(now);
    let fragments = decompose(&plan);
    let execute = |_host: &str, action: &PlanAction| {
        if action.target() == Some("svc-1") {
            ExecutionOutcome::Failed(ono_change_core::error::tool_failed(
                "/usr/bin/systemctl",
                "the unit refused",
            ))
        } else {
            ExecutionOutcome::Succeeded
        }
    };

    let run = run_fragments(&fragments, &execute);

    assert_eq!(
        run.host("api-04")
            .and_then(|host| host.status_of(plan.actions()[1].id())),
        Some(ActionStatus::Pending),
        "api-04's second action never ran"
    );
    assert!(
        run.host("api-05").is_some_and(|host| host.is_complete()),
        "§29.1: api-05 is a different host and a different truth"
    );
}

#[test]
fn should_name_the_link_state_a_person_reads() {
    assert_eq!(LinkState::Connected.as_str(), "connected");
    assert_eq!(LinkState::Lost.as_str(), "lost");
    assert_eq!(LinkState::NotReached.as_str(), "not-reached");
}
