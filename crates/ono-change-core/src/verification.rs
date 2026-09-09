//! Verification contracts and results (spec v0.6 §23).
//!
//! §2.14 is the invariant: *verification is separate from execution success*. A command that
//! returned zero has proved that it returned zero. Whether the intended state exists is a
//! different question, asked against the world afterwards, and §62.9 names answering it from an
//! exit code as a failure mode to avoid.
//!
//! Two rules shape the types:
//!
//! - §23.1 requires at least one contract on every plan that mutates. [`VerificationSet::is_empty`]
//!   is what `seal` checks, so a plan cannot become sealed without one.
//! - §23.5 forbids infinite waiting. [`VerificationContract`] carries a timeout that has no
//!   "none" — the constructor takes one, and [`VerificationStatus::Unknown`] is what a timeout
//!   produces when the contract says a timeout is not a failure.

use std::sync::Arc;

use jiff::Timestamp;
use ono_value::{Duration, Value};

use crate::id::{CheckId, PlanId};
use crate::vocab::vocabulary;

/// The timeout a contract gets when the caller does not choose one (§23.5).
pub const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

vocabulary! {
    /// How much a failing check says about the plan (§23.2).
    VerificationClass {
        Required => "required", "§23.2: a failing required postcondition makes the plan FAILED.";
        Advisory => "advisory", "§23.2: a failing advisory expectation may make the plan DEGRADED.";
        Observational => "observational", "§23.2: context only. It never changes the plan's state.";
    }
}

vocabulary! {
    /// The outcome of one check (§23.3).
    VerificationStatus {
        Passed => "passed", "§23.3: the observed state matched what was expected.";
        Failed => "failed", "§23.3: the observed state did not match.";
        Unknown => "unknown", "§23.3: the check could not be answered — a timeout, or a source that was not there. §23.5 forbids treating this as success.";
        Skipped => "skipped", "§23.3: the check was not run, because what it depends on did not happen.";
    }
}

vocabulary! {
    /// Which kind of equivalence a recovery verification claims (§25.1).
    ///
    /// §25.3 forbids the sentence "rollback successful" without a scope. These are the scopes,
    /// and a recovery verification reports each of them separately.
    EquivalenceDomain {
        PersistentState => "persistent-state", "§25.1: the bytes and metadata came back.";
        RuntimeState => "runtime-state", "§25.1: the service is running again, with new process identities, which is expected (§25.2).";
        ExternalSideEffect => "external-side-effect", "§25.1: what left the machine. Never recoverable by a local asset (§35.2).";
    }
}

/// One observable condition that says whether a plan achieved what it intended (§23.1).
#[derive(Debug, Clone, PartialEq)]
pub struct VerificationContract {
    id: CheckId,
    class: VerificationClass,
    subject: Arc<str>,
    expression: Arc<str>,
    expected: Option<Value>,
    timeout: std::time::Duration,
    timeout_is_failure: bool,
    equivalence: Option<EquivalenceDomain>,
}

impl VerificationContract {
    /// Declares a check of `expression` against `subject`, in `plan`.
    #[must_use]
    pub fn new(
        plan: &PlanId,
        class: VerificationClass,
        subject: impl Into<Arc<str>>,
        expression: impl Into<Arc<str>>,
    ) -> Self {
        let subject = subject.into();
        let expression = expression.into();
        Self {
            id: CheckId::of(plan, &subject, &expression),
            class,
            subject,
            expression,
            expected: None,
            timeout: DEFAULT_TIMEOUT,
            timeout_is_failure: class == VerificationClass::Required,
            equivalence: None,
        }
    }

    /// States the value the check expects.
    #[must_use]
    pub fn expecting(mut self, expected: Value) -> Self {
        self.expected = Some(expected);
        self
    }

