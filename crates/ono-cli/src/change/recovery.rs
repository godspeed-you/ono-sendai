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
use ono_change_core::{
    Execution, FrozenTarget, ImpactGraph, NewerStateClass, PlanAction, RecoveryAsset, RecoveryGoal,
    RecoveryPlan, RestoreAcceptance, RestoreMethod, error,
};
use ono_change_executor::ExecutionOutcome;
use ono_change_impact::derive::ImpactRequest;
use ono_change_protection::retention::{PlanRetention, cleanup_preview};
use ono_change_recovery::{ObservedState, RecoveryRequest};
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
            let restore: Vec<String> = source.as_ref().map_or_else(Vec::new, |plan| {
                plan.targets()
                    .iter()
                    .map(|target| target.label().to_owned())
                    .collect()
            });
            let method = arguments
                .option("method")
                .and_then(|value| value.as_str().ok());
            if let Some(method) = method {
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
            }
            // §2.12: the recovery is impact-checked over the session's topology like any other
            // plan. The walk runs before the seal, and the guard goes as soon as it has.
            let (recovery, notes) = {
                let spatial = crate::spatial::spatial_session().await;
                let derive = |targets: &[FrozenTarget], actions: &[PlanAction]| {
                    ono_change_impact::derive::derive(&ImpactRequest::new(
                        spatial.index(),
                        targets,
                        actions,
                        now,
                    ))
                };
                analyse(
                    &state,
                    source.as_ref(),
                    &assets,
                    goal,
                    method,
                    &restore,
                    Some(&derive),
                    now,
                )?
            };
            // §24.1: the recovery plan is stored so it can be inspected and then applied. Storing
            // it changes nothing about the system it would recover.
            state.store().put_recovery(&recovery, &notes)?;
            super::session::note_last_plan(recovery.plan().id());
            record_planned(source.as_ref(), &recovery, now);
            Ok(Outcome::Values(ValueStream::from_values([Value::Record(
                Arc::new(ono_change_core::value::recovery_plan_record_with(
                    &recovery, &notes,
                )?),
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
        return Ok((
            None,
            vec![fresh_asset(state, state.store().get_asset(&id)?)],
        ));
    }
    if let Ok(asset) = super::resolve_asset(state.store(), reference)
        && let Ok(asset) = state.store().get_asset(&asset)
    {
        return Ok((None, vec![fresh_asset(state, asset)]));
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
            assets.push(fresh_asset(state, asset));
        }
    }
    // §18.2: a plan protected early by `protect` and again at `apply` holds two assets of the
    // same domain, and only the newer one is the state just before the change. The store answers
    // in identity order, which is a hash; choosing by it would restore whichever sorted first.
    assets.sort_by_key(|asset| std::cmp::Reverse(asset.created_at()));
    let mut kept: Vec<RecoveryAsset> = Vec::with_capacity(assets.len());
    for asset in assets {
        if !kept.iter().any(|newer| {
            newer.provider() == asset.provider() && newer.scope().domain() == asset.scope().domain()
        }) {
            kept.push(asset);
        }
    }
    Ok(kept)
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
    // The object is the path as the plan froze it, `@` and all (§7.1).
    let path = std::path::Path::new(object);
    if !path.is_absolute() {
        return ObservedState::Unestablished {
            reason: Arc::from(
                "this build observes a path's current content and nothing else, so the object's \
                 state could not be established (§56.3)",
            ),
        };
    }
    match std::fs::read(path) {
        Ok(bytes) => match modified(path) {
            Some(at) => ObservedState::changed(at, hex(&bytes)),
            None => ObservedState::present(hex(&bytes)),
        },
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {
            ObservedState::Absent { changed_at: None }
        }
        Err(failure) => ObservedState::Unestablished {
            reason: Arc::from(failure.to_string()),
        },
    }
}

/// The digest of the file at `object` now, in the form a manifest records it, where it is a file
/// that can be read (§18.3, Appendix C.3).
pub fn current_digest(object: &str) -> Option<String> {
    let path = std::path::Path::new(object);
    if !path.is_absolute() {
        return None;
    }
    std::fs::read(path).ok().map(|bytes| hex(&bytes))
}

/// Whether a recovery asset made earlier for `plan` still captures the state about to change
/// (§18.2, §18.3).
///
/// The provider is asked what its asset holds, object by object, and each is held against the
/// object as it is now. An asset whose provider is gone, or which says nothing about what it
/// captured, is of unknown vintage — never fresh.
pub fn early_freshness(
    state: &ChangeState,
    plan: &ono_change_core::ChangePlan,
    asset: &RecoveryAsset,
) -> ono_change_protection::freshness::FreshnessVerdict {
    let require_fresh = plan.protection_mode() == ono_change_core::ProtectionMode::Require;
    let captured = state
        .providers()
        .get(asset.provider())
        .and_then(|provider| {
            provider
                .plan_recovery(asset, Some(plan), RecoveryGoal::RestoreChangedObjects)
                .ok()
        })
        .map(|fragment| fragment.captured().to_vec())
        .unwrap_or_default();
    ono_change_protection::freshness::assess_captured(
        asset,
        &captured,
        &current_digest,
        require_fresh,
    )
}

/// When `path` last changed, where the filesystem keeps that (Appendix C.3).
fn modified(path: &std::path::Path) -> Option<Timestamp> {
    let at = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()?;
    Timestamp::try_from(at).ok()
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
            // §11.4: `ready` is a claim a validation made, and only a validation can keep making it.
            // An asset whose stored copy went away since is shown as what it is now.
            let assets: Vec<RecoveryAsset> = assets
                .into_iter()
                .map(|asset| fresh_asset(&state, asset))
                .collect();
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
            let asset = fresh_asset(&state, state.store().get_asset(&id)?);
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
            let asset = fresh_asset(&state, state.store().get_asset(&id)?);
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
                // §37.3: the answer a person asked for is the preview itself, so it is drawn where the
                // shell's notes go; the value on stdout carries the same answer for a script.
                for line in ono_change_render::cleanup_preview(
                    &ono_change_core::value::asset_record(&asset)?,
                    &blocked,
                    80,
                ) {
                    eprintln!("{line}");
                }
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
            // §12.1: only the provider that created an asset can remove it. An asset whose provider is
            // not registered here still exists, so calling it removed would be a success that did
            // not happen, and a store that no longer tracks a snapshot nobody deleted.
            let Some(owner) = state.providers().get(asset.provider()) else {
                return Err(error::provider_unavailable(
                    asset.provider(),
                    "the provider that created this asset is not registered here, so it cannot be \
                     removed. Nothing was removed, and the asset is still recorded",
                ));
            };
            let value = match owner.cleanup(&asset) {
                Ok(()) => {
                    state.store().put_asset(&asset.clone().removed())?;
                    record_removed(&state, &asset, now);
                    result(
                        &id,
                        ActionStatus::Success,
                        true,
                        &format!("{} is gone", id.short()),
                    )
                }
                Err(refusal) => return Err(refusal),
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
    // ADR-0821: the source plan was created once, when it was planned. This is a later event about
    // it, so it continues the plan's history rather than beginning it again.
    let mut lifecycle =
        ono_change_executor::PlanLifecycle::continuing(source, crate::spatial::local_scope());
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
        ono_change_executor::PlanLifecycle::continuing(&plan, crate::spatial::local_scope());
    lifecycle.asset_removed(asset.id(), now);
    let ledger = crate::temporal::session::writable_ledger();
    let _ = lifecycle.record(ledger.as_ref());
}

/// Appendix C.3's analysis over `assets`, as `recover` runs it and as `apply` runs it again.
///
/// One function for both, because §7.3's revalidation of a recovery plan is exactly this question
/// asked a second time: what would this restore discard *now*. Two builders would drift, and the
/// second answer would stop being comparable to the first.
fn analyse(
    state: &ChangeState,
    source: Option<&ono_change_core::ChangePlan>,
    assets: &[RecoveryAsset],
    goal: RecoveryGoal,
    method: Option<&str>,
    restore: &[String],
    derive: Option<&dyn Fn(&[FrozenTarget], &[PlanAction]) -> ImpactGraph>,
    now: Timestamp,
) -> Result<
    (
        ono_change_core::RecoveryPlan,
        ono_change_core::value::RecoveryPlanNotes,
    ),
    ErrorValue,
> {
    let observe = |object: &str| observed(object);
    let mut request = RecoveryRequest::new(
        state.providers(),
        assets,
        &observe,
        goal,
        state.session_id(),
        now,
    );
    if let Some(plan) = source {
        request = request.recovering(plan);
        // Appendix C.4: the plan's own write is what recovery undoes. When it settled is what
        // separates that write from a later edit, which is the only newer state there is.
        if let Some(applied) = state.store().applied_at(plan.id())? {
            request = request.applied_at(applied);
        }
    }
    for object in restore {
        request = request.restoring(object.as_str());
    }
    if let Some(method) = method {
        request = request.forcing_method(method);
    }
    if let Some(derive) = derive {
        request = request.deriving_impact(derive);
    }
    let (recovery, selection) = ono_change_recovery::plan_recovery_explained(&request)?;
    // Appendix I.5: the methods not chosen, and why, are part of what the operator is shown.
    let rejected = selection
        .rejected()
        .iter()
        .map(|rejected| {
            ono_change_core::value::RejectedMethodNote::new(
                rejected.provider(),
                rejected.method(),
                rejected.reason().as_str(),
                rejected.detail(),
            )
            .unmet(rejected.unmet().to_vec())
        })
        .collect();
    let notes = ono_change_core::value::RecoveryPlanNotes::default().with_rejected(rejected);
    Ok((recovery, notes))
}

/// The recovery `apply` is about to run, as the world stands now (§7.3, §24.5, Appendix C.4).
///
/// The stored plan is what the operator was shown and what an acceptance was given for. The
/// analysis is run again because hours may have passed (§62.8): a loss that appeared since is one
/// nobody was shown, so it refuses whatever was accepted, and the gate is then decided on the
/// current analysis — an acceptance covers the losses that were on the plan, and no others.
///
/// Answers the gated recovery and the assets its actions restore from.
///
/// # Errors
///
/// - `recovery.plan_incomplete` where the plan carries no stored analysis, or the analysis
///   cannot be completed now (§56.3);
/// - `recovery.newer_state_conflict` where recovery would now discard state the stored plan did
///   not show, or where it would discard state and `--accept-newer-state-loss` was not given;
/// - `recovery.destructive_history_not_accepted` where it would destroy provider history.
pub fn current(
    state: &ChangeState,
    plan: &ono_change_core::ChangePlan,
    accepted: bool,
    now: Timestamp,
) -> Result<(ono_change_core::RecoveryPlan, Vec<RecoveryAsset>), ErrorValue> {
    let stored = state.store().get_recovery(plan.id())?;
    let mut assets = Vec::with_capacity(stored.source_assets().len());
    for id in stored.source_assets() {
        assets.push(state.store().get_asset(id)?);
    }
    let source = match stored.source_plan() {
        Some(id) => Some(state.store().get(id)?),
        None => None,
    };
    let restore: Vec<String> = stored.restores().iter().map(|o| o.to_string()).collect();
    let (fresh, _) = analyse(
        state,
        source.as_ref(),
        &assets,
        stored.goal(),
        Some(stored.method().as_str()),
        &restore,
        // The stored plan keeps the impact the operator was shown; this pass weighs losses only.
        None,
        now,
    )?;
    let shown = losses(&stored);
    let unshown: Vec<String> = losses(&fresh)
        .into_iter()
        .filter(|loss| !shown.contains(loss))
        .collect();
    if !unshown.is_empty() {
        return Err(error::newer_state_conflict(&unshown).with_help(
            "the world changed after `recover` built this plan, and recovery would now discard \
             state it never showed (§7.3, §24.5). Run `recover` again to see what it would \
             discard now; an acceptance covers only the losses a plan showed",
        ));
    }
    let gated = if accepted {
        fresh.destruction_accepted()
    } else {
        fresh
    };
    ono_change_recovery::gate::check(&gated)?;
    Ok((gated, assets))
}

/// Everything a recovery would take away, as comparable text (§24.3, §13.6).
fn losses(recovery: &ono_change_core::RecoveryPlan) -> Vec<String> {
    let mut losses: Vec<String> = recovery
        .newer_state()
        .items()
        .iter()
        .filter(|item| item.class().is_loss() || item.class() == NewerStateClass::Unknown)
        .map(|item| format!("{} ({})", item.object(), item.class()))
        .collect();
    losses.extend(
        recovery
            .newer_state()
            .destroyed_assets()
            .iter()
            .map(|asset| format!("{asset} (destroyed)")),
    );
    losses
}

/// Carries out one action of a recovery plan through the provider that owns its asset (§12.2,
/// §24.2).
///
/// Every action goes to the provider, whatever form it was planned in: the provider emitted it,
/// and only the provider can re-establish, at the moment of the act, the facts its safety rests
/// on (§56.1). Running a planned program directly would skip exactly that check. The acceptance
/// travels with the call, so a provider can refuse a destruction nobody accepted even where the
/// world moved between the gate and the act.
pub fn restore(
    state: &ChangeState,
    plan: &ono_change_core::PlanId,
    assets: &[RecoveryAsset],
    acceptance: &RestoreAcceptance,
    action: &PlanAction,
) -> ExecutionOutcome {
    let asset = match owning_asset(assets, action) {
        Ok(asset) => asset,
        Err(refusal) => return ExecutionOutcome::Failed(refusal),
    };
    let Some(provider) = state.providers().get(asset.provider()) else {
        return ExecutionOutcome::Failed(error::provider_unavailable(
            asset.provider(),
            "the provider that created this asset is not registered here, and only it can \
             restore from it (§11.4, §12.1)",
        ));
    };
    let outcome = match provider.restore_with(action, asset, acceptance) {
        Ok(outcome) => outcome,
        Err(refusal) => return ExecutionOutcome::Failed(refusal),
    };
    // §14.5: what the restore derived is an asset with a lifecycle of its own, attributed to the
    // recovery plan that made it. One the store could not record is a real object nothing
    // tracks, so the action is not reported as simply done.
    for created in outcome.created() {
        if let Err(failure) = state
            .store()
            .put_asset(&created.clone().for_plan(plan.clone()))
        {
            return ExecutionOutcome::Unknown(failure.with_help(format!(
                "the restore ran and created {}, which the plan store could not record; \
                 `get recovery` will not show it until it is recorded",
                created.reference()
            )));
        }
    }
    ExecutionOutcome::Succeeded
}

/// The asset `action` restores from: the one it names, or the only one it could mean.
fn owning_asset<'a>(
    assets: &'a [RecoveryAsset],
    action: &PlanAction,
) -> Result<&'a RecoveryAsset, ErrorValue> {
    let (named, provider) = match action.execution() {
        Execution::RecoveryOperation {
            provider,
            arguments,
            ..
        } => (
            arguments
                .iter()
                .find(|(name, _)| name.as_ref() == "asset")
                .and_then(|(_, value)| value.as_str().ok().map(str::to_owned)),
            Some(provider.as_ref()),
        ),
        _ => (None, None),
    };
    if let Some(named) = named {
        return assets
            .iter()
            .find(|asset| asset.id().as_str() == named)
            .ok_or_else(|| {
                error::action_not_plannable(
                    action.summary(),
                    "it names an asset this recovery plan does not restore from",
                )
            });
    }
    let candidates: Vec<&RecoveryAsset> = assets
        .iter()
        .filter(|asset| provider.is_none_or(|provider| asset.provider() == provider))
        .collect();
    match candidates.as_slice() {
        [only] => Ok(only),
        [] => Err(error::action_not_plannable(
            action.summary(),
            "no asset this recovery restores from belongs to the provider that planned it",
        )),
        _ => Err(error::action_not_plannable(
            action.summary(),
            "it names no asset, and this recovery restores from several",
        )),
    }
}

/// `asset` as its provider finds it now (§11.4, §37).
///
/// A `ready` asset is validated again by the provider that made it, and a state that changed —
/// its stored copy deleted, its snapshot gone — is written back, so the store never keeps offering
/// a recovery point that no longer exists. A provider that cannot be asked here leaves the asset
/// as recorded: not being able to ask is not evidence that it is gone.
pub fn fresh_asset(state: &ChangeState, asset: RecoveryAsset) -> RecoveryAsset {
    match state.providers().revalidate(&asset) {
        Ok(fresh) => {
            if fresh.state() != asset.state() {
                let _ = state.store().put_asset(&fresh);
            }
            fresh
        }
        Err(_) => asset,
    }
}
