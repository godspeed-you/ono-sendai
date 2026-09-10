//! `impact`, `protect`, `apply` and `verify` — the plan's lifecycle (spec v0.6 §5.4 – §5.7, §40).
//!
//! Each of the four resolves a plan reference through `ono-change-plan`'s store and hands the
//! sealed object to `ono-change-executor`. What is left here is what §55.7 leaves the shell: the
//! session's identity, the providers the actions run through, the terminal a gate is asked at,
//! and the ledger the lifecycle is written to.
//!
//! # Where the gates live
//!
//! §19.4's acknowledgements are recorded in the *sealed revision*, so `apply --accept-risk` is
//! not a flag the executor reads: it revises the plan, seals the revision with the acknowledgement
//! and applies that. §7.5's rule holds for the same reason it holds for `rebase` — the plan that
//! was refused stays exactly as it was, and the operator can still inspect it.
//!
//! # What streams and what is drawn
//!
//! §5.6 makes `apply`'s output a stream of `ono.action-result/1`, and Appendix E.4's progress
//! display is a *presentation* of that stream: it goes to the diagnostic stream through
//! `crate::report`, never into the values, so `apply a82f | to json` is the results and nothing
//! else (§2.14, v0.2 §13.1).

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{ChangePlan, PlanState, error};
use ono_change_executor::{ApplyRequest, Authority, PrepareRequest, VerifyRequest};
use ono_change_impact::derive::ImpactRequest;
use ono_change_protection::CoverageAnalysis;
use ono_change_protection::coverage::{CoverageRequest, mutation_domains};
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture};
use ono_pipeline::ValueStream;
use ono_value::{ActionResult, ActionStatus, ErrorValue, Value, ValueRef};

use super::gates::Acknowledgements;
use super::session::{ChangeState, change_session};

/// `impact` (§5.4, §9).
#[derive(Debug)]
pub struct Impact;

impl CommandImpl for Impact {
    fn id(&self) -> &str {
        "ono.change.impact"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("impact"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let reference = super::reference_of(ctx, "plan").await?;
            let state = change_session().await?;
            let plan = super::plan_of(&state, &reference)?;
            // §9.5 and §52.2: a deeper or an unbounded walk is a request the operator made, so
            // the graph is derived again over the topology as it is now. Without one the stored
            // graph is the answer, because §2.1 makes `impact` a read and re-walking would only
            // change what it costs.
            let graph = match (arguments.option("depth"), arguments.flag("all")) {
                (None, false) => plan.impact().clone(),
                (depth, all) => {
                    let spatial = crate::spatial::spatial_session().await;
                    let mut request = ImpactRequest::new(
                        spatial.index(),
                        plan.targets(),
                        plan.actions(),
                        Timestamp::now(),
                    );
                    if let Some(Value::Int(depth)) = depth {
                        request = request.to_depth(usize::try_from(*depth).unwrap_or(1));
                    }
                    if all {
                        request = request.accepting_cost().within_nodes(usize::MAX);
                    }
                    ono_change_impact::derive::derive(&request)
                }
            };
            let record = ono_change_core::value::impact_record(plan.id(), &graph)?;
            Ok(Outcome::Values(ValueStream::from_values([Value::Record(
                Arc::new(record),
            )])))
        })
    }
}

/// `protect` (§5.5, §18).
#[derive(Debug)]
pub struct Protect;

impl CommandImpl for Protect {
    fn id(&self) -> &str {
        "ono.change.protect"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("protect"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let given = Acknowledgements::of(&arguments);
            let interactive = super::is_interactive(ctx);
            let reference = super::reference_of(ctx, "plan").await?;
            // §5.5: protection is a real mutation of the storage or control plane and is visible
            // in history, so it asks the same question `apply` does before it acts.
            super::gates::require_confirmation("protect", given, interactive)?;
            let state = change_session().await?;
            let plan = super::plan_of(&state, &reference)?;
            let now = Timestamp::now();
            let analysis = analysis_of(&state, &plan);
            let mut request =
                PrepareRequest::new(&plan, analysis.actions(), state.providers(), now)
                    .with_authority(Authority::full());
            let prepared = ono_change_executor::prepare(&mut request);
            let created = match prepared {
                Ok(prepared) => prepared.assets().to_vec(),
                Err(refusal) => {
                    // Appendix F.1: what preparation already created is retained and reported,
                    // and the targets are untouched. The refusal names the row it stopped on.
                    for asset in request.created() {
                        let _ = state.store().put_asset(&attributed(asset, &plan));
                    }
                    return Err(refusal);
                }
            };
            let mut values = Vec::with_capacity(created.len());
            for asset in &created {
                let asset = attributed(asset, &plan);
                state.store().put_asset(&asset)?;
                values.push(Value::Record(Arc::new(
                    ono_change_core::value::asset_record(&asset)?,
                )));
            }
            record_protection(&plan, created.len(), now);
            Ok(Outcome::Values(ValueStream::from_values(values)))
        })
    }
}

