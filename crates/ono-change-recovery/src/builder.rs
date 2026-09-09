//! Constructing a RecoveryPlan (spec §24.1, §24.3, §2.12, §35.2, Appendix F.2).
//!
//! §24.1 is the whole contract of this module: `recover @plan` *produces* a RecoveryPlan and does
//! not modify state. [`plan_recovery`] therefore reads nothing and writes nothing — the world
//! arrives as an observation function and a `now`, and the only provider method it calls is
//! `plan_recovery`, which §12.1 defines as the planning half.
//!
//! What comes out is a real plan, not a summary of one. §2.12 puts a recovery through the same
//! lifecycle as any other change, so the embedded `ChangePlan` is sealed, carries RECOVER-role
//! actions, carries verification contracts and carries its own risk. The recovery-specific part is
//! what §24.3 requires to be shown beside it, and two entries there decide whether the plan may
//! run at all: what it would do to newer state, and what it cannot reverse.
//!
//! # What "cannot reverse" covers
//!
//! [`plan_recovery`] fills `unrecoverable` from four sources, and none of them is a judgement
//! call:
//!
//! - an `EffectKind::Emit`, which §35.1 has already left the system. §35.3 lets a provider name a
//!   compensating action and calls the result `COMPENSATABLE`; §27.4 forbids the word rollback for
//!   it, so the effect stays listed with the compensation beside it rather than instead of it.
//! - an effect in an external or remote domain, which §35.2 says local snapshots do not reach.
//! - an object the original plan changed that no source asset covers. §55.8's partial case is the
//!   one this crate exists to get right: restoring a filesystem snapshot reverses what the
//!   snapshot holds, and nothing else.
//! - an action whose `ActionStatus` is `Unknown`. Appendix F.2 makes an unestablished outcome an
//!   uncertainty boundary for recovery planning, so the plan says the outcome could not be
//!   established rather than assuming the action did, or did not, happen.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ActionRole, ActionStatus, AssetState, ChangePlan, DirectoryRestorePolicy, EffectDomain,
    EffectKind, EquivalenceDomain, FrozenTarget, Intent, MetadataCoverage, PlanAction,
    ProposedEffect, ProviderBinding, RecoveryAsset, RecoveryGoal, RecoveryPlan, RestoreMethod,
    RiskAssessment, RiskClass, RiskDimension, RiskFinding, UnrecoverableEffect,
    VerificationClass, VerificationContract, VerificationSet, error,
};
use ono_change_protection::ProviderRegistry;
use ono_value::{ByteSize, ErrorValue, Value};

use crate::conflict::{ConflictRequest, ObjectObservation, ObservedState};
use crate::method::{MethodRequest, MethodSelection};

/// Everything [`plan_recovery`] needs, and nothing it could read for itself (§24.1, §24.2).
///
/// The observation function is the only door to the world, and it is a `Fn(&str) ->
/// ObservedState` rather than a filesystem so that Appendix C.4's 15:12 edit and Appendix I.5's
/// 12:00 package update are values a test writes down.
pub struct RecoveryRequest<'a> {
    registry: &'a ProviderRegistry,
    assets: &'a [RecoveryAsset],
    observe: &'a dyn Fn(&str) -> ObservedState,
    goal: RecoveryGoal,
    session: Arc<str>,
    now: Timestamp,
    source: Option<&'a ChangePlan>,
    applied_at: Option<Timestamp>,
    forced_method: Option<Arc<str>>,
    required_metadata: MetadataCoverage,
    required_semantics: Vec<Arc<str>>,
    restore_set: Vec<Arc<str>>,
    directory_policy: Option<DirectoryRestorePolicy>,
    captured: Vec<(Arc<str>, Arc<str>)>,
    destroyed_assets: Vec<Arc<str>>,
    discarded_bytes: Option<ByteSize>,
}

impl std::fmt::Debug for RecoveryRequest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryRequest")
            .field("assets", &self.assets.len())
            .field("goal", &self.goal)
            .field("source", &self.source.map(|plan| plan.id().short()))
            .field("restore_set", &self.restore_set)
            .finish_non_exhaustive()
    }
}

