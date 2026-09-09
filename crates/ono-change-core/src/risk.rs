//! The risk model, which is a separate axis from protection (spec v0.6 §19).
//!
//! §19.1 and §10.4 say the same thing from two sides: a strongly protected plan can still be
//! dangerous, and a plan nothing protects can be trivial. Restoring a root snapshot is
//! `PROTECTED` and needs a reboot; `plan get process` changes nothing at all. So risk is computed
//! from its own dimensions and never from the protection level.
//!
//! §19.2 says the classes are rule-based, not AI-generated, and §62.11 repeats it. Every class
//! here therefore comes out of [`RiskAssessment::classify`] — a fold over dimensions a rule
//! contributed — and there is no path by which a model, a plugin or a renderer raises or lowers
//! one.

use std::sync::Arc;

use crate::vocab::vocabulary;

vocabulary! {
    /// The canonical risk classes of §19.2.
    RiskClass {
        Low => "low", "§19.2: a bounded change whose consequences are understood and small.";
        Moderate => "moderate", "§19.2: a real change with understood consequences — replacing one config file and restarting one service (§19.3).";
        High => "high", "§19.2: a change whose consequences reach beyond its target, or which cannot be undone. §19.4 requires explicit acknowledgement.";
        Critical => "critical", "§19.2: a change that risks the availability of a whole role, or the operator's own link (§34.2). §19.4 requires explicit acknowledgement.";
        Unknown => "unknown", "§19.2: the risk could not be classified, which is not the same as low.";
    }
}

impl RiskClass {
    /// The stronger of two classes.
    ///
    /// `UNKNOWN` outranks `MODERATE` and is outranked by `HIGH`: a risk nobody could classify is
    /// worse than one that was classified as ordinary, and better than one that was classified as
    /// serious. §2.4's rule that unknown is never quietly promoted works in both directions here.
    #[must_use]
    pub fn max_of(self, other: Self) -> Self {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }

    const fn rank(self) -> u8 {
        match self {
            RiskClass::Low => 0,
            RiskClass::Moderate => 1,
            RiskClass::Unknown => 2,
            RiskClass::High => 3,
            RiskClass::Critical => 4,
        }
    }

    /// Whether §19.4 requires the operator to acknowledge this class before apply.
    #[must_use]
    pub const fn needs_acknowledgement(self) -> bool {
        matches!(self, RiskClass::High | RiskClass::Critical)
    }
}

vocabulary! {
    /// The dimensions §19.1 lists, each of which a rule may raise.
    RiskDimension {
        Scope => "scope", "§19.1: how much of the system the change reaches.";
        Privilege => "privilege", "§19.1: the authority the change needs (§43.3).";
        Downtime => "downtime", "§19.1: service interruption the change causes.";
        ExternalSideEffects => "external-side-effects", "§19.1: effects that leave the machine (§35.1).";
        Irreversibility => "irreversibility", "§19.1: effects nothing can undo (§2.13).";
        UnknownImpact => "unknown-impact", "§19.1: parts of the blast radius Ono cannot see (§9.6).";
        BulkCount => "bulk-count", "§19.1 and §28.3: how many objects one plan touches.";
        RemoteFanout => "remote-fanout", "§19.1 and §29.1: how many hosts one plan reaches.";
        RecoveryComplexity => "recovery-complexity", "§19.1: how hard recovery would be if it were needed.";
        RebootRequirement => "reboot-requirement", "§19.1 and §13.7: whether recovery or the change itself needs a reboot.";
    }
}

/// One rule's contribution to a plan's risk, with the sentence that justifies it (§19.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskFinding {
    dimension: RiskDimension,
    class: RiskClass,
    rule: Arc<str>,
    reason: Arc<str>,
}

impl RiskFinding {
    /// Records that `rule` found `class` risk in `dimension`, because of `reason`.
    ///
    /// `reason` is what §40.2 prints instead of "Are you sure?": a gate that cannot say why it
    /// is gating teaches the operator to type the flag without reading.
    #[must_use]
    pub fn new(
        dimension: RiskDimension,
        class: RiskClass,
        rule: impl Into<Arc<str>>,
        reason: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            dimension,
            class,
            rule: rule.into(),
            reason: reason.into(),
        }
    }

    /// The dimension the finding is about.
    #[must_use]
    pub const fn dimension(&self) -> RiskDimension {
        self.dimension
    }

    /// The class the rule assigned.
    #[must_use]
    pub const fn class(&self) -> RiskClass {
        self.class
    }

    /// The identity of the rule that found it, so `explain` can name it (§19.2).
    #[must_use]
    pub fn rule(&self) -> &str {
        &self.rule
    }

    /// The sentence a gate shows the operator.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// Every risk finding about one plan, and the class they compose to (§19.2, §19.4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RiskAssessment {
    findings: Vec<RiskFinding>,
    accepted_irreversible: bool,
    accepted_risk: bool,
}

