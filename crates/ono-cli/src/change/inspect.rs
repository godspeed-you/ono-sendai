//! `get plan`, `inspect plan`, `rebase plan` and `resume plan` (spec v0.6 §36.4, §7.5, §41.3).
//!
//! Everything here is a read except `rebase`, which writes a new revision and leaves the one it
//! came from exactly as it was (§7.5), and `resume`, which continues an apply the shell did not
//! finish (§41.3).
//!
//! # Why `inspect plan` answers with one record and a rendering
//!
//! §5.5's inventory and §20's plan view are two presentations of the same object, and v0.2 §13.1
//! keeps presentation out of the language. So every command here answers with an
//! `ono.change-plan/1` — `inspect plan a82f | to json` is the plan, field for field — and the
//! `--protection`, `--impact`, `--actions`, `--resolution`, `--verification` and `--recovery`
//! options choose which sections `crate::sink` draws (Appendix B.10, §9.5, §10.3, §23.1, §11.1).

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{PlanKind, PlanState, error};
use ono_change_plan::PlanFilter;
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture};
use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_value::{ActionResult, ActionStatus, ErrorValue, MapValue, SchemaId, Value, ValueRef};

use super::gates::Acknowledgements;
use super::session::change_session;

/// `get plan` (§5.5, §36.4).
#[derive(Debug)]
pub struct GetPlan;

impl CommandImpl for GetPlan {
    fn id(&self) -> &str {
        "ono.change-plan.get"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("get plan"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let state = change_session().await?;
            if let Some(reference) = arguments
                .selector("reference")
                .and_then(|value| value.as_str().ok())
                .filter(|text| !text.trim().is_empty())
            {
                // §7.5: a rebase leaves the revision it came from exactly as it was, and the
                // plan that was refused is the evidence for why. `--revision` is how it is read
                // back; without it the answer is the latest, which is what an operator acting on
                // a plan means by naming it.
                let plan = match revision_of(&arguments)? {
                    Some(revision) => {
                        let id = state.store().resolve(reference)?;
                        state.store().get_revision(&id, revision)?
                    }
                    None => super::plan_of(&state, reference)?,
                };
                return Ok(Outcome::Values(ValueStream::from_values([
                    super::plan_value(&plan)?,
                ])));
            }
            let wanted = states_of(&arguments)?;
            let since = window(&arguments, Timestamp::now());
            let summaries = state.store().list(&PlanFilter::all())?;
            let mut values = Vec::new();
            for summary in summaries {
                if !wanted.is_empty() && !wanted.contains(&summary.state) {
                    continue;
                }
                // §5.5: closed and expired plans are hidden by default, because the list an
                // operator wants is the one they can still act on. `--all` is the whole store.
                if wanted.is_empty() && !arguments.flag("all") && is_closed(summary.state) {
                    continue;
                }
                if let Some(since) = since
                    && summary.created_at < since
                {
                    continue;
                }
                let plan = state.store().get(&summary.id)?;
                values.push(super::plan_value(&plan)?);
            }
            Ok(Outcome::Values(ValueStream::from_values(values)))
        })
    }
}

/// `--revision`, refused rather than rounded when it is not a revision number (§7.5).
fn revision_of(arguments: &ono_command::BoundArguments) -> Result<Option<u32>, ErrorValue> {
    let Some(value) = arguments.option("revision") else {
        return Ok(None);
    };
    if matches!(value, Value::Null) {
        return Ok(None);
    }
    let number = value.as_int()?;
    u32::try_from(number).map(Some).map_err(|_| {
        ErrorValue::new(
            ono_core::ErrorCode::TypeMismatch,
            format!("`--revision {number}` is not a revision number"),
        )
        .with_help(
            "v0.6 §7.5: revisions are numbered from 1, and each rebase adds one. `get plan <id>` \
             without it answers with the latest"
                .to_owned(),
        )
    })
}