impl<'a> RecoveryRequest<'a> {
    /// A recovery from `assets`, observed through `observe`, aiming at `goal`.
    ///
    /// §5.8 allows recovering from an asset with no plan in hand, so the source plan is added
    /// separately by [`RecoveryRequest::recovering`] rather than required here.
    #[must_use]
    pub fn new(
        registry: &'a ProviderRegistry,
        assets: &'a [RecoveryAsset],
        observe: &'a dyn Fn(&str) -> ObservedState,
        goal: RecoveryGoal,
        session: impl Into<Arc<str>>,
        now: Timestamp,
    ) -> Self {
        Self {
            registry,
            assets,
            observe,
            goal,
            session: session.into(),
            now,
            source: None,
            applied_at: None,
            forced_method: None,
            required_metadata: MetadataCoverage::content_only(),
            required_semantics: Vec::new(),
            restore_set: Vec::new(),
            directory_policy: None,
            captured: Vec::new(),
            destroyed_assets: Vec::new(),
            discarded_bytes: None,
        }
    }

    /// Names the plan being recovered from (§24.1).
    #[must_use]
    pub const fn recovering(mut self, plan: &'a ChangePlan) -> Self {
        self.source = Some(plan);
        self
    }

    /// Records when that plan finished, which Appendix C.4 needs to tell its own write from a
    /// later one.
    #[must_use]
    pub const fn applied_at(mut self, at: Timestamp) -> Self {
        self.applied_at = Some(at);
        self
    }

    /// Names the method the operator asked for, overriding Appendix C.1's order.
    #[must_use]
    pub fn forcing_method(mut self, method: impl Into<Arc<str>>) -> Self {
        self.forced_method = Some(method.into());
        self
    }

    /// States which file metadata the recovery must put back (Appendix C.7).
    #[must_use]
    pub const fn requiring_metadata(mut self, required: MetadataCoverage) -> Self {
        self.required_metadata = required;
        self
    }

    /// Names an application semantic the recovery must preserve (Appendix C.1).
    #[must_use]
    pub fn requiring_semantic(mut self, semantic: impl Into<Arc<str>>) -> Self {
        self.required_semantics.push(semantic.into());
        self
    }

    /// Names an object the recovery should put back, narrowing Appendix C.2's goal.
    #[must_use]
    pub fn restoring(mut self, object: impl Into<Arc<str>>) -> Self {
        self.restore_set.push(object.into());
        self
    }

    /// Overrides what a directory restore does with files the asset never held (Appendix C.6).
    #[must_use]
    pub const fn directory_policy(mut self, policy: DirectoryRestorePolicy) -> Self {
        self.directory_policy = Some(policy);
        self
    }

    /// Records the digest the asset holds for `object` (§18.3).
    #[must_use]
    pub fn captured(mut self, object: impl Into<Arc<str>>, digest: impl Into<Arc<str>>) -> Self {
        self.captured.push((object.into(), digest.into()));
        self
    }

    /// Names a provider-native object the recovery would destroy (§13.6, §24.5).
    #[must_use]
    pub fn destroying(mut self, asset: impl Into<Arc<str>>) -> Self {
        self.destroyed_assets.push(asset.into());
        self
    }

    /// Records how much live data the recovery would discard (§24.5).
    #[must_use]
    pub const fn discarding(mut self, bytes: ByteSize) -> Self {
        self.discarded_bytes = Some(bytes);
        self
    }

    /// The assets the recovery would restore from.
    #[must_use]
    pub const fn assets(&self) -> &[RecoveryAsset] {
        self.assets
    }

    /// What the recovery is for (Appendix C.2).
    #[must_use]
    pub const fn goal(&self) -> RecoveryGoal {
        self.goal
    }

    /// The plan being recovered from, where there is one (§5.8).
    #[must_use]
    pub const fn source(&self) -> Option<&ChangePlan> {
        self.source
    }

    /// The instant the plan is being made at.
    #[must_use]
    pub const fn now(&self) -> Timestamp {
        self.now
    }
}

