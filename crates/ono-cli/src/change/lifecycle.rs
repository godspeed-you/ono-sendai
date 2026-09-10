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
use ono_change_executor::{ApplyRequest, PrepareRequest, VerifyRequest};
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
                    .with_authority(super::session::authority());
            let prepared = ono_change_executor::prepare(&mut request);
            let created = match prepared {
                Ok(prepared) => {
                    // §4.6: an optional asset that did not validate still exists, so it is
                    // recorded — as what it is, never counted as protection.
                    for shortfall in prepared.shortfall() {
                        if let Some(asset) = shortfall.asset() {
                            state.store().put_asset(&attributed(asset, &plan))?;
                        }
                    }
                    report_shortfall(prepared.shortfall());
                    prepared.assets().to_vec()
                }
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
            let given = super::gates::enforce(&plan, given, interactive, &machine_readable)?;
            // §5.6 and §40.3: a plan carrying any gate needs the commitment stated outside a
            // terminal. A plan with no gate applies on `apply` alone, which §40.1 keeps usable.
            if !super::gates::outstanding(&plan, Acknowledgements::default()).is_empty() {
                super::gates::require_confirmation("apply", given, interactive)?;
            }
            // §19.4 stores the acknowledgement in the sealed revision, so a plan that was just
            // acknowledged is a new revision of it. §7.5's rule applies here too: the revision
            // that was refused stays exactly as it was.
            let plan = accepted_revision(&state, plan, given, now)?;
            // §24.5 and §7.3: a recovery plan is gated on what it would discard as the world stands
            // now, and its actions go to the provider that owns the asset they restore from.
            let recovery = if plan.kind() == ono_change_core::PlanKind::Recovery {
                if given.newer_state_loss {
                    super::gates::require_confirmation("apply", given, interactive)?;
                }
                Some(super::recovery::current(
                    &state,
                    &plan,
                    given.newer_state_loss,
                    now,
                )?)
            } else {
                None
            };
            let acceptance = if given.newer_state_loss {
                ono_change_core::RestoreAcceptance::none().accepting_newer_state_loss()
            } else {
                ono_change_core::RestoreAcceptance::none()
            };

            let analysis = analysis_of(&state, &plan);
            // §2.3: the operator approved the coverage matrix the plan was sealed with. Where the
            // protection recomputed now cannot give a row that matrix showed as protected, running
            // would downgrade to unprotected execution without saying so.
            // §7.3 comes first: a plan whose world moved is refused as drift, by the executor's
            // revalidation, before anything is said about protection. A vanished target also has no
            // domain left to protect, and reporting that as a coverage shortfall would name the
            // wrong cause.
            // Only the mutating actions are the world. A PREPARE action is the protection, and its
            // revalidation failing is the protection being unavailable — which the check below
            // exists to refuse, rather than a drift that lets it be skipped.
            let drifted = plan
                .actions()
                .iter()
                .filter(|action| action.role() == ono_change_core::ActionRole::Mutate)
                .any(|action| {
                    // The executor's own test (§7.3): a blocking finding, or a revalidation that
                    // could not be answered. A finding the executor applies through must not
                    // make this check step aside, or neither of them refuses.
                    super::world::revalidate(&handle, &providers, action).map_or(true, |findings| {
                        findings
                            .iter()
                            .any(|finding| finding.verdict().blocks_apply())
                    })
                });
            let lost = if drifted {
                Vec::new()
            } else {
                ono_change_protection::coverage::protection_lost(
                    plan.protection(),
                    analysis.summary(),
                )
            };
            // §18.2: an asset `protect` made earlier may keep the promise, if it still captures
            // the state about to change. A stale one keeps it only with `--accept-stale-protection`
            // (§18.3's second option); nothing pretends it is a just-before-change point.
            let early = if lost.is_empty() {
                None
            } else {
                early_protection(&state, &plan, given.stale_protection)
            };
            let lost = if early.is_some() { Vec::new() } else { lost };
            if !lost.is_empty() {
                let stale = stale_early_protection(&state, &plan);
                let help_tail = if stale.is_empty() {
                    String::new()
                } else {
                    format!(
                        " An earlier asset exists but no longer captures the current state ({}); \
                         `--accept-stale-protection` applies on it anyway (§18.3)",
                        stale.join("; ")
                    )
                };
                // §2.3 and §4.5: the protection the plan showed cannot be created now, which is a
                // preparation that failed before its first mutation. The plan says so durably —
                // `prepare-failed` has no edge to APPLYING (§4.1) — and `rebase` is the way on.
                let _ = state.store().record_state(
                    plan.id(),
                    plan.revision(),
                    PlanState::PrepareFailed,
                );
                return Err(error::prepare_failed(
                    plan.id(),
                    "recovery asset creation",
                    &format!(
                        "nothing can create the protection the plan was sealed with for {}; it \
                         reaches {} where the plan showed {}",
                        lost.join(", "),
                        analysis.level(),
                        plan.protection().level()
                    ),
                    &[],
                )
                .with_help(format!(
                    "the plan was sealed showing {} protected, and nothing can protect it now. \
                     Nothing was changed. `rebase plan {}` shows what protection is available \
                     now (§7.5).{help_tail}",
                    lost.join(", "),
                    plan.id().short()
                )));
            }
            if let Some(note) = early {
                report_notes(&[note]);
            }
            // §22.2 and §22.3: the digest of every file target as it is immediately before
            // mutation, recorded on the plan's timeline so the state before and after compare.
            let before: Vec<(String, String)> = plan
                .targets()
                .iter()
                .filter(|target| target.schema() == ono_change_plan::freeze::FILE_SCHEMA)
                .filter_map(|target| {
                    std::fs::read(target.label()).ok().map(|bytes| {
                        (
                            target.identity().to_owned(),
                            ono_recovery_files::manifest::digest_of(&bytes),
                        )
                    })
                })
                .collect();
            // §22.2: immediately before mutation, a lightweight semantic checkpoint of the plan's
            // targets and the objects its impact reaches goes to the v0.5 ledger. It is written only
            // where the ledger is persistent — a session ledger dies with the session that could
            // not finish reading it — and it is independent of any storage snapshot.
            {
                let spatial = crate::spatial::spatial_session().await;
                let (objects, relations) = super::world::checkpoint_states(
                    &handle,
                    &providers,
                    &plan,
                    spatial.index(),
                    now,
                );
                drop(spatial);
                let ledger = crate::temporal::session::writable_ledger();
                let _ = ono_change_executor::events::checkpoint_before_mutation(
                    &plan,
                    &crate::spatial::local_scope(),
                    &objects,
                    &relations,
                    ledger.as_ref(),
                    now,
                );
            }
            let revalidate = |action: &_| super::world::revalidate(&handle, &providers, action);
            let execute = |action: &_| match &recovery {
                Some((_, assets)) => {
                    super::recovery::restore(&state, plan.id(), assets, &acceptance, action)
                }
                None => super::world::execute(&handle, &providers, action),
            };
            let clock = Timestamp::now;
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
            .with_authority(super::session::authority())
            .stamping_with(&clock);
            let outcome = ono_change_executor::apply(&mut request);
            // §4.6 and §11.1: an asset that exists is a real object with a lifecycle, and
            // `get recovery` is where an operator finds it. Appendix F.1 keeps the ones a failed
            // preparation created, so they are written whatever the apply reached.
            for asset in outcome.assets() {
                let _ = state.store().put_asset(&attributed(asset, &plan));
            }
            record_apply(&plan, &outcome, &before, now);
            report_progress(&plan, &outcome);
            report_shortfall(outcome.protection_shortfall());

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
            // `--timeout` caps every check of this run; a contract's own shorter timeout still wins.
            let cap = ctx
                .arguments()
                .evaluated(ctx.scope())
                .ok()
                .and_then(|arguments| arguments.option("timeout").cloned())
                .and_then(|value| value.as_duration().ok())
                .and_then(|cap| u64::try_from(cap.nanoseconds()).ok())
                .map(std::time::Duration::from_nanos);
            let observe = |contract: &ono_change_core::VerificationContract| match cap {
                Some(cap) if cap < contract.timeout() => {
                    super::world::observe(&handle, &providers, &contract.clone().within(cap))
                }
                _ => super::world::observe(&handle, &providers, contract),
            };
            let outcome = ono_change_executor::verify(&VerifyRequest {
                plan: &plan,
                now,
                observe: &observe,
            });
            // §23.3: the verdict is a lifecycle state of a plan that applied, and it is durable like
            // every other one. A plan that never applied has no verdict to record: checking it answers
            // what holds now and changes nothing about the plan.
            if plan.state().is_verdict() {
                let recovery = plan.kind() == ono_change_core::PlanKind::Recovery;
                let _ = state.store().record_state(
                    plan.id(),
                    plan.revision(),
                    outcome.state_for(recovery),
                );
            }
            let mut values = Vec::with_capacity(outcome.results().len());
            for result in outcome.results() {
                values.push(Value::Record(Arc::new(
                    ono_change_core::value::verification_record(result, result.expression())?,
                )));
            }
            if plan.kind() == ono_change_core::PlanKind::Recovery
                && let Ok(record) = ono_change_core::value::plan_record(&plan)
            {
                let results: Vec<_> = outcome
                    .results()
                    .iter()
                    .filter_map(|result| {
                        ono_change_core::value::verification_record(result, result.expression())
                            .ok()
                    })
                    .collect();
                for line in ono_change_render::recovery_verification(&record, &results, 80) {
                    eprintln!("{line}");
                }
            }
            // §23.3 and v0.2 §43: the per-check results are the answer, and a required check that
            // did not hold ends the stream as `change.verification_failed`, so a script reads the
            // results and still sees the failure in the exit status. An advisory check degrades
            // the plan and fails nothing (§23.2, §55.7 case 33).
            let failed: Vec<String> = outcome
                .results()
                .iter()
                .filter(|result| {
                    result.class() == ono_change_core::VerificationClass::Required
                        && result.status() != ono_change_core::VerificationStatus::Passed
                })
                .map(|result| result.expression().to_owned())
                .collect();
            let refusal =
                (!failed.is_empty()).then(|| error::verification_failed(plan.id(), &failed));
            Ok(Outcome::Values(ValueStream::spawn(
                ono_pipeline::PipelineConfig::new(),
                ono_pipeline::Boundedness::Bounded,
                move |sink| async move {
                    for value in values {
                        if sink.send(value).await.is_err() {
                            return;
                        }
                    }
                    if let Some(refusal) = refusal {
                        let _ = sink.fail(refusal).await;
                    }
                },
            )))
        })
    }
}

