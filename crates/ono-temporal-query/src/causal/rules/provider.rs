//! The generic provider transaction rule (spec v0.5 §15.2, §21.6, §37.4).
//!
//! §15.2's fourth example is "a provider explicitly exposes a causal transaction identifier", and
//! §21.6 makes that a declared capability rather than a habit. Where a provider is willing to say
//! "these two events are the same transaction", Ono relays the claim and attributes it to the
//! provider; where a plugin is the provider, §37.4 caps the link at `asserted`.

use std::collections::BTreeMap;

use ono_temporal_core::{
    CausalRelation, CausalRuleId, EventKind, EvidenceSource, EvidenceStrength, Ordering,
    TemporalEvent, happens_before,
};

use crate::causal::facts;
use crate::causal::link::CausalFinding;
use crate::causal::rule::{
    CausalContext, CausalRule, EventSet, EvidenceRequirements, RuleDescription,
};
use crate::causal::rules::{accepts, constraints};

/// The kinds a provider transaction may span.
const KINDS: &[EventKind] = &[
    EventKind::ProviderEvent,
    EventKind::ObjectChanged,
    EventKind::ObjectAppeared,
    EventKind::ObjectDisappeared,
];

/// Whether the provider's own record puts `cause` before `effect` (§26.1).
///
/// [`happens_before`] is asked first and its answer is taken whenever it has one: a monotonic
/// reading inside one boot, or a sequence inside one stream on one host. Where it answers
/// [`Ordering::Concurrent`] the two events carry no fact that orders them, and §26.1's remaining
/// evidence is a relation *between* them rather than a fact *on* one — which is precisely the
/// transaction identity this rule already joined on. So the fall-back is the provider's own
/// sequence numbers within that one transaction, which is §26.1's "same sequence stream" and is
/// what lets an evidence chain cross a host boundary (§26.4). Wall clocks are never consulted:
/// two events with no sequence stay unordered and produce no link.
fn ordered_by_the_provider(cause: &TemporalEvent, effect: &TemporalEvent) -> bool {
    match happens_before(cause, effect).0 {
        Ordering::Before => true,
        Ordering::After => false,
        Ordering::Concurrent => match (cause.times.source_sequence, effect.times.source_sequence) {
            (Some(here), Some(there)) => here < there,
            _ => false,
        },
    }
}

/// `ono.provider-causal-token`: two events the provider itself calls one transaction (§21.6).
#[derive(Debug, Clone, Copy, Default)]
pub struct ProviderCausalToken;

impl CausalRule for ProviderCausalToken {
    fn describe(&self) -> RuleDescription {
        RuleDescription {
            rule_id: CausalRuleId::new("ono.provider-causal-token"),
            input_event_kinds: KINDS.to_vec(),
            required_evidence: EvidenceRequirements::new(&[
                (EventKind::ProviderEvent, EvidenceStrength::Asserted),
                (EventKind::ObjectChanged, EvidenceStrength::Asserted),
                (EventKind::ObjectAppeared, EvidenceStrength::Asserted),
                (
                    EventKind::ObjectDisappeared,
                    EvidenceStrength::Authoritative,
                ),
            ]),
            identity_constraints: "the provider published an explicit causal transaction identifier on both events, \
                 the two identifiers are equal, and the provider advertises causal_tokens",
            time_constraints: Some(
                "the effect is at or after the cause within the provider's own sequence; wall \
                 clock order alone is never sufficient",
            ),
            output_relation: CausalRelation::CausedBy,
            source_constraints: constraints(&[
                "adapter:<adapter-id>",
                "remote:<link-id>/<provider-id>",
                "kuang:<package-id>/<provider-id>",
                "linux.systemd-dbus",
            ]),
            window: None,
        }
    }

    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding> {
        let description = self.describe();
        let sources = &description.source_constraints;
        let requirements = &description.required_evidence;

        // One bucket per (source, token). A transaction is a claim one source made about its own
        // work, so a token two different sources happen to spell the same way is two tokens.
        let mut buckets: BTreeMap<(EvidenceSource, &str), Vec<(&TemporalEvent, &_)>> =
            BTreeMap::new();
        for event in candidate.of_kinds(KINDS) {
            for (record, token) in facts::transaction_tokens(event, context) {
                if !accepts(sources, &record.source)
                    || !context.publishes_causal_tokens(&record.source)
                    || !requirements.satisfied_by(event.kind, record.strength)
                {
                    continue;
                }
                buckets
                    .entry((record.source.clone(), token))
                    .or_default()
                    .push((event, record));
            }
        }

        let mut findings = Vec::new();
        for ((source, _), members) in buckets {
            let ceiling = if source.is_plugin() {
                EvidenceStrength::Asserted
            } else {
                EvidenceStrength::Authoritative
            };
            for (cause, cause_evidence) in &members {
                for (effect, effect_evidence) in &members {
                    if cause.event_id == effect.event_id {
                        continue;
                    }
                    if !ordered_by_the_provider(cause, effect) {
                        continue;
                    }
                    let strength = cause_evidence
                        .strength
                        .weakest_of(effect_evidence.strength)
                        .weakest_of(ceiling);
                    if let Ok(finding) = CausalFinding::emit(
                        &description.rule_id,
                        CausalRelation::CausedBy,
                        &cause.event_id,
                        &effect.event_id,
                        vec![
                            cause_evidence.evidence_id.clone(),
                            effect_evidence.evidence_id.clone(),
                        ],
                        strength,
                        source.clone(),
                    ) {
                        findings.push(finding);
                    }
                }
            }
        }
        findings
    }
}
