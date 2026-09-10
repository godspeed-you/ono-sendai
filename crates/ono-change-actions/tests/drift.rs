#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! Revalidation: the facts §7.2 froze, rechecked, and §7.4's declared tolerances.

mod support;

use std::sync::Arc;

use ono_change_actions::registry::OperationRegistry;
use ono_change_core::{
    ChangeProvider, DriftVerdict, Intent, PlanAction, PreconditionKind, VerificationClass,
};
use ono_value::Value;
use support::{FakeObserver, FakeProvider, change_provider, service_provider};

fn restart_action(
    observer: FakeObserver,
) -> (PlanAction, ono_change_actions::ProviderChangeProvider) {
    let provider = Arc::new(FakeProvider::new("linux.systemd", &["service"]));
    let change = change_provider(provider, Arc::new(observer));
    let fragment = change
        .resolve(
            &Intent::new("restart service nginx", "restart service nginx"),
            &[],
        )
        .expect("a restart is plannable");
    (fragment.actions()[0].clone(), change)
}

#[test]
fn should_report_nothing_when_every_frozen_fact_still_holds() {
    let (action, change) = restart_action(FakeObserver::nginx());
    let findings = change.revalidate(&action).expect("the check could be made");
    assert!(
        findings.is_empty(),
        "§7.3: a plan whose world has not moved proceeds, and this found {findings:?}"
    );
}

#[test]
fn should_report_material_drift_when_the_service_state_moved() {
    let observer = FakeObserver::nginx().seeing("nginx", "state", Value::string("failed"));
    let (action, _) = restart_action(FakeObserver::nginx());
    let change = change_provider(
        Arc::new(FakeProvider::new("linux.systemd", &["service"])),
        Arc::new(observer),
    );
    let findings = change.revalidate(&action).expect("the check could be made");
    let generation = findings
        .iter()
        .find(|finding| finding.kind() == PreconditionKind::Generation)
        .expect("§7.2: the service's generation or state still equals what was resolved");
    assert_eq!(generation.verdict(), DriftVerdict::Material);
    assert!(
        generation.verdict().blocks_apply(),
        "§7.3: no mutation occurs"
    );
}

#[test]
fn should_report_what_was_expected_and_what_was_seen() {
    let observer = FakeObserver::nginx().seeing("nginx", "state", Value::string("failed"));
    let (action, _) = restart_action(FakeObserver::nginx());
    let change = change_provider(
        Arc::new(FakeProvider::new("linux.systemd", &["service"])),
        Arc::new(observer),
    );
    let finding = change
        .revalidate(&action)
        .expect("the check could be made")
        .into_iter()
        .find(|finding| finding.kind() == PreconditionKind::Generation)
        .expect("state is a frozen fact");
    assert_eq!(finding.expected(), &Value::string("running"));
    assert_eq!(finding.observed(), Some(&Value::string("failed")));
}

#[test]
fn should_tolerate_a_move_the_contract_declared_harmless() {
    let observer = FakeObserver::nginx().seeing("nginx", "cpu", Value::Float(41.0));
    let (action, _) = restart_action(FakeObserver::nginx());
    let change = change_provider(
        Arc::new(FakeProvider::new("linux.systemd", &["service"])),
        Arc::new(observer),
    );
    let findings = change.revalidate(&action).expect("the check could be made");
    let cpu = findings
        .iter()
        .find(|finding| finding.field() == "cpu")
        .expect("§7.4's own example: CPU usage moving while a service restart is planned");
    assert_eq!(cpu.verdict(), DriftVerdict::Tolerated);
    assert!(
        !cpu.verdict().blocks_apply(),
        "§7.4: such tolerance MUST be contract-declared, and it is"
    );
}

#[test]
fn should_block_when_a_precondition_could_not_be_observed_at_all() {
    let observer = FakeObserver::nginx().blind_to("nginx", "state");
    let (action, _) = restart_action(FakeObserver::nginx());
    let change = change_provider(
        Arc::new(FakeProvider::new("linux.systemd", &["service"])),
        Arc::new(observer),
    );
    let finding = change
        .revalidate(&action)
        .expect("the call itself succeeded")
        .into_iter()
        .find(|finding| finding.kind() == PreconditionKind::Generation)
        .expect("state is a frozen fact");
    assert_eq!(
        finding.verdict(),
        DriftVerdict::Unknown,
        "§2.4: a precondition nobody could check has not held"
    );
    assert!(finding.verdict().blocks_apply());
}

#[test]
fn should_say_so_when_a_fact_could_not_be_observed_at_resolution() {
    let observer = FakeObserver::nginx().blind_to("nginx", "state");
    let (action, _) = restart_action(observer);
    let generation = action
        .preconditions()
        .iter()
        .find(|p| p.kind() == PreconditionKind::Generation)
        .expect("state is a declared precondition");
    assert!(
        generation.detail().contains("could not be observed"),
        "§2.4: the plan says what it does not know rather than freezing a guess"
    );
}

#[test]
fn should_treat_an_object_that_appeared_since_resolution_as_material_drift() {
    // Planning to create a user that is not there freezes an absence. Somebody else creating it
    // in the meantime is exactly the drift §7.3 stops on.
    let provider = Arc::new(FakeProvider::new("linux.users", &["user"]));
    let planning = change_provider(
        Arc::clone(&provider),
        Arc::new(FakeObserver::new().seeing("deploy", "identity", Value::Null)),
    );
    let fragment = planning
        .resolve(&Intent::new("add user deploy", "add user deploy"), &[])
        .expect("adding a user is plannable");
    let applying = change_provider(
        provider,
        Arc::new(FakeObserver::new().seeing("deploy", "identity", Value::string("deploy"))),
    );
    let finding = applying
        .revalidate(&fragment.actions()[0])
        .expect("the check could be made")
        .into_iter()
        .find(|finding| finding.kind() == PreconditionKind::Existence)
        .expect("§7.2: the object still exists and is still this object");
    assert_eq!(finding.verdict(), DriftVerdict::Material);
}

