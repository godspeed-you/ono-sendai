//! Revalidation immediately before PREPARE (spec v0.6 §7.3, §7.4, §2.4, §2.7).
//!
//! §7.3: *"Immediately before PREPARE, Ono re-resolves all targets and preconditions. If material
//! drift exists, default behavior is `plan.drift_detected` and no mutation occurs."* Three things
//! follow from that sentence and are enforced here:
//!
//! - **Targets are re-resolved, not only preconditions.** Every frozen target is checked for
//!   existence, whether or not an action declared a precondition about it. §55.2 case 7 — a
//!   service target that disappeared — is otherwise invisible to a plan that only declared a
//!   content digest.
//! - **A fact nobody could observe blocks.** §2.4 forbids promoting unknown to expected, so
//!   [`ono_change_core::DriftVerdict::Unknown`] stops the apply exactly as material drift does.
//! - **Nothing here changes anything.** [`revalidate`] takes the plan by reference and observation
//!   by closure. There is no path from this module to the system it is asking about, which is
//!   what makes "no mutation occurs" a property rather than a promise.
//!
//! §7.4's tolerance is a contract, not a guess: a [`ono_change_core::Precondition`] is material
//! unless a provider declared otherwise, so §55.2 case 8's moving CPU number passes only because
//! the provider said it may.

use std::sync::Arc;

use ono_change_core::{
    ChangePlan, DriftFinding, DriftVerdict, FrozenTarget, PlanId, Precondition, PreconditionKind,
    error,
};
use ono_value::{ErrorValue, Value};

/// The field name a synthesised target existence check is recorded under (§7.3).
pub const EXISTENCE_FIELD: &str = "exists";

/// What revalidation concluded about a plan as a whole (§7.3, §7.4).
///
/// The order is the order of severity, so a report over many facts can answer with the worst thing
/// it found rather than with a list the caller has to reduce itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DriftVerdictSummary {
    /// Every fact still holds. §7.3 lets preparation begin.
    Clear,
    /// Something moved, and every move was contract-declared as harmless (§7.4).
    Tolerated,
    /// A fact could not be observed. §2.4 makes that a reason to stop, not a pass.
    Unobservable,
    /// A material fact moved. §7.3 stops the apply.
    Material,
    /// A frozen target is no longer the object the plan resolved (§7.1, §55.2 case 7).
    TargetChanged,
}

impl DriftVerdictSummary {
    /// Whether this conclusion stops the plan before anything is prepared (§7.3).
    #[must_use]
    pub const fn blocks_apply(self) -> bool {
        !matches!(self, DriftVerdictSummary::Clear | DriftVerdictSummary::Tolerated)
    }
}

/// What revalidation found across a whole plan (§7.3).
#[derive(Debug, Clone)]
pub struct DriftReport {
    findings: Vec<DriftFinding>,
    changed_targets: Vec<Arc<str>>,
    targets_checked: usize,
    preconditions_checked: usize,
}

impl DriftReport {
    /// Every precondition that did not simply hold, in the order they were checked.
    ///
    /// Tolerated moves are included. §7.4 makes them harmless rather than invisible, and an
    /// operator reading `explain` after a refusal elsewhere wants to see what else moved.
    #[must_use]
    pub fn findings(&self) -> &[DriftFinding] {
        &self.findings
    }

    /// The findings that stop the apply — material drift and unobservable facts (§7.3, §2.4).
    #[must_use]
    pub fn blocking(&self) -> Vec<&DriftFinding> {
        self.findings
            .iter()
            .filter(|finding| finding.verdict().blocks_apply())
            .collect()
    }

    /// The frozen targets that are no longer the objects the plan resolved (§7.1).
    #[must_use]
    pub fn changed_targets(&self) -> Vec<&str> {
        self.changed_targets.iter().map(AsRef::as_ref).collect()
    }

    /// How many targets were re-resolved (§7.3).
    #[must_use]
    pub const fn targets_checked(&self) -> usize {
        self.targets_checked
    }

    /// How many declared preconditions were re-checked (§7.2, §7.3).
    #[must_use]
    pub const fn preconditions_checked(&self) -> usize {
        self.preconditions_checked
    }

