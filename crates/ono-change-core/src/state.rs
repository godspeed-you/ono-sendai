//! The plan lifecycle state machine (spec v0.6 §4.1) and the recovery branch beside it.
//!
//! The state graph is written here as a total function on `(state, event)` rather than as a set
//! of assignments spread through the executor, because §4 makes several of the transitions load
//! bearing for safety: `PREPARE_FAILED` must be reachable without passing through `APPLYING`
//! (§4.5, Appendix F), an unknown apply outcome must not resolve to failure (Appendix F.2), and
//! `PROTECTED` is entered only when protection actually completed (§4.6).
//!
//! Rejecting an impossible transition is the point. [`PlanState::after`] returns `None` for one,
//! and the executor turns that into a structured refusal rather than a silent state change, so
//! "the plan was applying and is now sealed again" cannot happen by accident.

use crate::vocab::vocabulary;

vocabulary! {
    /// The canonical plan states of §4.1, including the recovery branch.
    PlanState {
        Draft => "draft", "§4.2: editable, and not executable.";
        Resolved => "resolved", "§4.3: selectors have become concrete object identities and provider bindings.";
        Sealed => "sealed", "§4.4: immutable, digest-bearing, and ready to apply.";
        Expired => "expired", "§4.1: the sealed plan outlived its validity window.";
        Preparing => "preparing", "§4.5: revalidation, capability checks and recovery asset creation, immediately before the first mutation.";
        PrepareFailed => "prepare-failed", "§4.5 and Appendix F: preparation failed, and no mutating action ran.";
        Protected => "protected", "§4.6: every protection action the policy required completed and validated.";
        Applying => "applying", "§4.7: mutating actions are executing.";
        ApplyFailed => "apply-failed", "§4.7 and Appendix F: a mutating action failed. What ran, ran.";
        Verifying => "verifying", "§4.8: postconditions are being observed.";
        Verified => "verified", "§4.8: every required postcondition held.";
        Degraded => "degraded", "§4.8: the intended primary state exists and some expectations are violated or unknown.";
        Failed => "failed", "§4.8: a required postcondition failed. §14 keeps this independent of the exit status of anything.";
        Closed => "closed", "§4.9: the plan is finished. Recovery assets are retained or not by policy, independently.";
        RecoveryPlanned => "recovery-planned", "§24.1: a RecoveryPlan exists for this plan and has not been applied.";
        Recovering => "recovering", "§4.1: the recovery plan is executing.";
        Recovered => "recovered", "§4.1: recovery actions completed.";
        RecoveryFailed => "recovery-failed", "§4.1 and Appendix F: recovery did not complete. The assets and the exact partial state are preserved.";
        RecoveryVerified => "recovery-verified", "§4.1 and §25: recovery verification passed for the domains it covers, and for no others.";
    }
}

impl PlanState {
    /// Whether the plan may be mutated by an editor (§4.2).
    #[must_use]
    pub const fn is_editable(self) -> bool {
        matches!(self, PlanState::Draft)
    }

    /// Whether the plan is sealed and therefore immutable (§4.4).
    #[must_use]
    pub const fn is_sealed(self) -> bool {
        !matches!(self, PlanState::Draft | PlanState::Resolved)
    }

    /// Whether `apply` may be invoked on a plan in this state (§5.6).
    #[must_use]
    pub const fn is_appliable(self) -> bool {
        matches!(self, PlanState::Sealed | PlanState::Protected)
    }

    /// Whether the plan has begun changing the system (§4.7).
    ///
    /// Appendix F turns on this predicate: everything at or after `APPLYING` must be told to the
    /// operator as "something may have happened", and everything before it as "nothing did".
    #[must_use]
    pub const fn has_mutated(self) -> bool {
        matches!(
            self,
            PlanState::Applying
                | PlanState::ApplyFailed
                | PlanState::Verifying
                | PlanState::Verified
                | PlanState::Degraded
                | PlanState::Failed
                | PlanState::Closed
                | PlanState::RecoveryPlanned
                | PlanState::Recovering
                | PlanState::Recovered
                | PlanState::RecoveryFailed
                | PlanState::RecoveryVerified
        )
    }

