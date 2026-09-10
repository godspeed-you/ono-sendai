//! The acknowledgement gates of §19.4 and §40, and the two ways they are answered.
//!
//! §40.1 keeps the normal path usable: a LOW or MODERATE plan with no irreversible action applies
//! on `apply` alone, because a gate on every plan is a gate nobody reads. §40.2 fixes what a gate
//! says when there is one — *the rule's own reason*, never a generic "are you sure" — and §40.3
//! fixes who may be asked: a script never waits for a prompt, so outside a terminal every gate is
//! a structured refusal carrying the machine-readable plan on its metadata, exactly as
//! `crate::kuang_install` does for an install plan.
//!
//! # The five flags, and which rule each answers
//!
//! | flag | what it acknowledges |
//! |---|---|
//! | `--accept-risk` | §19.4's HIGH or CRITICAL class, whatever produced it |
//! | `--accept-irreversible` | §19.4's irreversible actions, independently of the class |
//! | `--accept-service-outage` | §40.2's worked gate: `risk.bulk.whole-role` and `risk.downtime.service-restart`, the findings that say no healthy serving member is excluded (§28.3) |
//! | `--accept-newer-state-loss` | §24.5 and §13.6: the newer state and the provider-native history a recovery would destroy |
//! | `--accept-stale-protection` | §18.3: an asset captured before the state drifted |
//!
//! `--accept-service-outage` satisfies the risk gate only where *every* finding at the plan's own
//! class speaks to downtime. A plan that is HIGH for two reasons is not acknowledged by accepting
//! one of them, and §40.2's "the reason is in the metadata rather than behind a generic question"
//! is what makes that visible rather than surprising.

use std::sync::Arc;

use ono_change_core::{
    ChangePlan, RequiredAcknowledgement, RiskClass, RiskDimension, RiskFinding, error,
};
use ono_change_protection::ChangeSettings;
use ono_command::BoundArguments;
use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

/// The rule ids §40.2's worked gate is about (§28.3, §33.1).
const OUTAGE_RULES: [&str; 2] = ["risk.bulk.whole-role", "risk.downtime.service-restart"];

/// The acknowledgements one invocation supplied (§19.4, §40.3).
#[derive(Debug, Clone, Copy, Default)]
pub struct Acknowledgements {
    /// `--accept-risk` (§19.4).
    pub risk: bool,
    /// `--accept-irreversible` (§19.4).
    pub irreversible: bool,
    /// `--accept-service-outage` (§40.2, §28.3).
    pub service_outage: bool,
    /// `--accept-newer-state-loss` (§24.5, §13.6).
    pub newer_state_loss: bool,
    /// `--accept-stale-protection` (§18.3).
    pub stale_protection: bool,
    /// `--confirm` (§40.3).
    pub confirmed: bool,
}

impl Acknowledgements {
    /// These acknowledgements, with the one `flag` spells added (§19.4, §40.3).
    ///
    /// An operator who answers a gate at a terminal has given exactly what the flag gives, and
    /// §19.4 stores it in the sealed revision the same way.
    #[must_use]
    pub fn granting(mut self, flag: &str) -> Self {
        match flag {
            "--accept-risk" => self.risk = true,
            "--accept-irreversible" => self.irreversible = true,
            "--accept-service-outage" => self.service_outage = true,
            "--accept-newer-state-loss" => self.newer_state_loss = true,
            "--accept-stale-protection" => self.stale_protection = true,
            "--confirm" => self.confirmed = true,
            _ => {}
        }
        self
    }

    /// The acknowledgements `arguments` carries.
    #[must_use]
    pub fn of(arguments: &BoundArguments) -> Self {
        Self {
            risk: arguments.flag("accept-risk"),
            irreversible: arguments.flag("accept-irreversible"),
            service_outage: arguments.flag("accept-service-outage"),
            newer_state_loss: arguments.flag("accept-newer-state-loss"),
            stale_protection: arguments.flag("accept-stale-protection"),
            confirmed: arguments.flag("confirm"),
        }
    }
}

