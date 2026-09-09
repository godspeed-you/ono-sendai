//! §24.5's explicit acceptance, and the refusals that stand in for it.
//!
//! §24.5 is one sentence: *"No recovery execution occurs without the explicit gate."*
//! `RecoveryPlan::needs_destructive_acceptance` is that predicate and it already answers `true`
//! for an unanalysed recovery. What this module owns is what happens when it answers `true` — the
//! refusal, and everything the refusal has to name before an operator can reasonably accept it.
//!
//! [`check`] refuses in a fixed order, and the order is the decision worth recording.
//!
//! 1. **An analysis that did not run blocks, acceptance or no.** §62.8 names recovering without
//!    drift analysis as a failure mode, and §56.3 blocks a destructive recovery whose critical
//!    facts could not be established. Acceptance under §24.5 is the operator accepting a *known*
//!    loss; there is nothing to accept about a loss nobody measured, so an incomplete analysis is
//!    checked before the acceptance flag and cannot be waved through by it.
//! 2. **Then provider-native history**, because §13.6 forbids Ono ever adding a destructive flag
//!    silently and the objects that would go are the ones an operator cannot get back.
//! 3. **Then newer state**, naming every conflicting object (Appendix C.4).
//!
//! §40.1 applies to recovery the same way it applies to any other plan: a recovery whose analysis
//! is complete and whose losses are empty passes the gate with nothing to type. The ordinary
//! selective restore of §24.4 must stay a one-step operation, or the gate teaches operators to
//! reach for the flag without reading it (§40.2).

use ono_change_core::{NewerStateClass, RecoveryPlan, error};
use ono_value::ErrorValue;

/// Every provider-native object this recovery would destroy (§13.6, §24.5).
#[must_use]
pub fn destroyed_objects(plan: &RecoveryPlan) -> Vec<String> {
    plan.newer_state()
        .destroyed_assets()
        .iter()
        .map(|asset| asset.to_string())
        .collect()
}

/// Every object recovery would take newer state away from (Appendix C.4, §24.3).
#[must_use]
pub fn conflicting_objects(plan: &RecoveryPlan) -> Vec<String> {
    plan.newer_state()
        .losses()
        .iter()
        .map(|item| format!("{} — {}", item.object(), item.detail()))
        .collect()
}

/// Every object whose fate under this recovery could not be established (§56.3).
#[must_use]
pub fn unestablished_objects(plan: &RecoveryPlan) -> Vec<String> {
    plan.newer_state()
        .items()
        .iter()
        .filter(|item| item.class() == NewerStateClass::Unknown)
        .map(|item| format!("{} — {}", item.object(), item.detail()))
        .collect()
}

/// Whether §24.5's gate lets this recovery run (§24.5, §56.3, §40.1).
///
/// # Errors
///
/// - `recovery.plan_incomplete` when the newer-state analysis did not run to completion, or when
///   an object in the restore scope could not be observed. §56.3 blocks rather than guesses, and
///   an acceptance already recorded does not answer a question nobody asked.
/// - `recovery.destructive_history_not_accepted` naming every snapshot, bookmark or clone the
///   recovery would destroy (§13.6).
/// - `recovery.newer_state_conflict` naming every object whose newer state would go (Appendix
///   C.4).
pub fn check(plan: &RecoveryPlan) -> Result<(), ErrorValue> {
    if !plan.newer_state().is_complete() {
        return Err(error::recovery_plan_incomplete(
            "what this recovery would do to state written since the recovery point",
            "v0.6 §62.8: recovering hours later without considering newer state is unacceptable. \
             `inspect recovery` shows what each available method would preserve",
        ));
    }
    let unestablished = unestablished_objects(plan);
    if !unestablished.is_empty() {
        return Err(error::recovery_plan_incomplete(
            "the current state of every object in the restore scope",
            &format!(
                "{} object(s) could not be established: {}",
                unestablished.len(),
                unestablished.join("; ")
            ),
        ));
    }
    if !plan.needs_destructive_acceptance() {
        return Ok(());
    }
    let destroyed = destroyed_objects(plan);
    if !destroyed.is_empty() {
        return Err(error::destructive_history_not_accepted(&destroyed));
    }
    let conflicts = conflicting_objects(plan);
    if !conflicts.is_empty() {
        return Err(error::newer_state_conflict(&conflicts));
    }
    Ok(())
}
