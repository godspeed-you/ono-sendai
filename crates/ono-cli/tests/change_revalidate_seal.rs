//! What a plan freezes and seals, so that revalidation has something true to check (v0.6 §4.3,
//! §4.4, §28.4, §53).
//!
//! §4.3 turns selectors into frozen objects and §4.4 seals the result. A plan that sealed with
//! nothing to do would apply as `PREPARE none / APPLY none / VERIFY none` and change nothing
//! without saying why; a plan that dropped the configured strategy would seal a different risk
//! than the one the operator configured.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{home, one, ono_at, text};

/// The `mutate` actions of a plan record (§3.3).
fn mutations(plan: &serde_yaml_ng::Value) -> usize {
    plan["actions"]
        .as_sequence()
        .map(|actions| {
            actions
                .iter()
                .filter(|action| action["role"].as_str() == Some("mutate"))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn should_carry_the_mutation_when_the_destination_is_a_symlink() {
    let home = home();
    let source = home.path().join("source.conf");
    let real = home.path().join("real.conf");
    let link = home.path().join("link.conf");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&real, b"old\n").expect("the real file is written");
    std::os::unix::fs::symlink(&real, &link).expect("the link is made");

    let run = ono_at(
        home.path(),
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            link.display()
        ),
    );
    run.assert_success();
    let plan = one(&run);
    assert_eq!(
        mutations(&plan),
        1,
        "v0.6 §4.3: the destination resolved to one object, so the sealed plan carries the one \
         mutation that acts on it — never a plan with no action. Plan: {plan:?}"
    );
    let _ = text(&plan, "id");
}

#[test]
fn should_seal_the_configured_default_strategy_when_the_plan_states_none() {
    let home = home();
    let source = home.path().join("source.conf");
    let destination = home.path().join("app.conf");
    std::fs::write(&source, b"new\n").expect("the source is written");
    std::fs::write(&destination, b"old\n").expect("the destination is written");
    let run = change_support::ono_with(
        home.path(),
        "ONO_CHANGE_DEFAULT_STRATEGY",
        "batch 2",
        &format!(
            "plan copy file {} {} --overwrite | to json",
            source.display(),
            destination.display()
        ),
    );
    run.assert_success();
    let plan = one(&run);
    assert_eq!(
        plan["strategy"].as_str(),
        Some("batch 2"),
        "v0.6 §53 and §28.4: `change.default_strategy` is the strategy a plan takes when it states \
         none, and §28.5 seals it. Got {plan:?}"
    );
}
