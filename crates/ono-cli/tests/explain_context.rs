//! The execution context is part of the `ono.execution-plan/1` value, not only of its rendering
//! (spec §42.2; issue #219 (c), ADR-0942, ADR-1099).
//!
//! ADR-0106 printed `explain`'s EXECUTION CONTEXT block beside the plan and deferred the value
//! form; ADR-0942 made the plan one record and folded the context into it. This pins the public
//! contract: inside a link the value names the link, its transport, mode and identity; on the
//! local machine the field is null, never an empty context.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "remote")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::scratch;
use serde_json::{Value, json};

mod support;

fn context_of(script: &str) -> Value {
    let run = support::ono_at_home(&scratch(), script);
    run.assert_success();
    let rows: Value = serde_json::from_str(&support::last_line(&run))
        .unwrap_or_else(|_| panic!("JSON on the last line, got {:?}", run.stdout()));
    rows[0]["context"].clone()
}

#[test]
fn should_carry_the_remote_execution_context_in_the_plan_value_inside_a_link() {
    let context = context_of(
        "link host testbox --transport local; enter link testbox; \
         explain \"stop process 1\" | select context | to json",
    );
    assert_eq!(context["link"], "testbox (remote)", "got {context}");
    assert_eq!(context["host"], "testbox");
    assert_eq!(context["transport"], "local");
    assert_eq!(context["mode"], "agent");
    assert!(context["identity"].is_string(), "got {context}");
    assert_eq!(
        context["answers"],
        Value::Null,
        "an agent link is not reduced"
    );
}

#[test]
fn should_name_what_a_reduced_link_answers_in_the_plan_value() {
    let context = context_of(
        "link host testbox --transport local --agentless; enter link testbox; \
         explain \"get process\" | select context | to json",
    );
    assert!(
        context["mode"]
            .as_str()
            .is_some_and(|mode| mode.starts_with("agentless")),
        "got {context}"
    );
    assert!(
        context["answers"]
            .as_str()
            .is_some_and(|answers| answers.contains("process")),
        "got {context}"
    );
}

#[test]
fn should_leave_the_context_null_on_the_local_machine() {
    assert_eq!(
        context_of("explain \"get process\" | select context | to json"),
        json!(null)
    );
}