/// The `--state` values, refused by name rather than silently ignored (§4.1).
fn states_of(arguments: &ono_command::BoundArguments) -> Result<Vec<PlanState>, ErrorValue> {
    let given = match arguments.option("state") {
        Some(Value::List(items)) => items.to_vec(),
        Some(Value::Null) | None => Vec::new(),
        Some(other) => vec![other.clone()],
    };
    let mut states = Vec::new();
    for value in given {
        let Ok(name) = value.as_str() else {
            continue;
        };
        let state = PlanState::from_name(name).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeUnknownField,
                format!("`{name}` is not one of v0.6 §4.1's plan states"),
            )
            .with_help(format!(
                "the states are {}",
                PlanState::ALL
                    .iter()
                    .map(|state| state.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        states.push(state);
    }
    Ok(states)
}

/// `--since` as the instant a plan must have been created after.
fn window(arguments: &ono_command::BoundArguments, now: Timestamp) -> Option<Timestamp> {
    let Some(Value::Duration(span)) = arguments.option("since") else {
        return None;
    };
    Timestamp::from_nanosecond(now.as_nanosecond().saturating_sub(span.nanoseconds().abs())).ok()
}

/// Whether §4.1's state is one `get plan` hides without `--all`.
const fn is_closed(state: PlanState) -> bool {
    matches!(state, PlanState::Closed | PlanState::Expired)
}

/// `inspect plan` (§5.5, §20, Appendix B.10).
#[derive(Debug)]
pub struct InspectPlan;

impl CommandImpl for InspectPlan {
    fn id(&self) -> &str {
        "ono.change-plan.inspect"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("inspect plan"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let reference = super::reference_of(ctx, "reference").await?;
            let state = change_session().await?;
            let plan = super::plan_of(&state, &reference)?;
            let sections = super::render::Sections::of(&arguments);
            let mut record = ono_change_core::value::plan_record(&plan)?;
            // Appendix B.10: the resolution expansion is the view that explains why several
            // recovery assets are planned, and the persistence resolution behind it is not a
            // field of §46.1's schema. §10.4's namespaced extension is where a provider-specific
            // fact travels, so it travels there and `| to json` still carries it.
            if sections.resolution {
                record = super::extended(
                    &record,
                    "ono.change/resolution",
                    resolution_of(&state, &plan),
                );
            }
            super::render::publish_sections(sections);
            Ok(Outcome::Values(ValueStream::from_values([Value::Record(
                Arc::new(record),
            )])))
        })
    }
}

/// Appendix B.10's expansion: the persistence resolution of every file target the plan holds.
///
/// Mount, filesystem, filesystem root, backing object and recovery boundary — the pipeline
/// Appendix B walks, per target, so a reader can see *why* two targets under one path produced
/// two recovery assets rather than one.
fn resolution_of(state: &super::session::ChangeState, plan: &ono_change_core::ChangePlan) -> Value {
    let mut rows = Vec::new();
    for target in plan.targets() {
        if target.schema() != ono_change_plan::freeze::FILE_SCHEMA {
            continue;
        }
        let domain = state.mounts().resolve(std::path::Path::new(target.label()));
        let mut row = MapValue::new();
        row.insert("target".into(), Value::string(target.identity()));
        row.insert("path".into(), Value::string(domain.path()));
        row.insert(
            "mount_point".into(),
            Value::string(domain.mount().mount_point()),
        );
        row.insert(
            "filesystem".into(),
            Value::string(domain.mount().filesystem()),
        );
        row.insert("source".into(), Value::string(domain.mount().source()));
        row.insert(
            "filesystem_root".into(),
            Value::string(domain.mount().root()),
        );
        row.insert(
            "object".into(),
            domain.object().map_or(Value::Null, Value::string),
        );
        row.insert("object_kind".into(), Value::string(domain.object_kind()));
        row.insert(
            "recovery_boundary".into(),
            domain.boundary().map_or(Value::Null, Value::string),
        );
        row.insert(
            "not_persistent".into(),
            domain
                .refusal()
                .map_or(Value::Null, |reason| Value::string(reason.as_str())),
        );
        rows.push(Value::Map(Arc::new(row)));
    }
    Value::list(rows)
}

/// `rebase plan` (§7.5).
#[derive(Debug)]
pub struct RebasePlan;

impl CommandImpl for RebasePlan {
    fn id(&self) -> &str {
        "ono.change-plan.rebase"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("rebase plan"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let reference = super::reference_of(ctx, "reference").await?;
            let providers = ctx.providers().clone();
            let state = change_session().await?;
            let plan = super::plan_of(&state, &reference)?;
            let now = Timestamp::now();
            // §7.5 resolves the plan again against the world as it is now, so the targets are
            // frozen a second time rather than copied: a unit that was restarted since the seal
            // has a new generation, and that is the fact the new revision is about.
            let mut targets = Vec::with_capacity(plan.targets().len());
            for target in plan.targets() {
                targets.push(
                    super::world::refreeze(&providers, state.mounts(), target)
                        .await
                        .unwrap_or_else(|_| target.clone()),
                );
            }
            let revised = ono_change_plan::rebase(&plan, targets, now)?;
            state.store().put(&revised)?;
            super::session::note_last_plan(revised.id());
            Ok(Outcome::Values(ValueStream::from_values([
                super::plan_value(&revised)?,
            ])))
        })
    }
}

/// `resume plan` (§41.3).
#[derive(Debug)]
pub struct ResumePlan;

