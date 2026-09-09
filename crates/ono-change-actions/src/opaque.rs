//! Arbitrary external commands, and the one escape §6.3 allows (spec §6.2, §6.3).
//!
//! §6.2 is the default and it is a refusal: `plan sh -c 'rm -rf /somewhere'` MUST fail, because
//! Ono cannot reason about target scope or side effects. There is no fallback that turns an
//! unrecognised command into a planned one — §6.3's escape is an explicit request from an
//! operator who has accepted what it costs, not a route the resolver takes when it runs out of
//! ideas.
//!
//! §6.2's second sentence is the way out that keeps the model honest: *"An external-command
//! adapter from v0.3 MAY expose a plannable action if its contract is explicit."* [`AdapterAction`]
//! is what "explicit" means here — §6.1's list, declared by the adapter rather than inferred from
//! the program's name. An adapter that declares one gets a plannable action; an adapter that does
//! not leaves the refusal exactly where §6.2 put it. No first-party adapter pack in
//! `docs/contracts/adapters/` declares one today: every one of them states that mutations stay
//! ordinary external commands (v0.3 §1.36).
//!
//! §6.3's escape produces an action whose single effect is [`EffectDomain::Unknown`] at
//! [`EffectConfidence::Unknown`]. That pair is what makes Appendix A.7 cap the plan it appears in
//! at partially protected, which is §6.3's own requirement: an opaque action MUST NOT receive a
//! `PROTECTED` status merely because a filesystem snapshot exists somewhere on the host.

use std::sync::Arc;

use ono_change_core::{
    ActionRole, EffectConfidence, EffectDomain, EffectKind, Execution, Idempotency, PlanAction,
    PlanId, ProposedEffect, error,
};
use ono_value::ErrorValue;

use crate::registry::{EffectSpec, VerificationSpec};

/// What §6.3's escape says about an action nobody can reason about.
const OPAQUE_EXPLANATION: &str = "§6.3: the operator declared this action opaque. Ono has no \
                                  model of what it touches, so its impact and its reversibility \
                                  are unknown, and Appendix A.7 caps the plan it appears in at \
                                  partially protected — a snapshot taken for something else is \
                                  not protection for this";

/// A plannable action an external-command adapter declares (§6.2).
///
/// The fields are §6.1's list for a program: what it runs, what it does, whether running it twice
/// is running it once, what comes back afterwards and how anyone would know it worked. An adapter
/// that cannot fill all of them has not made its contract explicit, and [`AdapterAction::declare`]
/// refuses rather than filling a gap with a guess.
#[derive(Debug, Clone, PartialEq)]
pub struct AdapterAction {
    adapter: Arc<str>,
    program: Arc<str>,
    argv: Vec<Arc<str>>,
    summary: Arc<str>,
    idempotency: Idempotency,
    recovery_semantics: Arc<str>,
    effects: Vec<EffectSpec>,
    verification: Vec<VerificationSpec>,
}

impl AdapterAction {
    /// Declares that `adapter` exposes `program` as a plannable action (§6.2).
    ///
    /// # Errors
    ///
    /// `change.action_not_plannable` when the declaration is missing an effect or a verification
    /// contract. §6.1 requires expected direct effects and verification options, and §23.1
    /// requires at least one contract on any plan that mutates; a declaration without them is
    /// exactly the opaque command §6.2 refuses, wearing an adapter's name.
    pub fn declare(
        adapter: impl Into<Arc<str>>,
        program: impl Into<Arc<str>>,
        summary: impl Into<Arc<str>>,
        idempotency: Idempotency,
        recovery_semantics: impl Into<Arc<str>>,
        effects: Vec<EffectSpec>,
        verification: Vec<VerificationSpec>,
    ) -> Result<Self, ErrorValue> {
        let program = program.into();
        if effects.is_empty() {
            return Err(error::action_not_plannable(
                &program,
                "the adapter declares no expected direct effects, and §6.1 makes them part of \
                 what a plannable operation states.",
            ));
        }
        if verification.is_empty() {
            return Err(error::action_not_plannable(
                &program,
                "the adapter declares no verification options, and §23.1 requires every plan that \
                 mutates to carry at least one contract.",
            ));
        }
        Ok(Self {
            adapter: adapter.into(),
            program,
            argv: Vec::new(),
            summary: summary.into(),
            idempotency,
            recovery_semantics: recovery_semantics.into(),
            effects,
            verification,
        })
    }

