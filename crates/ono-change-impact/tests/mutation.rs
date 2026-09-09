//! Mutation domains — spec v0.6 Appendix A.1 and A.2.
//!
//! Appendix A.1's two worked examples are the first two tests, verbatim in outcome: a service
//! restart touches the two runtime domains and no persistent one, and the same restart preceded
//! by a configuration replacement touches the persistent domain as well.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{EffectConfidence, EffectDomain, EffectKind, RecoveryObjective};
use ono_change_impact::{mutation_domains, objective_for};

mod common;

use common::{effect, mutate, opaque, verify};

/// Appendix A.1's `restart service nginx`: the two runtime domains a provider declares for it.
fn restart_nginx(ordinal: usize) -> ono_change_core::PlanAction {
    let action = mutate(ordinal, "restart service nginx", "nginx.service");
    action
        .clone()
        .effecting(effect(
            &action,
            EffectDomain::ProcessRuntime,
            EffectKind::Replace,
            EffectConfidence::Expected,
            "the worker set is replaced",
            "nginx.service",
        ))
        .effecting(effect(
            &action,
            EffectDomain::NetworkRuntime,
            EffectKind::Interrupt,
            EffectConfidence::Possible,
            "connections being served may be cut",
            "nginx.service",
        ))
}

/// Appendix A.1's `replace nginx.conf`.
fn replace_conf(ordinal: usize) -> ono_change_core::PlanAction {
    let action = mutate(
        ordinal,
        "replace /etc/nginx/nginx.conf",
        "/etc/nginx/nginx.conf",
    );
    action.clone().effecting(effect(
        &action,
        EffectDomain::FilesystemPersistent,
        EffectKind::Modify,
        EffectConfidence::Guaranteed,
        "the file's contents are replaced",
        "/etc/nginx/nginx.conf",
    ))
}

fn domains_of(actions: &[ono_change_core::PlanAction]) -> Vec<EffectDomain> {
    mutation_domains(actions)
        .iter()
        .map(ono_change_impact::MutationDomain::domain)
        .collect()
}

#[test]
fn should_derive_the_two_runtime_domains_of_a_service_restart() {
    let actions = vec![restart_nginx(0)];
    assert_eq!(
        domains_of(&actions),
        vec![EffectDomain::ProcessRuntime, EffectDomain::NetworkRuntime],
        "Appendix A.1: `restart service nginx` has mutation domains process-runtime and \
         network-runtime"
    );
}

#[test]
fn should_derive_no_persistent_domain_for_a_service_restart() {
    let actions = vec![restart_nginx(0)];
    assert!(
        !mutation_domains(&actions)
            .iter()
            .any(ono_change_impact::MutationDomain::is_persistent),
        "Appendix A.1: `related persistent domain: none, unless the plan also changes \
         configuration`"
    );
}

#[test]
fn should_derive_the_persistent_domain_when_the_plan_also_replaces_the_configuration() {
    let actions = vec![replace_conf(0), restart_nginx(1)];
    assert_eq!(
        domains_of(&actions),
        vec![
            EffectDomain::FilesystemPersistent,
            EffectDomain::ProcessRuntime,
            EffectDomain::NetworkRuntime,
        ],
        "Appendix A.1: `replace nginx.conf and restart nginx` has mutation domains \
         filesystem-persistent, process-runtime and network-runtime"
    );
}

#[test]
fn should_give_the_persistent_domain_the_strict_objective() {
    let actions = vec![replace_conf(0), restart_nginx(1)];
    let domains = mutation_domains(&actions);
    let filesystem = domains
        .iter()
        .find(|domain| domain.domain() == EffectDomain::FilesystemPersistent)
        .expect("the plan changes a file");
    assert_eq!(
        filesystem.objective(),
        RecoveryObjective::PreserveExact,
        "Appendix A.2: PRESERVE_EXACT is appropriate for file and configuration persistent state"
    );
}

#[test]
fn should_give_the_runtime_domains_the_semantic_objective() {
    let actions = vec![restart_nginx(0)];
    for domain in mutation_domains(&actions) {
        assert_eq!(
            domain.objective(),
            RecoveryObjective::RestoreSemantic,
            "Appendix A.2: RESTORE_SEMANTIC is appropriate when identity may legitimately change, \
             such as service worker PIDs"
        );
    }
}

#[test]
fn should_record_the_objects_each_domain_involves() {
    let actions = vec![replace_conf(0), restart_nginx(1)];
    let domains = mutation_domains(&actions);
    let filesystem = domains
        .iter()
        .find(|domain| domain.domain() == EffectDomain::FilesystemPersistent)
        .expect("the plan changes a file");
    assert_eq!(
        filesystem.objects(),
        &[std::sync::Arc::from("/etc/nginx/nginx.conf")],
        "Appendix A.3 asks providers for candidates per domain, and the objects are what it asks \
         about"
    );
}