    /// Whether the plan has reached an outcome and nothing further will happen on its own.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            PlanState::Expired
                | PlanState::Closed
                | PlanState::PrepareFailed
                | PlanState::RecoveryVerified
        )
    }

    /// Whether §37.2 forbids ordinary success retention from removing this plan's assets.
    ///
    /// A plan that failed is a plan somebody may still need to recover, so its recovery assets
    /// outlive the 24-hour rule until an operator decides otherwise.
    #[must_use]
    pub const fn retains_assets_indefinitely(self) -> bool {
        matches!(
            self,
            PlanState::Failed
                | PlanState::Degraded
                | PlanState::ApplyFailed
                | PlanState::PrepareFailed
                | PlanState::RecoveryFailed
        )
    }

    /// Whether a recovery plan may be built for a plan in this state (§24.1).
    #[must_use]
    pub const fn is_recoverable(self) -> bool {
        matches!(
            self,
            PlanState::Verified
                | PlanState::Degraded
                | PlanState::Failed
                | PlanState::ApplyFailed
                | PlanState::Closed
                | PlanState::RecoveryPlanned
                | PlanState::RecoveryFailed
        )
    }

    /// The state `event` moves this one to, or `None` when §4.1 has no such edge.
    #[must_use]
    pub const fn after(self, event: LifecycleEvent) -> Option<Self> {
        use LifecycleEvent as E;
        use PlanState as S;
        let next = match (self, event) {
            (S::Draft, E::Resolve) => S::Resolved,
            (S::Draft | S::Resolved, E::Seal) => S::Sealed,
            (S::Sealed, E::Expire) => S::Expired,
            (S::Sealed | S::Protected, E::BeginPrepare) => S::Preparing,
            (S::Preparing, E::PrepareFailed) => S::PrepareFailed,
            (S::Preparing, E::Protected) => S::Protected,
            (S::Preparing | S::Protected, E::BeginApply) => S::Applying,
            (S::Applying, E::ApplyFailed) => S::ApplyFailed,
            (S::Applying, E::BeginVerify) => S::Verifying,
            (S::Verifying, E::Verified) => S::Verified,
            (S::Verifying, E::Degraded) => S::Degraded,
            (S::Verifying, E::VerificationFailed) => S::Failed,
            (S::Verified | S::Degraded | S::Failed, E::Close) => S::Closed,
            (
                S::Verified
                | S::Degraded
                | S::Failed
                | S::ApplyFailed
                | S::Closed
                | S::RecoveryFailed,
                E::PlanRecovery,
            ) => S::RecoveryPlanned,
            (S::RecoveryPlanned, E::BeginRecovery) => S::Recovering,
            (S::Recovering, E::Recovered) => S::Recovered,
            (S::Recovering, E::RecoveryFailed) => S::RecoveryFailed,
            (S::Recovered, E::RecoveryVerified) => S::RecoveryVerified,
            (S::Recovered, E::RecoveryVerificationFailed) => S::RecoveryFailed,
            _ => return None,
        };
        Some(next)
    }
}

