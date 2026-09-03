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
    for id in ["ono.plugin.install", "ono.host.link"] {
        assert!(
            !table.contains(id),
            "`{id}` is scheduled for a later phase, and a stub would be worse than an honest \
             absence"
        );
    }
}

#[tokio::test]
async fn should_report_an_unimplemented_command_by_name_rather_than_failing_halfway() {
    // `set process` is declared stable and bound only by a provider advertising `process.set`
    // (ADR-0068 §3, ADR-0092), which the fixture does not — the same shape `watch process` had
    // before phase F delivered it.
    let error = run(
        "set process 1 --priority 5",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect_err("`set process` has no implementation here");

    assert_eq!(error.code(), ErrorCode::ResolveCommandNotFound);
    assert!(
        error.message().contains("ono.process.set"),
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
            "ono.container.enter",
            "ono.dir.enter",
            // No provider implements a file mutation yet. Registering one would give the user a
            // command that always fails rather than one that is honestly not there (spec §50).
            "ono.file.copy",
            "ono.file.move",
            "ono.file.read",
            "ono.file.remove",
            "ono.file.write",
            "ono.filesystem.mount",
            "ono.filesystem.unmount",
            // The runtime limits are the session's catalogue: v0.4.1 §12.4 wants the centralized
            // limits printed, and there is no provider that could know what this shell enforces
            // (ADR-0456).
            "ono.limits.inspect",
            // A provider delivers the package mutations by advertising `package.manage`
            // (ADR-0068 §3); a table built without providers binds none of them.
            "ono.package-source.refresh",
            "ono.package.add",
            "ono.package.remove",
            "ono.package.set",
            // ADR-0020 §9: setting a variable changes the session's own scope, which the
            // evaluator owns.
            "ono.env.set",
            // A provider delivers `set process` by advertising `process.set` (ADR-0068 §3,
            // ADR-0092); a table built without providers binds only the verbs every `act`
            // speaks.
            "ono.process.set",
            // Likewise `send signal`, delivered by the provider that claims `signal` and
            // advertises `process.signal` (ADR-0092 §2).
            "ono.signal.send",
        ]
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>(),
        "every other delivered command has an implementation (spec §27.2)"
    );
}

// --- context frames (spec §14.3, ADR-0023) ---------------------------------------------------

#[tokio::test]
async fn should_narrow_a_producer_with_the_ambient_selector_of_a_context_frame() {
    // Inside `enter owner root`, `get process` asks for root's objects: the frame's selector is
    // pushed into the provider query exactly as `get process --owner root` would be (§14.5).
    let frame = ono_command::ContextFrame::new("owner", Value::string("root"));
    let ran = fixture::run_with_context(
        "get process",
        &providers(FixtureProvider::new()),
        vec![frame],
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(
        ran.values().len(),
        2,
        "root owns exactly two fixture objects; a full answer would mean the frame fell back to \
         global scope"
    );
    for value in ran.values() {
        let record = value.as_record().expect("a record");
        assert_eq!(
            record.get("owner"),
            Some(&Value::string("root")),
            "every answer belongs to the entered object (spec §14.3)"
        );
    }
}

#[tokio::test]
async fn should_refuse_a_query_the_context_cannot_narrow_rather_than_widening() {
    // Spec §14.3: a command with no meaning in the active context fails saying why — it never
    // quietly runs globally. The widget schema has no `service` field, so a service frame
    // cannot narrow it.
    let frame = ono_command::ContextFrame::new("service", Value::string("nginx.service"));
    let error = fixture::run_with_context(
        "get process",
        &providers(FixtureProvider::new()),
        vec![frame],
    )
    .await
    .expect_err("a context that cannot narrow the query must refuse it");

    assert_eq!(error.code(), ono_core::ErrorCode::ResolveTargetNotFound);
    assert!(
        error.message().contains("service"),
        "the refusal names the frame: {}",
        error.message()
    );
    assert!(
        error.help().is_some_and(|help| help.contains("leave")),
        "the refusal says how to widen explicitly (spec §14.5): {:?}",
        error.help()
    );
}
