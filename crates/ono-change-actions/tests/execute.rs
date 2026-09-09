#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! Carrying an action out (§4.7) and observing whether it worked (§23).

mod support;

use std::sync::Arc;

use ono_change_actions::{Observer, ProviderChangeProvider};
use ono_change_core::{
    ChangeProvider, Execution, Intent, PlanAction, PlanFragment, PlanId, VerificationClass,
    VerificationContract, VerificationStatus,
};
use ono_value::Value;
use support::{FakeObserver, FakeProvider, change_provider, service, socket};

fn plan() -> PlanId {
    PlanId::of("session-1", support::AT, "verify")
}

fn contract(subject: &str, expression: &str) -> VerificationContract {
    VerificationContract::new(&plan(), VerificationClass::Required, subject, expression)
}

fn restart(records: Vec<Value>) -> (Arc<FakeProvider>, ProviderChangeProvider, PlanFragment) {
    let provider = Arc::new(FakeProvider::new("linux.systemd", &["service"]).holding(records));
    let change = change_provider(Arc::clone(&provider), Arc::new(FakeObserver::nginx()));
    let fragment = change
        .resolve(
            &Intent::new("restart service nginx", "restart service nginx"),
            &[],
        )
        .expect("a restart is plannable");
    (provider, change, fragment)
}

fn only(fragment: &PlanFragment) -> &PlanAction {
    &fragment.actions()[0]
}

// --- §4.7: execution --------------------------------------------------------------------------

#[test]
fn should_ask_the_provider_once_per_action() {
    let (provider, change, fragment) = restart(vec![service("nginx", "running")]);
    change.execute(only(&fragment)).expect("the provider acts");
    assert_eq!(provider.act_count(), 1);
}

#[test]
fn should_ask_the_provider_in_its_own_verb() {
    let (provider, change, fragment) = restart(vec![service("nginx", "running")]);
    change.execute(only(&fragment)).expect("the provider acts");
    let acted = provider.acted();
    assert_eq!(
        acted[0].operation(),
        "restart",
        "a provider's `act` speaks the verb the user typed, and the plan carries the command id"
    );
    assert_eq!(acted[0].target_name(), "service");
}

#[test]
fn should_tell_the_provider_where_the_object_was_observed() {
    let (provider, change, fragment) = restart(vec![service("nginx", "running")]);
    change.execute(only(&fragment)).expect("the provider acts");
    assert_eq!(
        provider.acted()[0].source(),
        Some("nginx"),
        "ADR-0082 §4: an identity says which object, and the source is how it is found again"
    );
}

#[test]
fn should_report_the_command_id_the_plan_named() {
    let (_, change, fragment) = restart(vec![service("nginx", "running")]);
    let result = change.execute(only(&fragment)).expect("the provider acts");
    assert_eq!(
        result.operation(),
        "ono.service.restart",
        "v0.5 §17.4: the result names the command that ran, where the provider was asked in its \
         own verb"
    );
}

#[test]
fn should_report_that_something_changed() {
    let (_, change, fragment) = restart(vec![service("nginx", "running")]);
    let result = change.execute(only(&fragment)).expect("the provider acts");
    assert!(result.is_changed());
    assert_eq!(result.status(), ono_value::ActionStatus::Success);
}

#[test]
fn should_report_a_failed_action_as_an_outcome_rather_than_as_an_error() {
    let provider = Arc::new(FakeProvider::new("linux.systemd", &["service"]).failing());
    let change = change_provider(Arc::clone(&provider), Arc::new(FakeObserver::nginx()));
    let fragment = change
        .resolve(
            &Intent::new("restart service nginx", "restart service nginx"),
            &[],
        )
        .expect("plannable");
    let result = change
        .execute(only(&fragment))
        .expect("§16.5: an attempted action that failed is an outcome, not an error");
    assert_eq!(result.status(), ono_value::ActionStatus::Failed);
    assert!(result.error().is_some());
}

#[test]
fn should_carry_a_declared_option_to_the_provider_as_a_typed_value() {
    let provider = Arc::new(FakeProvider::new("linux.procfs", &["process"]));
    let observer = FakeObserver::new().seeing("4421", "identity", Value::Int(4_421));
    let change = change_provider(Arc::clone(&provider), Arc::new(observer));
    let fragment = change
        .resolve(
            &Intent::new(
                "kill process 4421 --signal SIGKILL",
                "plan kill process 4421 --signal SIGKILL",
            ),
            &[],
        )
        .expect("signalling a process is plannable");
    change.execute(only(&fragment)).expect("the provider acts");
    assert_eq!(
        provider.acted()[0].argument("signal"),
        Some(&Value::string("SIGKILL")),
        "§2.17: the argument travels as a typed value, never interpolated into text"
    );
}

#[test]
fn should_refuse_to_execute_an_action_that_is_not_a_provider_action() {
    let (provider, change, _) = restart(Vec::new());
    let opaque = ono_change_actions::opaque_action(&plan(), 0, "something else", None, &[]);
    change
        .execute(&opaque)
        .expect_err("§6.3's escape runs through the tool runner, not through a provider action");
    assert_eq!(provider.act_count(), 0, "nothing was attempted");
}