/// Produces the RecoveryPlan for `request`, and changes nothing (§24.1).
///
/// # Errors
///
/// - `recovery.asset_expired`, `recovery.asset_invalid` or `recovery.asset_not_found` when an
///   asset is not in a state §11.4 calls usable. A plan built on it could not run, and §2.4
///   forbids producing one that pretends otherwise.
/// - `change.action_not_plannable` when the request names a restore method outside Appendix C.1's
///   closed list (Appendix C.5).
/// - `recovery.plan_incomplete` when no offered method can meet the goal, or when the chosen
///   method's provider contributed no action to run (§56.3).
/// - whatever `ChangePlan::seal` refuses with, since §2.12 seals a recovery like any other plan.
pub fn plan_recovery(request: &RecoveryRequest<'_>) -> Result<RecoveryPlan, ErrorValue> {
    if request.assets.is_empty() {
        return Err(error::recovery_plan_incomplete(
            "a recovery asset to restore from",
            "the request named no asset, and §11.4 makes a validated asset the only thing a \
             restore can be planned against",
        ));
    }
    for asset in request.assets {
        usable(asset)?;
    }
    let offers = crate::method::offers(
        request.registry,
        request.assets,
        request.source,
        request.goal,
    );
    let mut selection_request = MethodRequest::new(request.goal, &offers)
        .requiring_metadata(request.required_metadata);
    for semantic in &request.required_semantics {
        selection_request = selection_request.requiring_semantic(Arc::clone(semantic));
    }
    if let Some(method) = &request.forced_method {
        selection_request = selection_request.forcing(Arc::clone(method));
    }
    let selection = crate::method::select(&selection_request)?;

    let restore_set = restore_set(request);
    // The observations are taken once, here, so the analysis itself is a pure function of values
    // and two runs of it over the same world produce the same plan (§4.4).
    let observations = observations(request, &restore_set);
    let impact = crate::conflict::analyse(&conflict_request(
        request,
        &selection,
        &restore_set,
        &observations,
    ));
    let unrecoverable = unrecoverable(request, &selection, &restore_set);
    let plan = change_plan(request, &selection, &restore_set)?;

    let mut recovery = RecoveryPlan::new(
        plan,
        request.goal,
        selection.chosen().method(),
        selection.chosen().reference(),
    )
    .with_newer_state(impact)
    .restoring_metadata(selection.chosen().metadata())
    .directory_policy(
        request
            .directory_policy
            .unwrap_or_else(|| selection.chosen().fragment().directory_policy()),
    );
    if let Some(source) = request.source {
        recovery = recovery.recovering(source.id().clone());
    }
    for asset in request.assets {
        recovery = recovery.using(asset.id().clone());
    }
    for object in &restore_set {
        recovery = recovery.restoring(Arc::clone(object));
    }
    for effect in unrecoverable {
        recovery = recovery.leaving(effect);
    }
    if selection.chosen().fragment().requires_reboot() {
        recovery = recovery.needing_reboot();
    }
    if selection.chosen().fragment().requires_offline() {
        recovery = recovery.needing_offline();
    }
    Ok(recovery)
}

/// Refuses an asset that is not in a state §11.4 calls usable.
fn usable(asset: &RecoveryAsset) -> Result<(), ErrorValue> {
    match asset.state() {
        AssetState::Ready => Ok(()),
        AssetState::Expired => Err(error::asset_expired(
            asset.id(),
            &asset
                .expires_at()
                .map_or_else(|| "an instant the store did not record".to_owned(), |at| at.to_string()),
        )),
        AssetState::Removed => Err(error::asset_not_found(asset.reference())),
        AssetState::Failed => Err(error::asset_invalid(
            asset.id(),
            &["the asset's creation did not succeed, so it holds no state to restore"],
        )),
        AssetState::Invalid => Err(error::asset_invalid(
            asset.id(),
            &asset
                .validation()
                .map(ono_change_core::RecoveryValidation::failures)
                .filter(|failures| !failures.is_empty())
                .unwrap_or_else(|| {
                    vec!["the asset exists and can no longer satisfy protection"]
                }),
        )),
        AssetState::Proposed | AssetState::Creating => Err(error::asset_invalid(
            asset.id(),
            &["the asset has not been created and validated yet (§11.4)"],
        )),
    }
}