/// The earlier assets of `plan` that cover every file it targets (§18.2).
fn early_assets(state: &ChangeState, plan: &ChangePlan) -> Vec<ono_change_core::RecoveryAsset> {
    let Ok(ids) = state.store().assets_for(plan.id()) else {
        return Vec::new();
    };
    ids.iter()
        .filter_map(|id| state.store().get_asset(id).ok())
        .map(|asset| super::recovery::fresh_asset(state, asset))
        .filter(|asset| asset.is_usable())
        .filter(|asset| {
            plan.targets()
                .iter()
                .filter(|target| target.schema() == ono_change_plan::freeze::FILE_SCHEMA)
                .all(|target| asset.scope().covers_object(target.label()))
        })
        .collect()
}

/// An earlier asset that may stand for the protection the plan showed, and the note saying so.
fn early_protection(state: &ChangeState, plan: &ChangePlan, accept_stale: bool) -> Option<String> {
    early_assets(state, plan).into_iter().find_map(|asset| {
        let verdict = super::recovery::early_freshness(state, plan, &asset);
        let reference = state
            .store()
            .render_asset_reference(asset.id())
            .unwrap_or_else(|_| asset.id().short().to_owned());
        if verdict.is_fresh() {
            Some(format!(
                "protection rests on {reference}, made earlier by `protect` and still capturing \
                 the state about to change (§18.2)"
            ))
        } else if accept_stale
            && verdict.freshness() == ono_change_protection::freshness::Freshness::StaleAcceptable
        {
            Some(format!(
                "protection rests on {reference}, which is STALE and was accepted as such: {} \
                 (§18.3). It is not a just-before-change recovery point",
                verdict.detail()
            ))
        } else {
            None
        }
    })
}

