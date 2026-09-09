//! Recovery verification (spec §25.1, §25.2, §25.3, Appendix F).
//!
//! §25.1 splits equivalence three ways — persistent state, runtime state, external side effects —
//! and §25.2 shows why: a service restart restores the configuration and legitimately creates new
//! worker PIDs, and a verification that cannot tell those apart has to call one of them wrong.
//! `RecoveryOutcome` already has one field per domain and no summary field, so the shape of the
//! answer carries §25.3's prohibition rather than a renderer remembering it.
//!
//! Two mappings do the work here, and both are decisions rather than transcriptions.
//!
//! **A check's status is not its equivalence state.** §23.3's four statuses say whether the
//! condition held; §25.2's five states say what happened to the subject. A `Required` check that
//! failed did not come back. A check the plan declared `Observational` about runtime state is
//! §25.2's `DIFFERENT / EXPECTED` — the plan wrote it as observational precisely because the
//! identity was never promised, and §23.2 says an observational result never changes the plan's
//! state.
//!
//! **What cannot be recovered is evidence too.** The TCP sessions and the delivered webhook of
//! §25.2 are not checks that failed; they are effects the recovery plan already said it cannot
//! reverse, and [`outcome_of`] records each one as `NOT_RECOVERABLE` in its own domain so
//! `RecoveryOutcome::has_unrecoverable` answers the question §25.2 prints beside the verdict.

use ono_change_core::{
    EffectDomain, EquivalenceDomain, EquivalenceState, RecoveryOutcome, RecoveryPlan,
    UnrecoverableEffect, VerificationClass, VerificationContract, VerificationResult,
    VerificationStatus, error,
};
use ono_value::{ErrorValue, Value};

use jiff::Timestamp;

/// Observes every contract the recovery plan carries, per equivalence domain (§25.1).
///
/// The result of each check keeps its class and its domain, which is what [`refusal_for`] needs
/// to tell an advisory difference from a required failure, and what §25.2's rendering needs to
/// print three sections instead of one word.
#[must_use]
pub fn results(
    plan: &RecoveryPlan,
    observe: &dyn Fn(&VerificationContract) -> VerificationStatus,
    now: Timestamp,
) -> Vec<VerificationResult> {
    plan.plan()
        .verification()
        .contracts()
        .iter()
        .map(|contract| {
            let status = observe(contract);
            VerificationResult::new(plan.plan().id().clone(), contract, status, now)
                .equivalent(equivalence_state(contract, status))
        })
        .collect()
}

/// What a recovery actually achieved, per equivalence domain (§25.1, §25.2).
#[must_use]
pub fn verify(
    plan: &RecoveryPlan,
    observe: &dyn Fn(&VerificationContract) -> VerificationStatus,
    now: Timestamp,
) -> RecoveryOutcome {
    outcome_of(plan, &results(plan, observe, now))
}

/// Composes §25.2's three sections out of observed results and the plan's own exclusions.
#[must_use]
pub fn outcome_of(plan: &RecoveryPlan, results: &[VerificationResult]) -> RecoveryOutcome {
    let mut outcome = RecoveryOutcome::empty();
    for result in results {
        outcome = outcome.recording(
            result
                .equivalence()
                .unwrap_or(EquivalenceDomain::PersistentState),
            result.subject(),
            result
                .equivalence_state()
                .unwrap_or(EquivalenceState::Unknown),
        );
    }
    for effect in plan.unrecoverable() {
        outcome = outcome.recording(
            domain_of(effect),
            effect.subject(),
            EquivalenceState::NotRecoverable,
        );
    }
    outcome
}

/// The refusal a failed recovery verification produces (§25.3, Appendix F).
///
/// Appendix F's last two rows are the contract: a recovery whose verification fails leaves the
/// plan `RECOVERY_FAILED` and *does not claim recovered*. The refusal names the equivalence
/// domains the failure is in, so the sentence a person reads is scoped the way §25.3 requires.
#[must_use]
pub fn refusal_for(results: &[VerificationResult]) -> Option<ErrorValue> {
    let mut domains: Vec<String> = Vec::new();
    for domain in EquivalenceDomain::ALL {
        let failed = results.iter().any(|result| {
            result.class() == VerificationClass::Required
                && result
                    .equivalence()
                    .unwrap_or(EquivalenceDomain::PersistentState)
                    == *domain
                && matches!(
                    result.equivalence_state(),
                    Some(EquivalenceState::NotRestored | EquivalenceState::Unknown)
                )
        });
        if failed {
            domains.push(domain.as_str().to_owned());
        }
    }
    if domains.is_empty() {
        return None;
    }
    Some(error::recovery_verification_failed(&domains))
}

/// The refusal a recovery action's own failure produces (§41.3, Appendix F).
///
/// Appendix F's `recovery action fails` row requires the remaining assets and the exact partial
/// state to be preserved, so the refusal carries both: which assets are still there to try again
/// from, and which of the recovery's actions had already run. §41.3 then decides per action
/// whether resume may continue, and it cannot decide that from a sentence.
#[must_use]
pub fn recovery_failed(plan: &RecoveryPlan, action: &str, detail: &str) -> ErrorValue {
    let assets: Vec<Value> = plan
        .source_assets()
        .iter()
        .map(|asset| Value::string(asset.as_str()))
        .collect();
    let settled: Vec<Value> = plan
        .plan()
        .actions()
        .iter()
        .filter(|planned| planned.status().is_settled())
        .map(|planned| Value::string(planned.summary()))
        .collect();
    error::recovery_apply_failed(action, detail)
        .with_metadata("retained_assets", Value::list(assets))
        .with_metadata("settled_actions", Value::list(settled))
        .with_metadata("method", Value::string(plan.method().as_str()))
}

/// What one observed check says happened to its subject (§25.2).
fn equivalence_state(
    contract: &VerificationContract,
    status: VerificationStatus,
) -> EquivalenceState {
    match status {
        VerificationStatus::Passed => EquivalenceState::Restored,
        VerificationStatus::Unknown | VerificationStatus::Skipped => EquivalenceState::Unknown,
        VerificationStatus::Failed => {
            if contract.class() == VerificationClass::Required {
                EquivalenceState::NotRestored
            } else if contract.equivalence() == Some(EquivalenceDomain::RuntimeState) {
                EquivalenceState::DifferentAsExpected
            } else {
                EquivalenceState::NotRestored
            }
        }
    }
}

/// Which equivalence domain an unrecoverable effect belongs to (§25.1).
///
/// An effect whose domain the provider could not classify is recorded against persistent state.
/// That is the direction §56.3 asks for: it stops §25.2's "PERSISTENT STATE VERIFIED" from being
/// claimed, and it asserts nothing about a domain nobody established.
fn domain_of(effect: &UnrecoverableEffect) -> EquivalenceDomain {
    match effect.domain() {
        EffectDomain::ExternalSideEffect | EffectDomain::RemoteSystem => {
            EquivalenceDomain::ExternalSideEffect
        }
        EffectDomain::ProcessRuntime
        | EffectDomain::KernelRuntime
        | EffectDomain::NetworkRuntime
        | EffectDomain::ProviderTransactionState => EquivalenceDomain::RuntimeState,
        EffectDomain::FilesystemPersistent
        | EffectDomain::BlockStoragePersistent
        | EffectDomain::ApplicationPersistent
        | EffectDomain::IdentitySecurityState
        | EffectDomain::Unknown => EquivalenceDomain::PersistentState,
    }
}