impl CommandImpl for ResumePlan {
    fn id(&self) -> &str {
        "ono.change-plan.resume"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("resume plan"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let given = Acknowledgements::of(&arguments);
            let interactive = super::is_interactive(ctx);
            let reference = super::reference_of(ctx, "reference").await?;
            super::gates::require_confirmation("resume plan", given, interactive)?;
            let providers = ctx.providers().clone();
            let handle = super::runtime_handle()?;
            let state = change_session().await?;
            let plan = super::plan_of(&state, &reference)?;
            let now = Timestamp::now();
            // §7.3 and §62.8: the world may have moved while the plan was interrupted, so the
            // decision is made with revalidation in hand rather than from the records alone.
            let mut drift = Vec::new();
            for action in plan.actions() {
                if let Ok(findings) = super::world::revalidate(&handle, &providers, action) {
                    drift.extend(findings);
                }
            }
            let outcome = ono_change_executor::resume_with(&plan, state.store(), now, &drift);
            let mut values = Vec::new();
            for blocked in outcome.blocked() {
                values.push(blocked_result(&plan, blocked));
            }
            if let Some(refusal) = outcome.refusal() {
                return Err(refusal.clone());
            }
            // §41.3: what may not be rerun is a recovery or rebase decision rather than a
            // silent no-op. The refusal names the actions and the reason each carries, so the
            // operator can see which of §41.1's classes stopped it (Appendix F.2).
            if !outcome.may_continue() {
                let blocked: Vec<(String, String)> = outcome
                    .blocked()
                    .iter()
                    .map(|action| {
                        (
                            action.action().as_str().to_owned(),
                            action.reason().to_owned(),
                        )
                    })
                    .collect();
                if !blocked.is_empty() {
                    return Err(error::resume_refused(plan.id(), &blocked));
                }
                // Nothing blocked and nothing to continue: the plan is complete. §41.3 asks for
                // a decision rather than a silent no-op, and an empty answer with a zero exit
                // reads as "resumed" to a script.
                return Err(error::resume_complete(plan.id(), outcome.state()));
            }
            // §41.3: what may be rerun is rerun through the ordinary apply path, so the same
            // gates, the same claim and the same ledger events apply to a resumed plan as to a
            // fresh one.
            let analysis = super::lifecycle::analysis_of(&state, &plan);
            let revalidate = |action: &_| super::world::revalidate(&handle, &providers, action);
            let execute =
                |action: &_| super::world::execute_with(&handle, &providers, Some(&state), action);
            let observe = |contract: &_| super::world::observe(&handle, &providers, contract);
            let mut request = ono_change_executor::ApplyRequest::new(
                &plan,
                state.store(),
                state.session_id(),
                now,
                analysis.actions(),
                state.providers(),
                &revalidate,
                &execute,
                &observe,
            );
            let applied = ono_change_executor::apply(&mut request);
            for (id, status) in applied.statuses() {
                let action = plan.actions().iter().find(|action| action.id() == id);
                let mut identity = MapValue::new();
                identity.insert("plan".into(), Value::string(plan.id().as_str()));
                identity.insert("action".into(), Value::string(id.as_str()));
                let target = ValueRef::object(SchemaId::new("ono.change-action", 1), identity);
                values.push(
                    ActionResult::new(
                        target,
                        action.map_or(
                            "ono.change-plan.resume",
                            ono_change_core::PlanAction::summary,
                        ),
                        match status {
                            ono_change_core::ActionStatus::Succeeded => ActionStatus::Success,
                            ono_change_core::ActionStatus::Failed => ActionStatus::Failed,
                            _ => ActionStatus::Skipped,
                        },
                    )
                    .changed(status.may_have_mutated())
                    .into_value(),
                );
            }
            Ok(Outcome::Values(ValueStream::from_values(values)))
        })
    }
}

/// One `ono.action-result/1` for an action §41.2 will not rerun (Appendix F.2).
fn blocked_result(
    plan: &ono_change_core::ChangePlan,
    blocked: &ono_change_executor::BlockedAction,
) -> Value {
    let mut identity = MapValue::new();
    identity.insert("plan".into(), Value::string(plan.id().as_str()));
    identity.insert("action".into(), Value::string(blocked.action().as_str()));
    let target = ValueRef::object(SchemaId::new("ono.change-action", 1), identity);
    ActionResult::new(target, blocked.summary(), ActionStatus::Skipped)
        .changed(false)
        .with_message(blocked.reason())
        .into_value()
}

/// Whether a plan is a recovery plan, for a caller choosing which view to draw (§24.1).
#[must_use]
pub const fn is_recovery(kind: PlanKind) -> bool {
    matches!(kind, PlanKind::Recovery)
}
