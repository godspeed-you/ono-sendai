#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! Arbitrary external commands, and the one escape §6.3 allows.

mod support;

use ono_change_actions::registry::{EffectSpec, VerificationSpec};
use ono_change_actions::{AdapterAction, opaque_action, plan_external, refuse};
use ono_change_core::{
    ChangeProvider, EffectConfidence, EffectDomain, EffectKind, Execution, Idempotency, Intent,
    PlanId, Support, VerificationClass,
};
use support::service_provider;

fn plan() -> PlanId {
    PlanId::of("session-1", support::AT, "plan sh -c 'rm -rf /somewhere'")
}

#[test]
fn should_refuse_an_arbitrary_external_command() {
    let error = refuse("sh -c 'rm -rf /somewhere'");
    assert_eq!(
        error.code().name(),
        "change.opaque_action_forbidden",
        "§6.2: `plan sh -c 'rm -rf /somewhere'` MUST fail by default"
    );
}

#[test]
fn should_say_why_an_arbitrary_command_cannot_be_reasoned_about() {
    let error = refuse("sh -c 'rm -rf /somewhere'");
    assert!(
        error
            .message()
            .contains("cannot reason about what it touches"),
        "§6.2: Ono cannot reason about target scope or side effects"
    );
}

#[test]
fn should_point_an_operator_at_the_explicit_escape() {
    let error = refuse("sh -c 'rm -rf /somewhere'");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("opaque action")),
        "§6.3's entry exists where an operator accepts what it costs, and the refusal says so"
    );
}

#[test]
fn should_refuse_an_external_command_no_adapter_declared() {
    let error = plan_external(&plan(), 0, "sh -c 'rm -rf /somewhere'", "/bin/sh", &[])
        .expect_err("§6.2 is a refusal by default");
    assert_eq!(error.code().name(), "change.opaque_action_forbidden");
}

#[test]
fn should_answer_none_when_supports_is_asked_about_an_arbitrary_command() {
    let (_, change) = service_provider();
    assert_eq!(
        change.supports(&Intent::new("sh -c 'rm -rf /somewhere'", "plan sh -c …")),
        Support::None,
        "§6.2: an arbitrary external command is not automatically plannable"
    );
}

// --- §6.3's explicit escape ---------------------------------------------------------------------

#[test]
fn should_classify_an_opaque_action_s_impact_as_unknown() {
    let action = opaque_action(&plan(), 0, "rotate the vendor appliance", None, &[]);
    let effect = &action.effects()[0];
    assert_eq!(
        effect.domain(),
        EffectDomain::Unknown,
        "§6.3: an opaque action MUST classify impact as unknown"
    );
    assert_eq!(effect.kind(), EffectKind::Unknown);
}

#[test]
fn should_classify_an_opaque_action_s_reversibility_as_unknown() {
    let action = opaque_action(&plan(), 0, "rotate the vendor appliance", None, &[]);
    let effect = &action.effects()[0];
    assert_eq!(effect.confidence(), EffectConfidence::Unknown);
    assert!(
        effect.is_irreversible(),
        "§6.3: reversibility is unknown unless separately protected, and §2.4 forbids promoting it"
    );
}

#[test]
fn should_give_an_opaque_action_exactly_one_effect() {
    let action = opaque_action(&plan(), 0, "rotate the vendor appliance", None, &[]);
    assert_eq!(
        action.effects().len(),
        1,
        "an action nobody can reason about has one thing to say, and Appendix A.7 reads it"
    );
}

#[test]
fn should_keep_an_opaque_action_out_of_blind_retry() {
    let action = opaque_action(&plan(), 0, "rotate the vendor appliance", None, &[]);
    assert_eq!(action.idempotency(), Idempotency::Unknown);
    assert!(
        !action.idempotency().permits_blind_retry(),
        "§41.2: Ono MUST NOT blindly rerun an unknown action"
    );
}

#[test]
fn should_say_an_opaque_action_earns_no_protection_from_a_neighbouring_snapshot() {
    let action = opaque_action(&plan(), 0, "rotate the vendor appliance", None, &[]);
    assert!(
        action.effects()[0]
            .explanation()
            .contains("not protection for this"),
        "§6.3: an opaque action MUST NOT receive PROTECTED merely because a filesystem snapshot \
         exists somewhere on the host"
    );
}

#[test]
fn should_carry_an_opaque_execution_rather_than_a_provider_action() {
    let action = opaque_action(
        &plan(),
        0,
        "rotate the vendor appliance",
        Some("/opt/vendor/rotate"),
        &["--now"],
    );
    match action.execution() {
        Execution::Opaque { program, argv, .. } => {
            assert_eq!(program.as_deref(), Some("/opt/vendor/rotate"));
            assert_eq!(argv.len(), 1, "§12.3: one element per argument, unexpanded");
        }
        other => panic!("§6.3's escape is an opaque execution, and this is {other:?}"),
    }
    assert!(action.execution().is_opaque());
}

