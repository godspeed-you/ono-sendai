//! `recover`, `get recovery`, `inspect recovery` and `remove recovery`
//! (spec v0.6 §5.8, §24, §37.3, §37.5, §2.15).
//!
//! §24.1 is the rule the first of them is written to: *`recover` plans recovery and does not
//! perform it*. The extra step is deliberate, because recovery may destroy newer state (§24.2),
//! and the plan it produces is stored so it can be inspected and then applied like any other
//! (§2.12).
//!
//! `remove recovery` is the destructive one, and §2.15 is what shapes it: cleanup never outruns
//! recovery policy. `--dry-run` reports which plans would become unrecoverable and removes
//! nothing, `--confirm` is required before anything is removed, and an asset a retained plan
//! still depends on is refused with `recovery.cleanup_blocked` naming the plans unless `--force`.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{RecoveryAsset, RecoveryGoal, RecoveryPlan, RestoreMethod, error};
use ono_change_protection::retention::{PlanRetention, cleanup_preview};
use ono_change_recovery::{ObservedState, RecoveryRequest, plan_recovery};
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture};
use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_value::{ActionResult, ActionStatus, ErrorValue, MapValue, SchemaId, Value, ValueRef};

use super::gates::Acknowledgements;
use super::session::{ChangeState, change_session};

/// `recover` (§5.8, §24.1).
#[derive(Debug)]
pub struct Recover;