/// `apply` (§5.6, §4.5, §4.7, §40).
#[derive(Debug)]
pub struct Apply;

impl CommandImpl for Apply {
    fn id(&self) -> &str {
        "ono.change.apply"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("apply"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let given = Acknowledgements::of(&arguments);
            let interactive = super::is_interactive(ctx);
            let reference = super::reference_of(ctx, "plan").await?;
            let providers = ctx.providers().clone();
            let handle = super::runtime_handle()?;
            let state = change_session().await?;
            let plan = super::plan_of(&state, &reference)?;
            let now = Timestamp::now();

            // §19.4, §40.2, §40.3: the gates, before anything is prepared and before the claim is
            // taken. A refused gate has changed nothing, and the plan travels on its metadata.
            let machine_readable = super::gates::machine_readable(&plan)?;
            super::gates::enforce(&plan, given, interactive, &machine_readable)?;
            // §5.6 and §40.3: a plan carrying any gate needs the commitment stated outside a
            // terminal. A plan with no gate applies on `apply` alone, which §40.1 keeps usable.
            if !super::gates::outstanding(&plan, Acknowledgements::default()).is_empty() {
                super::gates::require_confirmation("apply", given, interactive)?;
            }
            // §19.4 stores the acknowledgement in the sealed revision, so a plan that was just
            // acknowledged is a new revision of it. §7.5's rule applies here too: the revision
            // that was refused stays exactly as it was.
            let plan = accepted_revision(&state, plan, given, now)?;

            let analysis = analysis_of(&state, &plan);
            let revalidate = |action: &_| super::world::revalidate(&handle, &providers, action);
            let execute =
                |action: &_| super::world::execute_with(&handle, &providers, Some(&state), action);
            let observe = |contract: &_| super::world::observe(&handle, &providers, contract);
            let mut request = ApplyRequest::new(
                &plan,
                state.store(),
                state.session_id(),
                now,
                analysis.actions(),
                state.providers(),
                &revalidate,
                &execute,
                &observe,
            )
            .with_authority(Authority::full());
            let outcome = ono_change_executor::apply(&mut request);
            // §4.6 and §11.1: an asset that exists is a real object with a lifecycle, and
            // `get recovery` is where an operator finds it. Appendix F.1 keeps the ones a failed
            // preparation created, so they are written whatever the apply reached.
            for asset in outcome.assets() {
                let _ = state.store().put_asset(&attributed(asset, &plan));
            }
            record_apply(&plan, &outcome, now);
            report_progress(&plan, &outcome);

            let mut values = Vec::with_capacity(outcome.statuses().len());
            for (id, status) in outcome.statuses() {
                let action = plan.actions().iter().find(|action| action.id() == id);
                values.push(action_result(&plan, action, *status));
            }
            if let Some(refusal) = outcome.error() {
                return Err(refusal.clone().with_metadata("plan", machine_readable));
            }
            Ok(Outcome::Values(ValueStream::from_values(values)))
        })
    }
}

/// `verify` (§5.7, §23).
#[derive(Debug)]
pub struct Verify;

impl CommandImpl for Verify {
    fn id(&self) -> &str {
        "ono.change.verify"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("verify"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let reference = super::reference_of(ctx, "plan").await?;
            let providers = ctx.providers().clone();
            let handle = super::runtime_handle()?;
            let state = change_session().await?;
            let plan = super::plan_of(&state, &reference)?;
            let now = Timestamp::now();
            let observe = |contract: &_| super::world::observe(&handle, &providers, contract);
            let outcome = ono_change_executor::verify(&VerifyRequest {
                plan: &plan,
                now,
                observe: &observe,
            });
            let mut values = Vec::with_capacity(outcome.results().len());
            for result in outcome.results() {
                values.push(Value::Record(Arc::new(
                    ono_change_core::value::verification_record(result, result.expression())?,
                )));
            }
            Ok(Outcome::Values(ValueStream::from_values(values)))
        })
    }
}

