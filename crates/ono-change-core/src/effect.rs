//! Proposed effects and the confidence classes that keep the future honest (spec v0.6 §8).
//!
//! §1.3 states the rule this module exists to enforce: *do not invent the future*. An effect Ono
//! cannot establish is [`EffectConfidence::Unknown`], and §8.1 forbids promoting it. The lattice
//! in [`EffectConfidence::weakest_of`] is therefore one-directional in exactly the way
//! `EvidenceStrength::weakest_of` is in v0.5 — there is no counterpart that raises anything.
//!
//! §8.3 forbids probability theatre. There is no percentage anywhere in this module and no room
//! to put one: confidence is a member of a four-element closed set, and a provider that wants to
//! say more says it in [`ProposedEffect::explanation`] where a person reads it.

use std::sync::Arc;

use ono_value::Value;

use crate::id::{ActionId, EffectId};
use crate::vocab::vocabulary;

vocabulary! {
    /// How strongly Ono can assert that a proposed effect will occur (§8.1).
    EffectConfidence {
        Guaranteed => "guaranteed", "§8.1: the effect follows directly from the successfully completed operation and the provider contract, scoped to that provider's observable domain.";
        Expected => "expected", "§8.1: the provider has strong semantics for the effect, but external system behaviour can still intervene.";
        Possible => "possible", "§8.1: the object is in the known impact graph, or the provider documents the effect as possible, and Ono cannot assert that it will occur.";
        Unknown => "unknown", "§8.1: Ono lacks a justified model. Unknown MUST remain visible, and §2.4 forbids promoting it silently.";
    }
}

impl EffectConfidence {
    /// The weaker of two confidences.
    ///
    /// §2.4 and §8.1 make this the only combining operation the type offers. Composing two
    /// statements about the future can never produce a stronger statement than either, so there
    /// is deliberately no `strongest_of` for a renderer, a plugin or a model to reach for.
    #[must_use]
    pub fn weakest_of(self, other: Self) -> Self {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }

    /// The position in the lattice, strongest first.
    const fn rank(self) -> u8 {
        match self {
            EffectConfidence::Guaranteed => 0,
            EffectConfidence::Expected => 1,
            EffectConfidence::Possible => 2,
            EffectConfidence::Unknown => 3,
        }
    }

    /// Whether this confidence permits Ono to state the effect as something that will happen.
    #[must_use]
    pub const fn is_assertable(self) -> bool {
        matches!(self, EffectConfidence::Guaranteed)
    }
}

vocabulary! {
    /// The domain a proposed effect acts in — Appendix A.1's canonical mutation domains (§10.3).
    ///
    /// Protection is computed per domain rather than per plan, so this vocabulary is what makes
    /// "filesystem protected, process runtime unprotected" a statable answer instead of a
    /// footnote under a boolean.
    EffectDomain {
        FilesystemPersistent => "filesystem-persistent", "Appendix A.1: file and directory content and metadata that survives a reboot.";
        BlockStoragePersistent => "block-storage-persistent", "Appendix A.1: state below the filesystem — a logical volume, an image, a device.";
        ApplicationPersistent => "application-persistent", "Appendix A.1: state an application owns and only the application can restore consistently (§39.1).";
        ProcessRuntime => "process-runtime", "Appendix A.1: process identity and in-memory state. §33.1 makes this generally unrecoverable.";
        KernelRuntime => "kernel-runtime", "Appendix A.1: kernel state such as modules, sysctls and namespaces.";
        NetworkRuntime => "network-runtime", "Appendix A.1: routes, addresses, firewall rules and live sessions (§34).";
        RemoteSystem => "remote-system", "Appendix A.1: state on another host, reached over a link (§29).";
        ExternalSideEffect => "external-side-effect", "Appendix A.1: an effect outside the machine entirely — a webhook, an email, a cloud API call (§35.1).";
        IdentitySecurityState => "identity-security-state", "Appendix A.1: users, groups, keys, credentials and policy.";
        ProviderTransactionState => "provider-transaction-state", "Appendix A.1: state inside a provider's own transaction boundary (§27.1).";
        Unknown => "unknown", "Appendix A.1: a domain the provider declared and Ono cannot classify. Appendix A.7 caps plan protection at partially protected while one is present.";
    }
}