vocabulary! {
    /// The events that move a plan through §4.1.
    LifecycleEvent {
        Resolve => "resolve", "§4.3: selectors became concrete identities.";
        Seal => "seal", "§4.4: the plan became immutable and gained a digest.";
        Expire => "expire", "§4.1: the sealed plan's validity window closed.";
        BeginPrepare => "begin-prepare", "§4.5: preparation started, immediately before the first mutation.";
        PrepareFailed => "prepare-failed", "§4.5: a required prepare action failed, and §2.3 forbids mutating anyway.";
        Protected => "protected", "§4.6: every required protection action completed and validated.";
        BeginApply => "begin-apply", "§4.7: mutating actions started.";
        ApplyFailed => "apply-failed", "§4.7: a mutating action failed.";
        BeginVerify => "begin-verify", "§4.8: mutations finished and verification started.";
        Verified => "verified", "§4.8: every required postcondition held.";
        Degraded => "degraded", "§4.8: an advisory expectation was violated or is unknown.";
        VerificationFailed => "verification-failed", "§4.8: a required postcondition failed.";
        Close => "close", "§4.9: the plan was closed. Asset retention is independent.";
        PlanRecovery => "plan-recovery", "§24.1: a RecoveryPlan was constructed. Nothing has been changed by it.";
        BeginRecovery => "begin-recovery", "§4.1: the recovery plan started executing.";
        Recovered => "recovered", "§4.1: the recovery actions completed.";
        RecoveryFailed => "recovery-failed", "§4.1: a recovery action failed. Appendix F preserves the assets and the partial state.";
        RecoveryVerified => "recovery-verified", "§25: recovery verification passed for the domains it covers.";
        RecoveryVerificationFailed => "recovery-verification-failed", "§25.3 and Appendix F: recovery verification failed, so nothing may claim the state was recovered.";
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_walk_the_happy_path_of_section_four() {
        let mut state = PlanState::Draft;
        for event in [
            LifecycleEvent::Resolve,
            LifecycleEvent::Seal,
            LifecycleEvent::BeginPrepare,
            LifecycleEvent::Protected,
            LifecycleEvent::BeginApply,
            LifecycleEvent::BeginVerify,
            LifecycleEvent::Verified,
            LifecycleEvent::Close,
        ] {
            state = state
                .after(event)
                .unwrap_or_else(|| panic!("§4.1 has an edge for {event} from {state}"));
        }
        assert_eq!(state, PlanState::Closed);
    }

    #[test]
    fn should_reach_prepare_failed_without_ever_applying() {
        let state = PlanState::Sealed
            .after(LifecycleEvent::BeginPrepare)
            .and_then(|state| state.after(LifecycleEvent::PrepareFailed))
            .expect("§4.5 reaches PREPARE_FAILED from PREPARING");
        assert_eq!(state, PlanState::PrepareFailed);
        assert!(
            !state.has_mutated(),
            "§2.3 and §55.7 case 31: a failed prepare means zero mutate actions executed"
        );
    }

    #[test]
    fn should_refuse_to_apply_a_draft() {
        assert!(
            !PlanState::Draft.is_appliable(),
            "§4.2: a draft MUST NOT be executable"
        );
        assert!(
            PlanState::Draft.after(LifecycleEvent::BeginApply).is_none(),
            "there is no edge from DRAFT to APPLYING (§4.1)"
        );
    }

    #[test]
    fn should_refuse_to_apply_an_expired_plan() {
        assert!(
            !PlanState::Expired.is_appliable(),
            "§5.6: apply MUST refuse expired plans"
        );
    }

    #[test]
    fn should_refuse_a_transition_section_four_does_not_draw() {
        assert!(
            PlanState::Applying.after(LifecycleEvent::Seal).is_none(),
            "a plan that is changing the system cannot become a fresh seal"
        );
        assert!(
            PlanState::Verified
                .after(LifecycleEvent::BeginApply)
                .is_none(),
            "a verified plan is not re-applied by moving a state variable"
        );
        assert!(
            PlanState::PrepareFailed
                .after(LifecycleEvent::BeginApply)
                .is_none(),
            "§2.3: mutation MUST NOT begin after a required recovery asset could not be created"
        );
    }

    #[test]
    fn should_treat_every_state_from_applying_onwards_as_having_touched_the_system() {
        for state in [
            PlanState::Draft,
            PlanState::Resolved,
            PlanState::Sealed,
            PlanState::Expired,
            PlanState::Preparing,
            PlanState::PrepareFailed,
            PlanState::Protected,
        ] {
            assert!(
                !state.has_mutated(),
                "{state} is before the first mutation (Appendix F)"
            );
        }
        for state in [
            PlanState::Applying,
            PlanState::ApplyFailed,
            PlanState::Failed,
            PlanState::Recovered,
        ] {
            assert!(
                state.has_mutated(),
                "{state} is at or after the first mutation (Appendix F)"
            );
        }
    }

    #[test]
    fn should_keep_recovery_assets_for_a_plan_that_did_not_succeed() {
        for state in [
            PlanState::Failed,
            PlanState::Degraded,
            PlanState::RecoveryFailed,
            PlanState::PrepareFailed,
            PlanState::ApplyFailed,
        ] {
            assert!(
                state.retains_assets_indefinitely(),
                "§37.2: {state} assets MUST NOT be removed by ordinary success retention"
            );
        }
        assert!(
            !PlanState::Verified.retains_assets_indefinitely(),
            "§37.1: a verified plan's assets expire on the ordinary retention rule"
        );
    }

    #[test]
    fn should_fail_recovery_when_its_verification_does_not_hold() {
        let state = PlanState::Recovering
            .after(LifecycleEvent::Recovered)
            .and_then(|state| state.after(LifecycleEvent::RecoveryVerificationFailed))
            .expect("§25.3 has this edge");
        assert_eq!(
            state,
            PlanState::RecoveryFailed,
            "Appendix F: a failed recovery verification MUST NOT claim the state was recovered"
        );
    }

    #[test]
    fn should_let_a_failed_recovery_be_planned_again() {
        assert_eq!(
            PlanState::RecoveryFailed.after(LifecycleEvent::PlanRecovery),
            Some(PlanState::RecoveryPlanned),
            "§41.3: after a failed recovery a new recovery or rebase decision is possible"
        );
    }

    #[test]
    fn should_offer_recovery_only_where_the_plan_actually_changed_something() {
        assert!(
            !PlanState::Sealed.is_recoverable(),
            "there is nothing to recover from a plan that never ran"
        );
        assert!(
            !PlanState::PrepareFailed.is_recoverable(),
            "§55.7 case 31: a failed prepare left the system untouched"
        );
        assert!(
            PlanState::Failed.is_recoverable(),
            "Appendix F: a failed plan is exactly where a RecoveryPlan is offered"
        );
    }
}