/// The objects the recovery will put back (Appendix C.2).
///
/// Appendix C.2 is explicit that the goal is not automatically "return the whole persistence
/// domain to the snapshot timestamp": what the operator asked for wins, then what the original
/// plan actually changed and the assets actually cover, and only then everything the assets hold.
fn restore_set(request: &RecoveryRequest<'_>) -> Vec<Arc<str>> {
    if !request.restore_set.is_empty() {
        return dedupe(request.restore_set.clone());
    }
    if let Some(source) = request.source {
        let changed: Vec<Arc<str>> = source
            .effects()
            .into_iter()
            .filter(|effect| effect.domain().is_persistent())
            .filter_map(ProposedEffect::object)
            .filter(|object| covered(request, object))
            .map(Arc::from)
            .collect();
        if !changed.is_empty() {
            return dedupe(changed);
        }
    }
    dedupe(
        request
            .assets
            .iter()
            .flat_map(|asset| asset.scope().covers().iter().cloned())
            .collect(),
    )
}

/// Whether any source asset covers `object` (§11.2, §13.4).
fn covered(request: &RecoveryRequest<'_>, object: &str) -> bool {
    request
        .assets
        .iter()
        .any(|asset| asset.scope().covers_object(object))
}

/// The candidate restore scope, as Appendix C.3 defines the set it analyses over.
fn scope_objects(request: &RecoveryRequest<'_>, restore_set: &[Arc<str>]) -> Vec<Arc<str>> {
    let mut objects: Vec<Arc<str>> = restore_set.to_vec();
    for asset in request.assets {
        objects.extend(asset.scope().covers().iter().cloned());
    }
    let mut objects = dedupe(objects);
    objects.sort_by(|left, right| left.as_ref().cmp(right.as_ref()));
    objects
}

fn dedupe(objects: Vec<Arc<str>>) -> Vec<Arc<str>> {
    let mut kept: Vec<Arc<str>> = Vec::with_capacity(objects.len());
    for object in objects {
        if !kept.iter().any(|seen| seen == &object) {
            kept.push(object);
        }
    }
    kept
}

/// Asks the observation function about every object in the candidate restore scope.
fn observations(request: &RecoveryRequest<'_>, restore_set: &[Arc<str>]) -> Vec<ObjectObservation> {
    scope_objects(request, restore_set)
        .into_iter()
        .map(|object| {
            let state = (request.observe)(&object);
            ObjectObservation::new(object, state)
        })
        .collect()
}

fn conflict_request<'a>(
    request: &RecoveryRequest<'_>,
    selection: &MethodSelection,
    restore_set: &[Arc<str>],
    observations: &'a [ObjectObservation],
) -> ConflictRequest<'a> {
    let captured_at = request
        .assets
        .iter()
        .map(RecoveryAsset::created_at)
        .max()
        .unwrap_or(request.now);
    let mut conflict = ConflictRequest::new(selection.chosen().method(), captured_at, observations)
        .directory_policy(
            request
                .directory_policy
                .unwrap_or_else(|| selection.chosen().fragment().directory_policy()),
        );
    if let Some(at) = request.applied_at {
        conflict = conflict.applied_at(at);
    }
    for object in restore_set {
        conflict = conflict.restoring(Arc::clone(object));
    }
    for (object, digest) in &request.captured {
        conflict = conflict.captured(Arc::clone(object), Arc::clone(digest));
    }
    for asset in &request.destroyed_assets {
        conflict = conflict.destroying(Arc::clone(asset));
    }
    if let Some(bytes) = request.discarded_bytes {
        conflict = conflict.discarding(bytes);
    }
    conflict
}

