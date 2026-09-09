//! Mutation domains — spec v0.6 Appendix A.1 and A.2.
//!
//! Appendix A.1's two worked examples are the first three tests, verbatim in outcome: a service
//! restart touches the two runtime domains and no persistent one, and the same restart preceded
//! by a configuration replacement touches the persistent domain as well.
//!
//! §13.4 is why two objects in one domain stay two records: `/etc/nginx/nginx.conf` on the root
//! dataset and `/data/customer.db` on `tank/data` are both `filesystem-persistent` and have
//! entirely different coverage answers, so a derivation that merged them would make the claim
//! that appendix exists to forbid.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{
    ActionRole, EffectConfidence, EffectDomain, EffectKind, Execution, PlanAction, PlanId,
    ProposedEffect, RecoveryObjective,
};
use ono_change_protection::coverage::{MutationDomain, mutation_domains};

fn plan() -> PlanId {
    PlanId::derive(&["appendix-a1"])
}

/// A mutating action against `target`.
fn mutate(ordinal: usize, summary: &str, target: &str) -> PlanAction {
    PlanAction::new(
        &plan(),
        ordinal,
        ActionRole::Mutate,
        summary,
        Execution::ProviderAction {
            provider: "ono.change.systemd".into(),
            operation: "ono.service.restart".into(),
            arguments: Vec::new(),
        },
    )
    .on(target)
}

/// An action the operator declared opaque (§6.3).
fn opaque(ordinal: usize, summary: &str, description: &str) -> PlanAction {
    PlanAction::new(
        &plan(),
        ordinal,
        ActionRole::Mutate,
        summary,
        Execution::Opaque {
            description: description.into(),
            program: None,
            argv: Vec::new(),
        },
    )
}

/// A verifying action, which changes nothing (§3.3).
fn verify(ordinal: usize, summary: &str) -> PlanAction {
    PlanAction::new(
        &plan(),
        ordinal,
        ActionRole::Verify,
        summary,
        Execution::ProviderAction {
            provider: "ono.change.systemd".into(),
            operation: "ono.service.get".into(),
            arguments: Vec::new(),
        },
    )
}

/// An effect of `action`, landing on `object`.
fn effect(
    action: &PlanAction,
    domain: EffectDomain,
    kind: EffectKind,
    confidence: EffectConfidence,
    explanation: &str,
    object: &str,
) -> ProposedEffect {
    ProposedEffect::new(action.id().clone(), domain, kind, confidence, explanation).on(object)
}

/// Appendix A.1's `restart service nginx`: the two runtime domains a provider declares for it.
fn restart_nginx(ordinal: usize) -> PlanAction {
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
fn replace_conf(ordinal: usize) -> PlanAction {
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

fn domains_of(actions: &[PlanAction]) -> Vec<EffectDomain> {
    mutation_domains(actions)
        .iter()
        .map(MutationDomain::domain)
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
            .any(|mutation| mutation.domain().is_persistent()),
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
        .find(|mutation| mutation.domain() == EffectDomain::FilesystemPersistent)
        .expect("the plan changes a file");
    assert_eq!(
        filesystem.objective(),
        RecoveryObjective::PreserveExact,
        "Appendix A.2: PRESERVE_EXACT is appropriate for file and configuration persistent state"
    );
}

#[test]
fn should_give_the_runtime_domains_an_objective_a_compensation_can_meet() {
    let actions = vec![restart_nginx(0)];
    for mutation in mutation_domains(&actions) {
        assert_ne!(
            mutation.objective(),
            RecoveryObjective::PreserveExact,
            "Appendix A.2: identity may legitimately change for a restarted service's workers, so \
             the strict objective would make a plan Appendix A.6 calls PROTECTED impossible"
        );
    }
}

#[test]
fn should_name_the_object_each_record_is_about() {
    let actions = vec![replace_conf(0), restart_nginx(1)];
    let domains = mutation_domains(&actions);
    let filesystem = domains
        .iter()
        .find(|mutation| mutation.domain() == EffectDomain::FilesystemPersistent)
        .expect("the plan changes a file");
    assert_eq!(
        filesystem.subject(),
        "/etc/nginx/nginx.conf",
        "Appendix A.3 asks providers for candidates per domain, and the object is what it asks \
         about"
    );
}

#[test]
fn should_keep_two_objects_of_one_domain_apart() {
    // §13.4 is the whole reason. Both files are `filesystem-persistent` and they can live on
    // different datasets, where a snapshot of one covers nothing of the other. One row for the
    // pair would be the claim that appendix forbids.
    let etc = replace_conf(0);
    let data = {
        let action = mutate(1, "replace /data/customer.db", "/data/customer.db");
        action.clone().effecting(effect(
            &action,
            EffectDomain::FilesystemPersistent,
            EffectKind::Modify,
            EffectConfidence::Guaranteed,
            "the database file is replaced",
            "/data/customer.db",
        ))
    };
    let subjects: Vec<String> = mutation_domains(&[etc, data])
        .iter()
        .filter(|mutation| mutation.domain() == EffectDomain::FilesystemPersistent)
        .map(|mutation| mutation.subject().to_owned())
        .collect();
    assert_eq!(
        subjects,
        vec![
            "/data/customer.db".to_owned(),
            "/etc/nginx/nginx.conf".to_owned()
        ],
        "§13.4: two objects in one domain have two coverage answers, so they are two records"
    );
}

#[test]
fn should_fold_two_effects_on_one_object_into_one_record() {
    let action = mutate(0, "rewrite the config twice", "/etc/nginx/nginx.conf");
    let doubled = action
        .clone()
        .effecting(effect(
            &action,
            EffectDomain::FilesystemPersistent,
            EffectKind::Modify,
            EffectConfidence::Guaranteed,
            "the file's contents are replaced",
            "/etc/nginx/nginx.conf",
        ))
        .effecting(effect(
            &action,
            EffectDomain::FilesystemPersistent,
            EffectKind::Modify,
            EffectConfidence::Guaranteed,
            "and its mode is reset",
            "/etc/nginx/nginx.conf",
        ));
    assert_eq!(
        mutation_domains(&[doubled]).len(),
        1,
        "one object in one domain has one coverage answer, and two rows saying so is one row an \
         operator has to reconcile"
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
fn should_give_an_external_side_effect_a_compensating_objective_at_most() {
    let action = mutate(
        0,
        "call the deployment webhook",
        "https://example.test/hook",
    );
    let emitting = action.clone().effecting(effect(
        &action,
        EffectDomain::ExternalSideEffect,
        EffectKind::Emit,
        EffectConfidence::Expected,
        "a deployment webhook is posted",
        "https://example.test/hook",
    ));
    let domains = mutation_domains(&[emitting]);
    let webhook = domains.first().expect("the effect is a mutation domain");
    assert_ne!(
        webhook.objective(),
        RecoveryObjective::PreserveExact,
        "§35.2: a request already served is not recoverable through a local snapshot, so nothing \
         may ask a provider to preserve it exactly"
    );
}
