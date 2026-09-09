//! Auto-recovery policy (spec §26).
//!
//! §26.1 makes automatic recovery after a failed verification **off by default**, and §26.2 says
//! why in one sentence: *"A failed verification does not prove that rollback is safer than leaving
//! the new state in place."* So this module never decides that a recovery should run — it decides
//! only whether a plan is allowed to *declare* that it may, and it refuses at seal time otherwise.
//!
//! §26.3's six conditions are conjunctive, and [`admits_auto_recovery`] names every one that does
//! not hold rather than the first. A declaration rejected for one reason gets fixed and rejected
//! again for the next, and §45's structured refusal exists so an operator sees the whole list at
//! once.
//!
//! Two of the six need a reading, because the specification states them in prose:
//!
//! - *"recovery does not destroy unrelated newer state"* is `DISCARDED_BY_METHOD` and any
//!   provider-native object that would be destroyed. A `CONFLICTING` object is the target of the
//!   recovery itself and is therefore related by construction (Appendix C.4), so it does not
//!   disqualify the declaration on its own.
//! - *"recovery verification exists"* is read as §25.1's verification: at least one contract that
//!   says which equivalence domain it reports on. A recovery plan carrying only checks that
//!   decline to say what kind of equivalence they establish cannot produce §25.2's answer, and a
//!   verification that cannot be scoped is the thing §25.3 forbids acting on.

use ono_change_core::{
    ActionRole, ChangePlan, DomainProtection, EffectKind, NewerStateClass, RecoveryPlan, error,
};
use ono_value::ErrorValue;

/// Whether `plan` may declare automatic recovery through `recovery` (§26.3).
///
/// # Errors
///
/// `change.auto_recovery_rejected`, naming every one of §26.3's conditions that does not hold.
/// §26.3's own sentence is that the declaration "MUST be rejected at seal time", so this is the
/// check a sealer runs and not a runtime decision to roll back.
pub fn admits_auto_recovery(
    plan: &ChangePlan,
    recovery: &RecoveryPlan,
    policy_enabled: bool,
) -> Result<(), ErrorValue> {
    let mut unmet: Vec<String> = Vec::new();

    if !fully_constructed(recovery) {
        unmet.push(
            "the recovery plan cannot be fully constructed before mutation: it is not sealed, it \
             carries no RECOVER action, or its newer-state analysis did not run (§26.3, §62.8)"
                .to_owned(),
        );
    }
    let irreversible = irreversible_external(plan, recovery);
    if !irreversible.is_empty() {
        unmet.push(format!(
            "known irreversible external side effects exist: {} (§26.3, §35.1)",
            irreversible.join(", ")
        ));
    }
    let destroyed = unrelated_newer_state(recovery);
    if !destroyed.is_empty() {
        unmet.push(format!(
            "recovery would destroy unrelated newer state: {} (§26.3, Appendix C.3)",
            destroyed.join(", ")
        ));
    }
    let shortfall = protection_shortfall(plan);
    if !shortfall.is_empty() {
        unmet.push(format!(
            "protection is neither PROTECTED nor TRANSACTIONAL for {} (§26.3, §10.2)",
            shortfall.join(", ")
        ));
    }
    if !recovery_verification_exists(recovery) {
        unmet.push(
            "the recovery carries no verification that states which equivalence domain it \
             establishes (§26.3, §25.1)"
                .to_owned(),
        );
    }
    if !policy_enabled {
        unmet.push(
            "user policy does not enable automatic recovery, and §26.1 makes it off by default"
                .to_owned(),
        );
    }

    if unmet.is_empty() {
        return Ok(());
    }
    Err(error::auto_recovery_rejected(plan.id(), &unmet))
}

/// §26.3's first condition: the recovery plan can be fully constructed before mutation.
fn fully_constructed(recovery: &RecoveryPlan) -> bool {
    recovery.plan().state().is_sealed()
        && !recovery.plan().actions_of(ActionRole::Recover).is_empty()
        && recovery.newer_state().is_complete()
}

/// §26.3's second condition: no known irreversible external side effect exists.
fn irreversible_external(plan: &ChangePlan, recovery: &RecoveryPlan) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for effect in plan.effects() {
        let irreversible = effect.kind() == EffectKind::Emit
            || (effect.is_irreversible() && effect.domain().is_external());
        if irreversible {
            found.push(effect.object().unwrap_or(effect.explanation()).to_owned());
        }
    }
    for effect in recovery.unrecoverable() {
        if effect.domain().is_external() && !found.iter().any(|seen| seen == effect.subject()) {
            found.push(effect.subject().to_owned());
        }
    }
    found
}

/// §26.3's third condition: recovery destroys no unrelated newer state.
fn unrelated_newer_state(recovery: &RecoveryPlan) -> Vec<String> {
    let mut found: Vec<String> = recovery
        .newer_state()
        .items()
        .iter()
        .filter(|item| item.class() == NewerStateClass::DiscardedByMethod)
        .map(|item| item.object().to_owned())
        .collect();
    found.extend(
        recovery
            .newer_state()
            .destroyed_assets()
            .iter()
            .map(std::string::ToString::to_string),
    );
    found
}

/// §26.3's fourth condition: PROTECTED or TRANSACTIONAL for the required mutation domains.
fn protection_shortfall(plan: &ChangePlan) -> Vec<String> {
    if plan.protection().required_rows().next().is_none() {
        return vec![
            "no mutation domain carries a coverage row at all, so the condition cannot be shown \
             to hold (§10.3)"
                .to_owned(),
        ];
    }
    plan.protection()
        .required_rows()
        .filter(|row| !row.protection().is_state_image())
        .map(|row| {
            format!(
                "{} ({})",
                row.domain().as_str(),
                match row.protection() {
                    DomainProtection::Unprotected => "unprotected",
                    DomainProtection::Compensatable => "compensatable",
                    DomainProtection::Unknown => "unknown",
                    DomainProtection::Protected | DomainProtection::Transactional => "covered",
                }
            )
        })
        .collect()
}

/// §26.3's fifth condition: recovery verification exists (§25.1).
fn recovery_verification_exists(recovery: &RecoveryPlan) -> bool {
    recovery
        .plan()
        .verification()
        .contracts()
        .iter()
        .any(|contract| contract.equivalence().is_some())
}
