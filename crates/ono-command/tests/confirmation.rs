//! When a command's `--confirm` must be written (spec §11.6, §17.4; v0.6 §40.1, §40.3).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_command::CommandRegistry;
use ono_command::Confirmation;

/// v0.6 §40.1: a plan with no gate applies on `apply` alone, and §40.3 asks for `--confirm`
/// only where the plan carries a gate. A contract saying `always` described a command that
/// does not exist.
#[test]
fn should_declare_that_apply_needs_a_confirmation_only_for_a_gated_plan() {
    let registry = CommandRegistry::load().expect("the registry loads");
    let apply = registry.get("ono.change.apply").expect("apply is declared");
    assert_eq!(apply.confirmation(), Confirmation::Gated);
}

#[test]
fn should_keep_protect_confirmed_on_every_run() {
    let registry = CommandRegistry::load().expect("the registry loads");
    let protect = registry
        .get("ono.change.protect")
        .expect("protect is declared");
    assert_eq!(
        protect.confirmation(),
        Confirmation::Always,
        "§5.5: protection is a real mutation and asks every time"
    );
}
