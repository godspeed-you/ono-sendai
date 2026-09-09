//! The two process rules (spec v0.5 §15.2, §22.1, §22.2, §6.3).
//!
//! §15.2's third example is "a parent process creation event from the kernel identifies its
//! parent/child creation relationship". The kernel is the authority for process parentage, so the
//! claim is authoritative even though procfs is a snapshot source for everything else: the
//! strength belongs to the fact rather than to the polling.

use ono_temporal_core::{CausalRelation, CausalRuleId, EventKind, EvidenceStrength};

use crate::causal::facts;
use crate::causal::link::CausalFinding;
use crate::causal::rule::{
    CausalContext, CausalRule, EventSet, EvidenceRequirements, RuleDescription,
};
use crate::causal::rules::{accepts, constraints};

/// `ono.process-parent`: the kernel named the parent at creation (§15.2).
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessParent;

impl CausalRule for ProcessParent {
    fn describe(&self) -> RuleDescription {
        RuleDescription {
            rule_id: CausalRuleId::new("ono.process-parent"),
            input_event_kinds: vec![EventKind::ObjectAppeared],
            required_evidence: EvidenceRequirements::new(&[(
                EventKind::ObjectAppeared,
                EvidenceStrength::Authoritative,
            )]),
            identity_constraints: "the child's lifetime-safe parent identity — boot, pid, start time, pid \
                 namespace — equals the parent process's identity as the kernel reported it",
            time_constraints: Some(
                "the child's creation instant lies within the parent's lifetime",
            ),
            output_relation: CausalRelation::CausedBy,
            source_constraints: constraints(&["linux.procfs", "kuang:<package-id>/<provider-id>"]),
            window: None,
        }
    }

    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding> {
        let description = self.describe();
        let sources = &description.source_constraints;
        let requirements = &description.required_evidence;
        let mut findings = Vec::new();

        for cause in candidate.of_kind(EventKind::ObjectAppeared) {
            let Some(parent) = facts::subject_id(cause) else {
                continue;
            };
            let Some(parent_evidence) = facts::qualifying_evidence(
                cause,
                context,
                EvidenceStrength::Authoritative,
                &|source| accepts(sources, source),
            ) else {
                continue;
            };
            // The parent stops being able to create children when it is reported gone.
            let parent_gone = candidate
                .of_kind(EventKind::ObjectDisappeared)
                .filter(|event| facts::subject_id(event) == Some(parent))
                .map(facts::instant)
                .min();

            for effect in candidate.of_kind(EventKind::ObjectAppeared) {
                let Some(child) = facts::subject_id(effect) else {
                    continue;
                };
                if child == parent || !facts::same_clock_domain(cause, effect) {
                    continue;
                }
                let Some(parentage) = context.relation_between(facts::PARENT_OF, parent, child)
                else {
                    continue;
                };
                if parentage.strength < EvidenceStrength::Authoritative
                    || !accepts(sources, &parentage.source)
                {
                    continue;
                }
                let Some(child_evidence) = facts::qualifying_evidence(
                    effect,
                    context,
                    EvidenceStrength::Authoritative,
                    &|source| accepts(sources, source),
                ) else {
                    continue;
                };
                if !requirements.satisfied_by(effect.kind, child_evidence.strength) {
                    continue;
                }
                let born = facts::instant(effect);
                if born < facts::instant(cause) || parent_gone.is_some_and(|gone| born > gone) {
                    continue;
                }
                if let Ok(finding) = CausalFinding::emit(
                    &description.rule_id,
                    CausalRelation::CausedBy,
                    &cause.event_id,
                    &effect.event_id,
                    vec![
                        parentage.evidence_id.clone(),
                        parent_evidence.evidence_id.clone(),
                        child_evidence.evidence_id.clone(),
                    ],
                    parentage
                        .strength
                        .weakest_of(parent_evidence.strength)
                        .weakest_of(child_evidence.strength),
                    parentage.source.clone(),
                ) {
                    findings.push(finding);
                }
            }
        }
        findings
    }
}

/// `ono.service-controls-process`: the service that owns the cgroup was doing something (§15.2).
///
/// The disappearance side needs `authoritative` evidence because §6.3 already forbids inventing a
/// disappearance, and a causal claim about one must not be the thing that launders it.
///
/// Cause and effect are the same class of event on the two objects: a service that came up
/// explains the processes that came up in its cgroup, and a service that went away explains the
/// processes that went with it. Pairing the two directions the other way round would produce a
/// sentence no evidence supports.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServiceControlsProcess;

impl CausalRule for ServiceControlsProcess {
    fn describe(&self) -> RuleDescription {
        RuleDescription {
            rule_id: CausalRuleId::new("ono.service-controls-process"),
            input_event_kinds: vec![EventKind::ObjectAppeared, EventKind::ObjectDisappeared],
            required_evidence: EvidenceRequirements::new(&[
                (EventKind::ObjectAppeared, EvidenceStrength::Derived),
                (
                    EventKind::ObjectDisappeared,
                    EvidenceStrength::Authoritative,
                ),
            ]),
            identity_constraints: "the process's cgroup at the observed instant is the service's, through v0.4's \
                 service.controls_process edge, and both ends resolve to canonical identities",
            time_constraints: Some(
                "the process event lies within an interval where the service held that cgroup",
            ),
            output_relation: CausalRelation::CausedBy,
            source_constraints: constraints(&["linux.systemd-dbus", "linux.procfs"]),
            window: None,
        }
    }

    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding> {
        let description = self.describe();
        let sources = &description.source_constraints;
        let requirements = &description.required_evidence;
        let kinds = [EventKind::ObjectAppeared, EventKind::ObjectDisappeared];
        let mut findings = Vec::new();

        for cause in candidate.of_kinds(&kinds) {
            let Some(service) = facts::subject_id(cause) else {
                continue;
            };
            let Some(service_evidence) = facts::qualifying_evidence(
                cause,
                context,
                requirements
                    .minimum_for(cause.kind)
                    .unwrap_or(EvidenceStrength::Authoritative),
                &|source| accepts(sources, source),
            ) else {
                continue;
            };
            for effect in candidate.of_kinds(&kinds) {
                if effect.kind != cause.kind || effect.event_id == cause.event_id {
                    continue;
                }
                let Some(process) = facts::subject_id(effect) else {
                    continue;
                };
                let Some(membership) =
                    context.relation_between(facts::CONTROLS_PROCESS, service, process)
                else {
                    continue;
                };
                if !accepts(sources, &membership.source) {
                    continue;
                }
                let at = facts::instant(effect);
                let held = facts::relation_interval(membership)
                    .is_some_and(|interval| interval.contains(at));
                if !held {
                    continue;
                }
                let Some(process_evidence) = facts::qualifying_evidence(
                    effect,
                    context,
                    requirements
                        .minimum_for(effect.kind)
                        .unwrap_or(EvidenceStrength::Authoritative),
                    &|source| accepts(sources, source),
                ) else {
                    continue;
                };
                if facts::same_clock_domain(cause, effect) && at < facts::instant(cause) {
                    continue;
                }
                if let Ok(finding) = CausalFinding::emit(
                    &description.rule_id,
                    CausalRelation::CausedBy,
                    &cause.event_id,
                    &effect.event_id,
                    vec![
                        membership.evidence_id.clone(),
                        service_evidence.evidence_id.clone(),
                        process_evidence.evidence_id.clone(),
                    ],
                    membership
                        .strength
                        .weakest_of(service_evidence.strength)
                        .weakest_of(process_evidence.strength),
                    membership.source.clone(),
                ) {
                    findings.push(finding);
                }
            }
        }
        findings
    }
}