#[test]
fn should_pass_when_an_absence_is_still_an_absence() {
    let provider = Arc::new(FakeProvider::new("linux.users", &["user"]));
    let observer = || Arc::new(FakeObserver::new().seeing("deploy", "identity", Value::Null));
    let planning = change_provider(Arc::clone(&provider), observer());
    let fragment = planning
        .resolve(&Intent::new("add user deploy", "add user deploy"), &[])
        .expect("plannable");
    let applying = change_provider(provider, observer());
    assert!(
        applying
            .revalidate(&fragment.actions()[0])
            .expect("the check could be made")
            .iter()
            .all(|finding| finding.verdict() != DriftVerdict::Material),
        "spec §35.3: null is the absence of a value, and an absence that has not moved is no drift"
    );
}

#[test]
fn should_freeze_the_capability_the_action_needs() {
    let (action, _) = restart_action(FakeObserver::nginx());
    let capability = action
        .preconditions()
        .iter()
        .find(|p| p.kind() == PreconditionKind::Capability)
        .expect("§43.2: the capability the action needs is still held");
    assert_eq!(
        capability.expected(),
        &Value::string("service.manage"),
        "`restart service` declares `service.manage`, and that is the fact frozen"
    );
}

#[test]
fn should_require_the_provider_to_still_be_available_at_apply_time() {
    let (action, _) = restart_action(FakeObserver::nginx());
    let available = action
        .preconditions()
        .iter()
        .find(|p| p.kind() == PreconditionKind::ProviderAvailable)
        .expect("§7.2: the recovery provider is still available");
    assert_eq!(available.subject(), "linux.systemd");
    assert_eq!(available.expected(), &Value::Bool(true));
}

#[test]
fn should_carry_the_provider_availability_on_the_fragment_as_well_as_the_action() {
    let (_, change) = service_provider();
    let fragment = change
        .resolve(
            &Intent::new("restart service nginx", "restart service nginx"),
            &[],
        )
        .expect("plannable");
    assert!(
        fragment
            .preconditions()
            .iter()
            .any(|p| p.kind() == PreconditionKind::ProviderAvailable),
        "§7.2: the fragment depends on the provider as a whole, not only on one action"
    );
}

#[test]
fn should_default_every_frozen_fact_to_material() {
    let (action, _) = restart_action(FakeObserver::nginx());
    let tolerated: Vec<&str> = OperationRegistry::embedded()
        .expect("the embedded registry typechecks")
        .get("ono.service.restart")
        .expect("plannable")
        .tolerances()
        .iter()
        .map(|tolerance| tolerance.field())
        .collect();
    assert!(
        !action.preconditions().is_empty(),
        "precondition: a restart freezes facts"
    );
    for precondition in action.preconditions() {
        let declared_tolerant = tolerated.contains(&precondition.field())
            && precondition.kind() == PreconditionKind::Field;
        assert_eq!(
            precondition.is_material(),
            !declared_tolerant,
            "§7.4: tolerance is contract-declared, so every fact the contract did not tolerate is \
             material, and `{}` on `{}` reads otherwise",
            precondition.field(),
            precondition.subject()
        );
    }
}

#[test]
fn should_explain_a_persistence_domain_move_as_a_recovery_domain_move() {
    let observer = FakeObserver::new()
        .seeing("/srv/app.conf", "identity", Value::string("/srv/app.conf"))
        .seeing("/srv/app.conf", "sha256", Value::string("aaa"))
        .seeing(
            "/srv/app.conf",
            "persistence-domain",
            Value::string("tank/srv"),
        );
    let provider = Arc::new(FakeProvider::new("linux.files", &["file"]));
    let change = change_provider(provider, Arc::new(observer));
    let fragment = change
        .resolve(
            &Intent::new("remove file /srv/app.conf", "remove file /srv/app.conf"),
            &[],
        )
        .expect("plannable");
    let domain = fragment.actions()[0]
        .preconditions()
        .iter()
        .find(|p| p.kind() == PreconditionKind::PersistenceDomain)
        .expect("§7.2 lists it");
    assert!(
        domain.detail().contains("moved recovery domains"),
        "§7.2 and Appendix B: a target that moved filesystems moved recovery domains"
    );
}

#[test]
fn should_keep_verification_separate_from_the_preconditions() {
    let (_, change) = service_provider();
    let fragment = change
        .resolve(
            &Intent::new("restart service nginx", "restart service nginx"),
            &[],
        )
        .expect("plannable");
    assert!(
        !fragment.verification().is_empty(),
        "§23.1: a restart carries its postcondition as a verification contract"
    );
    assert!(
        fragment
            .verification()
            .iter()
            .all(|contract| contract.class() == VerificationClass::Required),
        "§23.2: a restart's postcondition is required, and §2.14 keeps it apart from execution \
         success"
    );
    let preconditions: Vec<_> = fragment
        .preconditions()
        .iter()
        .chain(
            fragment
                .actions()
                .iter()
                .flat_map(|action| action.preconditions()),
        )
        .collect();
    for contract in fragment.verification() {
        assert!(
            preconditions
                .iter()
                .all(|precondition| precondition.field() != contract.expression()
                    && !precondition.detail().contains(contract.expression())),
            "§2.14: the postcondition `{}` is checked after the action, not frozen before it",
            contract.expression()
        );
    }
}