/// Everything the recovery cannot reverse (§24.3, §35.2, §35.3, §55.8, Appendix F.2).
fn unrecoverable(
    request: &RecoveryRequest<'_>,
    selection: &MethodSelection,
    restore_set: &[Arc<str>],
) -> Vec<UnrecoverableEffect> {
    let mut effects: Vec<UnrecoverableEffect> = Vec::new();
    for declared in selection.chosen().fragment().unrecoverable() {
        push_unique(&mut effects, declared.clone());
    }
    let Some(source) = request.source else {
        return effects;
    };
    for action in source.actions() {
        if !action.role().mutates_target() {
            continue;
        }
        if action.status() == ActionStatus::Unknown {
            push_unique(
                &mut effects,
                UnrecoverableEffect::new(
                    action.summary(),
                    EffectDomain::Unknown,
                    "Appendix F.2: what this action did could not be established, so recovery \
                     planning treats it as an uncertainty boundary rather than assuming it \
                     happened or did not",
                ),
            );
        }
        for effect in action.effects() {
            if let Some(unrecoverable) = classify_effect(effect, restore_set, request) {
                push_unique(&mut effects, unrecoverable);
            }
        }
    }
    effects
}

/// Whether one of the original plan's effects is beyond this recovery's reach.
fn classify_effect(
    effect: &ProposedEffect,
    restore_set: &[Arc<str>],
    request: &RecoveryRequest<'_>,
) -> Option<UnrecoverableEffect> {
    let subject = effect.object().unwrap_or_else(|| effect.explanation());
    if effect.kind() == EffectKind::Emit {
        let unrecoverable = UnrecoverableEffect::new(
            subject,
            effect.domain(),
            "§35.1: the call left the system before recovery could reconsider it, and §35.3 makes \
             a provider's inverse action COMPENSATABLE rather than a restore of prior state",
        );
        return Some(match effect.compensation() {
            Some(compensation) => unrecoverable.compensated_by(compensation),
            None => unrecoverable,
        });
    }
    match effect.domain() {
        EffectDomain::ExternalSideEffect | EffectDomain::RemoteSystem => Some(
            UnrecoverableEffect::new(
                subject,
                effect.domain(),
                "§35.2: an effect outside this machine is not recoverable through a local \
                 snapshot, and it stays separately visible",
            ),
        ),
        EffectDomain::NetworkRuntime => Some(UnrecoverableEffect::new(
            subject,
            effect.domain(),
            "§34: live sessions and in-flight connections are not restored by any recovery asset",
        )),
        EffectDomain::Unknown => Some(UnrecoverableEffect::new(
            subject,
            effect.domain(),
            "Appendix A.7 and §6.3: the provider declared a domain Ono cannot classify, so no \
             restore elsewhere on the host says anything about it",
        )),
        domain if domain.is_persistent() => {
            let restored = restore_set.iter().any(|object| object.as_ref() == subject);
            if restored && covered(request, subject) {
                return None;
            }
            Some(UnrecoverableEffect::new(
                subject,
                domain,
                "§55.8: the source assets cover only part of what the plan changed, and restoring \
                 what they hold puts nothing back here",
            ))
        }
        _ if effect.is_irreversible() => Some(UnrecoverableEffect::new(
            subject,
            effect.domain(),
            "§2.13: the plan declared this effect irreversible, and no asset changes that",
        )),
        _ => None,
    }
}

fn push_unique(effects: &mut Vec<UnrecoverableEffect>, effect: UnrecoverableEffect) {
    let duplicate = effects
        .iter()
        .any(|kept| kept.subject() == effect.subject() && kept.domain() == effect.domain());
    if !duplicate {
        effects.push(effect);
    }
}