/// The asset with the plan that created it recorded on it (§11.1's `source_plan`).
///
/// A recovery provider creates an asset over a persistence domain and does not know which plan
/// asked for it — §50.1 keeps that knowledge out of the provider. The shell does know, and §11.1
/// makes the attribution part of the asset's identity, which is what lets `get recovery --plan`
/// and §2.15's cleanup guard answer at all.
fn attributed(
    asset: &ono_change_core::RecoveryAsset,
    plan: &ChangePlan,
) -> ono_change_core::RecoveryAsset {
    match asset.source_plan() {
        Some(_) => asset.clone(),
        None => asset.clone().for_plan(plan.id().clone()),
    }
}

/// Appendix A over a plan that was already sealed (§10.3, §18.2).
///
/// `apply` and `protect` both need the PREPARE actions, and a sealed plan carries the coverage
/// *matrix* rather than the actions that would produce it. Recomputing them here is also what
/// §18.2 asks for: protection created earlier is reassessed at the moment it would be used.
#[must_use]
pub fn analysis_of(state: &ChangeState, plan: &ChangePlan) -> CoverageAnalysis {
    let policy = state.policy(Some(plan.protection_mode()));
    let mut request = CoverageRequest::new(state.providers(), &policy);
    for mutation in mutation_domains(plan.actions()) {
        request = request.mutating(mutation);
    }
    for target in plan.targets() {
        if target.schema() == ono_change_plan::freeze::FILE_SCHEMA {
            request = request.over(state.mounts().resolve(std::path::Path::new(target.label())));
        }
    }
    ono_change_protection::analyse(&request)
}

/// The sealed revision the acknowledgements belong to (§19.4).
///
/// A plan with nothing outstanding is returned unchanged, so the ordinary path writes no revision
/// and `get plan` does not fill up with revisions nobody asked for.
fn accepted_revision(
    state: &ChangeState,
    plan: ChangePlan,
    given: Acknowledgements,
    now: Timestamp,
) -> Result<ChangePlan, ErrorValue> {
    if plan.outstanding_acknowledgements().is_empty() {
        return Ok(plan);
    }
    let mut risk = plan.risk().clone();
    if given.risk || given.service_outage {
        risk = risk.risk_accepted();
    }
    if given.irreversible {
        risk = risk.irreversible_accepted();
    }
    let revised = plan.revise().with_risk(risk).seal(now)?;
    state.store().put(&revised)?;
    Ok(revised)
}

/// One `ono.action-result/1` for an action the executor settled (§5.6).
fn action_result(
    plan: &ChangePlan,
    action: Option<&ono_change_core::PlanAction>,
    status: ono_change_core::ActionStatus,
) -> Value {
    // The TARGET column answers "what did this touch", so it carries the object the action
    // changes rather than the plan and action identities: those are a fact about the plan, and a
    // `{plan: …, action: …}` map in the column an operator scans for a path or a unit name tells
    // them nothing they came to find. The two identities stay in the message.
    let target = action
        .and_then(ono_change_core::PlanAction::target)
        .and_then(|identity| {
            plan.targets()
                .iter()
                .find(|target| target.identity() == identity)
                .map(|target| ValueRef::name(target.label()))
        })
        .unwrap_or_else(|| ValueRef::name(plan.id().short()));
    let operation = action.map_or("ono.change.apply", ono_change_core::PlanAction::summary);
    // Appendix F.2: an outcome that could not be established is neither a success nor a failure,
    // and `skipped` is the honest word for an action nothing ran. Mapping `unknown` onto either
    // of the other two is exactly the guess §2.4 forbids.
    let mapped = match status {
        ono_change_core::ActionStatus::Succeeded => ActionStatus::Success,
        ono_change_core::ActionStatus::Failed => ActionStatus::Failed,
        _ => ActionStatus::Skipped,
    };
    ActionResult::new(target, operation, mapped)
        .changed(status.may_have_mutated())
        .with_message(&format!(
            "{} — {} (plan {}, action {})",
            action.map_or("action", ono_change_core::PlanAction::summary),
            status.as_str(),
            plan.id().short(),
            action.map_or("none", |action| action.id().as_str()),
        ))
        .into_value()
}