impl RiskAssessment {
    /// An assessment with no findings — the class of a plan no rule objected to.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            findings: Vec::new(),
            accepted_irreversible: false,
            accepted_risk: false,
        }
    }

    /// Builds an assessment over `findings`.
    #[must_use]
    pub fn of(findings: Vec<RiskFinding>) -> Self {
        Self {
            findings,
            accepted_irreversible: false,
            accepted_risk: false,
        }
    }

    /// Adds a finding.
    #[must_use]
    pub fn with(mut self, finding: RiskFinding) -> Self {
        self.findings.push(finding);
        self
    }

    /// Records the operator's acknowledgement of the plan's risk class (§19.4).
    #[must_use]
    pub const fn risk_accepted(mut self) -> Self {
        self.accepted_risk = true;
        self
    }

    /// Records the operator's acknowledgement of the plan's irreversible actions (§19.4).
    #[must_use]
    pub const fn irreversible_accepted(mut self) -> Self {
        self.accepted_irreversible = true;
        self
    }

    /// Every finding.
    #[must_use]
    pub fn findings(&self) -> &[RiskFinding] {
        &self.findings
    }

    /// The plan's risk class: the strongest any rule found (§19.2).
    #[must_use]
    pub fn classify(&self) -> RiskClass {
        self.findings
            .iter()
            .map(RiskFinding::class)
            .fold(RiskClass::Low, RiskClass::max_of)
    }

    /// The findings at the plan's own class, which are the ones a gate shows (§40.2).
    #[must_use]
    pub fn leading(&self) -> Vec<&RiskFinding> {
        let class = self.classify();
        self.findings
            .iter()
            .filter(|finding| finding.class() == class)
            .collect()
    }

    /// Whether the plan carries an irreversible action any rule found (§19.4).
    #[must_use]
    pub fn has_irreversible(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.dimension() == RiskDimension::Irreversibility)
    }

    /// Whether the operator has acknowledged the plan's risk class.
    #[must_use]
    pub const fn is_risk_accepted(&self) -> bool {
        self.accepted_risk
    }

    /// Whether the operator has acknowledged the plan's irreversible actions.
    #[must_use]
    pub const fn is_irreversible_accepted(&self) -> bool {
        self.accepted_irreversible
    }

    /// The acknowledgements §19.4 still requires before this plan may be applied.
    ///
    /// An empty answer means the plan may proceed on `apply` alone, which §40.1 makes the normal
    /// case: a gate on every plan is a gate nobody reads.
    #[must_use]
    pub fn outstanding_acknowledgements(&self) -> Vec<RequiredAcknowledgement> {
        let mut needed = Vec::new();
        if self.classify().needs_acknowledgement() && !self.accepted_risk {
            needed.push(RequiredAcknowledgement::Risk(self.classify()));
        }
        if self.has_irreversible() && !self.accepted_irreversible {
            needed.push(RequiredAcknowledgement::Irreversible);
        }
        needed
    }

    /// The canonical text this assessment contributes to a plan digest (§4.4).
    ///
    /// The acknowledgements are part of it because §19.4 stores them in the sealed revision: a
    /// plan that has been accepted is not the same sealed object as one that has not.
    #[must_use]
    pub fn digest_text(&self) -> String {
        let mut text = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}",
            self.classify().as_str(),
            u8::from(self.accepted_risk),
            u8::from(self.accepted_irreversible),
        );
        for finding in &self.findings {
            use std::fmt::Write as _;
            let _ = write!(
                text,
                "{}:{}:{}\u{1f}",
                finding.rule(),
                finding.dimension().as_str(),
                finding.class().as_str()
            );
        }
        text
    }
}

/// An acknowledgement §19.4 requires before a plan may be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredAcknowledgement {
    /// The plan's risk class is `HIGH` or `CRITICAL` (§19.4, §40.2).
    Risk(RiskClass),
    /// The plan contains an action nothing can undo (§19.4).
    Irreversible,
}

impl RequiredAcknowledgement {
    /// The flag a non-interactive caller supplies to give this acknowledgement (§40.3).
    #[must_use]
    pub const fn flag(self) -> &'static str {
        match self {
            RequiredAcknowledgement::Risk(_) => "--accept-risk",
            RequiredAcknowledgement::Irreversible => "--accept-irreversible",
        }
    }
}