    /// Whether every fact still holds exactly as the plan froze it.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.findings.is_empty() && self.changed_targets.is_empty()
    }

    /// Whether §7.3 stops the plan here.
    #[must_use]
    pub fn blocks_apply(&self) -> bool {
        self.verdict().blocks_apply()
    }

    /// The worst thing revalidation found.
    #[must_use]
    pub fn verdict(&self) -> DriftVerdictSummary {
        if !self.changed_targets.is_empty() {
            return DriftVerdictSummary::TargetChanged;
        }
        self.findings
            .iter()
            .map(|finding| match finding.verdict() {
                DriftVerdict::Unchanged => DriftVerdictSummary::Clear,
                DriftVerdict::Tolerated => DriftVerdictSummary::Tolerated,
                DriftVerdict::Unknown => DriftVerdictSummary::Unobservable,
                DriftVerdict::Material => DriftVerdictSummary::Material,
            })
            .max()
            .unwrap_or(DriftVerdictSummary::Clear)
    }

    /// The refusal §7.3 raises, or `None` where preparation may begin.
    ///
    /// A target that disappeared is `change.target_changed` rather than `change.plan_drift_detected`
    /// (§55.2 case 7): the plan did not merely resolve against state that has since moved, it
    /// resolved against an object that is no longer there, and `rebase` is a different answer from
    /// re-planning.
    #[must_use]
    pub fn refusal(&self, plan: &PlanId) -> Option<ErrorValue> {
        if let Some(first) = self.changed_targets.first() {
            let rest = self.changed_targets.len().saturating_sub(1);
            let detail = if rest == 0 {
                "§7.1: the plan froze this object's identity, and re-resolution no longer finds \
                 it. Nothing was changed."
                    .to_owned()
            } else {
                format!(
                    "§7.1: the plan froze this object's identity, and re-resolution no longer \
                     finds it, nor {rest} other target(s) of this plan. Nothing was changed."
                )
            };
            return Some(error::target_changed(first, &detail));
        }
        if !self.blocks_apply() {
            return None;
        }
        let moved: Vec<(String, String, String)> = self
            .blocking()
            .into_iter()
            .map(|finding| {
                (
                    finding.subject().to_owned(),
                    finding.field().to_owned(),
                    describe(finding),
                )
            })
            .collect();
        Some(error::drift_detected(plan, &moved))
    }
}

/// Re-resolves every target and every precondition of `plan` against `observe` (§7.3).
///
/// `observe` answers with what the world says about one precondition, or `None` where it could not
/// find out. `None` is not a pass: §2.4 keeps unknown unknown, and [`DriftReport::blocks_apply`]
/// says so.
///
/// The plan is borrowed and nothing is written. §7.3's "no mutation occurs" and §2.1's
/// "planning is side-effect free" are both properties of this signature.
#[must_use]
pub fn revalidate(
    plan: &ChangePlan,
    observe: &dyn Fn(&Precondition) -> Option<Value>,
) -> DriftReport {
    let mut findings = Vec::new();
    let mut changed_targets = Vec::new();
    let mut preconditions_checked = 0;

    for target in plan.targets() {
        let declared = declared_existence(plan, target);
        let check = declared
            .cloned()
            .unwrap_or_else(|| existence_precondition(target));
        let observed = observe(&check);
        let verdict = check.check(observed.as_ref());
        if declared.is_some() {
            preconditions_checked += 1;
        }
        match verdict {
            DriftVerdict::Unchanged | DriftVerdict::Tolerated => {}
            DriftVerdict::Material => changed_targets.push(Arc::from(target.identity())),
            DriftVerdict::Unknown => {
                findings.push(DriftFinding::new(&check, verdict, observed));
            }
        }
    }

    for action in plan.actions() {
        for precondition in action.preconditions() {
            if is_target_existence(plan, precondition) {
                continue;
            }
            preconditions_checked += 1;
            let observed = observe(precondition);
            let verdict = precondition.check(observed.as_ref());
            if verdict != DriftVerdict::Unchanged {
                findings.push(DriftFinding::new(precondition, verdict, observed));
            }
        }
    }

    DriftReport {
        findings,
        changed_targets,
        targets_checked: plan.targets().len(),
        preconditions_checked,
    }
}