    /// Fixes the argument vector the action runs with (§12.3, §43.6).
    ///
    /// One element per argument, unquoted and unexpanded. There is no shell here and nowhere for
    /// a command line to live.
    #[must_use]
    pub fn with_argv<I, S>(mut self, argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Arc<str>>,
    {
        self.argv = argv.into_iter().map(Into::into).collect();
        self
    }

    /// The adapter that declared it.
    #[must_use]
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// The resolved program.
    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The argument vector.
    #[must_use]
    pub fn argv(&self) -> &[Arc<str>] {
        &self.argv
    }

    /// The verification options the adapter declared (§6.1, §23.1).
    #[must_use]
    pub fn verification(&self) -> &[VerificationSpec] {
        &self.verification
    }

    /// The action this declaration contributes to a plan.
    #[must_use]
    pub fn action(&self, plan: &PlanId, ordinal: usize) -> PlanAction {
        let execution = Execution::Program {
            program: Arc::clone(&self.program),
            argv: self.argv.clone(),
        };
        let mut action = PlanAction::new(
            plan,
            ordinal,
            ActionRole::Mutate,
            Arc::clone(&self.summary),
            execution,
        )
        .with_idempotency(self.idempotency)
        .recovery_semantics(Arc::clone(&self.recovery_semantics));
        for effect in &self.effects {
            let mut proposed = ProposedEffect::new(
                action.id().clone(),
                effect.domain(),
                effect.kind(),
                effect.confidence(),
                effect.explanation(),
            )
            .on(Arc::clone(&self.program))
            .citing(format!("adapter:{}", self.adapter));
            if effect.is_irreversible() {
                proposed = proposed.irreversible();
            }
            if let Some(compensation) = effect.compensation() {
                proposed = proposed.compensated_by(compensation);
            }
            action = action.effecting(proposed);
        }
        action
    }
}

/// The refusal §6.2 requires for an arbitrary external command.
///
/// `plan sh -c 'rm -rf /somewhere'` lands here: nothing declares what it touches, so nothing can
/// say what it would cost. The error names §6.3's escape, because an operator who genuinely wants
/// this needs to know it exists and what it gives up.
#[must_use]
pub fn refuse(command: &str) -> ErrorValue {
    error::opaque_action_forbidden(command)
}

/// Plans an external command, through an adapter's declaration or not at all (§6.2).
///
/// # Errors
///
/// `change.opaque_action_forbidden` when no adapter declares the program. That is §6.2's default
/// and it is a refusal rather than a downgrade: §6.3's escape is asked for by name.
pub fn plan_external(
    plan: &PlanId,
    ordinal: usize,
    command: &str,
    program: &str,
    declared: &[AdapterAction],
) -> Result<PlanAction, ErrorValue> {
    declared
        .iter()
        .find(|action| action.program() == program)
        .map(|action| action.action(plan, ordinal))
        .ok_or_else(|| refuse(command))
}

/// §6.3's explicit escape: an action the operator declared opaque.
///
/// The single effect is `UNKNOWN` in the `UNKNOWN` domain, and both halves matter. The domain is
/// what Appendix A.7 keys on to cap the plan's protection; the confidence is §8.1's floor, which
/// §2.4 forbids promoting. Together they are the reason a snapshot taken for a neighbouring
/// action does not make this one protected.
#[must_use]
pub fn opaque_action(
    plan: &PlanId,
    ordinal: usize,
    description: &str,
    program: Option<&str>,
    argv: &[&str],
) -> PlanAction {
    let execution = Execution::Opaque {
        description: Arc::from(description),
        program: program.map(Arc::from),
        argv: argv.iter().map(|word| Arc::from(*word)).collect(),
    };
    let action = PlanAction::new(plan, ordinal, ActionRole::Mutate, description, execution)
        .with_idempotency(Idempotency::Unknown)
        .recovery_semantics(
            "§6.3: the operator accepted that Ono cannot reason about this action. There are no \
             declared recovery semantics, and an asset taken for something else does not cover \
             it.",
        );
    let effect = ProposedEffect::new(
        action.id().clone(),
        EffectDomain::Unknown,
        EffectKind::Unknown,
        EffectConfidence::Unknown,
        OPAQUE_EXPLANATION,
    )
    .on(description)
    .irreversible();
    action.effecting(effect)
}
