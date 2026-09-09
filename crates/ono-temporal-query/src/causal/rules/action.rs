//! The two rules an Ono action anchors (spec v0.5 §15.2, §15.3, §17.3, §17.6).
//!
//! §17.1: "A shell has one source of causal information that external monitoring systems often
//! lack: it knows exactly which actions the operator requested through the shell." Both rules
//! here spend that advantage and neither exceeds it — §17.6 fixes the boundary at the process the
//! shell itself created, and nothing downstream of it is claimed.

use ono_temporal_core::{
    CausalRelation, CausalRuleId, EventKind, EvidenceSource, EvidenceStrength,
};

use crate::causal::facts;
use crate::causal::link::CausalFinding;
use crate::causal::rule::{
    CausalContext, CausalRule, EventSet, EvidenceRequirements, RuleDescription,
};
use crate::causal::rules::{accepts, constraints};

/// The §7.1 sources both rules read the action side from.
const ACTION_SOURCES: &[&str] = &[
    "ono.session",
    "linux.systemd-dbus",
    "adapter:<adapter-id>",
    "remote:<link-id>/<provider-id>",
    "kuang:<package-id>/<provider-id>",
];

/// `ono.action-to-transaction`: an Ono action reached a provider transaction (§15.3).
#[derive(Debug, Clone, Copy, Default)]
pub struct ActionToTransaction;

impl CausalRule for ActionToTransaction {
    fn describe(&self) -> RuleDescription {
        RuleDescription {
            rule_id: CausalRuleId::new("ono.action-to-transaction"),
            input_event_kinds: vec![
                EventKind::ActionExecuted,
                EventKind::ProviderEvent,
                EventKind::ObjectChanged,
            ],
            required_evidence: EvidenceRequirements::new(&[
                (EventKind::ActionExecuted, EvidenceStrength::Authoritative),
                (EventKind::ProviderEvent, EvidenceStrength::Asserted),
                (EventKind::ObjectChanged, EvidenceStrength::Asserted),
            ]),
            identity_constraints: "the ActionId the shell minted appears in the provider's transaction identifier \
                 for the effect event, or both events carry one transaction token",
            time_constraints: Some(
                "the effect's presentation instant is at or after the action's, where the two \
                 share a clock domain; across domains the shared token is the continuity",
            ),
            output_relation: CausalRelation::TriggeredBy,
            source_constraints: constraints(ACTION_SOURCES),
            window: None,
        }
    }

    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding> {
        let description = self.describe();
        let sources = &description.source_constraints;
        let requirements = &description.required_evidence;
        let effect_kinds = [EventKind::ProviderEvent, EventKind::ObjectChanged];
        let mut findings = Vec::new();

        for cause in candidate.of_kind(EventKind::ActionExecuted) {
            let Some((action, action_evidence)) = facts::action_of(cause, context) else {
                continue;
            };
            if action_evidence.strength < EvidenceStrength::Authoritative
                || !accepts(sources, &action_evidence.source)
            {
                continue;
            }
            let cause_tokens: Vec<&str> = facts::transaction_tokens(cause, context)
                .map(|(_, token)| token)
                .collect();

            for effect in candidate.of_kinds(&effect_kinds) {
                if effect.event_id == cause.event_id {
                    continue;
                }
                if facts::same_clock_domain(cause, effect)
                    && facts::instant(effect) < facts::instant(cause)
                {
                    continue;
                }
                for (record, token) in facts::transaction_tokens(effect, context) {
                    if !accepts(sources, &record.source)
                        || !requirements.satisfied_by(effect.kind, record.strength)
                    {
                        continue;
                    }
                    let joined = facts::token_carries(token, action.as_str())
                        || cause_tokens.contains(&token);
                    if !joined {
                        continue;
                    }
                    if let Ok(finding) = CausalFinding::emit(
                        &description.rule_id,
                        CausalRelation::TriggeredBy,
                        &cause.event_id,
                        &effect.event_id,
                        vec![
                            action_evidence.evidence_id.clone(),
                            record.evidence_id.clone(),
                        ],
                        action_evidence.strength.weakest_of(record.strength),
                        record.source.clone(),
                    ) {
                        findings.push(finding);
                    }
                    break;
                }
            }
        }
        findings
    }
}

/// `ono.action-launched-process`: the shell forked the process itself (§17.6).
///
/// §17.6 grants exactly this claim and no more: "Ono can safely claim external process P was
/// launched because command C executed, because the shell owns process creation", and "Ono MUST
/// NOT claim arbitrary downstream effects of P". The rule therefore emits one edge from the
/// action to the one process the shell created, and never walks past it.
#[derive(Debug, Clone, Copy, Default)]
pub struct ActionLaunchedProcess;

impl CausalRule for ActionLaunchedProcess {
    fn describe(&self) -> RuleDescription {
        RuleDescription {
            rule_id: CausalRuleId::new("ono.action-launched-process"),
            input_event_kinds: vec![EventKind::ActionExecuted, EventKind::ObjectAppeared],
            required_evidence: EvidenceRequirements::new(&[
                (EventKind::ActionExecuted, EvidenceStrength::Authoritative),
                (EventKind::ObjectAppeared, EvidenceStrength::Authoritative),
            ]),
            identity_constraints: "the shell's own record of the fork names the canonical process identity, and \
                 the appearing process is that identity",
            time_constraints: None,
            output_relation: CausalRelation::CausedBy,
            source_constraints: constraints(&["ono.session", "linux.procfs"]),
            window: None,
        }
    }

    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding> {
        let description = self.describe();
        let sources = &description.source_constraints;
        let requirements = &description.required_evidence;
        let session = EvidenceSource::session();
        let mut findings = Vec::new();

        for cause in candidate.of_kind(EventKind::ActionExecuted) {
            for launched in context.evidence_of(cause) {
                if launched.source != session
                    || launched.strength < EvidenceStrength::Authoritative
                    || !matches!(
                        launched.claim,
                        ono_temporal_core::EvidenceClaim::ObjectExisted { .. }
                    )
                {
                    continue;
                }
                let Some(process) = launched.subject.as_ref() else {
                    continue;
                };
                for effect in candidate.of_kind(EventKind::ObjectAppeared) {
                    if facts::subject_id(effect) != Some(process) {
                        continue;
                    }
                    let Some(observed) = facts::qualifying_evidence(
                        effect,
                        context,
                        EvidenceStrength::Authoritative,
                        &|source| accepts(sources, source),
                    ) else {
                        continue;
                    };
                    if !requirements.satisfied_by(effect.kind, observed.strength) {
                        continue;
                    }
                    if let Ok(finding) = CausalFinding::emit(
                        &description.rule_id,
                        CausalRelation::CausedBy,
                        &cause.event_id,
                        &effect.event_id,
                        vec![launched.evidence_id.clone(), observed.evidence_id.clone()],
                        launched.strength.weakest_of(observed.strength),
                        launched.source.clone(),
                    ) {
                        findings.push(finding);
                    }
                }
            }
        }
        findings
    }
}