impl CommandImpl for Recover {
    fn id(&self) -> &str {
        "ono.change.recover"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("recover"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let reference = super::reference_of(ctx, "source").await?;
            let state = change_session().await?;
            let now = Timestamp::now();

            // §5.8 takes either a plan to recover *from* or an asset to recover *toward*, and
            // `--to` names the second explicitly. The assets are what a restore can be planned
            // against at all (§11.4), so both spellings end at the same list.
            let (source, assets) = sources(&state, &arguments, &reference)?;
            if assets.is_empty() {
                return Err(error::recovery_plan_incomplete(
                    "a recovery asset to restore from",
                    "nothing this shell retains covers that plan, so there is no point to \
                     restore toward (§11.4)",
                ));
            }
            let goal = goal_of(&arguments)?;
            let observe = |object: &str| observed(object);
            let mut request = RecoveryRequest::new(
                state.providers(),
                &assets,
                &observe,
                goal,
                state.session_id(),
                now,
            );
            if let Some(plan) = source.as_ref() {
                request = request.recovering(plan);
                for target in plan.targets() {
                    request = request.restoring(target.label());
                }
            }
            if let Some(method) = arguments
                .option("method")
                .and_then(|value| value.as_str().ok())
            {
                // Appendix C.5: a method outside the closed list is refused by name rather than
                // silently replaced with the one Ono would have chosen.
                if RestoreMethod::from_name(method).is_none() {
                    return Err(ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("`--method {method}` is not one of Appendix C.1's restore methods"),
                    )
                    .with_help(format!(
                        "the methods are {}",
                        RestoreMethod::ALL
                            .iter()
                            .map(|method| method.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
                request = request.forcing_method(method);
            }
            let recovery = plan_recovery(&request)?;
            // §24.1: the recovery plan is stored so it can be inspected and then applied. Storing
            // it changes nothing about the system it would recover.
            state.store().put(recovery.plan())?;
            super::session::note_last_plan(recovery.plan().id());
            record_planned(source.as_ref(), &recovery, now);
            Ok(Outcome::Values(ValueStream::from_values([Value::Record(
                Arc::new(ono_change_core::value::recovery_plan_record(&recovery)?),
            )])))
        })
    }
}

/// What `recover` was pointed at: the plan to recover from, and the assets to restore through.
fn sources(
    state: &ChangeState,
    arguments: &ono_command::BoundArguments,
    reference: &str,
) -> Result<(Option<ono_change_core::ChangePlan>, Vec<RecoveryAsset>), ErrorValue> {
    if let Some(toward) = arguments.option("to").and_then(|value| value.as_str().ok()) {
        let id = super::resolve_asset(state.store(), toward)?;
        return Ok((None, vec![state.store().get_asset(&id)?]));
    }
    if let Ok(asset) = super::resolve_asset(state.store(), reference)
        && let Ok(asset) = state.store().get_asset(&asset)
    {
        return Ok((None, vec![asset]));
    }
    let plan = super::plan_of(state, reference)?;
    let assets = assets_of(state, plan.id())?;
    Ok((Some(plan), assets))
}

/// The assets a plan rests on that the store actually holds (§11.1, §11.4).
///
/// A sealed plan cites the assets its coverage *proposed*, and §2.1 means a proposed asset does
/// not exist. Only the ones the store has a record for are things a restore can be planned
/// against, so an id with no record is left out rather than made into a refusal — the plan is not
/// broken, it simply has not been protected yet.
fn assets_of(
    state: &ChangeState,
    plan: &ono_change_core::PlanId,
) -> Result<Vec<RecoveryAsset>, ErrorValue> {
    let ids = state.store().assets_for(plan)?;
    let mut assets = Vec::with_capacity(ids.len());
    for id in ids {
        if let Ok(asset) = state.store().get_asset(&id) {
            assets.push(asset);
        }
    }
    Ok(assets)
}

/// `--goal` as Appendix C.2's goal, defaulting to the one §5.8 writes.
fn goal_of(arguments: &ono_command::BoundArguments) -> Result<RecoveryGoal, ErrorValue> {
    let Some(text) = arguments
        .option("goal")
        .and_then(|value| value.as_str().ok())
    else {
        return Ok(RecoveryGoal::RestoreChangedObjects);
    };
    RecoveryGoal::from_name(text).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`--goal {text}` is not one of Appendix C.2's recovery goals"),
        )
        .with_help(format!(
            "the goals are {}",
            RecoveryGoal::ALL
                .iter()
                .map(|goal| goal.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

/// What `object` holds now, as Appendix C.3's conflict analysis reads it.
///
/// A path is digested; anything else is [`ObservedState::Unestablished`], which §56.3 gates on
/// rather than guessing — an object whose current state nobody established is a reason to show
/// the conflict, never a reason to assume there is none.
fn observed(object: &str) -> ObservedState {
    let path = std::path::Path::new(object.split('@').next().unwrap_or(object));
    if !path.is_absolute() {
        return ObservedState::Unestablished {
            reason: Arc::from(
                "this build observes a path's current content and nothing else, so the object's \
                 state could not be established (§56.3)",
            ),
        };
    }
    match std::fs::read(path) {
        Ok(bytes) => ObservedState::present(hex(&bytes)),
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {
            ObservedState::Absent { changed_at: None }
        }
        Err(failure) => ObservedState::Unestablished {
            reason: Arc::from(failure.to_string()),
        },
    }
}

/// A content digest, as the manifest of §15 writes one.
fn hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `get recovery` (§37.5).
#[derive(Debug)]
pub struct GetRecovery;

impl CommandImpl for GetRecovery {
    fn id(&self) -> &str {
        "ono.recovery.get"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("get recovery"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let state = change_session().await?;
            let assets = if let Some(reference) = arguments
                .selector("reference")
                .and_then(|value| value.as_str().ok())
                .filter(|text| !text.trim().is_empty())
            {
                let id = super::resolve_asset(state.store(), reference)?;
                vec![state.store().get_asset(&id)?]
            } else if let Some(plan) = arguments
                .option("plan")
                .and_then(|value| value.as_str().ok())
            {
                let id = super::resolve_plan(state.store(), plan)?;
                assets_of(&state, &id)?
            } else {
                state.store().list_assets()?
            };
            let wanted = states_of(&arguments);
            let mut values = Vec::with_capacity(assets.len());
            for asset in &assets {
                let word = asset.state().as_str();
                if !wanted.is_empty() && !wanted.iter().any(|state| state == word) {
                    continue;
                }
                // §37.5's default list is what can still be restored from. Removed and expired
                // assets are history rather than options, and `--all` is how history is read.
                if wanted.is_empty() && !arguments.flag("all") && is_gone(asset) {
                    continue;
                }
                values.push(Value::Record(Arc::new(
                    ono_change_core::value::asset_record(asset)?,
                )));
            }
            Ok(Outcome::Values(ValueStream::from_values(values)))
        })
    }
}

/// The `--state` words, as text, so §11.1's vocabulary stays the asset's to spell.
fn states_of(arguments: &ono_command::BoundArguments) -> Vec<String> {
    match arguments.option("state") {
        Some(Value::List(items)) => items
            .iter()
            .filter_map(|value| value.as_str().ok().map(str::to_owned))
            .collect(),
        Some(Value::String(text)) => vec![text.to_string()],
        _ => Vec::new(),
    }
}

/// Whether an asset is one `get recovery` hides without `--all` (§11.1).
fn is_gone(asset: &RecoveryAsset) -> bool {
    matches!(
        asset.state(),
        ono_change_core::AssetState::Removed | ono_change_core::AssetState::Expired
    )
}

/// `inspect recovery` (§37.5, §11.5).
#[derive(Debug)]
pub struct InspectRecovery;

impl CommandImpl for InspectRecovery {
    fn id(&self) -> &str {
        "ono.recovery.inspect"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("inspect recovery"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let reference = super::reference_of(ctx, "reference").await?;
            let state = change_session().await?;
            let id = super::resolve_asset(state.store(), &reference)?;
            let asset = state.store().get_asset(&id)?;
            Ok(Outcome::Values(ValueStream::from_values([Value::Record(
                Arc::new(ono_change_core::value::asset_record(&asset)?),
            )])))
        })
    }
}

/// `remove recovery` (§37.3, §2.15).
#[derive(Debug)]
pub struct RemoveRecovery;

impl CommandImpl for RemoveRecovery {
    fn id(&self) -> &str {
        "ono.recovery.remove"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("remove recovery"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let given = Acknowledgements::of(&arguments);
            let reference = super::reference_of(ctx, "reference").await?;
            let state = change_session().await?;
            let id = super::resolve_asset(state.store(), &reference)?;
            let asset = state.store().get_asset(&id)?;
            let now = Timestamp::now();

            // §37.3: what would become unrecoverable is computed before anything is decided, so
            // the dry run and the removal answer out of the same analysis.
            let retentions = retentions(&state)?;
            let preview = cleanup_preview(std::slice::from_ref(&asset), &retentions, now);
            let entry = preview.entry_for(&id);
            let blocked: Vec<String> = entry
                .map(|entry| {
                    entry
                        .unrecoverable_plans()
                        .iter()
                        .map(|plan| plan.short().to_owned())
                        .collect()
                })
                .unwrap_or_default();

            if arguments.flag("dry-run") {
                let message = if blocked.is_empty() {
                    format!(
                        "{} would be removed, and no retained plan depends on it",
                        id.short()
                    )
                } else {
                    format!(
                        "removing {} would make {} unrecoverable: {}",
                        id.short(),
                        blocked.len(),
                        blocked.join(", ")
                    )
                };
                return Ok(Outcome::Values(ValueStream::from_values([result(
                    &id,
                    ActionStatus::Skipped,
                    false,
                    &message,
                )])));
            }
            // §40.3: removal is irreversible, so the flag is the confirmation and nothing waits.
            if !given.confirmed {
                return Err(ErrorValue::new(
                    ErrorCode::SafetyConfirmationRequired,
                    format!("removing recovery asset {} was not confirmed", id.short()),
                )
                .with_help(
                    "nothing was removed. `remove recovery <ref> --confirm` removes it, and \
                     `--dry-run` says which plans would become unrecoverable (v0.6 §37.3)",
                ));
            }
            // §2.15: cleanup never outruns recovery policy. `--force` is the explicit override,
            // and the refusal names the plans rather than the count.
            if !blocked.is_empty() && !arguments.flag("force") {
                return Err(error::cleanup_blocked(&id, &blocked));
            }
            let removed = state
                .providers()
                .get(asset.provider())
                .map(|provider| provider.cleanup(&asset));
            let value = match removed {
                Some(Ok(())) | None => {
                    state.store().put_asset(&asset.clone().removed())?;
                    record_removed(&state, &asset, now);
                    result(
                        &id,
                        ActionStatus::Success,
                        true,
                        &format!("{} is gone", id.short()),
                    )
                }
                Some(Err(refusal)) => return Err(refusal),
            };
            Ok(Outcome::Values(ValueStream::from_values([value])))
        })
    }
}

/// What each retained plan still depends on, which is what §2.15's guard is computed from.
fn retentions(state: &ChangeState) -> Result<Vec<PlanRetention>, ErrorValue> {
    let mut retentions = Vec::new();
    for summary in state.store().list(&ono_change_plan::PlanFilter::all())? {
        let mut retention = PlanRetention::new(summary.id.clone(), summary.state);
        for asset in state.store().assets_for(&summary.id)? {
            retention = retention.resting_on(asset);
        }
        retentions.push(retention);
    }
    Ok(retentions)
}

/// One `ono.action-result/1` about a recovery asset.
fn result(
    id: &ono_change_core::RecoveryAssetId,
    status: ActionStatus,
    changed: bool,
    message: &str,
) -> Value {
    let mut identity = MapValue::new();
    identity.insert("asset".into(), Value::string(id.as_str()));
    let target = ValueRef::object(SchemaId::new("ono.recovery-asset", 1), identity);
    ActionResult::new(target, "ono.recovery.remove", status)
        .changed(changed)
        .with_message(message)
        .into_value()
}

/// Writes §22.1's `RecoveryPlanned` into the v0.5 ledger (§22.1).
fn record_planned(
    source: Option<&ono_change_core::ChangePlan>,
    recovery: &RecoveryPlan,
    now: Timestamp,
) {
    let Some(source) = source else {
        return;
    };
    let mut lifecycle =
        ono_change_executor::PlanLifecycle::created(source, crate::spatial::local_scope(), now);
    lifecycle.recovery_planned(recovery.plan().id(), now);
    let ledger = crate::temporal::session::writable_ledger();
    let _ = lifecycle.record(ledger.as_ref());
}

/// Writes §22.1's `RecoveryAssetRemoved` into the v0.5 ledger (§37).
///
/// The event is written against the plan the asset was created for, because §22.4 makes the plan
/// id the causal anchor and an asset with no plan has nothing to anchor to. An asset that came
/// from no plan is therefore not recorded rather than recorded against an invented one.
fn record_removed(state: &ChangeState, asset: &RecoveryAsset, now: Timestamp) {
    let Some(source) = asset.source_plan() else {
        return;
    };
    let Ok(plan) = state.store().get(source) else {
        return;
    };
    let mut lifecycle =
        ono_change_executor::PlanLifecycle::created(&plan, crate::spatial::local_scope(), now);
    lifecycle.asset_removed(asset.id(), now);
    let ledger = crate::temporal::session::writable_ledger();
    let _ = lifecycle.record(ledger.as_ref());
}