/// One gate a plan raises: what it is about, and the sentence §40.2 requires it to carry.
#[derive(Debug, Clone)]
pub struct Gate {
    /// The flag that acknowledges it non-interactively (§40.3).
    pub flag: &'static str,
    /// The refusal, already carrying the rule's own reasons on its metadata (§40.2).
    pub refusal: ErrorValue,
    /// The lines a person is shown before being asked (§40.2).
    pub reasons: Vec<String>,
}

/// Every gate `plan` raises that `given` has not answered (§19.4, §40.1), under the settings
/// this shell resolved.
///
/// An empty answer is §40.1's normal path, and it is the common one on purpose.
#[must_use]
pub fn outstanding(plan: &ChangePlan, given: Acknowledgements) -> Vec<Gate> {
    outstanding_under(plan, given, &super::session::configured())
}

/// Every gate `plan` raises that `given` has not answered, under `settings` (§19.4, Appendix H).
///
/// The plan's own assessment gates HIGH and CRITICAL. A profile whose risk gate starts lower —
/// `cautious`'s `moderate+` (Appendix H.2) — gates the classes between as well, with the same
/// flag, so a plan §40.1 would let through on `apply` alone needs `--accept-risk` under it.
#[must_use]
pub fn outstanding_under(
    plan: &ChangePlan,
    given: Acknowledgements,
    settings: &ChangeSettings,
) -> Vec<Gate> {
    let risk = plan.risk();
    let mut gates = Vec::new();
    let class = risk.classify();
    if !class.needs_acknowledgement()
        && settings.requires_acknowledgement(class)
        && !risk.is_risk_accepted()
        && !given.risk
    {
        let mut reasons = sentences(&risk.leading());
        if let Some(profile) = settings.profile() {
            reasons.push(format!(
                "the `{}` profile's risk gate is `{}+`, and this plan is {} (v0.6 Appendix H)",
                profile.as_str(),
                profile.risk_gate().as_str(),
                class.as_str()
            ));
        }
        gates.push(Gate {
            flag: "--accept-risk",
            refusal: error::risk_not_accepted(plan.id(), class.as_str(), &reasons),
            reasons,
        });
    }
    for required in risk.outstanding_acknowledgements() {
        match required {
            RequiredAcknowledgement::Risk(class) => {
                let leading = risk.leading();
                if given.risk || satisfied_by_outage(&leading, given) {
                    continue;
                }
                let reasons = sentences(&leading);
                let flag = flag_for(&leading);
                let refusal = if flag == "--accept-service-outage" {
                    outage_not_accepted(plan, class, &reasons)
                } else {
                    error::risk_not_accepted(plan.id(), class.as_str(), &reasons)
                };
                gates.push(Gate {
                    flag,
                    refusal,
                    reasons,
                });
            }
            RequiredAcknowledgement::Irreversible => {
                if given.irreversible {
                    continue;
                }
                let findings: Vec<&RiskFinding> = risk
                    .findings()
                    .iter()
                    .filter(|finding| finding.dimension() == RiskDimension::Irreversibility)
                    .collect();
                let reasons = sentences(&findings);
                gates.push(Gate {
                    flag: "--accept-irreversible",
                    refusal: error::irreversible_not_accepted(plan.id(), &reasons),
                    reasons,
                });
            }
        }
    }
    gates
}

/// §40.2's worked gate, refused as §45's bulk guard (`change.bulk_guard_failed`).
///
/// `risk.yaml` declares that error for the service-outage gate: what fired is the guard §28.3
/// puts on a bulk change that leaves no healthy serving member of its group, rather than a risk
/// class in general, so a script can tell the two refusals apart by code. The rules' sentences
/// travel on the metadata as the risk refusal's do, and the help names the one flag that answers
/// it.
fn outage_not_accepted(plan: &ChangePlan, class: RiskClass, reasons: &[String]) -> ErrorValue {
    let short = plan.id().short();
    ErrorValue::new(
        ErrorCode::ChangeBulkGuardFailed,
        format!(
            "plan {short} would leave no healthy serving member of its group, and the outage was \
             not acknowledged"
        ),
    )
    .with_help(format!(
        "v0.6 §40.2 and §28.3: the reason is in the metadata rather than behind a generic \
         question. `apply plan/{short} --accept-service-outage` acknowledges the outage. A script \
         supplies it as a flag and never waits for a prompt (§40.3). Nothing was changed"
    ))
    .with_metadata("plan", Value::string(plan.id().as_str()))
    .with_metadata("risk", Value::string(class.as_str()))
    .with_metadata(
        "reasons",
        Value::list(reasons.iter().map(|reason| Value::string(reason))),
    )
}

