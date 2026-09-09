//! The rule registry and the engine that runs it (spec v0.5 §15.8, §36.4, §41).
//!
//! §15.8: "Every built-in rule that emits `caused_by`, `triggered_by` or `resulted_in` MUST be
//! machine-readable and inspectable", and "No renderer may create causal language outside this
//! registry." [`BUILTIN_RULE_IDS`] is the engine's half of that sentence: the ten ids it will
//! evaluate, exported so `xtask spec-check` compares `docs/contracts/temporal/causality.yaml`
//! against the implementation in both directions rather than against a second copy of the
//! specification (ADR-0627).
//!
//! The engine validates what a rule returns before it becomes an answer. A rule already cannot
//! build a link without evidence — [`crate::causal::CausalFinding`] sees to that — and the
//! engine additionally refuses a link whose evidence it cannot resolve, whose strength exceeds
//! the weakest evidence behind it, whose source the rule's registry row does not admit, or whose
//! ends are of kinds the rule never declared. Two walls, because §55.3 is the failure mode this
//! crate exists against.

use std::collections::{BTreeMap, BTreeSet};

use ono_temporal_core::{
    CausalLink, CausalRuleId, EvidenceStrength, TemporalEvent, presentation_order,
};

use crate::causal::rule::{CausalContext, CausalRule, EventSet};
use crate::causal::rules;

/// The seven built-in causal rules of §15.2 and §15.8, in registry order.
pub const BUILTIN_CAUSAL_RULE_IDS: &[&str] = &[
    "ono.action-to-transaction",
    "ono.systemd-job-result",
    "ono.systemd-job-to-unit-state",
    "ono.process-parent",
    "ono.service-controls-process",
    "ono.action-launched-process",
    "ono.provider-causal-token",
];

/// The three built-in correlation rules of §15.5, in registry order.
///
/// Held separately from the causal ones because a correlation rule may never emit a causal
/// relation, and a list that mixed them would let a rule change class by moving a line.
pub const BUILTIN_CORRELATION_RULE_IDS: &[&str] = &[
    "ono.change-before-failure",
    "ono.resource-pressure-overlap",
    "ono.remote-endpoint-retry-spike",
];

/// Every rule the engine evaluates, causal and correlation together.
///
/// This is the list `xtask/src/temporal.rs` reads to hold the registry against the engine.
pub const BUILTIN_RULE_IDS: &[&str] = &[
    "ono.action-to-transaction",
    "ono.systemd-job-result",
    "ono.systemd-job-to-unit-state",
    "ono.process-parent",
    "ono.service-controls-process",
    "ono.action-launched-process",
    "ono.provider-causal-token",
    "ono.change-before-failure",
    "ono.resource-pressure-overlap",
    "ono.remote-endpoint-retry-spike",
];

/// The registered rules and the graph they produce (§41).
#[derive(Debug)]
pub struct CausalEngine {
    rules: Vec<Box<dyn CausalRule>>,
}

impl Default for CausalEngine {
    fn default() -> Self {
        Self::builtin()
    }
}

impl CausalEngine {
    /// The engine with the ten built-in rules of `causality.yaml`.
    #[must_use]
    pub fn builtin() -> Self {
        let mut rules = rules::causal();
        rules.extend(rules::correlation());
        Self { rules }
    }

    /// An engine over exactly these rules, for a test or a host that contributes its own (§37.4).
    #[must_use]
    pub fn with_rules(rules: Vec<Box<dyn CausalRule>>) -> Self {
        Self { rules }
    }

    /// The rules, in registry order.
    pub fn rules(&self) -> impl Iterator<Item = &dyn CausalRule> {
        self.rules.iter().map(AsRef::as_ref)
    }

    /// The id of every rule the engine will evaluate (§15.8).
    #[must_use]
    pub fn rule_ids(&self) -> Vec<CausalRuleId> {
        self.rules.iter().map(|rule| rule.id()).collect()
    }

    /// One rule by id, for `inspect`.
    #[must_use]
    pub fn rule(&self, id: &str) -> Option<&dyn CausalRule> {
        self.rules().find(|rule| rule.id().as_str() == id)
    }

    /// Every link the registered rules find in `events` (§15).
    ///
    /// The events are put into presentation order first, so the same set produces the same links
    /// whatever order it arrived in — §41's determinism requirement. The result is deduplicated
    /// by link identity and ordered by effect, so a caller never has to sort it again.
    #[must_use]
    pub fn links(&self, events: &[TemporalEvent], context: &CausalContext) -> Vec<CausalLink> {
        let mut canonical = events.to_vec();
        presentation_order(&mut canonical);
        let candidate = EventSet::new(&canonical);

        let mut links = Vec::new();
        for rule in self.rules() {
            for finding in rule.evaluate(&candidate, context) {
                let link = finding.into_link();
                if admissible(rule, &link, &candidate, context) {
                    links.push(link);
                }
            }
        }
        links.sort_by(|left, right| {
            left.effect
                .cmp(&right.effect)
                .then_with(|| left.relation.cmp(&right.relation))
                .then_with(|| left.rule.cmp(&right.rule))
                .then_with(|| left.cause.cmp(&right.cause))
        });
        let mut seen = BTreeSet::new();
        links.retain(|link| seen.insert(link.link_id.clone()));
        links
    }

    /// The links whose effect is each event, for the explanation walk.
    pub(crate) fn by_effect(
        links: &[CausalLink],
    ) -> BTreeMap<&ono_temporal_core::EventId, Vec<&CausalLink>> {
        let mut index: BTreeMap<&ono_temporal_core::EventId, Vec<&CausalLink>> = BTreeMap::new();
        for link in links {
            index.entry(&link.effect).or_default().push(link);
        }
        index
    }
}

/// Whether a link a rule returned survives the engine's own check (§15.8, §7.2).
fn admissible(
    rule: &dyn CausalRule,
    link: &CausalLink,
    candidate: &EventSet<'_>,
    context: &CausalContext,
) -> bool {
    let description = rule.describe();
    if link.rule != description.rule_id || link.relation != description.output_relation {
        return false;
    }
    if link.evidence.is_empty() || link.cause == link.effect {
        return false;
    }
    if !rules::accepts(&description.source_constraints, &link.source) {
        return false;
    }
    let (Some(cause), Some(effect)) = (candidate.get(&link.cause), candidate.get(&link.effect))
    else {
        return false;
    };
    if !description.input_event_kinds.contains(&cause.kind)
        || !description.input_event_kinds.contains(&effect.kind)
    {
        return false;
    }
    let mut weakest: Option<EvidenceStrength> = None;
    for id in &link.evidence {
        let Some(record) = context.evidence(id) else {
            return false;
        };
        weakest = Some(match weakest {
            None => record.strength,
            Some(held) => held.weakest_of(record.strength),
        });
    }
    let Some(weakest) = weakest else {
        return false;
    };
    // §7.2: nothing raises a strength, so a link is never stronger than the evidence behind it.
    if link.strength > weakest {
        return false;
    }
    let (Some(for_cause), Some(for_effect)) = (
        description.required_evidence.minimum_for(cause.kind),
        description.required_evidence.minimum_for(effect.kind),
    ) else {
        return false;
    };
    if link.strength < for_cause.weakest_of(for_effect) {
        return false;
    }
    // §15.5: an association is never presented at a strength that reads as proof.
    if !link.relation.is_causal() && link.strength > EvidenceStrength::Correlated {
        return false;
    }
    true
}