/// The recovery's own `ChangePlan`: RECOVER actions, contracts, risk, sealed (§2.12, §24.1).
fn change_plan(
    request: &RecoveryRequest<'_>,
    selection: &MethodSelection,
    restore_set: &[Arc<str>],
) -> Result<ChangePlan, ErrorValue> {
    let fragment = selection.chosen().fragment();
    if fragment.actions().is_empty() {
        return Err(error::recovery_plan_incomplete(
            "the actions that would carry the recovery out",
            &format!(
                "{} offered {} and contributed no action to run, so there is nothing to apply",
                selection.chosen().provider(),
                selection.chosen().method()
            ),
        ));
    }
    let intent = Intent::new(
        format!(
            "restore {} from {}",
            describe(restore_set),
            selection.chosen().reference()
        ),
        request.source.map_or_else(
            || format!("recover {}", selection.chosen().reference()),
            |source| format!("recover plan/{}", source.id().short()),
        ),
    );
    let mut plan = ChangePlan::draft(intent, Arc::clone(&request.session), request.now)
        .as_recovery()
        .resolve(
            restore_set
                .iter()
                .map(|object| {
                    FrozenTarget::new("ono.recovery.object", Arc::clone(object), Arc::clone(object))
                        .in_domain(selection.chosen().reference())
                })
                .collect(),
        )?;
    let plan_id = plan.id().clone();
    let mut mapping: Vec<(Arc<str>, ono_change_core::ActionId)> = Vec::new();
    for (index, action) in fragment.actions().iter().enumerate() {
        let recovery_action = recover_action(&plan_id, index, action, &mapping);
        mapping.push((
            Arc::from(action.id().as_str()),
            recovery_action.id().clone(),
        ));
        plan = plan.with_action(recovery_action)?;
    }
    plan = plan
        .with_verification(verification(&plan_id, selection, restore_set, request))
        .with_risk(risk(request, selection));
    for binding in bindings(request, selection) {
        plan = plan.binding(binding);
    }
    plan.seal(request.now)
}

/// One provider action, re-anchored to this plan and put in the RECOVER role (§3.3).
///
/// A fragment's action belongs to the provider's answer, not to a plan: its identity is derived
/// from whichever plan it was built against. Rebuilding it here gives the recovery plan actions
/// whose ids belong to it, which is what §4.4's seal digest is taken over, and re-anchoring the
/// dependencies keeps the provider's ordering intact.
fn recover_action(
    plan: &ono_change_core::PlanId,
    index: usize,
    action: &PlanAction,
    mapping: &[(Arc<str>, ono_change_core::ActionId)],
) -> PlanAction {
    let mut rebuilt = PlanAction::new(
        plan,
        index + 1,
        ActionRole::Recover,
        action.summary(),
        action.execution().clone(),
    )
    .with_idempotency(action.idempotency());
    if let Some(target) = action.target() {
        rebuilt = rebuilt.on(target);
    }
    if let Some(semantics) = action.declared_recovery() {
        rebuilt = rebuilt.recovery_semantics(semantics);
    }
    if action.needs_privilege() {
        rebuilt = rebuilt.privileged();
    }
    for precondition in action.preconditions() {
        rebuilt = rebuilt.requiring(precondition.clone());
    }
    for dependency in action.depends_on() {
        if let Some((_, rebuilt_id)) = mapping
            .iter()
            .find(|(original, _)| original.as_ref() == dependency.as_str())
        {
            rebuilt = rebuilt.after(rebuilt_id.clone());
        }
    }
    rebuilt
}

/// The recovery's verification contracts (§23.1, §25.1).
///
/// The provider's own contracts come first. Where it offered none for an object the recovery
/// restores, one is added: §23.1 requires a mutating plan to carry a contract, §25.1 requires a
/// recovery verification to say which equivalence domain it reports on, and a restore whose
/// success nobody stated a condition for cannot be verified at all (§62.9).
fn verification(
    plan: &ono_change_core::PlanId,
    selection: &MethodSelection,
    restore_set: &[Arc<str>],
    request: &RecoveryRequest<'_>,
) -> VerificationSet {
    let mut set = VerificationSet::empty();
    for contract in selection.chosen().fragment().verification() {
        set = set.with(rebind(plan, contract));
    }
    for object in restore_set {
        if set
            .contracts()
            .iter()
            .any(|contract| contract.subject() == object.as_ref())
        {
            continue;
        }
        let digest = request
            .captured
            .iter()
            .find(|(name, _)| name == object)
            .map(|(_, digest)| Arc::clone(digest));
        let mut contract = VerificationContract::new(
            plan,
            VerificationClass::Required,
            Arc::clone(object),
            digest.as_ref().map_or_else(
                || "content == recovery-point".to_owned(),
                |digest| format!("digest == {digest}"),
            ),
        )
        .about(EquivalenceDomain::PersistentState);
        if let Some(digest) = digest {
            contract = contract.expecting(Value::string(&*digest));
        }
        set = set.with(contract);
    }
    set
}