/// Whether `--accept-service-outage` answers this risk class on its own (§40.2).
fn satisfied_by_outage(leading: &[&RiskFinding], given: Acknowledgements) -> bool {
    given.service_outage
        && !leading.is_empty()
        && leading
            .iter()
            .all(|finding| OUTAGE_RULES.contains(&finding.rule()))
}

/// The flag whose name a refusal should name first.
fn flag_for(leading: &[&RiskFinding]) -> &'static str {
    if !leading.is_empty()
        && leading
            .iter()
            .all(|finding| OUTAGE_RULES.contains(&finding.rule()))
    {
        return "--accept-service-outage";
    }
    "--accept-risk"
}

/// The rules' own sentences, which §40.2 puts in place of a generic question.
fn sentences(findings: &[&RiskFinding]) -> Vec<String> {
    findings
        .iter()
        .map(|finding| format!("{}: {}", finding.rule(), finding.reason()))
        .collect()
}

/// Answers every gate, at a terminal by asking and everywhere else by refusing (§40.2, §40.3).
///
/// `plan_value` is the machine-readable plan the refusal carries, so a script that was refused
/// has the same object a person would have been shown rather than a sentence to parse.
///
/// # Errors
///
/// The gate's own structured refusal, with the plan on its metadata. Nothing has been changed
/// when this returns an error, which is the sentence every one of those refusals ends with.
pub fn enforce(
    plan: &ChangePlan,
    given: Acknowledgements,
    interactive: bool,
    plan_value: &Value,
) -> Result<Acknowledgements, ErrorValue> {
    let settings = super::session::configured();
    let mut granted = given;
    for gate in outstanding_under(plan, given, &settings) {
        if interactive && settings.prompts() && ask(plan, &gate)? {
            // §19.4: the answer is an acknowledgement, and it is stored like the flag would be.
            granted = granted.granting(gate.flag);
            continue;
        }
        return Err(unprompted(spelled(&gate), &settings).with_metadata("plan", plan_value.clone()));
    }
    Ok(granted)
}

/// The refusal, saying so where it is the profile rather than the terminal that forbade asking
/// (Appendix H.4, §40.3).
///
/// An operator at a terminal who expected to be asked reads why they were not; without this the
/// refusal would look like a gate that never offered its question.
fn unprompted(refusal: ErrorValue, settings: &ChangeSettings) -> ErrorValue {
    match settings.profile() {
        Some(profile) if !profile.prompts() => {
            let note = format!(
                "the `{}` profile never prompts (v0.6 Appendix H.4), so every acknowledgement is \
                 given by its flag",
                profile.as_str()
            );
            let help = match refusal.help() {
                Some(existing) => format!("{existing}. {note}"),
                None => note,
            };
            refusal.with_help(help)
        }
        _ => refusal,
    }
}

/// The refusal with the rules' own sentences on it (§40.2).
///
/// §40.2 asks the refusal to carry "the rule's own reason rather than a generic question", and a
/// reader of a terminal sees a message and a help line rather than the metadata beneath them. So
/// the sentences travel in both: on the metadata, where a script reads them, and in the help,
/// where a person does.
fn spelled(gate: &Gate) -> ErrorValue {
    if gate.reasons.is_empty() {
        return gate.refusal.clone();
    }
    let reasons = gate.reasons.join("; ");
    let help = match gate.refusal.help() {
        Some(existing) => format!("{existing}. The rules that fired: {reasons}"),
        None => format!("The rules that fired: {reasons}"),
    };
    gate.refusal.clone().with_help(help)
}

