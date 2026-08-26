//! `get <target>`: one implementation, built from the contract, over whichever provider answers.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, no_providers, providers, run, table};
use ono_core::ErrorCode;
use ono_value::Value;

#[tokio::test]
async fn should_stream_every_object_a_provider_answers_with() {
    let ran = run("get process", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    assert_eq!(ran.values().len(), 3);
    assert!(
        ran.values().iter().all(|value| value
            .as_record()
            .is_ok_and(|record| record.schema_id().name() == "ono.widget")),
        "the provider's own schema reaches the pipeline unchanged (spec §5)"
    );
}

#[tokio::test]
async fn should_push_a_selector_down_into_the_provider() {
    // The fixture narrows itself, so a single result proves the selector reached the provider
    // rather than being filtered afterwards (spec §27.1).
    let ran = run("get process 2", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    let record = ran.only().as_record().expect("a record");
    assert_eq!(record.get("pid"), Some(&Value::Int(2)));
}

#[tokio::test]
async fn should_bind_a_word_selector_by_the_type_its_contract_declares() {
    // `get process beta` binds `name`, not `pid`, because `beta` is not an int (ADR-0021 §1).
    let ran = run("get process beta", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    let record = ran.only().as_record().expect("a record");
    assert_eq!(record.get("name"), Some(&Value::string("beta")));
}

#[tokio::test]
async fn should_answer_with_nothing_when_a_selector_matches_nothing() {
    let ran = run("get process 99", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    assert!(
        ran.values().is_empty(),
        "there are none, which is not the same as not being able to look"
    );
}

#[tokio::test]
async fn should_say_the_provider_cannot_answer_here_rather_than_answering_with_nothing() {
    let error = run(
        "get process",
        &providers(FixtureProvider::unavailable("there is no /proc here")),
    )
    .await
    .expect_err("an unavailable provider says so");

    assert_eq!(error.code(), ErrorCode::ProviderUnavailable);
    assert!(
        error.message().contains("there is no /proc here"),
        "the provider's own reason survives: {}",
        error.message()
    );
}

#[tokio::test]
async fn should_say_nothing_answers_a_target_when_no_provider_claims_it() {
    let error = run("get process", &no_providers())
        .await
        .expect_err("nothing claims `process`");

    assert_eq!(error.code(), ErrorCode::ResolveTargetNotFound);
}

// --- what the table does and does not carry -----------------------------------------------------

#[test]
fn should_register_a_producer_for_every_delivered_target() {
    let table = table();
    for id in [
        "ono.process.get",
        "ono.file.get",
        "ono.user.get",
        "ono.service.get",
        "ono.socket.get",
        "ono.interface.get",
        "ono.mount.get",
        "ono.file.find",
    ] {
        assert!(table.contains(id), "`{id}` is delivered by phase C");
    }
}

#[test]
fn should_not_register_a_command_whose_phase_has_not_been_delivered() {
    let table = table();
    for id in [
        "ono.plugin.install",
        "ono.host.link",
        "ono.process.watch",
        "ono.process.trace",
        "ono.container.get",
    ] {
        assert!(
            !table.contains(id),
            "`{id}` is scheduled for a later phase, and a stub would be worse than an honest \
             absence"
        );
    }
}

#[tokio::test]
async fn should_report_an_unimplemented_command_by_name_rather_than_failing_halfway() {
    let error = run("watch process", &providers(FixtureProvider::new()))
        .await
        .expect_err("`watch process` arrives in phase F");

    assert_eq!(error.code(), ErrorCode::ResolveCommandNotFound);
    assert!(
        error.message().contains("ono.process.watch"),
        "the error names the command: {}",
        error.message()
    );
}

#[test]
fn should_leave_unbound_only_the_delivered_commands_nothing_here_can_answer() {
    let unbound = ono_command::unbound_stable_commands(fixture::registry(), &table());
    let mut delivered_but_unbound: Vec<&str> = unbound
        .iter()
        .copied()
        .filter(|id| {
            fixture::registry().get(id).is_some_and(|command| {
                matches!(
                    command.phase(),
                    ono_command::Phase::Delivered('A' | 'B' | 'C' | 'D')
                )
            })
        })
        .collect();
    delivered_but_unbound.sort_unstable();

    assert_eq!(
        delivered_but_unbound,
        [
            // Configuration is the session's: ADR-0010 puts its layers and provenance in the
            // evaluator, so a provider claiming it would put the authority in the wrong place.
            "ono.config.get",
            "ono.config.set",
            // The context stack of spec §14.1 is the session's too.
            "ono.dir.enter",
            // Nothing claims the `dns` or `port` targets, so there is no provider to ask.
            "ono.dns.resolve",
            // No provider implements a file mutation yet. Registering one would give the user a
            // command that always fails rather than one that is honestly not there (spec §50).
            "ono.file.copy",
            "ono.file.move",
            "ono.file.read",
            "ono.file.remove",
            "ono.file.write",
            "ono.filesystem.mount",
            "ono.filesystem.unmount",
            // ADR-0020 §9: setting a variable changes the session's own scope, which the
            // evaluator owns.
            "ono.env.set",
            "ono.port.test",
            // `inspect process` promises `ono.process-detail/1`, which no provider produces yet.
            // Answering it with `ono.process/1` would be a different value wearing the name.
            "ono.process.inspect",
        ]
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>(),
        "every other delivered command has an implementation (spec §27.2)"
    );
}
