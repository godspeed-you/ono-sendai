//! Historical context is read-only, and the rule lives at the one seam every implementation runs
//! through (v0.5 §4.7).
//!
//! §4.7 lists four classes of operation the rule covers — native mutations, KUANG/11 mutation
//! tools, remote mutations, and shell-state changes whose meaning would be confusing in a
//! historical world — and `CommandTable::run` is where all four meet: it sees the contract, the
//! verb's `mutating` flag from the registry, and the session's temporal coordinate. These tests
//! assert the refusal at that boundary, not the code that produces it.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, providers, run_at};
use ono_core::ErrorCode;
use ono_temporal_core::TemporalContext;

#[tokio::test]
async fn should_refuse_a_native_mutation_when_the_session_stands_in_the_past() {
    let refusal = run_at(
        "get process | stop process",
        &providers(FixtureProvider::new()),
        &fixture::historical(),
    )
    .await
    .expect_err("v0.5 §4.7: every Ono mutation fails while historical context is active");

    assert_eq!(refusal.code(), ErrorCode::TemporalReadOnly);
    assert!(
        refusal.message().contains("stop process"),
        "the refusal names the command that was refused: {}",
        refusal.message()
    );
    assert!(
        format!("{refusal:?}").contains("now"),
        "§4.7's message offers `now` as the way back: {refusal:?}"
    );
}

#[tokio::test]
async fn should_let_a_read_only_command_run_when_the_session_stands_in_the_past() {
    let ran = run_at(
        "get process",
        &providers(FixtureProvider::new()),
        &fixture::historical(),
    )
    .await
    .expect("v0.5 §4.7 refuses mutations, and nothing else");

    assert_eq!(
        ran.values().len(),
        3,
        "a query is exactly what historical context exists for"
    );
}

#[tokio::test]
async fn should_let_a_mutation_run_when_the_session_stands_in_the_present() {
    let ran = run_at(
        "get process | stop process",
        &providers(FixtureProvider::new()),
        &TemporalContext::Present,
    )
    .await
    .expect("the present is unchanged by v0.5 (§4.1)");

    assert_eq!(ran.actions().len(), 3);
}

#[tokio::test]
async fn should_carry_the_temporal_coordinate_into_the_implementation_that_runs() {
    let context = fixture::historical();
    let ran = run_at("get process", &providers(FixtureProvider::new()), &context)
        .await
        .expect("the query runs");

    assert_eq!(
        ran.values().len(),
        3,
        "§4.5 and §14.1 need the coordinate to reach the implementation, not stop at the guard"
    );
    assert!(
        context.is_historical(),
        "the fixture context is the historical one these tests are about"
    );
}
