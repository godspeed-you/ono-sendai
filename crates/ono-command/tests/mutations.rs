//! Mutating commands answer with one outcome per target, and never with a collapsed status.
//!
//! Spec §16.5 forbids `97 succeeded, 3 failed` from becoming one ambiguous answer, and spec §11.5
//! asks for a structured result rather than an exit code. These tests are that rule.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use fixture::{FixtureProvider, providers, run};
use ono_core::ErrorCode;
use ono_value::ActionStatus;

#[tokio::test]
async fn should_answer_with_one_outcome_per_object_that_arrived() {
    let ran = run(
        "get process | stop process",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(
        ran.actions().len(),
        3,
        "spec §11.5: one ActionResult per target, not a count"
    );
    assert!(
        ran.actions()
            .iter()
            .all(ono_provider_api::ActionOutcome::is_success)
    );
}

#[tokio::test]
async fn should_keep_a_mixed_result_apart_rather_than_collapsing_it() {
    let ran = run(
        "get process | stop process",
        &providers(FixtureProvider::new().failing_on(2)),
    )
    .await
    .expect("the pipeline runs");

    let statuses: Vec<ActionStatus> = ran
        .actions()
        .iter()
        .map(ono_provider_api::ActionOutcome::status)
        .collect();
    assert_eq!(
        statuses,
        [
            ActionStatus::Success,
            ActionStatus::Failed,
            ActionStatus::Success
        ],
        "spec §16.5: the one that failed stays identifiable among the ones that did not"
    );

    let failed = ran
        .actions()
        .iter()
        .find(|outcome| !outcome.is_success())
        .expect("one failed");
    assert_eq!(
        failed
            .target()
            .values()
            .first()
            .and_then(|value| value.as_int().ok()),
        Some(2),
        "the failure names which object it was about"
    );
    assert_eq!(
        failed.error().map(ono_value::ErrorValue::code),
        Some(ErrorCode::IoPermissionDenied),
        "the provider's own reason survives"
    );
}

#[tokio::test]
async fn should_resolve_a_selector_into_a_full_identity_before_acting() {
    // Resolving first is what makes the identity complete — a process is `(pid, started)`, not a
    // pid — which is what keeps a signal from reaching a recycled pid (ADR-0015 T13).
    let ran = run("stop process 2", &providers(FixtureProvider::new()))
        .await
        .expect("the pipeline runs");

    assert_eq!(ran.actions().len(), 1);
    assert_eq!(ran.actions()[0].operation(), "stop");
}

#[tokio::test]
async fn should_carry_an_option_through_to_the_provider() {
    let ran = run(
        "kill process 2 --signal SIGHUP",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.actions()[0].operation(), "kill");
}

#[tokio::test]
async fn should_report_an_action_that_could_not_be_attempted_as_that_object_s_outcome() {
    // Nothing claims `service` here, and a provider that cannot attempt an action is still an
    // outcome for each target rather than the end of the pipeline (spec §16.5).
    let ran = run(
        "get process | stop service",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.actions().len(), 3, "every target still gets an answer");
    assert!(ran.actions().iter().all(|outcome| {
        outcome.error().map(ono_value::ErrorValue::code) == Some(ErrorCode::ResolveTargetNotFound)
    }));
}

#[tokio::test]
async fn should_refuse_to_act_when_nothing_names_a_target() {
    let error = run("stop process", &providers(FixtureProvider::new()))
        .await
        .expect_err("`stop process` with neither a selector nor input has nothing to act on");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[tokio::test]
async fn should_refuse_to_act_on_a_projection_that_has_no_identity() {
    let error = run(
        "get process | select name | stop process",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect_err("a projection is a value, not an object");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
}

#[tokio::test]
async fn should_act_only_on_the_objects_a_filter_kept() {
    let ran = run(
        "get process | where size > 1KiB | stop process",
        &providers(FixtureProvider::new()),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.actions().len(), 1);
    assert_eq!(
        ran.actions()[0]
            .target()
            .values()
            .first()
            .and_then(|value| value.as_int().ok()),
        Some(2)
    );
}

#[tokio::test]
async fn should_refuse_a_bulk_mutation_over_the_threshold_and_change_nothing() {
    // Spec §11.6 and §17.4: destructive scope is shown before acting, and a selection over the
    // bulk threshold mutates nothing without the written confirmation. The refusal comes before
    // the first action — a refused bulk never half-ran.
    let provider = FixtureProvider::live();
    let handle = provider.handle();
    for pid in 100..112 {
        handle.add(pid, &format!("bulk-{pid}"), Some(64), "root");
    }

    let error = run("get process | stop process", &providers(provider))
        .await
        .expect_err("fifteen objects is over the threshold");
    assert_eq!(error.code(), ErrorCode::SafetyConfirmationRequired);
    assert!(
        error.message().contains("15"),
        "the refusal names the scope: {}",
        error.message()
    );
    assert!(
        error.help().is_some_and(|help| help.contains("--confirm")),
        "the refusal says how to proceed: {:?}",
        error.help()
    );
}

#[tokio::test]
async fn should_act_on_every_object_when_the_bulk_was_confirmed() {
    let provider = FixtureProvider::live();
    let handle = provider.handle();
    for pid in 100..112 {
        handle.add(pid, &format!("bulk-{pid}"), Some(64), "root");
    }

    let ran = run("get process | stop process --confirm", &providers(provider))
        .await
        .expect("the confirmed bulk runs");
    assert_eq!(
        ran.actions().len(),
        15,
        "spec §11.5: one outcome per confirmed target"
    );
}

// --- generic verb binding: a provider's capability decides what is bound (ADR-0068 §3) --------

/// A provider for `service` that advertises `service.manage` and answers `set` — the verb the
/// registry declares for `ono.service.set` and that no code in the command crate names.
#[derive(Debug)]
struct ServiceFixture;

fn service_schema() -> std::sync::Arc<ono_value::Schema> {
    std::sync::Arc::new(
        ono_value::Schema::builder(ono_value::SchemaId::new("ono.unit-fixture", 1), "Unit")
            .field(ono_value::FieldDef::new("name", ono_value::FieldType::String).required())
            .identity(["name"])
            .default_view(["name"])
            .build()
            .expect("the fixture schema is valid"),
    )
}

fn unit(name: &str) -> ono_value::RecordValue {
    let schema = service_schema();
    let provenance = ono_value::Provenance::local("test.service", schema.id().clone());
    ono_value::RecordValue::builder(schema, provenance)
        .set("name", ono_value::Value::string(name))
        .map(ono_value::RecordBuilder::build)
        .expect("the fixture record is valid")
}

#[async_trait::async_trait]
impl ono_provider_api::Provider for ServiceFixture {
    fn id(&self) -> &str {
        "test.service"
    }

    fn targets(&self) -> &[&str] {
        &["service"]
    }

    fn schemas(&self) -> Vec<std::sync::Arc<ono_value::Schema>> {
        vec![service_schema()]
    }

    fn capabilities(&self) -> Vec<ono_provider_api::Capability> {
        vec![
            ono_provider_api::Capability::new("service.list", ono_provider_api::Risk::Read),
            ono_provider_api::Capability::new("service.manage", ono_provider_api::Risk::Mutate),
        ]
    }

    fn snapshot(
        &self,
        _query: &ono_provider_api::Query,
    ) -> Result<ono_pipeline::ValueStream, ono_value::ErrorValue> {
        Ok(ono_pipeline::ValueStream::from_values([
            unit("nginx").into_value()
        ]))
    }

    async fn resolve(
        &self,
        selector: &ono_provider_api::Selector,
    ) -> Result<Vec<ono_provider_api::ObjectRef>, ono_value::ErrorValue> {
        Ok([unit("nginx")]
            .iter()
            .filter(|record| selector.matches(record))
            .filter_map(ono_provider_api::ObjectRef::of)
            .collect())
    }

    async fn act(
        &self,
        action: &ono_provider_api::Action,
    ) -> Result<ono_provider_api::ActionOutcome, ono_value::ErrorValue> {
        // The verb and the option arrive as the contract spells them; the outcome echoes them
        // so the test can see what reached the provider.
        Ok(ono_provider_api::ActionOutcome::skipped(
            action,
            format!(
                "{} enabled={:?} on {}",
                action.operation(),
                action.argument("enabled"),
                action.target()
            ),
        ))
    }
}

fn service_and_process() -> ono_provider_api::ProviderRegistry {
    let mut registry = providers(FixtureProvider::new());
    registry.register(std::sync::Arc::new(ServiceFixture));
    registry
}

#[tokio::test]
async fn should_bind_a_mutating_verb_when_a_provider_advertises_its_capability() {
    // `ono.service.set` names `service.manage`; the fixture advertises it, so `set service`
    // reaches `act` with the verb, the selector resolved to an identity, and the option.
    let ran = fixture::run_bound("set service nginx --enabled false", &service_and_process())
        .await
        .expect("the pipeline runs");

    assert_eq!(ran.actions().len(), 1, "one target, one outcome");
    let outcome = &ran.actions()[0];
    assert_eq!(
        outcome.operation(),
        "set",
        "the provider is asked in the verb's own name"
    );
    assert_eq!(
        outcome.status(),
        ActionStatus::Skipped,
        "the outcome is the provider's own answer"
    );
    let echoed = outcome_message(outcome);
    assert!(
        echoed.contains("enabled=Some(Bool(false))") && echoed.contains("nginx"),
        "the option and the resolved target reached `act`, got {echoed:?}"
    );
}

#[tokio::test]
async fn should_carry_piped_objects_into_a_generically_bound_verb() {
    let ran = fixture::run_bound(
        "get service | set service --enabled true",
        &service_and_process(),
    )
    .await
    .expect("the pipeline runs");

    assert_eq!(ran.actions().len(), 1);
    assert!(
        outcome_message(&ran.actions()[0]).contains("enabled=Some(Bool(true))"),
        "the piped unit and the option both reached `act`"
    );
}

#[tokio::test]
async fn should_leave_a_mutating_verb_unbound_when_no_provider_advertises_its_capability() {
    // Nothing here advertises `file.set` or `mount.manage`: the contracts stay unbound, so the
    // shell answers E0101 rather than running a stub that fails halfway (spec §50).
    let table = ono_command::builtin_commands_for(fixture::registry(), &service_and_process());

    assert!(table.contains("ono.service.set"), "advertised, so bound");
    assert!(table.contains("ono.process.kill"), "advertised, so bound");
    assert!(
        !table.contains("ono.file.set"),
        "no provider advertises `file.set`"
    );
    assert!(
        !table.contains("ono.filesystem.unmount"),
        "no provider advertises `mount.manage`"
    );
}

#[tokio::test]
async fn should_refuse_before_acting_when_the_provider_that_would_act_lacks_the_capability() {
    // The table was built for a registry that advertised `service.manage`; the registry the
    // pipeline runs against does not. The mutation refuses before resolving anything, with the
    // same E0101 an unbound command answers, instead of asking a provider that cannot do it.
    let table = ono_command::builtin_commands_for(fixture::registry(), &service_and_process());
    let mut without = providers(FixtureProvider::new());
    without.register(std::sync::Arc::new(ReadOnlyService));

    let error = fixture::run_with_table(&table, "set service nginx --enabled false", &without)
        .await
        .expect_err("the provider that would act does not advertise `service.manage`");
    assert_eq!(error.code(), ErrorCode::ResolveCommandNotFound);
    assert!(
        error.message().contains("ono.service.set") && error.message().contains("service.manage"),
        "the refusal names the command and the capability, got {}",
        error.message()
    );
}

/// A `service` provider that only lists.
#[derive(Debug)]
struct ReadOnlyService;

#[async_trait::async_trait]
impl ono_provider_api::Provider for ReadOnlyService {
    fn id(&self) -> &str {
        "test.service-readonly"
    }

    fn targets(&self) -> &[&str] {
        &["service"]
    }

    fn schemas(&self) -> Vec<std::sync::Arc<ono_value::Schema>> {
        vec![service_schema()]
    }

    fn capabilities(&self) -> Vec<ono_provider_api::Capability> {
        vec![ono_provider_api::Capability::new(
            "service.list",
            ono_provider_api::Risk::Read,
        )]
    }

    fn snapshot(
        &self,
        _query: &ono_provider_api::Query,
    ) -> Result<ono_pipeline::ValueStream, ono_value::ErrorValue> {
        Ok(ono_pipeline::ValueStream::from_values([
            unit("nginx").into_value()
        ]))
    }

    async fn resolve(
        &self,
        _selector: &ono_provider_api::Selector,
    ) -> Result<Vec<ono_provider_api::ObjectRef>, ono_value::ErrorValue> {
        panic!("a provider without the capability must never be asked to resolve for a mutation")
    }
}

fn outcome_message(outcome: &ono_provider_api::ActionOutcome) -> String {
    outcome
        .clone()
        .into_record(ono_value::Duration::ZERO)
        .message()
        .unwrap_or_default()
        .to_owned()
}