/// Shows the gate and asks, with the rule's own reason rather than a generic question (§40.2).
fn ask(plan: &ChangePlan, gate: &Gate) -> Result<bool, ErrorValue> {
    eprintln!();
    eprintln!(
        "{} — {} risk",
        plan.id().short(),
        plan.risk().classify().as_str()
    );
    eprintln!("{}", plan.intent().text());
    eprintln!();
    for reason in &gate.reasons {
        eprintln!("  {reason}");
    }
    eprintln!();
    let answer = crate::kuang_permissions::read_answer(&format!(
        "Apply it? {} acknowledges this without asking. [y/N] ",
        gate.flag
    ))
    .unwrap_or_default();
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// The `--confirm` §40.3 requires outside a terminal, for a command that commits (§5.5, §5.6).
///
/// A gate is a question about *this plan*; this is the question about *acting at all*, and the
/// contracts that declare `confirmation: always` are the ones that ask it. At a terminal the
/// invocation is the confirmation, which is what keeps §40.1's normal path usable.
///
/// # Errors
///
/// `safety.confirmation_required` naming the spelling and saying that nothing was changed.
pub fn require_confirmation(
    spelling: &str,
    given: Acknowledgements,
    interactive: bool,
) -> Result<(), ErrorValue> {
    let settings = super::session::configured();
    if given.confirmed || (interactive && settings.prompts()) {
        return Ok(());
    }
    Err(unprompted(
        ErrorValue::new(
            ErrorCode::SafetyConfirmationRequired,
            format!("`{spelling}` is a commitment and was not confirmed"),
        )
        .with_help(format!(
            "nothing was changed. Write `{spelling} --confirm` to act; v0.6 §40.3 forbids a script \
         waiting for a prompt, so the confirmation is a flag"
        )),
        &settings,
    ))
}

/// The risk class as a word a refusal quotes, for a caller with the assessment and not the plan.
#[must_use]
pub fn class_of(plan: &ChangePlan) -> RiskClass {
    plan.risk().classify()
}

/// The plan value a refusal carries on its metadata (§40.3).
///
/// # Errors
///
/// Whatever the schema refused the record for.
pub fn machine_readable(plan: &ChangePlan) -> Result<Value, ErrorValue> {
    Ok(Value::Record(Arc::new(
        ono_change_core::value::plan_record(plan)?,
    )))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;
    use ono_change_core::{Intent, RiskAssessment};

    fn plan_with(findings: Vec<RiskFinding>) -> ChangePlan {
        ChangePlan::draft(
            Intent::new("restart service web-*", "gates"),
            "s-gates",
            jiff::Timestamp::UNIX_EPOCH,
        )
        .with_risk(RiskAssessment::of(findings))
    }

    fn whole_role() -> RiskFinding {
        RiskFinding::new(
            RiskDimension::Downtime,
            RiskClass::Critical,
            "risk.bulk.whole-role",
            "no healthy serving member of the role is excluded",
        )
    }

    #[test]
    fn should_refuse_a_whole_role_outage_as_a_bulk_guard_naming_its_flag() {
        let gates = outstanding_under(
            &plan_with(vec![whole_role()]),
            Acknowledgements::default(),
            &ChangeSettings::defaults(),
        );
        let gate = gates.first().expect("§40.2's worked gate is raised");
        assert_eq!(gate.flag, "--accept-service-outage");
        assert_eq!(
            gate.refusal.code().name(),
            "change.bulk_guard_failed",
            "risk.yaml declares §45's bulk guard error for the service-outage gate"
        );
        assert!(
            gate.refusal
                .help()
                .is_some_and(|help| help.contains("--accept-service-outage")),
            "the refusal names the flag that answers it: {:?}",
            gate.refusal.help()
        );
        assert!(
            gate.reasons
                .iter()
                .any(|reason| reason.contains("risk.bulk.whole-role")),
            "§40.2: the rule's own reason, not a generic question"
        );
    }

    #[test]
    fn should_keep_the_risk_refusal_when_the_outage_is_not_the_only_reason() {
        let reboot = RiskFinding::new(
            RiskDimension::RebootRequirement,
            RiskClass::Critical,
            "risk.reboot.required",
            "a reboot is required",
        );
        let gates = outstanding_under(
            &plan_with(vec![whole_role(), reboot]),
            Acknowledgements::default(),
            &ChangeSettings::defaults(),
        );
        let gate = gates.first().expect("the risk gate is raised");
        assert_eq!(gate.flag, "--accept-risk");
        assert_eq!(gate.refusal.code().name(), "change.risk_not_accepted");
    }
}