#[test]
fn should_state_that_an_opaque_action_has_no_declared_recovery() {
    let action = opaque_action(&plan(), 0, "rotate the vendor appliance", None, &[]);
    assert!(
        action
            .declared_recovery()
            .is_some_and(|text| text.contains("§6.3")),
        "§6.1 accepts recovery semantics or their explicit absence, and this is the absence"
    );
}

// --- §6.2's second sentence: an adapter with an explicit contract ---------------------------------

fn nginx_test() -> AdapterAction {
    AdapterAction::declare(
        "dev.example.nginx",
        "/usr/sbin/nginx",
        "validate the nginx configuration",
        Idempotency::Idempotent,
        "the check writes nothing, so there is nothing to recover.",
        vec![EffectSpec::new(
            EffectDomain::ProcessRuntime,
            EffectKind::Create,
            EffectConfidence::Guaranteed,
            "a short-lived process reads the configuration and exits",
        )],
        vec![VerificationSpec::new(
            VerificationClass::Required,
            "the configuration",
            "exists",
        )],
    )
    .expect("the contract is explicit")
    .with_argv(["-t"])
}

#[test]
fn should_let_an_adapter_expose_a_plannable_action_when_its_contract_is_explicit() {
    let action = plan_external(&plan(), 0, "nginx -t", "/usr/sbin/nginx", &[nginx_test()])
        .expect("§6.2: an adapter MAY expose a plannable action if its contract is explicit");
    assert_eq!(action.summary(), "validate the nginx configuration");
    assert!(!action.effects().is_empty());
}

#[test]
fn should_run_an_adapter_action_as_a_program_rather_than_a_command_line() {
    let action = plan_external(&plan(), 0, "nginx -t", "/usr/sbin/nginx", &[nginx_test()])
        .expect("declared");
    match action.execution() {
        Execution::Program { program, argv } => {
            assert_eq!(program.as_ref(), "/usr/sbin/nginx");
            assert_eq!(argv.as_slice(), [std::sync::Arc::from("-t")]);
        }
        other => panic!("§2.17 forbids a command line, and this is {other:?}"),
    }
}

#[test]
fn should_cite_the_adapter_that_declared_an_effect() {
    let action = plan_external(&plan(), 0, "nginx -t", "/usr/sbin/nginx", &[nginx_test()])
        .expect("declared");
    assert!(
        action.effects()[0]
            .evidence()
            .iter()
            .any(|reference| reference.contains("dev.example.nginx")),
        "§8.2's `evidence` names who made the claim"
    );
}

#[test]
fn should_still_refuse_a_program_no_adapter_declared() {
    plan_external(&plan(), 0, "rm -rf /somewhere", "/bin/rm", &[nginx_test()])
        .expect_err("§6.2's default stands for everything an adapter did not declare");
}

#[test]
fn should_refuse_an_adapter_declaration_with_no_effects() {
    let error = AdapterAction::declare(
        "dev.example.nginx",
        "/usr/sbin/nginx",
        "do something",
        Idempotency::Unknown,
        "none stated.",
        Vec::new(),
        vec![VerificationSpec::new(
            VerificationClass::Required,
            "it",
            "exists",
        )],
    )
    .expect_err("§6.1 requires expected direct effects");
    assert_eq!(error.code().name(), "change.action_not_plannable");
}

#[test]
fn should_refuse_an_adapter_declaration_with_no_verification() {
    AdapterAction::declare(
        "dev.example.nginx",
        "/usr/sbin/nginx",
        "do something",
        Idempotency::Unknown,
        "none stated.",
        vec![EffectSpec::new(
            EffectDomain::Unknown,
            EffectKind::Unknown,
            EffectConfidence::Unknown,
            "something happens",
        )],
        Vec::new(),
    )
    .expect_err("§23.1 requires at least one verification contract");
}

#[test]
fn should_mark_an_emitted_effect_irreversible_without_being_told() {
    let effect = EffectSpec::new(
        EffectDomain::ExternalSideEffect,
        EffectKind::Emit,
        EffectConfidence::Expected,
        "an HTTP POST leaves the machine",
    );
    assert!(
        effect.is_irreversible(),
        "§35.1: the request has been served by the time Ono could reconsider"
    );
}

#[test]
fn should_carry_an_adapter_s_compensation_where_it_names_one() {
    let effect = EffectSpec::new(
        EffectDomain::ExternalSideEffect,
        EffectKind::Create,
        EffectConfidence::Expected,
        "a cloud resource is created",
    )
    .compensated_by("delete the newly created resource");
    assert_eq!(
        effect.compensation(),
        Some("delete the newly created resource"),
        "§35.3: a provider MAY define a compensating action, and it is COMPENSATABLE, not rollback"
    );
}
