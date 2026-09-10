//! Rebasing a sealed plan onto current state (spec v0.6 §7.5, §4.4).
//!
//! §7.5 is two sentences: *"`rebase plan @plan` MAY create a new plan revision against current
//! state. Rebase MUST NOT mutate the sealed original."* The second is a property of the signature
//! here — [`rebase`] borrows the original and returns a new plan — rather than a rule a reviewer
//! has to check, and §55.2 case 9 is the test that says so.
//!
//! What a rebase is *not* is a re-plan. The intent, the actions, the strategy, the protection
//! policy and the verification contracts are the ones the operator already approved; what moves is
//! the frozen target set, because that is the thing §7.3 found stale. A plan whose actions should
//! change is a new plan, and the revision counter would be lying about it.

use std::collections::BTreeMap;

use jiff::Timestamp;
use ono_change_core::{ChangePlan, FrozenTarget, PlanAction, PlanKind, Precondition, error};
use ono_core::ErrorCode;
use ono_value::ErrorValue;

/// Creates the next revision of `plan` against `targets`, resolved now (§7.5).
///
/// The returned plan carries the same [`ono_change_core::PlanId`] at revision + 1, sealed at
/// `now`, with a digest of its own. `plan` is untouched: §4.4 makes a sealed plan immutable, and
/// §7.5 makes that explicit for rebase in particular.
///
/// # Errors
///
/// - `change.plan_not_sealed` when `plan` is still a draft. §7.5 rebases a sealed plan; a draft is
///   edited instead, and calling this on one would silently skip a revision.
/// - `change.target_unresolved` when `targets` is empty — a plan with nothing left to act on is
///   not a new revision of anything (§4.3).
/// - whatever [`ChangePlan::seal`] refuses: a cyclic action graph (§3.2) or a mutating plan with
///   no verification contract (§23.1).
pub fn rebase(
    plan: &ChangePlan,
    targets: Vec<FrozenTarget>,
    now: Timestamp,
) -> Result<ChangePlan, ErrorValue> {
    if !plan.state().is_sealed() {
        return Err(error::plan_not_sealed(plan.id(), plan.state()));
    }
    if targets.is_empty() {
        return Err(error::target_unresolved(
            plan.intent().source(),
            "§7.5 creates a new revision against current state, and current state holds none of \
             the objects this plan was about.",
        ));
    }
    plan.revise().resolve(targets)?.seal(now)
}

/// What one action of the next revision is resolved against (§7.5, §7.2).
///
/// The object the action now names and the §7.2 facts frozen about it as it is now: a file's
/// bytes, a unit's generation, the persistence domain that holds the state.
#[derive(Debug, Clone, PartialEq)]
pub struct Refrozen {
    target: String,
    preconditions: Vec<Precondition>,
}

impl Refrozen {
    /// `target`'s identity, with the preconditions frozen against it now.
    #[must_use]
    pub fn new(target: impl Into<String>, preconditions: Vec<Precondition>) -> Self {
        Self {
            target: target.into(),
            preconditions,
        }
    }
}