#[test]
fn should_keep_the_execution_free_of_anything_a_shell_could_expand() {
    let provider = Arc::new(FakeProvider::new("linux.files", &["file"]));
    let hostile = "/tmp/$(whoami) && rm -rf ~";
    let observer = FakeObserver::new()
        .seeing(hostile, "identity", Value::string(hostile))
        .seeing(hostile, "sha256", Value::string("aaa"))
        .seeing(hostile, "persistence-domain", Value::string("tank/tmp"));
    let change = change_provider(Arc::clone(&provider), Arc::new(observer));
    let fragment = change
        .resolve(
            &Intent::new("remove file /tmp/placeholder", "remove file …"),
            &[ono_change_core::FrozenTarget::new(
                "ono.file/1",
                hostile,
                hostile,
            )],
        )
        .expect("plannable");
    change.execute(only(&fragment)).expect("the provider acts");
    assert_eq!(
        provider.acted()[0].source(),
        Some(hostile),
        "§43.6: a path that looks like shell syntax is a path, because there is no shell"
    );
}

// --- §23: verification ------------------------------------------------------------------------

fn verifying(records: Vec<Value>, targets: &'static [&'static str]) -> ProviderChangeProvider {
    let provider = Arc::new(FakeProvider::new("linux.systemd", targets).holding(records));
    ProviderChangeProvider::new(
        provider,
        Arc::new(FakeObserver::new()),
        Arc::new(support::TestBridge),
        "session-1",
        support::at(),
    )
    .expect("the registries typecheck")
    .for_plan(plan())
}

#[test]
fn should_pass_when_the_provider_reports_the_state_the_plan_expected() {
    let change = verifying(vec![service("nginx", "running")], &["service"]);
    let result = change
        .verify(&contract("service nginx", "state == running"))
        .expect("the observation could be made");
    assert_eq!(result.status(), VerificationStatus::Passed);
}

#[test]
fn should_fail_when_the_provider_reports_another_state() {
    let change = verifying(vec![service("nginx", "failed")], &["service"]);
    let result = change
        .verify(&contract("service nginx", "state == running"))
        .expect("the observation could be made");
    assert_eq!(
        result.status(),
        VerificationStatus::Failed,
        "§2.14: verification is separate from execution success"
    );
    assert_eq!(result.observed(), Some(&Value::string("failed")));
}

#[test]
fn should_fail_when_the_object_the_plan_named_is_not_there() {
    let change = verifying(Vec::new(), &["service"]);
    let result = change
        .verify(&contract("service nginx", "state == running"))
        .expect("the observation could be made");
    assert_eq!(result.status(), VerificationStatus::Failed);
}

#[test]
fn should_acknowledge_provider_level_state_as_the_minimum_verification() {
    let change = verifying(vec![service("nginx", "running")], &["service"]);
    let result = change
        .verify(&contract("service nginx", "exists"))
        .expect("the observation could be made");
    assert_eq!(
        result.status(),
        VerificationStatus::Passed,
        "§23.1: the minimum verification is only provider-level state acknowledgement"
    );
}

#[test]
fn should_pass_an_absence_check_when_the_object_is_gone() {
    let change = verifying(Vec::new(), &["service"]);
    let result = change
        .verify(&contract("service nginx", "exists == false"))
        .expect("the observation could be made");
    assert_eq!(result.status(), VerificationStatus::Passed);
}

#[test]
fn should_fail_an_absence_check_when_the_object_is_still_there() {
    let change = verifying(vec![service("nginx", "running")], &["service"]);
    let result = change
        .verify(&contract("service nginx", "exists == false"))
        .expect("the observation could be made");
    assert_eq!(result.status(), VerificationStatus::Failed);
}

#[test]
fn should_answer_unknown_for_a_condition_it_cannot_observe() {
    let change = verifying(vec![service("nginx", "running")], &["service"]);
    let result = change
        .verify(&contract("service nginx", "cpu > 20"))
        .expect("the observation could be made");
    assert_eq!(
        result.status(),
        VerificationStatus::Unknown,
        "§23.3: a check that ran and could not answer is UNKNOWN, and §23.5 forbids reading it as \
         success"
    );
}

#[test]
fn should_answer_unknown_for_a_field_the_object_does_not_carry() {
    let change = verifying(vec![service("nginx", "running")], &["service"]);
    let result = change
        .verify(&contract("service nginx", "generation == 7"))
        .expect("the observation could be made");
    assert_eq!(result.status(), VerificationStatus::Unknown);
}

#[test]
fn should_say_why_it_could_not_answer() {
    let change = verifying(vec![service("nginx", "running")], &["service"]);
    let result = change
        .verify(&contract("service nginx", "cpu > 20"))
        .expect("the observation could be made");
    assert!(
        result.detail().is_some_and(|text| text.contains("§23.3")),
        "a result that says only UNKNOWN has told the operator nothing they can act on"
    );
}