/// The earlier assets that exist but no longer capture the current state, as a refusal names them.
fn stale_early_protection(state: &ChangeState, plan: &ChangePlan) -> Vec<String> {
    early_assets(state, plan)
        .into_iter()
        .filter_map(|asset| {
            let verdict = super::recovery::early_freshness(state, plan, &asset);
            (verdict.freshness() == ono_change_protection::freshness::Freshness::StaleAcceptable)
                .then(|| format!("{}: {}", asset.id().short(), verdict.detail()))
        })
        .collect()
}

/// Prints `lines` as the shell's notes.
fn report_notes(lines: &[String]) {
    let reporter = crate::report::Reporter::new(ono_render::Presentation::choose(
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
        &[],
    ));
    for line in lines {
        reporter.note(line);
    }
}

/// Says which optional protection did not come to be, and why (§4.6, §17.2).
///
/// `maximize` asks for protection it may not get, and that is allowed; getting less than was
/// asked for without a word is not. Each shortfall is a note naming the provider and the reason.
fn report_shortfall(shortfall: &[ono_change_executor::ProtectionShortfall]) {
    if shortfall.is_empty() {
        return;
    }
    let reporter = crate::report::Reporter::new(ono_render::Presentation::choose(
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
        &[],
    ));
    for missing in shortfall {
        reporter.note(&format!(
            "protection not obtained: {} ({}): {}",
            missing.summary(),
            missing.provider(),
            missing.reason().message()
        ));
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
    // §13.4 and §32.3: the mount table is what shows a recursive mutation reaching into another
    // filesystem, and a candidate that captures only the top one is not protecting the rest.
    let mut request = CoverageRequest::new(state.providers(), &policy).within(state.mounts());
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
    let mut lines = if outcome.is_success() {
        ono_change_render::apply_progress(&record, &results, width)
    } else {
        // Appendix E.5: the assets this run created are what `recover` can use, and the checks
        // are what failed. Each goes where the display expects it.
        let assets: Vec<_> = outcome
            .assets()
            .iter()
            .filter_map(|asset| ono_change_core::value::asset_record(asset).ok())
            .collect();
        ono_change_render::failure_display(
            &record,
            &assets,
            &results,
            width,
            super::render::charset(),
        )
    };
    // §25.3: a recovery that ran says which scope its verification established, and never more.
    if plan.kind() == ono_change_core::PlanKind::Recovery && !results.is_empty() {
        lines.extend(ono_change_render::recovery_verification(
            &record, &results, width,
        ));
    }
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
fn record_apply(
    plan: &ChangePlan,
    outcome: &ono_change_executor::ApplyOutcome,
    before: &[(String, String)],
    now: Timestamp,
) {
    let mut lifecycle =
        ono_change_executor::PlanLifecycle::continuing(plan, crate::spatial::local_scope());
    // §22.1's `PlanProtected` belongs to the run that created the assets, and `apply` creates
    // them at §4.5 rather than leaving them to `protect`. Without this the only plan that ever
    // recorded protecting itself was one an operator had protected by hand.
    // §22.1 and §22.2: the timeline orders what happened before mutation ahead of what the run
    // did. The checkpoint was taken and the protection created before the first action, at the
    // instant the apply began; the run's own events carry the instant it finished, because the
    // outcome records no per-action instants and a shared one leaves their order to chance.
    let finished = Timestamp::now().max(now);
    if !outcome.assets().is_empty() {
        lifecycle.protected(outcome.assets().len(), now);
    }
    for (target, observed) in before {
        lifecycle.target_checkpointed(target, observed, now);
    }
    for (id, status) in outcome.statuses() {
        if let Some(action) = plan.actions().iter().find(|action| action.id() == id) {
            lifecycle.action_started(action, finished);
            lifecycle.action_settled(action, *status, finished);
        }
    }
    for result in outcome.verification() {
        lifecycle.verification_observed(result, finished);
    }
    lifecycle.verified(verdict_of(outcome.state()), finished);
    let ledger = crate::temporal::session::writable_ledger();
    let _ = lifecycle.record(ledger.as_ref());
}

/// §4.8's verdict, read off the state the apply reached.
const fn verdict_of(state: PlanState) -> ono_change_core::Verdict {
    match state {
        // §25.3: a recovery that verified reached `recovery-verified`, and the ledger records it
        // as the success it is — not as the failure every other state falls through to.
        PlanState::Verified | PlanState::RecoveryVerified => ono_change_core::Verdict::Verified,
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