#[test]
fn should_gather_two_actions_touching_one_domain_into_one_record() {
    let actions = vec![restart_nginx(0), {
        let action = mutate(1, "restart service backend", "backend.service");
        action.clone().effecting(effect(
            &action,
            EffectDomain::ProcessRuntime,
            EffectKind::Replace,
            EffectConfidence::Expected,
            "the worker set is replaced",
            "backend.service",
        ))
    }];
    let domains = mutation_domains(&actions);
    let runtime = domains
        .iter()
        .find(|domain| domain.domain() == EffectDomain::ProcessRuntime)
        .expect("both actions touch process runtime");
    assert_eq!(
        runtime.objects().len(),
        2,
        "Appendix A.5 composes coverage per domain, so one domain is one record over every object \
         in it"
    );
}

#[test]
fn should_ignore_an_action_that_changes_nothing() {
    let actions = vec![verify(0, "read the unit's state")];
    assert!(
        mutation_domains(&actions).is_empty(),
        "Appendix A.1 derives records for each MUTATE action, and a verification is not one"
    );
}

#[test]
fn should_derive_an_unknown_domain_for_an_opaque_action() {
    let actions =
        vec![opaque(0, "run vendor installer", "an installer nobody reads").on("installer")];
    assert_eq!(
        domains_of(&actions),
        vec![EffectDomain::Unknown],
        "§6.3 and Appendix A.7: an opaque action's domain is unknown, and the record has to exist \
         for the cap to apply"
    );
}

#[test]
fn should_derive_an_unknown_domain_for_a_mutating_action_that_declares_no_effect() {
    let actions = vec![mutate(0, "do something to nginx", "nginx.service")];
    let domains = mutation_domains(&actions);
    assert_eq!(domains_of(&actions), vec![EffectDomain::Unknown]);
    assert_eq!(
        domains
            .first()
            .expect("the unknown record exists")
            .objective(),
        RecoveryObjective::Unknown,
        "Appendix A.2 and A.7: an objective nobody could establish is satisfied by nothing"
    );
}

#[test]
fn should_answer_in_appendix_a1s_own_order() {
    // Declared in the reverse of Appendix A.1's listing, to prove the answer is ordered rather
    // than merely observed.
    let actions = vec![restart_nginx(0), replace_conf(1)];
    assert_eq!(
        domains_of(&actions),
        vec![
            EffectDomain::FilesystemPersistent,
            EffectDomain::ProcessRuntime,
            EffectDomain::NetworkRuntime,
        ],
        "Appendix A.1 lists the canonical domains in a fixed order, and two derivations of one \
         plan must compare equal (§4.4)"
    );
}

#[test]
fn should_derive_the_same_domains_from_the_same_plan_twice() {
    let actions = vec![replace_conf(0), restart_nginx(1)];
    assert_eq!(
        mutation_domains(&actions),
        mutation_domains(&actions),
        "§4.4 seals the plan, so what it seals must be stable"
    );
}

#[test]
fn should_give_an_external_side_effect_the_compensating_objective() {
    let action = mutate(0, "call deployment webhook", "webhook");
    let actions = vec![action.clone().effecting(effect(
        &action,
        EffectDomain::ExternalSideEffect,
        EffectKind::Emit,
        EffectConfidence::Expected,
        "a deployment webhook is posted",
        "https://example.invalid/deployments",
    ))];
    let domains = mutation_domains(&actions);
    assert_eq!(
        domains
            .first()
            .expect("the webhook is a mutation domain")
            .objective(),
        RecoveryObjective::Compensate,
        "§35.3: a provider MAY define a compensating action, and that is COMPENSATABLE rather \
         than rollback"
    );
}

#[test]
fn should_fall_back_to_the_actions_target_when_an_effect_names_no_object() {
    let action = mutate(0, "restart service nginx", "nginx.service");
    let actions = vec![
        action
            .clone()
            .effecting(ono_change_core::ProposedEffect::new(
                action.id().clone(),
                EffectDomain::ProcessRuntime,
                EffectKind::Replace,
                EffectConfidence::Expected,
                "the worker set is replaced",
            )),
    ];
    let domains = mutation_domains(&actions);
    assert_eq!(
        domains
            .first()
            .expect("the restart is a mutation domain")
            .objects(),
        &[std::sync::Arc::from("nginx.service")],
        "Appendix A.3 needs something to ask a provider about, and the action's target is what \
         the plan named"
    );
}

#[test]
fn should_assign_the_transaction_domain_no_recovery_of_its_own() {
    assert_eq!(
        objective_for(EffectDomain::ProviderTransactionState),
        RecoveryObjective::NoRecoveryRequired,
        "§27.1: a provider-local transaction rolls itself back"
    );
}

#[test]
fn should_keep_a_domain_record_free_of_provider_knowledge() {
    let actions = vec![replace_conf(0)];
    let domain = mutation_domains(&actions)
        .into_iter()
        .next()
        .expect("the plan changes a file");
    // Appendix A.3 is where providers enter; a record that already named a dataset would have
    // decided the answer before the question was asked.
    assert_eq!(domain.domain(), EffectDomain::FilesystemPersistent);
    assert_eq!(domain.objective(), RecoveryObjective::PreserveExact);
    assert_eq!(domain.objects().len(), 1);
}