impl EffectDomain {
    /// Whether the domain holds state that survives the process that made it.
    ///
    /// Appendix A.5 makes this the predicate that decides which domains a plan must cover before
    /// it may be called `PROTECTED`. A runtime or external domain stays visible as an exclusion;
    /// a persistent one that nothing covers prevents the word.
    #[must_use]
    pub const fn is_persistent(self) -> bool {
        matches!(
            self,
            EffectDomain::FilesystemPersistent
                | EffectDomain::BlockStoragePersistent
                | EffectDomain::ApplicationPersistent
                | EffectDomain::IdentitySecurityState
        )
    }

    /// Whether an effect in this domain leaves the machine (§35.1).
    #[must_use]
    pub const fn is_external(self) -> bool {
        matches!(
            self,
            EffectDomain::ExternalSideEffect | EffectDomain::RemoteSystem
        )
    }
}

vocabulary! {
    /// What a proposed effect does to its object (§8.2's `kind`).
    EffectKind {
        Create => "create", "The object does not exist now and is proposed to exist afterwards.";
        Modify => "modify", "The object exists and its state is proposed to change.";
        Remove => "remove", "The object exists and is proposed not to exist afterwards.";
        Replace => "replace", "The object's identity is proposed to be replaced by another of the same kind — a restarted service's worker set (§21.4).";
        Interrupt => "interrupt", "Something in flight may be cut — an open connection, a request being served (§8.1's POSSIBLE example).";
        Emit => "emit", "An outward call or message leaves the system and cannot be taken back (§35.1).";
        Unknown => "unknown", "The provider declared an effect it cannot classify.";
    }
}

impl EffectKind {
    /// Whether an effect of this kind is irreversible on its own terms (§2.13, §35.2).
    ///
    /// An emitted request has already been served by the time Ono could reconsider, so no local
    /// recovery asset touches it. This predicate is what keeps §62.2's "universal undo button"
    /// from being written by accident.
    #[must_use]
    pub const fn is_inherently_irreversible(self) -> bool {
        matches!(self, EffectKind::Emit)
    }
}

/// One thing Ono expects may result from one action (§8.2).
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedEffect {
    id: EffectId,
    action: ActionId,
    object: Option<Arc<str>>,
    domain: EffectDomain,
    kind: EffectKind,
    confidence: EffectConfidence,
    before: Option<Value>,
    proposed: Option<Value>,
    evidence: Vec<Arc<str>>,
    explanation: Arc<str>,
    irreversible: bool,
    compensation: Option<Arc<str>>,
}

impl ProposedEffect {
    /// Declares an effect of `action` in `domain`.
    ///
    /// `explanation` is not decoration: §20.1 question 5 — "what does Ono not know?" — is
    /// answered out of these sentences, and an `UNKNOWN` effect with nothing to say is a blank
    /// where a reason belongs.
    #[must_use]
    pub fn new(
        action: ActionId,
        domain: EffectDomain,
        kind: EffectKind,
        confidence: EffectConfidence,
        explanation: impl Into<Arc<str>>,
    ) -> Self {
        let explanation = explanation.into();
        let id = EffectId::of(&action, domain.as_str(), &explanation);
        Self {
            id,
            action,
            object: None,
            domain,
            kind,
            confidence,
            before: None,
            proposed: None,
            evidence: Vec::new(),
            explanation,
            irreversible: kind.is_inherently_irreversible(),
            compensation: None,
        }
    }