/// The existence check a plan already declares about `target`, where an action declared one.
fn declared_existence<'plan>(
    plan: &'plan ChangePlan,
    target: &FrozenTarget,
) -> Option<&'plan Precondition> {
    plan.actions().iter().flat_map(|action| action.preconditions()).find(|precondition| {
        precondition.kind() == PreconditionKind::Existence
            && (precondition.subject() == target.identity()
                || precondition.subject() == target.label())
    })
}

/// Whether this precondition is the one the target loop already checked.
fn is_target_existence(plan: &ChangePlan, precondition: &Precondition) -> bool {
    precondition.kind() == PreconditionKind::Existence
        && plan.targets().iter().any(|target| {
            precondition.subject() == target.identity() || precondition.subject() == target.label()
        })
}

/// The existence check §7.3 makes of a target whose plan declared none.
///
/// §7.3 re-resolves *all targets*, and a plan that declared only a content digest has still frozen
/// an object identity. Synthesising the check is what makes "the service is gone" an answer rather
/// than an absence.
fn existence_precondition(target: &FrozenTarget) -> Precondition {
    Precondition::new(
        PreconditionKind::Existence,
        target.identity(),
        EXISTENCE_FIELD,
        Value::Bool(true),
    )
    .explained("§7.1: the plan froze this object's identity, and apply re-resolves it")
}