/// Appendix E.4's progress, on the diagnostic stream rather than in the values.
///
/// Appendix E.4 requires PREPARE, APPLY and VERIFY to stay separately visible so that no single
/// bar hides whether protection completed, and Appendix E.5 forbids presenting recovery as the
/// only next step. Both are `ono-change-render`'s to draw; this hands it the records.
fn report_progress(plan: &ChangePlan, outcome: &ono_change_executor::ApplyOutcome) {
    // A refusal that happened before anything was prepared has no progress to report, and
    // Appendix E.5's failure display would report the *plan's* history instead of this run's —
    // an already-applied plan would print "completed: 1 mutate action" beside a refusal that
    // reached no provider. §2.14: the structured error is the whole of what happened.
    if outcome.error().is_some() && !outcome.has_mutated() && outcome.assets().is_empty() {
        return;
    }
    // The record is built from the plan as it was *sealed*, where every action is `pending`.
    // Appendix E's PREPARE, APPLY and VERIFY lines count settled actions, so the statuses this
    // run reached are put back before it is rendered — otherwise the display reports the plan
    // and not the run, and says `pending` beside an action that has finished.
    let Ok(record) = ono_change_core::value::plan_record(&plan.clone().settled(outcome.statuses()))
    else {
        return;
    };
    let mut results = Vec::new();
    for result in outcome.verification() {
        if let Ok(record) = ono_change_core::value::verification_record(result, result.expression())
        {
            results.push(record);
        }
    }
    let width = 80;
    let lines = if outcome.is_success() {
        ono_change_render::apply_progress(&record, &results, width)
    } else {
        ono_change_render::apply_failure(&record, &results, width, super::render::charset())
    };
    let reporter = crate::report::Reporter::new(ono_render::Presentation::choose(
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
        &[],
    ));
    for line in lines {
        reporter.note(&line);
    }
}

/// Writes §22.1's `PlanProtected` and the assets it created into the v0.5 ledger.
fn record_protection(plan: &ChangePlan, assets: usize, now: Timestamp) {
    let mut lifecycle =
        ono_change_executor::PlanLifecycle::continuing(plan, crate::spatial::local_scope());
    lifecycle.protected(assets, now);
    let ledger = crate::temporal::session::writable_ledger();
    let _ = lifecycle.record(ledger.as_ref());
}

/// Writes §22.1's action and verification events into the v0.5 ledger (§22.1, §22.4).
fn record_apply(plan: &ChangePlan, outcome: &ono_change_executor::ApplyOutcome, now: Timestamp) {
    let mut lifecycle =
        ono_change_executor::PlanLifecycle::continuing(plan, crate::spatial::local_scope());
    // §22.1's `PlanProtected` belongs to the run that created the assets, and `apply` creates
    // them at §4.5 rather than leaving them to `protect`. Without this the only plan that ever
    // recorded protecting itself was one an operator had protected by hand.
    if !outcome.assets().is_empty() {
        lifecycle.protected(outcome.assets().len(), now);
    }
    for (id, status) in outcome.statuses() {
        if let Some(action) = plan.actions().iter().find(|action| action.id() == id) {
            lifecycle.action_started(action, now);
            lifecycle.action_settled(action, *status, now);
        }
    }
    for result in outcome.verification() {
        lifecycle.verification_observed(result, now);
    }
    lifecycle.verified(verdict_of(outcome.state()), now);
    let ledger = crate::temporal::session::writable_ledger();
    let _ = lifecycle.record(ledger.as_ref());
}

/// §4.8's verdict, read off the state the apply reached.
const fn verdict_of(state: PlanState) -> ono_change_core::Verdict {
    match state {
        PlanState::Verified => ono_change_core::Verdict::Verified,
        PlanState::Degraded => ono_change_core::Verdict::Degraded,
        _ => ono_change_core::Verdict::Failed,
    }
}

/// The refusal a command answers with where a plan is in a state it cannot serve (§4.1).
///
/// # Errors
///
/// `change.plan_not_sealed`, always: it is the sentence, not the decision.
pub fn not_sealed(plan: &ChangePlan) -> ErrorValue {
    error::plan_not_sealed(plan.id(), plan.state())
}