#[test]
fn should_cite_the_provider_the_observation_came_from() {
    let change = verifying(vec![service("nginx", "running")], &["service"]);
    let result = change
        .verify(&contract("service nginx", "exists"))
        .expect("the observation could be made");
    assert!(
        result
            .evidence()
            .iter()
            .any(|source| source.contains("linux.systemd")),
        "§23.1's provider-level acknowledgement names the provider that acknowledged"
    );
}

#[test]
fn should_read_an_endpoint_the_way_the_reference_workflow_writes_it() {
    let change = verifying(vec![socket(443)], &["socket"]);
    let result = change
        .verify(&contract("socket :443", "exists"))
        .expect("the observation could be made");
    assert_eq!(
        result.status(),
        VerificationStatus::Passed,
        "§31: `verify socket :443 exists` is a check a provider can answer"
    );
}

#[test]
fn should_fail_when_the_listener_the_workflow_expected_is_not_there() {
    let change = verifying(Vec::new(), &["socket"]);
    let result = change
        .verify(&contract("socket :443", "exists"))
        .expect("the observation could be made");
    assert_eq!(result.status(), VerificationStatus::Failed);
}

#[test]
fn should_not_change_anything_while_verifying() {
    let provider = Arc::new(
        FakeProvider::new("linux.systemd", &["service"]).holding(vec![service("nginx", "running")]),
    );
    let change = ProviderChangeProvider::new(
        Arc::clone(&provider) as Arc<dyn ono_provider_api::Provider>,
        Arc::new(FakeObserver::new()),
        Arc::new(support::TestBridge),
        "session-1",
        support::at(),
    )
    .expect("the registries typecheck");
    change
        .verify(&contract("service nginx", "exists"))
        .expect("the observation could be made");
    assert_eq!(
        provider.act_count(),
        0,
        "§2.14 and §5.5: observing is reading, and reading changes nothing"
    );
}

#[test]
fn should_carry_the_class_the_contract_declared_into_the_result() {
    let change = verifying(vec![service("nginx", "running")], &["service"]);
    let advisory = VerificationContract::new(
        &plan(),
        VerificationClass::Advisory,
        "service nginx",
        "exists",
    );
    let result = change.verify(&advisory).expect("observed");
    assert_eq!(
        result.class(),
        VerificationClass::Advisory,
        "§23.2: how much a failure says about the plan travels with the result"
    );
}

#[test]
fn should_observe_a_quoted_literal_the_way_it_was_written() {
    let change = verifying(vec![service("nginx", "running")], &["service"]);
    let result = change
        .verify(&contract("service nginx", "state == \"running\""))
        .expect("observed");
    assert_eq!(result.status(), VerificationStatus::Passed);
}

#[test]
fn should_pass_an_inequality_when_the_field_differs() {
    let change = verifying(vec![service("nginx", "running")], &["service"]);
    let result = change
        .verify(&contract("service nginx", "state != failed"))
        .expect("observed");
    assert_eq!(result.status(), VerificationStatus::Passed);
}

#[test]
fn should_never_report_more_than_the_observer_saw() {
    let change = verifying(Vec::new(), &["service"]);
    let result = change
        .verify(&contract("service nginx", "state == running"))
        .expect("observed");
    assert_eq!(
        result.observed(),
        None,
        "§35.3: what was not seen is not reported as a value"
    );
}

#[test]
fn should_declare_the_change_capabilities_it_exercises() {
    let (_, change, _) = restart(Vec::new());
    let declared = change.capabilities();
    assert!(declared.has_change(ono_change_core::ChangeCapability::ActionExecute));
    assert!(declared.has_change(ono_change_core::ChangeCapability::VerificationObserve));
    assert!(
        declared.may_execute(),
        "§48.3: a provider that carries mutations declares that it does"
    );
}

#[test]
fn should_answer_to_the_provider_it_wraps() {
    let (_, change, _) = restart(Vec::new());
    assert_eq!(ChangeProvider::id(&change), "linux.systemd");
}

#[test]
fn should_carry_the_provider_id_into_the_execution_so_the_executor_can_route() {
    let (_, _, fragment) = restart(Vec::new());
    match only(&fragment).execution() {
        Execution::ProviderAction { provider, .. } => {
            assert_eq!(provider.as_ref(), "linux.systemd");
        }
        other => panic!("§51 resolves to a provider action, and this is {other:?}"),
    }
}

#[test]
fn should_leave_the_observer_alone_when_nothing_asks_it_anything() {
    // A trait object with no mutating method cannot mutate: this is the shape §51's prohibition
    // rests on rather than a rule a reviewer has to remember.
    let observer: Arc<dyn Observer> = Arc::new(FakeObserver::new());
    assert!(
        observer
            .fact(
                "nothing",
                ono_change_core::PreconditionKind::Existence,
                "identity"
            )
            .is_some(),
        "an observer that has seen nothing answers `there is no such object`, and it answers"
    );
}