/// The sentence a refusal shows for one finding.
fn describe(finding: &DriftFinding) -> String {
    match finding.verdict() {
        DriftVerdict::Unknown => {
            "could not be observed, and §2.4 does not promote unknown to expected".to_owned()
        }
        _ => format!(
            "was {} at seal and is {} now",
            ono_change_core::value_text(finding.expected()),
            finding
                .observed()
                .map_or_else(|| "unobservable".to_owned(), ono_change_core::value_text)
        ),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use std::cell::RefCell;

    use jiff::Timestamp;
    use ono_change_core::{
        ActionRole, Execution, Idempotency, Intent, PlanAction, PlanFragment, VerificationClass,
        VerificationContract,
    };
    use ono_core::ErrorCode;

    use super::*;
    use crate::builder::PlanBuilder;
    use crate::freeze::ServiceTarget;

    fn instant() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    fn later() -> Timestamp {
        Timestamp::from_second(60).expect("a valid instant")
    }

    fn plan_with(preconditions: Vec<Precondition>) -> ChangePlan {
        let builder = PlanBuilder::for_intent(
            Intent::new("restart nginx", "plan restart service nginx"),
            "session-1",
            instant(),
        );
        let id = builder.plan_id().clone();
        let mut action = PlanAction::new(
            &id,
            1,
            ActionRole::Mutate,
            "restart nginx.service",
            Execution::ProviderAction {
                provider: Arc::from("ono.service.systemd"),
                operation: Arc::from("ono.service.restart"),
                arguments: Vec::new(),
            },
        )
        .on("systemd:nginx.service")
        .with_idempotency(Idempotency::Idempotent);
        for precondition in preconditions {
            action = action.requiring(precondition);
        }
        let fragment = PlanFragment::empty().acting(action).verifying(
            VerificationContract::new(
                &id,
                VerificationClass::Required,
                "systemd:nginx.service",
                "state == running",
            )
            .expecting(Value::string("running")),
        );
        builder
            .contributing(&fragment)
            .expect("a fragment is accepted")
            .resolve(vec![
                ServiceTarget::new("systemd", "nginx.service")
                    .freeze()
                    .expect("a namespaced unit freezes"),
            ])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals")
    }

    fn digest_precondition() -> Precondition {
        Precondition::new(
            PreconditionKind::ContentDigest,
            "/etc/nginx/nginx.conf",
            "sha256",
            Value::string("abc123"),
        )
    }

    /// An observer that answers the target's existence and one named field.
    fn observer(alive: bool, answers: Vec<(&'static str, Option<Value>)>) -> impl Fn(&Precondition) -> Option<Value> {
        move |precondition: &Precondition| {
            if precondition.kind() == PreconditionKind::Existence {
                return Some(Value::Bool(alive));
            }
            answers
                .iter()
                .find(|(field, _)| *field == precondition.field())
                .and_then(|(_, answer)| answer.clone())
        }
    }

    #[test]
    fn should_let_preparation_begin_when_every_frozen_fact_still_holds() {
        let plan = plan_with(vec![digest_precondition()]);
        let report = revalidate(
            &plan,
            &observer(true, vec![("sha256", Some(Value::string("abc123")))]),
        );
        assert!(report.is_clear(), "§7.3: nothing moved, so nothing blocks");
        assert_eq!(report.verdict(), DriftVerdictSummary::Clear);
        assert!(report.refusal(plan.id()).is_none());
    }

    #[test]
    fn should_refuse_when_a_material_fact_moved_after_the_seal() {
        let plan = plan_with(vec![digest_precondition()]);
        let report = revalidate(
            &plan,
            &observer(true, vec![("sha256", Some(Value::string("def456")))]),
        );
        let refusal = report
            .refusal(plan.id())
            .expect("§7.3 refuses when material drift exists");
        assert_eq!(
            refusal.code(),
            ErrorCode::ChangePlanDriftDetected,
            "§55.2 case 6: a file changed after seal means apply refuses"
        );
    }

    #[test]
    fn should_name_every_fact_that_moved_in_the_refusal() {
        let plan = plan_with(vec![
            digest_precondition(),
            Precondition::new(
                PreconditionKind::Version,
                "nginx",
                "version",
                Value::string("1.24"),
            ),
        ]);
        let report = revalidate(
            &plan,
            &observer(
                true,
                vec![
                    ("sha256", Some(Value::string("def456"))),
                    ("version", Some(Value::string("1.26"))),
                ],
            ),
        );
        let refusal = report.refusal(plan.id()).expect("§7.3 refuses");
        let drift = refusal
            .metadata()
            .get("drift")
            .cloned()
            .expect("§7.3's refusal names what moved");
        let Value::List(items) = drift else {
            panic!("the drift list is a list");
        };
        assert_eq!(
            items.len(),
            2,
            "§7.3: the refusal names every fact that moved, not the first one"
        );
    }

    #[test]
    fn should_change_nothing_about_the_plan_when_it_revalidates() {
        let plan = plan_with(vec![digest_precondition()]);
        let before = plan.clone();
        let _report = revalidate(
            &plan,
            &observer(true, vec![("sha256", Some(Value::string("def456")))]),
        );
        assert_eq!(
            plan, before,
            "§7.3: material drift stops execution before anything is prepared or mutated"
        );
        assert!(plan.digest_holds(), "§4.4: the seal is untouched by revalidation");
    }

    #[test]
    fn should_only_observe_and_never_act_while_revalidating() {
        let plan = plan_with(vec![digest_precondition()]);
        let seen = RefCell::new(Vec::new());
        let observe = |precondition: &Precondition| {
            seen.borrow_mut().push(precondition.field().to_owned());
            Some(Value::Bool(true))
        };
        let report = revalidate(&plan, &observe);
        assert_eq!(
            seen.borrow().len(),
            report.targets_checked() + report.preconditions_checked(),
            "§2.1: revalidation asks and does not act, so every call is an observation"
        );
    }

    #[test]
    fn should_let_a_contract_declared_tolerance_through() {
        // §7.4's own example, and §55.2 case 8: CPU usage moves while a restart is planned.
        let cpu = Precondition::new(
            PreconditionKind::Field,
            "systemd:nginx.service",
            "cpu",
            Value::Float(2.0),
        )
        .tolerant()
        .explained("cpu usage does not invalidate a restart");
        let plan = plan_with(vec![cpu]);
        let report = revalidate(&plan, &observer(true, vec![("cpu", Some(Value::Float(41.0)))]));
        assert_eq!(
            report.verdict(),
            DriftVerdictSummary::Tolerated,
            "§55.2 case 8: non-material CPU drift leaves the plan valid"
        );
        assert!(!report.blocks_apply());
        assert!(report.refusal(plan.id()).is_none());
    }

    #[test]
    fn should_still_report_a_tolerated_move_so_explain_can_show_it() {
        let cpu = Precondition::new(
            PreconditionKind::Field,
            "systemd:nginx.service",
            "cpu",
            Value::Float(2.0),
        )
        .tolerant();
        let plan = plan_with(vec![cpu]);
        let report = revalidate(&plan, &observer(true, vec![("cpu", Some(Value::Float(41.0)))]));
        assert_eq!(
            report.findings().len(),
            1,
            "§7.4 makes a move harmless, not invisible"
        );
    }

    #[test]
    fn should_block_when_a_precondition_could_not_be_observed() {
        let plan = plan_with(vec![digest_precondition()]);
        let report = revalidate(&plan, &observer(true, vec![("sha256", None)]));
        assert_eq!(
            report.verdict(),
            DriftVerdictSummary::Unobservable,
            "§2.4: unknown MUST NOT be silently promoted to expected"
        );
        assert!(
            report.blocks_apply(),
            "a precondition nobody could check has not held (§2.4)"
        );
        let refusal = report.refusal(plan.id()).expect("§7.3 refuses");
        assert_eq!(refusal.code(), ErrorCode::ChangePlanDriftDetected);
    }

    #[test]
    fn should_refuse_with_target_changed_when_a_service_target_disappeared() {
        let plan = plan_with(vec![digest_precondition()]);
        let report = revalidate(
            &plan,
            &observer(false, vec![("sha256", Some(Value::string("abc123")))]),
        );
        let refusal = report
            .refusal(plan.id())
            .expect("§55.2 case 7 refuses when a target disappeared");
        assert_eq!(
            refusal.code(),
            ErrorCode::ChangeTargetChanged,
            "§55.2 case 7: a service target that disappeared means apply refuses"
        );
        assert_eq!(
            report.changed_targets(),
            vec!["systemd:nginx.service"],
            "§7.1: the refusal names the object that is no longer there"
        );
    }

    #[test]
    fn should_prefer_the_missing_target_over_the_drift_it_also_caused() {
        let plan = plan_with(vec![digest_precondition()]);
        let report = revalidate(
            &plan,
            &observer(false, vec![("sha256", Some(Value::string("def456")))]),
        );
        assert_eq!(
            report.verdict(),
            DriftVerdictSummary::TargetChanged,
            "a target that vanished is a different answer from a file that moved (§7.1, §7.3)"
        );
    }

    #[test]
    fn should_re_resolve_a_target_even_when_no_action_declared_a_precondition_about_it() {
        let plan = plan_with(Vec::new());
        let report = revalidate(&plan, &observer(false, Vec::new()));
        assert_eq!(
            report.targets_checked(),
            1,
            "§7.3: apply re-resolves all targets, not only the ones with declared preconditions"
        );
        assert_eq!(report.verdict(), DriftVerdictSummary::TargetChanged);
    }

    #[test]
    fn should_block_when_a_target_cannot_be_re_resolved_at_all() {
        let plan = plan_with(Vec::new());
        let report = revalidate(&plan, &|_| None);
        assert_eq!(
            report.verdict(),
            DriftVerdictSummary::Unobservable,
            "§2.4: a target Ono could not look for has not been found"
        );
        assert!(report.blocks_apply());
    }

    #[test]
    fn should_use_the_declared_existence_check_rather_than_synthesise_a_second_one() {
        let declared = Precondition::new(
            PreconditionKind::Existence,
            "systemd:nginx.service",
            "unit-file",
            Value::string("/lib/systemd/system/nginx.service"),
        );
        let plan = plan_with(vec![declared]);
        let report = revalidate(
            &plan,
            &|precondition: &Precondition| match precondition.field() {
                "unit-file" => Some(Value::string("/lib/systemd/system/nginx.service")),
                _ => None,
            },
        );
        assert!(
            report.is_clear(),
            "§7.2: a provider's own existence check is the one that runs"
        );
        assert_eq!(
            report.preconditions_checked(),
            1,
            "the declared check must not be counted twice"
        );
    }

    #[test]
    fn should_order_its_verdicts_by_how_much_they_stop() {
        assert!(DriftVerdictSummary::Clear < DriftVerdictSummary::Tolerated);
        assert!(DriftVerdictSummary::Tolerated < DriftVerdictSummary::Unobservable);
        assert!(DriftVerdictSummary::Unobservable < DriftVerdictSummary::Material);
        assert!(DriftVerdictSummary::Material < DriftVerdictSummary::TargetChanged);
        assert!(!DriftVerdictSummary::Clear.blocks_apply());
        assert!(!DriftVerdictSummary::Tolerated.blocks_apply());
        assert!(DriftVerdictSummary::Unobservable.blocks_apply());
    }
}