    /// Rebuilds an effect out of the fields a store read back.
    ///
    /// The identity travels rather than being re-derived, because §8.2 makes it the handle a
    /// verification result and an impact node both reference.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub(crate) fn restore(
        id: EffectId,
        action: ActionId,
        object: Option<Arc<str>>,
        domain: EffectDomain,
        kind: EffectKind,
        confidence: EffectConfidence,
        before: Option<Value>,
        proposed: Option<Value>,
        evidence: Vec<Arc<str>>,
        explanation: Arc<str>,
        irreversible: bool,
        compensation: Option<Arc<str>>,
    ) -> Self {
        Self {
            id,
            action,
            object,
            domain,
            kind,
            confidence,
            before,
            proposed,
            evidence,
            explanation,
            irreversible,
            compensation,
        }
    }

    /// Names the object the effect lands on.
    #[must_use]
    pub fn on(mut self, object: impl Into<Arc<str>>) -> Self {
        let object = object.into();
        self.id = EffectId::of(&self.action, self.domain.as_str(), &object);
        self.object = Some(object);
        self
    }

    /// Records the value the object holds now and the value proposed for it.
    #[must_use]
    pub fn from_to(mut self, before: Option<Value>, proposed: Option<Value>) -> Self {
        self.before = before;
        self.proposed = proposed;
        self
    }

    /// Cites the contract that supports the claim (§8.2's `evidence`).
    #[must_use]
    pub fn citing(mut self, contract: impl Into<Arc<str>>) -> Self {
        self.evidence.push(contract.into());
        self
    }

    /// Marks the effect as one no recovery asset can undo (§2.13, §19.4).
    #[must_use]
    pub const fn irreversible(mut self) -> Self {
        self.irreversible = true;
        self
    }

    /// Declares an inverse action that restores an acceptable semantic state (§27.4).
    ///
    /// §27.4 is explicit that this is not rollback: compensation restores semantics, not prior
    /// state, and [`crate::ProtectionLevel::Compensatable`] is the strongest word it earns.
    #[must_use]
    pub fn compensated_by(mut self, action: impl Into<Arc<str>>) -> Self {
        self.compensation = Some(action.into());
        self
    }

    /// The same effect, declared by `action` (§8.2).
    ///
    /// An effect belongs to the action that declares it, and its identity is derived from that
    /// action. A plan that rebuilds a provider's action under its own identity — a recovery plan
    /// re-anchoring a fragment's actions (§2.12, §3.3) — carries the effect across with it, so
    /// the plan's effect list states what the rebuilt action will do.
    #[must_use]
    pub fn for_action(&self, action: ActionId) -> Self {
        let id = EffectId::of(&action, self.domain.as_str(), &self.explanation);
        Self {
            id,
            action,
            ..self.clone()
        }
    }

    /// The effect's identity.
    #[must_use]
    pub const fn id(&self) -> &EffectId {
        &self.id
    }

    /// The action this effect belongs to.
    #[must_use]
    pub const fn action(&self) -> &ActionId {
        &self.action
    }

    /// The object the effect lands on, where one is known.
    #[must_use]
    pub fn object(&self) -> Option<&str> {
        self.object.as_deref()
    }

    /// The domain the effect acts in.
    #[must_use]
    pub const fn domain(&self) -> EffectDomain {
        self.domain
    }

    /// What the effect does to its object.
    #[must_use]
    pub const fn kind(&self) -> EffectKind {
        self.kind
    }

    /// How strongly Ono asserts the effect.
    #[must_use]
    pub const fn confidence(&self) -> EffectConfidence {
        self.confidence
    }

    /// The value the object holds now, where it is known.
    #[must_use]
    pub const fn before(&self) -> Option<&Value> {
        self.before.as_ref()
    }

    /// The value proposed for the object, where one is known.
    #[must_use]
    pub const fn proposed(&self) -> Option<&Value> {
        self.proposed.as_ref()
    }

    /// The contracts cited in support of the claim.
    #[must_use]
    pub fn evidence(&self) -> &[Arc<str>] {
        &self.evidence
    }

    /// The sentence a person reads.
    #[must_use]
    pub fn explanation(&self) -> &str {
        &self.explanation
    }

    /// Whether no recovery asset can undo this effect.
    #[must_use]
    pub const fn is_irreversible(&self) -> bool {
        self.irreversible
    }

    /// The declared inverse action, where the provider offers one.
    #[must_use]
    pub fn compensation(&self) -> Option<&str> {
        self.compensation.as_deref()
    }

    /// The canonical text this effect contributes to a plan digest (§4.4).
    #[must_use]
    pub fn digest_text(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.id.as_str(),
            self.domain.as_str(),
            self.kind.as_str(),
            self.confidence.as_str(),
            self.object.as_deref().unwrap_or(""),
            u8::from(self.irreversible),
        )
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

    fn action() -> ActionId {
        ActionId::derive(&["a"])
    }

    #[test]
    fn should_take_the_weaker_confidence_when_two_are_combined() {
        assert_eq!(
            EffectConfidence::Guaranteed.weakest_of(EffectConfidence::Unknown),
            EffectConfidence::Unknown,
            "combining a guarantee with an unknown must not yield a guarantee (§2.4)"
        );
        assert_eq!(
            EffectConfidence::Expected.weakest_of(EffectConfidence::Possible),
            EffectConfidence::Possible,
            "the weaker of expected and possible is possible (§8.1)"
        );
    }

    #[test]
    fn should_offer_no_way_to_raise_a_confidence() {
        let combined = EffectConfidence::ALL
            .iter()
            .fold(EffectConfidence::Guaranteed, |carry, next| {
                carry.weakest_of(*next)
            });
        assert_eq!(
            combined,
            EffectConfidence::Unknown,
            "folding every class through the only combinator must land on the weakest (§1.3)"
        );
    }

    #[test]
    fn should_treat_only_a_guarantee_as_assertable() {
        for confidence in EffectConfidence::ALL {
            assert_eq!(
                confidence.is_assertable(),
                *confidence == EffectConfidence::Guaranteed,
                "§1.1 permits stating a future state only where a provider proves it"
            );
        }
    }

    #[test]
    fn should_classify_only_state_that_outlives_the_process_as_persistent() {
        assert!(EffectDomain::FilesystemPersistent.is_persistent());
        assert!(EffectDomain::ApplicationPersistent.is_persistent());
        assert!(
            !EffectDomain::ProcessRuntime.is_persistent(),
            "§33.1: runtime state is not persistent, so a snapshot never covers it"
        );
        assert!(
            !EffectDomain::ExternalSideEffect.is_persistent(),
            "§35.2: an external effect is not local persistent state"
        );
        assert!(
            !EffectDomain::Unknown.is_persistent(),
            "an unknown domain is not silently treated as coverable (Appendix A.7)"
        );
    }

    #[test]
    fn should_keep_an_emitted_effect_irreversible_without_being_asked() {
        let effect = ProposedEffect::new(
            action(),
            EffectDomain::ExternalSideEffect,
            EffectKind::Emit,
            EffectConfidence::Expected,
            "a deployment webhook is posted",
        );
        assert!(
            effect.is_irreversible(),
            "§35.2: a request already served cannot be taken back, whatever else is protected"
        );
    }

    #[test]
    fn should_not_mark_an_ordinary_modification_irreversible() {
        let effect = ProposedEffect::new(
            action(),
            EffectDomain::FilesystemPersistent,
            EffectKind::Modify,
            EffectConfidence::Guaranteed,
            "the configuration file is replaced",
        );
        assert!(
            !effect.is_irreversible(),
            "a file write is exactly the case §32.1 says can be strongly protected"
        );
    }

    #[test]
    fn should_change_the_digest_when_the_effect_changes() {
        let base = ProposedEffect::new(
            action(),
            EffectDomain::FilesystemPersistent,
            EffectKind::Modify,
            EffectConfidence::Guaranteed,
            "x",
        );
        let harder = base.clone().irreversible();
        assert_ne!(
            base.digest_text(),
            harder.digest_text(),
            "a plan whose effect became irreversible is not the same sealed plan (§4.4)"
        );
    }
}