    /// Bounds how long the check may wait (§23.5).
    #[must_use]
    pub const fn within(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Declares that a timeout answers `UNKNOWN` rather than `FAILED` (§23.5, Appendix F).
    #[must_use]
    pub const fn timeout_is_unknown(mut self) -> Self {
        self.timeout_is_failure = false;
        self
    }

    /// Marks the check as evidence about one equivalence domain of a recovery (§25.1).
    #[must_use]
    pub const fn about(mut self, domain: EquivalenceDomain) -> Self {
        self.equivalence = Some(domain);
        self
    }

    /// The check's identity.
    #[must_use]
    pub const fn id(&self) -> &CheckId {
        &self.id
    }

    /// How much a failure says about the plan.
    #[must_use]
    pub const fn class(&self) -> VerificationClass {
        self.class
    }

    /// What the check is about.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The condition, as a person and a machine both read it.
    #[must_use]
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// The value the check expects, where one is stated.
    #[must_use]
    pub const fn expected(&self) -> Option<&Value> {
        self.expected.as_ref()
    }

    /// How long the check may wait (§23.5).
    #[must_use]
    pub const fn timeout(&self) -> std::time::Duration {
        self.timeout
    }

    /// The status a timeout produces for this check.
    #[must_use]
    pub const fn timeout_status(&self) -> VerificationStatus {
        if self.timeout_is_failure {
            VerificationStatus::Failed
        } else {
            VerificationStatus::Unknown
        }
    }

    /// The equivalence domain this check reports on, for a recovery verification (§25.1).
    #[must_use]
    pub const fn equivalence(&self) -> Option<EquivalenceDomain> {
        self.equivalence
    }

    /// The canonical text this contract contributes to a plan digest (§4.4).
    #[must_use]
    pub fn digest_text(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.id.as_str(),
            self.class.as_str(),
            self.subject,
            self.expression,
            self.timeout.as_millis(),
        )
    }
}

/// The result of running one contract (§23.3).
#[derive(Debug, Clone, PartialEq)]
pub struct VerificationResult {
    plan: PlanId,
    check: CheckId,
    class: VerificationClass,
    subject: Arc<str>,
    expression: Arc<str>,
    status: VerificationStatus,
    observed: Option<Value>,
    expected: Option<Value>,
    evidence: Vec<Arc<str>>,
    detail: Option<Arc<str>>,
    equivalence: Option<EquivalenceDomain>,
    equivalence_state: Option<crate::recovery::EquivalenceState>,
    at: Timestamp,
}

impl VerificationResult {
    /// Records that `contract` in `plan` answered `status` at `at`.
    #[must_use]
    pub fn new(
        plan: PlanId,
        contract: &VerificationContract,
        status: VerificationStatus,
        at: Timestamp,
    ) -> Self {
        Self {
            plan,
            check: contract.id().clone(),
            class: contract.class(),
            subject: Arc::from(contract.subject()),
            expression: Arc::from(contract.expression()),
            status,
            observed: None,
            expected: contract.expected().cloned(),
            evidence: Vec::new(),
            detail: None,
            equivalence: contract.equivalence(),
            equivalence_state: None,
            at,
        }
    }

    /// Records what happened to this subject, for a recovery verification (§25.2).
    ///
    /// It is a different fact from [`VerificationResult::status`]. A restarted service's worker
    /// PIDs differ, and §25.2 reports that as `DIFFERENT / EXPECTED` rather than as a failure,
    /// because recovery never claimed to restore them — and no §23.3 status carries the
    /// difference between "did not come back" and "was never going to". A renderer without this
    /// would have to decide which one it was, which §25.3 is the sentence forbidding.
    #[must_use]
    pub const fn equivalent(mut self, state: crate::recovery::EquivalenceState) -> Self {
        self.equivalence_state = Some(state);
        self
    }

    /// Records what was actually seen.
    #[must_use]
    pub fn observing(mut self, observed: Value) -> Self {
        self.observed = Some(observed);
        self
    }

    /// Cites the evidence the observation rests on.
    #[must_use]
    pub fn citing(mut self, evidence: impl Into<Arc<str>>) -> Self {
        self.evidence.push(evidence.into());
        self
    }

    /// Adds the sentence a person reads beside the result.
    #[must_use]
    pub fn explained(mut self, detail: impl Into<Arc<str>>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// The plan the check belongs to.
    #[must_use]
    pub const fn plan(&self) -> &PlanId {
        &self.plan
    }

    /// The check's identity.
    #[must_use]
    pub const fn check(&self) -> &CheckId {
        &self.check
    }

    /// How much this result says about the plan.
    #[must_use]
    pub const fn class(&self) -> VerificationClass {
        self.class
    }

    /// What the check was about.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The condition that was checked, carried so a result can be read without its contract.
    ///
    /// §23.3 lists it beside `observed` and `expected` for the same reason both of those are
    /// there: a result that says only `FAILED` has told the operator nothing they can act on.
    #[must_use]
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// The outcome.
    #[must_use]
    pub const fn status(&self) -> VerificationStatus {
        self.status
    }

    /// What was seen, where anything was.
    #[must_use]
    pub const fn observed(&self) -> Option<&Value> {
        self.observed.as_ref()
    }

    /// What was expected, where the contract stated it.
    #[must_use]
    pub const fn expected(&self) -> Option<&Value> {
        self.expected.as_ref()
    }

    /// The evidence cited.
    #[must_use]
    pub fn evidence(&self) -> &[Arc<str>] {
        &self.evidence
    }

    /// The sentence a person reads.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// The equivalence domain this result reports on (§25.1).
    #[must_use]
    pub const fn equivalence(&self) -> Option<EquivalenceDomain> {
        self.equivalence
    }

    /// What happened to this subject, for a recovery verification (§25.2).
    #[must_use]
    pub const fn equivalence_state(&self) -> Option<crate::recovery::EquivalenceState> {
        self.equivalence_state
    }

    /// When it was observed.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }
}

/// A plan's verification contracts, and the verdict its results compose to (§23.2, §4.8).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VerificationSet {
    contracts: Vec<VerificationContract>,
}