/// Creates the next revision of `plan` with every action's preconditions frozen again (§7.5).
///
/// [`rebase`] moves the target set and keeps each action as it was sealed, preconditions
/// included — so a plan refused because a file's bytes moved would carry the old digest into its
/// next revision and be refused again for the same reason. §7.5's revision is created "against
/// current state", and §7.2's preconditions are the part of the plan that *is* state. `refreeze`
/// answers for each action with what it is resolved against now; an action it answers `None` for
/// — a PREPARE action, which names a recovery provider rather than an object — keeps what it had.
/// The intent, the actions, the strategy, the protection and the contracts are the ones the
/// operator approved, exactly as in [`rebase`].
///
/// # Errors
///
/// Everything [`rebase`] refuses with, and `change.plan_store_corrupt` for a plan whose identity
/// is not the one §3.2 derives from its intent, session and creation instant: its next revision
/// could not carry the same identity.
pub fn rebase_with(
    plan: &ChangePlan,
    targets: Vec<FrozenTarget>,
    refreeze: &dyn Fn(&PlanAction) -> Option<Refrozen>,
    now: Timestamp,
) -> Result<ChangePlan, ErrorValue> {
    if !plan.state().is_sealed() {
        return Err(error::plan_not_sealed(plan.id(), plan.state()));
    }
    if targets.is_empty() {
        return rebase(plan, targets, now);
    }
    let mut draft = ChangePlan::draft(plan.intent().clone(), plan.session(), plan.created_at());
    if draft.id() != plan.id() {
        return Err(ErrorValue::new(
            ErrorCode::ChangePlanStoreCorrupt,
            format!(
                "plan {} does not carry the identity its intent, session and creation instant \
                 derive",
                plan.id().short()
            ),
        )
        .with_help("§3.2 derives a plan's identity, and a revision keeps it; this one cannot"));
    }
    if plan.kind() == PlanKind::Recovery {
        draft = draft.as_recovery();
    }
    let remap = BTreeMap::new();
    for action in plan.actions() {
        let next = match refreeze(action) {
            Some(refrozen) => crate::builder::rebuilt(
                plan.id(),
                action.ordinal(),
                action,
                &remap,
                &refrozen.preconditions,
                Some(&refrozen.target),
                None,
            ),
            None => crate::builder::rebuilt(
                plan.id(),
                action.ordinal(),
                action,
                &remap,
                action.preconditions(),
                action.target(),
                None,
            ),
        };
        draft = draft.with_action(next)?;
    }
    draft = draft
        .with_impact(plan.impact().clone())
        .with_protection(plan.protection().clone())
        .with_protection_mode(plan.protection_mode())
        .with_risk(plan.risk().clone())
        .with_strategy(plan.strategy())
        .with_verification(plan.verification().clone());
    for binding in plan.providers() {
        draft = draft.binding(binding.clone());
    }
    if let Some(at) = plan.expires_at() {
        draft = draft.expiring_at(at);
    }
    // `revise` is what numbers a revision and records the one it supersedes, so the rebuilt draft
    // is revised up to the plan's own revision and one past it.
    for _ in 0..plan.revision() {
        draft = draft.revise();
    }
    draft.resolve(targets)?.seal(now)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use std::sync::Arc;

    use ono_change_core::{
        ActionRole, ActionStatus, ChangePlan, Execution, Idempotency, Intent, PlanAction,
        PlanFragment, PlanState, VerificationClass, VerificationContract,
    };
    use ono_core::ErrorCode;
    use ono_value::Value;

    use super::*;
    use crate::builder::PlanBuilder;
    use crate::freeze::ServiceTarget;

    fn instant() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).expect("a valid instant")
    }

    fn service(unit: &str) -> FrozenTarget {
        ServiceTarget::new("systemd", unit)
            .resolved_from("get service | where state == failed")
            .freeze()
            .expect("a namespaced unit freezes")
    }

    fn builder() -> PlanBuilder {
        PlanBuilder::for_intent(
            Intent::new(
                "restart the failed services",
                "get service | where state == failed | plan restart service",
            ),
            "session-1",
            instant(),
        )
    }

    fn sealed(units: &[&str]) -> ChangePlan {
        let builder = builder();
        let id = builder.plan_id().clone();
        let action = PlanAction::new(
            &id,
            1,
            ActionRole::Mutate,
            "restart the failed services",
            Execution::ProviderAction {
                provider: Arc::from("ono.service.systemd"),
                operation: Arc::from("ono.service.restart"),
                arguments: Vec::new(),
            },
        )
        .with_idempotency(Idempotency::Idempotent);
        let fragment = PlanFragment::empty().acting(action).verifying(
            VerificationContract::new(
                &id,
                VerificationClass::Required,
                "systemd:nginx.service",
                "state == running",
            )
            .expecting(Value::string("running")),
        );
        builder
            .contributing(&fragment)
            .expect("a fragment is accepted")
            .resolve(units.iter().map(|unit| service(unit)).collect())
            .expect("the targets resolve")
            .seal(at(60))
            .expect("a plan seals")
    }

    #[test]
    fn should_be_exactly_a_rebase_when_nothing_is_frozen_again() {
        let original = sealed(&["a.service", "b.service"]);
        let copied = rebase(&original, vec![service("a.service")], at(120))
            .expect("§7.5 creates a new revision");
        let rebuilt = rebase_with(&original, vec![service("a.service")], &|_| None, at(120))
            .expect("§7.5 creates a new revision");
        assert_eq!(
            rebuilt, copied,
            "§7.5: freezing no fact again changes nothing about the revision a rebase creates — \
             the intent, the actions, the protection and the contracts are the approved ones"
        );
    }

    #[test]
    fn should_freeze_each_actions_preconditions_again_against_current_state() {
        let original = sealed(&["a.service"]);
        let now = ono_change_core::Precondition::new(
            ono_change_core::PreconditionKind::ContentDigest,
            "/etc/app.conf",
            "sha256",
            Value::string("current"),
        );
        let refreeze = |_: &PlanAction| {
            Some(Refrozen::new(
                "systemd:a.service#generation=2",
                vec![now.clone()],
            ))
        };
        let next = rebase_with(&original, vec![service("a.service")], &refreeze, at(120))
            .expect("§7.5 creates a new revision");
        let action = next.actions().first().expect("the action is kept");
        assert_eq!(
            action.preconditions(),
            std::slice::from_ref(&now),
            "§7.5 and §7.2: the next revision carries the facts as they are now, not the ones the \
             refused revision froze"
        );
        assert_eq!(
            action.target(),
            Some("systemd:a.service#generation=2"),
            "and the action names the object as it was resolved now"
        );
        assert_eq!(next.revision(), 2);
        assert_eq!(next.id(), original.id());
        assert!(next.digest_holds(), "§4.4: the new revision is sealed");
    }

    #[test]
    fn should_create_a_new_revision_and_leave_the_original_unchanged() {
        let original = sealed(&["a.service", "b.service"]);
        let digest_before = original.digest().map(str::to_owned);
        let state_before = original.state();
        let next = rebase(&original, vec![service("a.service")], at(120))
            .expect("§7.5 creates a new revision");
        assert_eq!(
            next.revision(),
            2,
            "§55.2 case 9: rebase creates a new revision"
        );
        assert_eq!(
            original.digest().map(str::to_owned),
            digest_before,
            "§7.5: rebase MUST NOT mutate the sealed original"
        );
        assert_eq!(
            original.state(),
            state_before,
            "§4.4: a sealed plan is immutable, so its state does not move either"
        );
        assert_eq!(original.revision(), 1);
    }

    #[test]
    fn should_give_the_new_revision_a_digest_of_its_own() {
        let original = sealed(&["a.service", "b.service"]);
        let next = rebase(&original, vec![service("a.service")], at(120))
            .expect("§7.5 creates a new revision");
        assert!(next.digest_holds(), "§4.4: the new revision is sealed too");
        assert_ne!(
            next.digest(),
            original.digest(),
            "§4.4: the seal covers the revision and the target identities, and both moved"
        );
    }

    #[test]
    fn should_keep_the_identity_of_the_plan_it_rebased() {
        let original = sealed(&["a.service"]);
        let next = rebase(&original, vec![service("a.service")], at(120))
            .expect("§7.5 creates a new revision");
        assert_eq!(
            next.id(),
            original.id(),
            "§3.2: a plan has a stable PlanId and a monotonically increasing revision"
        );
        assert_eq!(
            next.supersedes(),
            Some(1),
            "§7.5: the new revision records the one it was derived from"
        );
    }

    #[test]
    fn should_re_seal_against_the_targets_current_state_offers() {
        let original = sealed(&["a.service", "b.service", "c.service"]);
        let next = rebase(
            &original,
            vec![service("a.service"), service("c.service")],
            at(120),
        )
        .expect("§7.5 creates a new revision");
        assert_eq!(
            next.targets().len(),
            2,
            "§7.5: the new revision is resolved against current state"
        );
        assert_eq!(
            original.targets().len(),
            3,
            "§2.6: the original's frozen membership did not move"
        );
    }

    #[test]
    fn should_change_the_digest_even_when_the_targets_are_the_same() {
        let original = sealed(&["a.service"]);
        let next = rebase(&original, vec![service("a.service")], at(120))
            .expect("§7.5 creates a new revision");
        assert_ne!(
            next.digest(),
            original.digest(),
            "§4.4: the plan revision is part of the seal, so a revision is a different plan"
        );
    }

    #[test]
    fn should_return_the_new_revision_to_draft_before_sealing_it_again() {
        let original = sealed(&["a.service"]);
        let next = rebase(&original, vec![service("a.service")], at(120))
            .expect("§7.5 creates a new revision");
        assert_eq!(
            next.state(),
            PlanState::Sealed,
            "§7.5's revision is a plan an operator can apply"
        );
        assert_eq!(
            next.sealed_at(),
            Some(at(120)),
            "the new revision was sealed when the rebase happened"
        );
    }

    #[test]
    fn should_reset_action_status_on_the_new_revision() {
        let original = sealed(&["a.service"]);
        let next = rebase(&original, vec![service("a.service")], at(120))
            .expect("§7.5 creates a new revision");
        assert!(
            next.actions()
                .iter()
                .all(|action| action.status() == ActionStatus::Pending),
            "§7.5: a new revision has not run, whatever the one before it did"
        );
    }

    #[test]
    fn should_refuse_to_rebase_a_draft() {
        let draft = ChangePlan::draft(
            Intent::new("restart nginx", "plan restart service nginx"),
            "session-1",
            instant(),
        );
        let refusal = rebase(&draft, vec![service("a.service")], at(120))
            .expect_err("§7.5 rebases a sealed plan");
        assert_eq!(
            refusal.code(),
            ErrorCode::ChangePlanNotSealed,
            "§7.5: a draft is edited, not rebased"
        );
    }

    #[test]
    fn should_refuse_to_rebase_onto_an_empty_target_set() {
        let original = sealed(&["a.service"]);
        let refusal =
            rebase(&original, Vec::new(), at(120)).expect_err("§4.3 has nothing to freeze");
        assert_eq!(refusal.code(), ErrorCode::ChangeTargetUnresolved);
    }

    #[test]
    fn should_rebase_a_revision_again_and_keep_counting() {
        let original = sealed(&["a.service"]);
        let second = rebase(&original, vec![service("a.service")], at(120))
            .expect("§7.5 creates a new revision");
        let third = rebase(&second, vec![service("a.service")], at(180))
            .expect("§7.5 creates a new revision");
        assert_eq!(
            third.revision(),
            3,
            "§3.2: the revision increases monotonically and never repeats"
        );
        assert_eq!(
            second.revision(),
            2,
            "§7.5: the one it came from is untouched"
        );
    }
}
