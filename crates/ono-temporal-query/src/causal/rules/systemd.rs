//! The two systemd job rules (spec v0.5 §15.2, §22.2).
//!
//! §15.2's second example is "systemd job result explicitly identifies the unit transition", and
//! it is the strongest built-in rule because one authoritative source states both ends. Its
//! companion explains the transitions a job produced while it was still running, which is a
//! different claim resting on different evidence — §15.8 entitles a reader of an explanation to
//! know which of the two fired, so they are two rules rather than one with a branch.

use ono_temporal_core::{CausalRelation, CausalRuleId, EventKind, EvidenceStrength, TemporalEvent};

use crate::causal::facts;
use crate::causal::link::CausalFinding;
use crate::causal::rule::{
    CausalContext, CausalRule, EventSet, EvidenceRequirements, RuleDescription,
};
use crate::causal::rules::{accepts, constraints};

/// The states a `start` job is consistent with reaching (§22.2).
const STARTING: &[&str] = &["activating", "active", "reloading"];
/// The states a `stop` job is consistent with reaching.
const STOPPING: &[&str] = &["deactivating", "inactive", "failed"];

/// Whether a job of `job_type` is consistent with a unit reaching `after`.
fn consistent(job_type: &str, after: &str) -> bool {
    match job_type {
        "start" | "start-or-reload" => STARTING.contains(&after),
        "stop" => STOPPING.contains(&after),
        "restart" | "try-restart" => STARTING.contains(&after) || STOPPING.contains(&after),
        "reload" => after == "reloading" || after == "active",
        _ => false,
    }
}

/// The state a unit event says it reached, from its typed field changes (§6.2).
fn reached_state(event: &TemporalEvent) -> Option<&str> {
    event
        .changed_fields
        .iter()
        .find(|change| {
            change.field.as_ref() == facts::UNIT_STATE_FIELD
                || change.field.as_ref() == facts::UNIT_STATE_FIELD_SHORT
        })
        .and_then(|change| match change.after.as_ref()? {
            ono_value::Value::String(text) => Some(text.as_ref()),
            _ => None,
        })
}

/// `ono.systemd-job-result`: the job result names the transition (§15.2).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemdJobResult;

impl CausalRule for SystemdJobResult {
    fn describe(&self) -> RuleDescription {
        RuleDescription {
            rule_id: CausalRuleId::new("ono.systemd-job-result"),
            input_event_kinds: vec![EventKind::ProviderEvent, EventKind::ObjectChanged],
            required_evidence: EvidenceRequirements::new(&[
                (EventKind::ProviderEvent, EvidenceStrength::Authoritative),
                (EventKind::ObjectChanged, EvidenceStrength::Authoritative),
            ]),
            identity_constraints: "a JobRemoved result names the unit whose transition the effect records, and the \
                 job path on the result equals the job path the effect carries",
            time_constraints: Some(
                "the transition lies within the job's lifetime, bounded by the creation and \
                 removal instants systemd itself reported",
            ),
            output_relation: CausalRelation::CausedBy,
            source_constraints: constraints(&["linux.systemd-dbus"]),
            window: None,
        }
    }

    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding> {
        let description = self.describe();
        let sources = &description.source_constraints;
        let requirements = &description.required_evidence;
        let mut findings = Vec::new();

        for cause in candidate.of_kind(EventKind::ProviderEvent) {
            if cause.subtype.as_deref() != Some(facts::SYSTEMD_JOB_REMOVED) {
                continue;
            }
            for (job_evidence, job_path) in facts::transaction_tokens(cause, context) {
                if !accepts(sources, &job_evidence.source)
                    || !requirements.satisfied_by(cause.kind, job_evidence.strength)
                {
                    continue;
                }
                let Some((from, until)) = facts::token_span(candidate.events(), context, job_path)
                else {
                    continue;
                };
                for effect in candidate.of_kind(EventKind::ObjectChanged) {
                    let Some(unit) = facts::subject_id(effect) else {
                        continue;
                    };
                    if !facts::mentions(cause, unit) {
                        continue;
                    }
                    let carries_job =
                        facts::transaction_tokens(effect, context).find(|(record, token)| {
                            *token == job_path
                                && accepts(sources, &record.source)
                                && requirements.satisfied_by(effect.kind, record.strength)
                        });
                    let Some((transition_evidence, _)) = carries_job else {
                        continue;
                    };
                    let at = facts::instant(effect);
                    if at < from || at > until {
                        continue;
                    }
                    if let Ok(finding) = CausalFinding::emit(
                        &description.rule_id,
                        CausalRelation::CausedBy,
                        &cause.event_id,
                        &effect.event_id,
                        vec![
                            job_evidence.evidence_id.clone(),
                            transition_evidence.evidence_id.clone(),
                        ],
                        job_evidence
                            .strength
                            .weakest_of(transition_evidence.strength),
                        job_evidence.source.clone(),
                    ) {
                        findings.push(finding);
                    }
                }
            }
        }
        findings
    }
}