impl VerificationSet {
    /// An empty set — which §23.1 forbids on a sealed plan that mutates.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            contracts: Vec::new(),
        }
    }

    /// Builds a set over `contracts`.
    #[must_use]
    pub const fn of(contracts: Vec<VerificationContract>) -> Self {
        Self { contracts }
    }

    /// Adds a contract.
    #[must_use]
    pub fn with(mut self, contract: VerificationContract) -> Self {
        self.contracts.push(contract);
        self
    }

    /// Every contract.
    #[must_use]
    pub fn contracts(&self) -> &[VerificationContract] {
        &self.contracts
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty()
    }

    /// Whether at least one contract is `REQUIRED`, which is what §23.1 asks a plan to carry.
    #[must_use]
    pub fn has_required(&self) -> bool {
        self.contracts
            .iter()
            .any(|contract| contract.class() == VerificationClass::Required)
    }

    /// The longest any single check may wait (§23.5).
    #[must_use]
    pub fn budget(&self) -> std::time::Duration {
        self.contracts
            .iter()
            .map(VerificationContract::timeout)
            .max()
            .unwrap_or(std::time::Duration::ZERO)
    }

    /// The plan-level verdict `results` compose to (§4.8, §23.2).
    ///
    /// The ordering is the specification's: a failing required check makes the plan `FAILED`
    /// however many advisory checks passed; an unknown required check is not a pass; and
    /// `DEGRADED` is the answer when the primary state exists and something else does not hold.
    #[must_use]
    pub fn verdict(results: &[VerificationResult]) -> Verdict {
        let mut verdict = Verdict::Verified;
        for result in results {
            let contribution = match (result.class(), result.status()) {
                (_, VerificationStatus::Passed | VerificationStatus::Skipped) => Verdict::Verified,
                (VerificationClass::Required, VerificationStatus::Failed) => Verdict::Failed,
                (VerificationClass::Required, VerificationStatus::Unknown) => Verdict::Degraded,
                (
                    VerificationClass::Advisory,
                    VerificationStatus::Failed | VerificationStatus::Unknown,
                ) => Verdict::Degraded,
                (VerificationClass::Observational, _) => Verdict::Verified,
            };
            verdict = verdict.worse_of(contribution);
        }
        verdict
    }

    /// The canonical text this set contributes to a plan digest (§4.4).
    #[must_use]
    pub fn digest_text(&self) -> String {
        let mut text = String::new();
        for contract in &self.contracts {
            text.push_str(&contract.digest_text());
            text.push('\u{1f}');
        }
        text
    }
}

vocabulary! {
    /// The plan-level outcome of verification (§4.8).
    Verdict {
        Verified => "verified", "§4.8: every required postcondition held.";
        Degraded => "degraded", "§4.8: the primary state exists and some expectation is violated or unknown.";
        Failed => "failed", "§4.8: a required postcondition failed.";
    }
}

impl Verdict {
    /// The worse of two verdicts.
    #[must_use]
    pub const fn worse_of(self, other: Self) -> Self {
        match (self, other) {
            (Verdict::Failed, _) | (_, Verdict::Failed) => Verdict::Failed,
            (Verdict::Degraded, _) | (_, Verdict::Degraded) => Verdict::Degraded,
            _ => Verdict::Verified,
        }
    }

    /// The lifecycle event this verdict raises (§4.8).
    #[must_use]
    pub const fn event(self) -> crate::state::LifecycleEvent {
        match self {
            Verdict::Verified => crate::state::LifecycleEvent::Verified,
            Verdict::Degraded => crate::state::LifecycleEvent::Degraded,
            Verdict::Failed => crate::state::LifecycleEvent::VerificationFailed,
        }
    }
}