impl std::fmt::Display for RequiredAcknowledgement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequiredAcknowledgement::Risk(class) => {
                write!(formatter, "{} risk", class.as_str())
            }
            RequiredAcknowledgement::Irreversible => formatter.write_str("irreversible actions"),
        }
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

    fn finding(dimension: RiskDimension, class: RiskClass) -> RiskFinding {
        RiskFinding::new(dimension, class, "test.rule", "because the test says so")
    }

    #[test]
    fn should_take_the_strongest_class_any_rule_found() {
        let assessment = RiskAssessment::of(vec![
            finding(RiskDimension::Scope, RiskClass::Low),
            finding(RiskDimension::Downtime, RiskClass::High),
            finding(RiskDimension::Privilege, RiskClass::Moderate),
        ]);
        assert_eq!(
            assessment.classify(),
            RiskClass::High,
            "§19.2: the plan is as risky as its riskiest dimension"
        );
    }

    #[test]
    fn should_rank_unknown_above_moderate_and_below_high() {
        assert_eq!(
            RiskClass::Unknown.max_of(RiskClass::Moderate),
            RiskClass::Unknown,
            "a risk nobody could classify is worse than an ordinary one (§19.2)"
        );
        assert_eq!(
            RiskClass::Unknown.max_of(RiskClass::High),
            RiskClass::High,
            "and better than one a rule classified as serious"
        );
    }

    #[test]
    fn should_call_a_plan_with_no_findings_low() {
        assert_eq!(
            RiskAssessment::empty().classify(),
            RiskClass::Low,
            "§40.1: apply on a low plan is sufficient intent, so the empty case must be usable"
        );
    }

    #[test]
    fn should_require_an_acknowledgement_for_a_high_plan_and_not_for_a_moderate_one() {
        let moderate = RiskAssessment::of(vec![finding(RiskDimension::Scope, RiskClass::Moderate)]);
        assert!(
            moderate.outstanding_acknowledgements().is_empty(),
            "§40.1: a moderate plan applies on `apply` alone"
        );
        let high = RiskAssessment::of(vec![finding(RiskDimension::Downtime, RiskClass::High)]);
        assert_eq!(
            high.outstanding_acknowledgements(),
            vec![RequiredAcknowledgement::Risk(RiskClass::High)],
            "§19.4: HIGH and CRITICAL plans require explicit acknowledgement"
        );
    }

    #[test]
    fn should_require_a_separate_acknowledgement_for_an_irreversible_plan() {
        let assessment = RiskAssessment::of(vec![finding(
            RiskDimension::Irreversibility,
            RiskClass::Low,
        )]);
        assert_eq!(
            assessment.outstanding_acknowledgements(),
            vec![RequiredAcknowledgement::Irreversible],
            "§19.4: irreversibility gates on its own, whatever the class"
        );
    }

    #[test]
    fn should_clear_an_acknowledgement_once_it_has_been_given() {
        let assessment = RiskAssessment::of(vec![
            finding(RiskDimension::Downtime, RiskClass::Critical),
            finding(RiskDimension::Irreversibility, RiskClass::High),
        ])
        .risk_accepted()
        .irreversible_accepted();
        assert!(
            assessment.outstanding_acknowledgements().is_empty(),
            "an accepted plan proceeds; §19.4 stores the acceptance in the sealed revision"
        );
    }

    #[test]
    fn should_change_the_digest_when_an_acknowledgement_is_recorded() {
        let before = RiskAssessment::of(vec![finding(RiskDimension::Downtime, RiskClass::High)]);
        let after = before.clone().risk_accepted();
        assert_ne!(
            before.digest_text(),
            after.digest_text(),
            "§4.4 puts accepted risk overrides in the seal, so accepting one re-seals the plan"
        );
    }

    #[test]
    fn should_name_the_rule_and_the_reason_a_gate_will_show() {
        let assessment = RiskAssessment::of(vec![RiskFinding::new(
            RiskDimension::RemoteFanout,
            RiskClass::Critical,
            "risk.bulk.whole-role",
            "18/18 frontend nodes will restart",
        )]);
        let leading = assessment.leading();
        assert_eq!(leading.len(), 1);
        assert_eq!(
            leading[0].reason(),
            "18/18 frontend nodes will restart",
            "§40.2: the confirmation summarises the actual reason, not a generic question"
        );
    }
}