/// `ono.systemd-job-to-unit-state`: the transitions a running job produced (§15.2, §22.2).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemdJobToUnitState;

impl CausalRule for SystemdJobToUnitState {
    fn describe(&self) -> RuleDescription {
        RuleDescription {
            rule_id: CausalRuleId::new("ono.systemd-job-to-unit-state"),
            input_event_kinds: vec![EventKind::ProviderEvent, EventKind::ObjectChanged],
            required_evidence: EvidenceRequirements::new(&[
                (EventKind::ProviderEvent, EvidenceStrength::Authoritative),
                (EventKind::ObjectChanged, EvidenceStrength::Authoritative),
            ]),
            identity_constraints: "the job's unit equals the changed object, and the job's type — start, stop, \
                 restart, reload — is consistent with the observed transition",
            time_constraints: Some("the state change lies between the job's creation and removal"),
            output_relation: CausalRelation::CausedBy,
            source_constraints: constraints(&["linux.systemd-dbus"]),
            window: None,
        }
    }

    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding> {
        let description = self.describe();
        let sources = &description.source_constraints;
        let requirements = &description.required_evidence;
        let mut findings = Vec::new();

        for cause in candidate.of_kind(EventKind::ProviderEvent) {
            if cause.subtype.as_deref() != Some(facts::SYSTEMD_JOB_NEW) {
                continue;
            }
            let Some(job_type) = facts::payload_text(cause, facts::JOB_TYPE_FIELD) else {
                continue;
            };
            for (job_evidence, job_path) in facts::transaction_tokens(cause, context) {
                if !accepts(sources, &job_evidence.source)
                    || !requirements.satisfied_by(cause.kind, job_evidence.strength)
                {
                    continue;
                }
                let Some((from, until)) = facts::token_span(candidate.events(), context, job_path)
                else {
                    continue;
                };
                for effect in candidate.of_kind(EventKind::ObjectChanged) {
                    let Some(unit) = facts::subject_id(effect) else {
                        continue;
                    };
                    if !facts::mentions(cause, unit) {
                        continue;
                    }
                    let Some(after) = reached_state(effect) else {
                        continue;
                    };
                    if !consistent(job_type, after) {
                        continue;
                    }
                    let at = facts::instant(effect);
                    if at < from || at > until {
                        continue;
                    }
                    let Some(state_evidence) =
                        facts::field_evidence(effect, context, facts::UNIT_STATE_FIELD)
                            .chain(facts::field_evidence(
                                effect,
                                context,
                                facts::UNIT_STATE_FIELD_SHORT,
                            ))
                            .find(|record| {
                                accepts(sources, &record.source)
                                    && requirements.satisfied_by(effect.kind, record.strength)
                            })
                    else {
                        continue;
                    };
                    if let Ok(finding) = CausalFinding::emit(
                        &description.rule_id,
                        CausalRelation::CausedBy,
                        &cause.event_id,
                        &effect.event_id,
                        vec![
                            job_evidence.evidence_id.clone(),
                            state_evidence.evidence_id.clone(),
                        ],
                        job_evidence.strength.weakest_of(state_evidence.strength),
                        job_evidence.source.clone(),
                    ) {
                        findings.push(finding);
                    }
                }
            }
        }
        findings
    }
}