/// A duration as the value model spells it, for records that carry a timeout.
#[must_use]
pub fn duration_value(duration: std::time::Duration) -> Value {
    let nanoseconds = i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX);
    Value::Duration(Duration::from_nanoseconds(nanoseconds))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    fn plan() -> PlanId {
        PlanId::derive(&["p"])
    }

    fn contract(class: VerificationClass, subject: &str) -> VerificationContract {
        VerificationContract::new(&plan(), class, subject, "state == running")
    }

    fn result(contract: &VerificationContract, status: VerificationStatus) -> VerificationResult {
        VerificationResult::new(plan(), contract, status, Timestamp::UNIX_EPOCH)
    }

    #[test]
    fn should_bound_every_check_without_being_asked() {
        let check = contract(VerificationClass::Required, "nginx.service");
        assert!(
            check.timeout() > std::time::Duration::ZERO,
            "§23.5: infinite waiting is forbidden, so there is no unbounded default"
        );
    }

    #[test]
    fn should_fail_the_plan_when_a_required_check_fails() {
        let required = contract(VerificationClass::Required, "nginx.service");
        let advisory = contract(VerificationClass::Advisory, "worker count");
        let results = vec![
            result(&required, VerificationStatus::Failed),
            result(&advisory, VerificationStatus::Passed),
        ];
        assert_eq!(
            VerificationSet::verdict(&results),
            Verdict::Failed,
            "§55.7 case 32: a required verification failure makes the plan FAILED"
        );
    }

    #[test]
    fn should_degrade_the_plan_when_only_an_advisory_check_fails() {
        let required = contract(VerificationClass::Required, "nginx.service");
        let advisory = contract(VerificationClass::Advisory, "worker count");
        let results = vec![
            result(&required, VerificationStatus::Passed),
            result(&advisory, VerificationStatus::Failed),
        ];
        assert_eq!(
            VerificationSet::verdict(&results),
            Verdict::Degraded,
            "§55.7 case 33: an advisory failure makes the plan DEGRADED"
        );
    }

    #[test]
    fn should_never_read_an_unknown_required_check_as_a_pass() {
        let required = contract(VerificationClass::Required, "nginx.service");
        let results = vec![result(&required, VerificationStatus::Unknown)];
        assert_ne!(
            VerificationSet::verdict(&results),
            Verdict::Verified,
            "§23.5 and Appendix F: a timeout must not be treated as success"
        );
    }

    #[test]
    fn should_let_an_observational_check_say_nothing_about_the_plan() {
        let required = contract(VerificationClass::Required, "nginx.service");
        let observed = contract(VerificationClass::Observational, "postgres connections");
        let results = vec![
            result(&required, VerificationStatus::Passed),
            result(&observed, VerificationStatus::Failed),
        ];
        assert_eq!(
            VerificationSet::verdict(&results),
            Verdict::Verified,
            "§23.2: observational checks provide context only (§23.4's example)"
        );
    }

    #[test]
    fn should_verify_a_plan_whose_every_required_check_passed() {
        let checks = [
            contract(VerificationClass::Required, "nginx.service"),
            contract(VerificationClass::Required, "listener :443"),
            contract(VerificationClass::Advisory, "worker count"),
        ];
        let results: Vec<VerificationResult> = checks
            .iter()
            .map(|check| result(check, VerificationStatus::Passed))
            .collect();
        assert_eq!(
            VerificationSet::verdict(&results),
            Verdict::Verified,
            "§55.7 case 34: the successful workflow verifies"
        );
    }

    #[test]
    fn should_answer_unknown_rather_than_failed_for_a_check_that_says_so() {
        let advisory = contract(VerificationClass::Advisory, "worker count");
        assert_eq!(
            advisory.timeout_status(),
            VerificationStatus::Unknown,
            "an advisory check that times out has not disproved anything"
        );
        let required = contract(VerificationClass::Required, "nginx.service");
        assert_eq!(
            required.timeout_status(),
            VerificationStatus::Failed,
            "a required postcondition that never became true did not hold"
        );
        assert_eq!(
            required.timeout_is_unknown().timeout_status(),
            VerificationStatus::Unknown,
            "unless the contract says a timeout is not evidence of failure"
        );
    }

    #[test]
    fn should_report_an_empty_set_as_the_gap_section_twenty_three_forbids() {
        assert!(
            VerificationSet::empty().is_empty(),
            "§23.1: a plan with a MUTATE action must carry at least one contract"
        );
        assert!(!VerificationSet::empty().has_required());
    }

    #[test]
    fn should_keep_a_recovery_result_scoped_to_the_domain_it_speaks_about() {
        let persistent = contract(VerificationClass::Required, "nginx.conf")
            .about(EquivalenceDomain::PersistentState);
        let outcome = result(&persistent, VerificationStatus::Passed);
        assert_eq!(
            outcome.equivalence(),
            Some(EquivalenceDomain::PersistentState),
            "§25.3: user-visible language must describe the verified scope"
        );
    }
}