/// The same contract, anchored to the recovery plan that will carry it (§4.4).
fn rebind(plan: &ono_change_core::PlanId, contract: &VerificationContract) -> VerificationContract {
    let mut rebound = VerificationContract::new(
        plan,
        contract.class(),
        contract.subject(),
        contract.expression(),
    )
    .within(contract.timeout());
    if let Some(expected) = contract.expected() {
        rebound = rebound.expecting(expected.clone());
    }
    if let Some(domain) = contract.equivalence() {
        rebound = rebound.about(domain);
    }
    if contract.timeout_status() == ono_change_core::VerificationStatus::Unknown {
        rebound = rebound.timeout_is_unknown();
    }
    rebound
}

/// The recovery's own risk (§19, §24.4).
///
/// §24.4's worked example prints `MODERATE` for a selective restore that preserves everything
/// unrelated, and that falls out of the first finding rather than being asserted: a recovery is a
/// change (§2.12), so it is never `LOW`, and it rises from there with what it would take away.
fn risk(request: &RecoveryRequest<'_>, selection: &MethodSelection) -> RiskAssessment {
    let method = selection.chosen().method();
    let mut assessment = RiskAssessment::empty().with(RiskFinding::new(
        RiskDimension::RecoveryComplexity,
        method_risk(method),
        "recovery.method",
        format!(
            "recovery by {method}, which restores {}",
            if method.restores_prior_state() {
                "prior state"
            } else {
                "an acceptable semantic state through inverse actions (§27.4)"
            }
        ),
    ));
    if !request.destroyed_assets.is_empty() {
        assessment = assessment.with(RiskFinding::new(
            RiskDimension::Scope,
            RiskClass::High,
            "recovery.destroyed-history",
            format!(
                "{} provider-native object(s) would be destroyed (§13.6)",
                request.destroyed_assets.len()
            ),
        ));
    }
    if selection.chosen().fragment().requires_offline() {
        assessment = assessment.with(RiskFinding::new(
            RiskDimension::Downtime,
            RiskClass::High,
            "recovery.offline",
            "the filesystem must be taken offline for this recovery (§13.7, §14.6)",
        ));
    }
    if selection.chosen().fragment().requires_reboot() {
        assessment = assessment.with(RiskFinding::new(
            RiskDimension::RebootRequirement,
            RiskClass::High,
            "recovery.reboot",
            "the recovery takes effect on the next boot (§13.7, §14.6)",
        ));
    }
    assessment
}

const fn method_risk(method: RestoreMethod) -> RiskClass {
    match method {
        RestoreMethod::ProviderNativeRestore
        | RestoreMethod::SelectiveFileRestore
        | RestoreMethod::CloneAndCopy
        | RestoreMethod::Compensation => RiskClass::Moderate,
        RestoreMethod::SubvolumeReplacement | RestoreMethod::DatasetRollback => RiskClass::High,
        RestoreMethod::OfflineRootRecovery => RiskClass::Critical,
    }
}

/// The providers the recovery plan is bound to, at the version they were resolved against (§4.4).
fn bindings(request: &RecoveryRequest<'_>, selection: &MethodSelection) -> Vec<ProviderBinding> {
    request
        .registry
        .get(selection.chosen().provider())
        .map(|provider| match provider.availability() {
            ono_change_core::ProviderAvailability::Available { version } => {
                vec![ProviderBinding::new(provider.id(), version)]
            }
            ono_change_core::ProviderAvailability::Unsupported { version, .. } => {
                vec![ProviderBinding::new(provider.id(), version)]
            }
            ono_change_core::ProviderAvailability::Unavailable { .. } => Vec::new(),
        })
        .unwrap_or_default()
}

fn describe(restore_set: &[Arc<str>]) -> String {
    match restore_set {
        [] => "the captured state".to_owned(),
        [only] => only.to_string(),
        [first, rest @ ..] => format!("{first} and {} more object(s)", rest.len()),
    }
}
