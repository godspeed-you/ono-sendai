//! Turning an intent into a sealed plan (spec v0.6 §4.2–§4.4, §5.1–§5.3, §6.2, §6.3).
//!
//! §4.2 opens a draft, §4.3 freezes the targets, §4.4 seals. [`PlanBuilder`] is those three steps
//! and nothing else: it resolves no providers, opens no files and consults no world. Actions
//! arrive as [`PlanFragment`]s that a change provider produced, which is what lets §55's cases be
//! written as tests rather than as observations of a machine.
//!
//! Two rules of §5 are properties of this module rather than of the caller:
//!
//! - **§5.3's default is one plan.** *"`get service | where state == failed | plan restart
//!   service` MUST produce one plan with frozen resolved targets, unless the user explicitly
//!   requests one plan per input object."* [`PlanBuilder::seal`] is the one plan;
//!   [`PlanGranularity::PlanPerObject`] is the explicit request, and it has to be asked for by
//!   name.
//! - **§5.2's block is not a language.** *"The v0.6 plan block MUST NOT introduce loops, arbitrary
//!   functions, background jobs or unbounded runtime control flow."* [`BlockPlan`] accepts a
//!   bounded list of action descriptions and refuses each of those four constructs by name. It
//!   parses nothing — `ono-parser` produced the block, and what is owned here is the judgement
//!   about what may be in it.
//!
//! §6.2 and §6.3 meet in [`external_command`]: an arbitrary command refuses, and the same call
//! with §6.3's explicit acknowledgement produces an [`Execution::Opaque`] action whose single
//! effect is [`EffectConfidence::Unknown`] in [`EffectDomain::Unknown`]. That effect is what makes
//! Appendix A.7 cap the plan, so an opaque command cannot inherit a filesystem snapshot's safety.

use std::collections::BTreeMap;
use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ActionId, ActionRole, ChangePlan, EffectConfidence, EffectDomain, EffectKind, Execution,
    FrozenTarget, Idempotency, ImpactGraph, Intent, PlanAction, PlanFragment, PlanId, PlanKind,
    ProposedEffect, ProtectionMode, ProtectionSummary, ProviderBinding, RiskAssessment, Strategy,
    VerificationContract, VerificationSet, VerificationStatus, error,
};
use ono_value::ErrorValue;

use crate::secrets::SecretRedaction;

/// How many action descriptions one §5.2 plan block may hold.
///
/// §5.2 requires the block to be bounded without naming a number, and a number is what a
/// refusal needs in order to be a refusal rather than an eventual memory problem. Sixty-four is
/// far above any block a person writes by hand and far below anything that could be a generated
/// program, which is the line §5.2 is drawing.
pub const MAX_BLOCK_ACTIONS: usize = 64;

/// The version a plan records for a provider that declared none (§2.4, §4.4).
const UNKNOWN_VERSION: &str = "unknown";

/// Whether a pipeline produces one plan or one plan per input object (§5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanGranularity {
    /// One plan over the whole frozen set. §5.3's default, and §28.2's frozen membership.
    #[default]
    SinglePlan,
    /// One plan per input object, which §5.3 requires the user to ask for explicitly.
    PlanPerObject,
}

/// Whether §6.3's acknowledgement was given for an opaque action (§6.2, §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpaqueEscape {
    /// No acknowledgement. §6.2 refuses the command.
    #[default]
    Refused,
    /// §6.3's explicit risk acknowledgement, which admits an opaque action classified as unknown.
    Acknowledged,
}

/// One action description inside a §5.2 plan block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDescription {
    verb: Arc<str>,
    source: Arc<str>,
}

impl ActionDescription {
    /// The description of `verb`, as the operator wrote it in `source`.
    #[must_use]
    pub fn new(verb: impl Into<Arc<str>>, source: impl Into<Arc<str>>) -> Self {
        Self {
            verb: verb.into(),
            source: source.into(),
        }
    }

    /// The verb the description begins with — `replace`, `validate`, `restart`, `verify`.
    #[must_use]
    pub fn verb(&self) -> &str {
        &self.verb
    }

    /// The line the operator wrote.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// One statement the parser found inside a `plan { … }` block (§5.2).
///
/// Everything except [`BlockStatement::Action`] exists so that a refusal can name the construct
/// it refused. §5.2 forbids four things by name, and a type that could not represent them would
/// leave the CLI deciding which of them to mention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockStatement {
    /// An action description, which is all §5.2 permits.
    Action(ActionDescription),
    /// A loop. §5.2: the plan block MUST NOT introduce loops.
    Loop {
        /// The statement as the operator wrote it.
        source: Arc<str>,
    },
    /// A function definition. §5.2: the plan block MUST NOT introduce arbitrary functions.
    Function {
        /// The statement as the operator wrote it.
        source: Arc<str>,
    },
    /// A background job. §5.2: the plan block MUST NOT introduce background jobs.
    BackgroundJob {
        /// The statement as the operator wrote it.
        source: Arc<str>,
    },
    /// A conditional, a branch or a jump. §5.2: no unbounded runtime control flow.
    ControlFlow {
        /// The statement as the operator wrote it.
        source: Arc<str>,
    },
}

impl BlockStatement {
    /// An action description statement.
    #[must_use]
    pub fn action(verb: impl Into<Arc<str>>, source: impl Into<Arc<str>>) -> Self {
        Self::Action(ActionDescription::new(verb, source))
    }

    /// The statement as the operator wrote it.
    #[must_use]
    pub fn source(&self) -> &str {
        match self {
            BlockStatement::Action(description) => description.source(),
            BlockStatement::Loop { source }
            | BlockStatement::Function { source }
            | BlockStatement::BackgroundJob { source }
            | BlockStatement::ControlFlow { source } => source,
        }
    }

    /// The name §5.2 refuses this statement under, or `None` for an action description.
    #[must_use]
    pub const fn forbidden_construct(&self) -> Option<&'static str> {
        match self {
            BlockStatement::Action(_) => None,
            BlockStatement::Loop { .. } => Some("a loop"),
            BlockStatement::Function { .. } => Some("an arbitrary function"),
            BlockStatement::BackgroundJob { .. } => Some("a background job"),
            BlockStatement::ControlFlow { .. } => Some("unbounded runtime control flow"),
        }
    }
}

/// A validated §5.2 plan block: a bounded sequence of action descriptions and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockPlan {
    actions: Vec<ActionDescription>,
}

impl BlockPlan {
    /// Validates `statements` as a plan block (§5.2).
    ///
    /// # Errors
    ///
    /// Returns `change.action_not_plannable` for an empty block, for a block longer than
    /// [`MAX_BLOCK_ACTIONS`], and for each construct §5.2 forbids, naming the one it found.
    pub fn of(statements: Vec<BlockStatement>) -> Result<Self, ErrorValue> {
        if statements.is_empty() {
            return Err(error::action_not_plannable(
                "plan { }",
                "§5.2: a plan block is a sequence of action descriptions, and this one describes \
                 no action.",
            ));
        }
        if statements.len() > MAX_BLOCK_ACTIONS {
            return Err(error::action_not_plannable(
                "plan { … }",
                &format!(
                    "§5.2: the plan block is a bounded list of action descriptions and is not a \
                     general-purpose workflow language. This block holds {} statements, and the \
                     bound is {MAX_BLOCK_ACTIONS}.",
                    statements.len()
                ),
            ));
        }
        let mut actions = Vec::with_capacity(statements.len());
        for statement in statements {
            if let Some(construct) = statement.forbidden_construct() {
                return Err(error::action_not_plannable(
                    statement.source(),
                    &format!(
                        "§5.2: the v0.6 plan block MUST NOT introduce loops, arbitrary functions, \
                         background jobs or unbounded runtime control flow, and this is \
                         {construct}."
                    ),
                ));
            }
            let BlockStatement::Action(description) = statement else {
                continue;
            };
            if description.verb().trim().is_empty() {
                return Err(error::action_not_plannable(
                    description.source(),
                    "§6.1: an operation is plannable only if it resolves to a provider contract, \
                     and a statement with no verb names no operation.",
                ));
            }
            actions.push(description);
        }
        Ok(Self { actions })
    }

    /// The action descriptions, in the order the block wrote them.
    #[must_use]
    pub fn actions(&self) -> &[ActionDescription] {
        &self.actions
    }

    /// How many actions the block describes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Always `false`: [`BlockPlan::of`] refuses an empty block (§5.2).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

/// An external command as a plan entry (§6.2, §6.3).
///
/// With [`OpaqueEscape::Refused`] this is §6.2: *"`plan sh -c 'rm -rf /somewhere'` MUST fail by
/// default because Ono cannot reason about target scope or side effects."*
///
/// With [`OpaqueEscape::Acknowledged`] it is §6.3's escape. The action's single proposed effect is
/// [`EffectKind::Unknown`] at [`EffectConfidence::Unknown`] in [`EffectDomain::Unknown`], which is
/// exactly what Appendix A.7 caps a plan on. §6.3's own sentence — *"Opaque actions MUST NOT
/// receive a `PROTECTED` status merely because a filesystem snapshot exists somewhere on the
/// host"* — is then a consequence of the coverage algorithm rather than a rule anybody has to
/// apply here.
///
/// # Errors
///
/// Returns `change.opaque_action_forbidden` when the acknowledgement was withheld (§6.2).
pub fn external_command(
    plan: &PlanId,
    ordinal: usize,
    command: &str,
    program: Option<&str>,
    argv: &[Arc<str>],
    escape: OpaqueEscape,
) -> Result<PlanAction, ErrorValue> {
    if escape == OpaqueEscape::Refused {
        return Err(error::opaque_action_forbidden(command));
    }
    let action = PlanAction::new(
        plan,
        ordinal,
        ActionRole::Mutate,
        format!("opaque action: {command}"),
        Execution::Opaque {
            description: Arc::from(command),
            program: program.map(Arc::from),
            argv: argv.to_vec(),
        },
    );
    let id = action.id().clone();
    Ok(action
        .with_idempotency(Idempotency::Unknown)
        .recovery_semantics(
            "none declared: §6.3 classifies an opaque action's impact and reversibility as unknown",
        )
        .effecting(ProposedEffect::new(
            id,
            EffectDomain::Unknown,
            EffectKind::Unknown,
            EffectConfidence::Unknown,
            "the operator acknowledged that Ono cannot reason about what this command touches \
             (§6.3), so its domain, its scope and its reversibility are all unknown",
        )))
}

/// A draft plan being assembled out of provider fragments (§4.2).
#[derive(Debug, Clone)]
pub struct PlanBuilder {
    id: PlanId,
    kind: PlanKind,
    intent: Intent,
    session: Arc<str>,
    created_at: Timestamp,
    targets: Vec<FrozenTarget>,
    actions: Vec<PlanAction>,
    verification: Vec<VerificationContract>,
    providers: Vec<ProviderBinding>,
    impact: ImpactGraph,
    protection: ProtectionSummary,
    protection_mode: ProtectionMode,
    risk: RiskAssessment,
    strategy: Strategy,
    expires_at: Option<Timestamp>,
    redaction: SecretRedaction,
}

impl PlanBuilder {
    /// Opens a draft for `intent` in `session` at `now` (§4.2).
    ///
    /// `now` is a parameter because §55.1 asks planning to be reproducible, and a plan's identity
    /// is derived from the session, the instant and the intent (§3.2).
    #[must_use]
    pub fn for_intent(intent: Intent, session: impl Into<Arc<str>>, now: Timestamp) -> Self {
        let session = session.into();
        let id = PlanId::of(&session, &now.to_string(), intent.text());
        Self {
            id,
            kind: PlanKind::Change,
            intent,
            session,
            created_at: now,
            targets: Vec::new(),
            actions: Vec::new(),
            verification: Vec::new(),
            providers: Vec::new(),
            impact: ImpactGraph::empty(),
            protection: ProtectionSummary::empty(),
            protection_mode: ProtectionMode::Prefer,
            risk: RiskAssessment::empty(),
            strategy: Strategy::Sequential,
            expires_at: None,
            redaction: SecretRedaction::new(),
        }
    }

    /// The identity the sealed plan will carry, so a provider can build a fragment against it.
    ///
    /// A [`PlanAction`]'s identity is derived from the plan it belongs to (§3.3), so a provider
    /// needs this before it can produce one.
    #[must_use]
    pub const fn plan_id(&self) -> &PlanId {
        &self.id
    }

    /// Marks the plan under construction as a recovery plan (§3.8, §24.1).
    #[must_use]
    pub const fn as_recovery(mut self) -> Self {
        self.kind = PlanKind::Recovery;
        self
    }

    /// The redaction applied to action arguments before the seal is computed (§36.3).
    ///
    /// Redacting before sealing is what keeps the seal verifiable after a round trip through the
    /// store: §4.4's digest covers the handle, so the plan a store reads back still verifies
    /// against its own digest.
    #[must_use]
    pub fn redacting(mut self, redaction: SecretRedaction) -> Self {
        self.redaction = redaction;
        self
    }

    /// Adds a change provider's contribution to the draft (§6.1).
    ///
    /// The fragment's actions are renumbered onto the end of the plan, and their dependencies are
    /// carried across, so two providers contributing to one plan cannot collide on an ordinal.
    /// A fragment's own preconditions join its first action, because §7.2 puts preconditions on
    /// actions and a fragment-level one is a condition of the fragment's first step.
    ///
    /// # Errors
    ///
    /// Returns `change.action_not_plannable` for a fragment that declares preconditions or
    /// verification and no action at all: §7.2's preconditions belong to an action, and a
    /// contribution with nothing to run cannot carry them.
    pub fn contributing(mut self, fragment: &PlanFragment) -> Result<Self, ErrorValue> {
        if fragment.actions().is_empty() {
            if fragment.preconditions().is_empty() && fragment.verification().is_empty() {
                return Ok(self);
            }
            return Err(error::action_not_plannable(
                self.intent.source(),
                "§7.2 declares preconditions on actions, and this provider contributed \
                 preconditions with no action to attach them to.",
            ));
        }
        let version = fragment.provider_version().unwrap_or(UNKNOWN_VERSION);
        let mut remap: BTreeMap<ActionId, ActionId> = BTreeMap::new();
        for (index, action) in fragment.actions().iter().enumerate() {
            let ordinal = self.actions.len() + 1;
            let extra = if index == 0 {
                fragment.preconditions()
            } else {
                &[]
            };
            let rebound = rebind(&self.id, ordinal, action, &remap, extra, None);
            remap.insert(action.id().clone(), rebound.id().clone());
            let binding = ProviderBinding::new(rebound.execution().actor(), version);
            if !self.providers.contains(&binding) {
                self.providers.push(binding);
            }
            self.actions.push(rebound);
        }
        for contract in fragment.verification() {
            self.verification.push(contract.clone());
        }
        Ok(self)
    }

    /// Adds one action a caller built itself, renumbered onto the end of the plan.
    #[must_use]
    pub fn acting(mut self, action: &PlanAction) -> Self {
        let ordinal = self.actions.len() + 1;
        let rebound = rebind(&self.id, ordinal, action, &BTreeMap::new(), &[], None);
        let binding = ProviderBinding::new(rebound.execution().actor(), UNKNOWN_VERSION);
        if !self.providers.contains(&binding) {
            self.providers.push(binding);
        }
        self.actions.push(rebound);
        self
    }

    /// Adds a verification contract the plan must carry (§23.1).
    #[must_use]
    pub fn verifying(mut self, contract: VerificationContract) -> Self {
        self.verification.push(contract);
        self
    }

    /// Freezes the resolved target set and leaves the draft in `RESOLVED` (§4.3).
    ///
    /// # Errors
    ///
    /// Returns `change.target_unresolved` for an empty set: §4.3 turns selectors into concrete
    /// object identities, and a selector that matched nothing produced no plan.
    pub fn resolve(self, targets: Vec<FrozenTarget>) -> Result<Self, ErrorValue> {
        self.resolve_streaming(targets.into_iter().map(Ok))
    }

    /// Freezes a target set arriving one object at a time (§4.3, §52.5).
    ///
    /// §52.5 requires plans with thousands of targets to use "bounded memory and streaming
    /// resolution, but final sealed target identity lists must be durable". The source is
    /// consumed as an iterator, so nothing between the world and the plan is materialised twice;
    /// what survives is the identity list, which is the part the seal and the store both need.
    ///
    /// # Errors
    ///
    /// Returns the first refusal the source produced, or `change.target_unresolved` for an empty
    /// set.
    pub fn resolve_streaming(
        mut self,
        targets: impl IntoIterator<Item = Result<FrozenTarget, ErrorValue>>,
    ) -> Result<Self, ErrorValue> {
        let mut frozen = Vec::new();
        for target in targets {
            frozen.push(target?);
        }
        if frozen.is_empty() {
            return Err(error::target_unresolved(
                self.intent.source(),
                "§4.3 freezes the set of objects that match now, and nothing matched.",
            ));
        }
        self.targets = frozen;
        Ok(self)
    }

    /// Sets the impact graph (§9).
    #[must_use]
    pub fn with_impact(mut self, impact: ImpactGraph) -> Self {
        self.impact = impact;
        self
    }

    /// Sets the coverage matrix (§10.3).
    #[must_use]
    pub fn with_protection(mut self, protection: ProtectionSummary) -> Self {
        self.protection = protection;
        self
    }

    /// Sets the protection policy mode the plan runs under (§17.3).
    #[must_use]
    pub const fn with_protection_mode(mut self, mode: ProtectionMode) -> Self {
        self.protection_mode = mode;
        self
    }

    /// Sets the risk assessment (§19).
    #[must_use]
    pub fn with_risk(mut self, risk: RiskAssessment) -> Self {
        self.risk = risk;
        self
    }

    /// Sets the execution strategy (§28.4).
    #[must_use]
    pub const fn with_strategy(mut self, strategy: Strategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Sets when the sealed plan stops being appliable (§4.1's `EXPIRED`, §5.6).
    #[must_use]
    pub const fn expiring_at(mut self, at: Timestamp) -> Self {
        self.expires_at = Some(at);
        self
    }

    /// The frozen targets, as the draft holds them.
    #[must_use]
    pub fn targets(&self) -> &[FrozenTarget] {
        &self.targets
    }

    /// The actions, in plan order.
    #[must_use]
    pub fn actions(&self) -> &[PlanAction] {
        &self.actions
    }

    /// Seals one plan over the whole frozen set (§4.4, §5.3's default, §28.2).
    ///
    /// # Errors
    ///
    /// - `change.target_unresolved` when nothing was resolved (§4.3);
    /// - `change.action_not_plannable` when the action graph has a cycle (§3.2);
    /// - `change.verification_missing` when a mutating plan carries no contract (§23.1).
    pub fn seal(self, now: Timestamp) -> Result<ChangePlan, ErrorValue> {
        let mut plans = self.seal_with(PlanGranularity::SinglePlan, now)?;
        plans.pop().ok_or_else(|| {
            error::target_unresolved(
                "plan",
                "§4.3 freezes the set of objects that match now, and nothing matched.",
            )
        })
    }

    /// Seals the plan or plans `granularity` asks for (§5.3).
    ///
    /// [`PlanGranularity::SinglePlan`] answers with one plan over the frozen set, which is what
    /// §5.3 requires of `get service | where state == failed | plan restart service`.
    /// [`PlanGranularity::PlanPerObject`] is the explicit request §5.3 permits: one plan per
    /// input object, each with its own identity, its own actions and its own seal.
    ///
    /// # Errors
    ///
    /// As [`PlanBuilder::seal`].
    pub fn seal_with(
        self,
        granularity: PlanGranularity,
        now: Timestamp,
    ) -> Result<Vec<ChangePlan>, ErrorValue> {
        if self.targets.is_empty() {
            return Err(error::target_unresolved(
                self.intent.source(),
                "§4.3 freezes the set of objects that match now, and nothing matched.",
            ));
        }
        match granularity {
            PlanGranularity::SinglePlan => {
                let plan = self.assemble(
                    self.id.clone(),
                    self.intent.clone(),
                    self.targets.clone(),
                    self.actions.clone(),
                    self.verification.clone(),
                    now,
                )?;
                Ok(vec![plan])
            }
            PlanGranularity::PlanPerObject => {
                let mut plans = Vec::with_capacity(self.targets.len());
                for target in &self.targets {
                    plans.push(self.per_object(target, now)?);
                }
                Ok(plans)
            }
        }
    }

    /// One plan covering `target` alone (§5.3's explicit per-object request).
    fn per_object(&self, target: &FrozenTarget, now: Timestamp) -> Result<ChangePlan, ErrorValue> {
        // A plan's identity is derived from its intent (§3.2), so one intent cannot produce
        // several plans. The object the plan is about is therefore part of the intent it states,
        // which is also how it reads: "restart service (systemd:nginx.service)".
        let intent = Intent::new(
            format!("{} ({})", self.intent.text(), target.identity()),
            self.intent.source(),
        );
        let id = PlanId::of(&self.session, &self.created_at.to_string(), intent.text());
        let actions: Vec<&PlanAction> = self
            .actions
            .iter()
            .filter(|action| {
                action
                    .target()
                    .is_none_or(|named| named == target.identity())
            })
            .collect();
        let mut remap: BTreeMap<ActionId, ActionId> = BTreeMap::new();
        let mut rebound = Vec::with_capacity(actions.len());
        for (index, action) in actions.iter().enumerate() {
            let next = rebind(&id, index + 1, action, &remap, &[], None);
            remap.insert(action.id().clone(), next.id().clone());
            rebound.push(next);
        }
        let named: Vec<VerificationContract> = self
            .verification
            .iter()
            .filter(|contract| {
                contract.subject() == target.identity() || contract.subject() == target.label()
            })
            .cloned()
            .collect();
        // §23.1 binds verification to the plan rather than to one object. Where no contract names
        // this object, the block's contracts travel with every plan it produced, because a
        // mutating plan without one cannot be sealed at all.
        let contracts = if named.is_empty() {
            self.verification.clone()
        } else {
            named
        };
        self.assemble(id, intent, vec![target.clone()], rebound, contracts, now)
    }

    /// Builds and seals one plan out of an already decided identity, target set and action list.
    fn assemble(
        &self,
        id: PlanId,
        intent: Intent,
        targets: Vec<FrozenTarget>,
        actions: Vec<PlanAction>,
        contracts: Vec<VerificationContract>,
        now: Timestamp,
    ) -> Result<ChangePlan, ErrorValue> {
        let mut plan = ChangePlan::draft(intent, Arc::clone(&self.session), self.created_at);
        if self.kind == PlanKind::Recovery {
            plan = plan.as_recovery();
        }
        plan = plan.resolve(targets)?;
        for action in &actions {
            // §36.3: the secret leaves the plan before §4.4's digest is taken over it, so a plan
            // read back out of the store still verifies against its own seal.
            let redacted = self.redaction.execution(action.execution());
            let ordinal = action.ordinal();
            plan = plan.with_action(rebind(
                &id,
                ordinal,
                action,
                &BTreeMap::new(),
                &[],
                Some(redacted),
            ))?;
        }
        let contracts = contracts
            .iter()
            .map(|contract| rebind_contract(&id, contract))
            .collect();
        plan = plan
            .with_impact(self.impact.clone())
            .with_protection(self.protection.clone())
            .with_protection_mode(self.protection_mode)
            .with_risk(self.risk.clone())
            .with_strategy(self.strategy)
            .with_verification(VerificationSet::of(contracts));
        for binding in &self.providers {
            plan = plan.binding(binding.clone());
        }
        if let Some(at) = self.expires_at {
            plan = plan.expiring_at(at);
        }
        plan.seal(now)
    }
}

/// One action rebuilt against `plan` at `ordinal`, with its dependencies remapped.
///
/// `PlanAction` derives its identity from the plan, the ordinal and the summary (§3.3), and every
/// effect derives its own from the action. So moving an action between plans, or renumbering it
/// inside one, means rebuilding both — a copy that kept the old identities would leave a plan
/// referring to actions that are not in it.
///
/// `extra` joins the action's own preconditions and `execution` replaces its execution where the
/// caller has one; both are how the builder attaches a fragment's preconditions and §36.3's
/// redaction without a second copy of this function.
fn rebind(
    plan: &PlanId,
    ordinal: usize,
    action: &PlanAction,
    remap: &BTreeMap<ActionId, ActionId>,
    extra: &[ono_change_core::Precondition],
    execution: Option<Execution>,
) -> PlanAction {
    let execution = execution.unwrap_or_else(|| action.execution().clone());
    let mut next = PlanAction::new(plan, ordinal, action.role(), action.summary(), execution);
    let id = next.id().clone();
    if let Some(target) = action.target() {
        next = next.on(target);
    }
    for dependency in action.depends_on() {
        next = next.after(remap.get(dependency).unwrap_or(dependency).clone());
    }
    for precondition in extra.iter().chain(action.preconditions()) {
        next = next.requiring(precondition.clone());
    }
    next = next.with_idempotency(action.idempotency());
    for effect in action.effects() {
        let mut moved = ProposedEffect::new(
            id.clone(),
            effect.domain(),
            effect.kind(),
            effect.confidence(),
            effect.explanation(),
        )
        .from_to(effect.before().cloned(), effect.proposed().cloned());
        if let Some(object) = effect.object() {
            moved = moved.on(object);
        }
        for evidence in effect.evidence() {
            moved = moved.citing(Arc::clone(evidence));
        }
        if effect.is_irreversible() {
            moved = moved.irreversible();
        }
        if let Some(compensation) = effect.compensation() {
            moved = moved.compensated_by(compensation);
        }
        next = next.effecting(moved);
    }
    if let Some(semantics) = action.declared_recovery() {
        next = next.recovery_semantics(semantics);
    }
    if action.needs_privilege() {
        next = next.privileged();
    }
    next.with_status(action.status())
}

/// One verification contract rebuilt against `plan` (§23.1).
///
/// A [`ono_change_core::CheckId`] is derived from the plan, the subject and the expression, so a
/// contract that travelled from one plan to another — a fragment built against the draft, a
/// per-object plan of §5.3 — carries an identity that belongs to a different plan. §4.4 seals the
/// verification contracts, and a store reading the plan back derives the identity from the plan it
/// is in, so a contract that was not rebound would make the seal stop verifying after a round trip.
fn rebind_contract(plan: &PlanId, contract: &VerificationContract) -> VerificationContract {
    let mut next = VerificationContract::new(
        plan,
        contract.class(),
        contract.subject(),
        contract.expression(),
    )
    .within(contract.timeout());
    if contract.timeout_status() == VerificationStatus::Unknown {
        next = next.timeout_is_unknown();
    }
    if let Some(expected) = contract.expected() {
        next = next.expecting(expected.clone());
    }
    if let Some(domain) = contract.equivalence() {
        next = next.about(domain);
    }
    next
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use ono_change_core::{
        ConsistencyClass, DomainCoverage, DomainProtection, PlanState, Precondition,
        PreconditionKind, ProtectionLevel, RecoveryObjective, VerificationClass,
    };
    use ono_core::ErrorCode;
    use ono_value::Value;

    use super::*;
    use crate::freeze::ServiceTarget;

    fn instant() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    fn later() -> Timestamp {
        Timestamp::from_second(60).expect("a valid instant")
    }

    fn builder() -> PlanBuilder {
        PlanBuilder::for_intent(
            Intent::new(
                "restart the failed services",
                "get service | where state == failed | plan restart service",
            ),
            "session-1",
            instant(),
        )
    }

    fn service(unit: &str) -> FrozenTarget {
        ServiceTarget::new("systemd", unit)
            .resolved_from("get service | where state == failed")
            .freeze()
            .expect("a namespaced unit freezes")
    }

    fn restart(plan: &PlanId, ordinal: usize, unit: &str) -> PlanAction {
        PlanAction::new(
            plan,
            ordinal,
            ActionRole::Mutate,
            format!("restart {unit}"),
            Execution::ProviderAction {
                provider: Arc::from("ono.service.systemd"),
                operation: Arc::from("ono.service.restart"),
                arguments: vec![(Arc::from("unit"), Value::string(unit))],
            },
        )
        .on(format!("systemd:{unit}"))
        .with_idempotency(Idempotency::Idempotent)
    }

    fn contract(plan: &PlanId, subject: &str) -> VerificationContract {
        VerificationContract::new(
            plan,
            VerificationClass::Required,
            subject,
            "state == running",
        )
        .expecting(Value::string("running"))
    }

    fn fragment(plan: &PlanId, units: &[&str]) -> PlanFragment {
        let mut fragment = PlanFragment::empty().at_version("255.7");
        for (index, unit) in units.iter().enumerate() {
            fragment = fragment
                .acting(restart(plan, index + 1, unit))
                .verifying(contract(plan, &format!("systemd:{unit}")));
        }
        fragment
    }

    // ---- §5.1, §4.2–§4.4 -----------------------------------------------------------------

    #[test]
    fn should_seal_a_single_action_plan_without_restarting_anything() {
        let builder = builder();
        let contribution = fragment(builder.plan_id(), &["nginx.service"]);
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment with actions is accepted")
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan with verification seals");
        assert_eq!(
            plan.state(),
            PlanState::Sealed,
            "§5.1: `plan restart service nginx` returns a ChangePlan and does not restart nginx"
        );
        assert!(
            plan.digest_holds(),
            "§4.4: the seal must describe the plan it was taken over"
        );
    }

    #[test]
    fn should_give_the_same_intent_at_the_same_instant_the_same_plan() {
        let first = builder();
        let second = builder();
        assert_eq!(
            first.plan_id(),
            second.plan_id(),
            "§55.1: planning is reproducible, so building twice must not mint two identities"
        );
    }

    #[test]
    fn should_refuse_to_seal_when_the_selector_resolved_to_nothing() {
        let refusal = builder()
            .resolve(Vec::new())
            .expect_err("§4.3 has nothing to freeze");
        assert_eq!(
            refusal.code(),
            ErrorCode::ChangeTargetUnresolved,
            "§4.3: resolution turns selectors into concrete object identities"
        );
    }

    #[test]
    fn should_refuse_to_seal_a_mutating_plan_that_carries_no_verification() {
        let builder = builder();
        let action = restart(builder.plan_id(), 1, "nginx.service");
        let refusal = builder
            .acting(&action)
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect_err("§23.1 requires a contract");
        assert_eq!(
            refusal.code(),
            ErrorCode::ChangeVerificationMissing,
            "§23.1: a plan that mutates carries verification"
        );
    }

    #[test]
    fn should_carry_the_provider_version_the_plan_was_resolved_against_into_the_seal() {
        let builder = builder();
        let contribution = fragment(builder.plan_id(), &["nginx.service"]);
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        let binding = plan
            .providers()
            .iter()
            .find(|binding| binding.id() == "ono.service.systemd")
            .expect("the acting provider is bound");
        assert_eq!(
            binding.version(),
            "255.7",
            "§4.4: the seal covers provider identities and relevant versions"
        );
    }

    #[test]
    fn should_record_an_unknown_version_when_a_provider_declares_none() {
        let builder = builder();
        let contribution =
            PlanFragment::empty().acting(restart(builder.plan_id(), 1, "nginx.service"));
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .verifying(contract(&PlanId::derive(&["x"]), "systemd:nginx.service"))
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        assert_eq!(
            plan.providers()
                .iter()
                .find(|binding| binding.id() == "ono.service.systemd")
                .map(ProviderBinding::version),
            Some(UNKNOWN_VERSION),
            "§2.4: an unknown provider version is recorded as unknown, never guessed"
        );
    }

    #[test]
    fn should_refuse_a_fragment_that_declares_preconditions_and_no_action() {
        let contribution = PlanFragment::empty().requiring(Precondition::new(
            PreconditionKind::Existence,
            "nginx.service",
            "exists",
            Value::Bool(true),
        ));
        let refusal = builder()
            .contributing(&contribution)
            .expect_err("§7.2 declares preconditions on actions");
        assert_eq!(refusal.code(), ErrorCode::ChangeActionNotPlannable);
    }

    #[test]
    fn should_accept_an_empty_fragment_as_a_provider_with_nothing_to_add() {
        let builder = builder()
            .contributing(&PlanFragment::empty())
            .expect("a provider that contributes nothing is not an error");
        assert!(builder.actions().is_empty());
    }

    #[test]
    fn should_attach_a_fragment_precondition_to_its_first_action() {
        let builder = builder();
        let contribution =
            fragment(builder.plan_id(), &["nginx.service"]).requiring(Precondition::new(
                PreconditionKind::ProviderAvailable,
                "ono.service.systemd",
                "available",
                Value::Bool(true),
            ));
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        assert!(
            plan.actions()[0]
                .preconditions()
                .iter()
                .any(|precondition| precondition.subject() == "ono.service.systemd"),
            "§7.2: a fragment's preconditions must reach an action, or nothing revalidates them"
        );
    }

    #[test]
    fn should_number_two_providers_contributions_without_colliding() {
        let builder = builder();
        let first = fragment(builder.plan_id(), &["nginx.service"]);
        let second = fragment(builder.plan_id(), &["postgres.service"]);
        let plan = builder
            .contributing(&first)
            .expect("a fragment is accepted")
            .contributing(&second)
            .expect("a second fragment is accepted")
            .resolve(vec![service("nginx.service"), service("postgres.service")])
            .expect("two targets resolve")
            .seal(later())
            .expect("a plan seals");
        let ids: Vec<&str> = plan
            .actions()
            .iter()
            .map(|action| action.id().as_str())
            .collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            ids.len(),
            unique.len(),
            "§3.3: two providers contributing to one plan must not produce one action twice"
        );
    }

    #[test]
    fn should_keep_a_dependency_between_two_actions_of_one_fragment() {
        let builder = builder();
        let plan_id = builder.plan_id().clone();
        let first = restart(&plan_id, 1, "nginx.service");
        let second = restart(&plan_id, 2, "postgres.service").after(first.id().clone());
        let contribution = PlanFragment::empty()
            .acting(first)
            .acting(second)
            .verifying(contract(&plan_id, "systemd:nginx.service"));
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        let second = &plan.actions()[1];
        assert_eq!(
            second.depends_on(),
            &[plan.actions()[0].id().clone()],
            "§3.2: the action graph survives being renumbered onto a plan"
        );
    }

    // ---- §5.3, §28.1, §28.2 --------------------------------------------------------------

    #[test]
    fn should_produce_one_plan_with_frozen_targets_when_a_pipeline_plans_a_bulk_restart() {
        let builder = builder();
        let contribution = fragment(builder.plan_id(), &["a.service", "b.service", "c.service"]);
        let plans = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![
                service("a.service"),
                service("b.service"),
                service("c.service"),
            ])
            .expect("three targets resolve")
            .seal_with(PlanGranularity::SinglePlan, later())
            .expect("a plan seals");
        assert_eq!(
            plans.len(),
            1,
            "§5.3: a pipeline MUST produce one plan unless one per object is asked for"
        );
        assert_eq!(
            plans[0].targets().len(),
            3,
            "§28.2: membership freezes at resolution, and all three are in it"
        );
    }

    #[test]
    fn should_default_to_one_plan_when_nothing_asks_for_one_per_object() {
        assert_eq!(
            PlanGranularity::default(),
            PlanGranularity::SinglePlan,
            "§5.3: one plan is what a pipeline produces unless the user requests otherwise"
        );
    }

    #[test]
    fn should_produce_one_plan_per_object_only_when_it_is_asked_for() {
        let builder = builder();
        let contribution = fragment(builder.plan_id(), &["a.service", "b.service"]);
        let plans = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("a.service"), service("b.service")])
            .expect("two targets resolve")
            .seal_with(PlanGranularity::PlanPerObject, later())
            .expect("both plans seal");
        assert_eq!(
            plans.len(),
            2,
            "§5.3: the explicit request is one per object"
        );
        for plan in &plans {
            assert_eq!(
                plan.targets().len(),
                1,
                "each per-object plan covers exactly its own object (§5.3)"
            );
        }
        assert_ne!(
            plans[0].id(),
            plans[1].id(),
            "§3.2: each plan needs a stable identity of its own"
        );
    }

    #[test]
    fn should_give_each_per_object_plan_a_seal_that_holds() {
        let builder = builder();
        let contribution = fragment(builder.plan_id(), &["a.service", "b.service"]);
        let plans = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("a.service"), service("b.service")])
            .expect("two targets resolve")
            .seal_with(PlanGranularity::PlanPerObject, later())
            .expect("both plans seal");
        for plan in &plans {
            assert!(
                plan.digest_holds(),
                "§4.4: every sealed plan carries a digest over what it actually is"
            );
            assert!(
                plan.actions().iter().all(|action| action
                    .effects()
                    .iter()
                    .all(|effect| effect.action() == action.id())),
                "§8.2: an effect belongs to the action it was moved with"
            );
        }
    }

    #[test]
    fn should_not_add_an_object_that_started_matching_after_resolution() {
        let builder = builder();
        let contribution = fragment(builder.plan_id(), &["a.service"]);
        // §4.3's own example: four services match, a fifth fails afterwards.
        let mut world = vec!["a.service".to_owned()];
        let resolved: Vec<FrozenTarget> = world.iter().map(|unit| service(unit)).collect();
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(resolved)
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        world.push("e.service".to_owned());
        assert_eq!(
            plan.targets().len(),
            1,
            "§2.6: newly matching objects MUST NOT silently join a sealed plan"
        );
        assert_eq!(plan.targets()[0].identity(), "systemd:a.service");
    }

    #[test]
    fn should_keep_the_selector_on_the_frozen_targets_without_re_running_it() {
        let builder = builder();
        let contribution = fragment(builder.plan_id(), &["a.service"]);
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("a.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        assert_eq!(
            plan.targets()[0].selector(),
            Some("get service | where state == failed"),
            "§4.3 keeps the selector as provenance for `explain`"
        );
    }

    #[test]
    fn should_resolve_a_target_set_that_arrives_one_object_at_a_time() {
        let builder = builder();
        let contribution = fragment(builder.plan_id(), &["a.service"]);
        let units = ["a.service", "b.service", "c.service"];
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve_streaming(units.iter().map(|unit| Ok(service(unit))))
            .expect("a streamed set resolves")
            .seal(later())
            .expect("a plan seals");
        assert_eq!(
            plan.targets().len(),
            3,
            "§52.5: streaming resolution still leaves a durable sealed identity list"
        );
    }

    #[test]
    fn should_stop_at_the_first_object_that_would_not_freeze() {
        let refusal = builder()
            .resolve_streaming(vec![
                Ok(service("a.service")),
                Err(error::target_unresolved(
                    "b",
                    "the object vanished mid-resolution",
                )),
            ])
            .expect_err("§4.3 refuses a set it could not freeze");
        assert_eq!(refusal.code(), ErrorCode::ChangeTargetUnresolved);
    }

    // ---- §5.2 ------------------------------------------------------------------------------

    #[test]
    fn should_accept_a_block_of_action_descriptions() {
        let block = BlockPlan::of(vec![
            BlockStatement::action(
                "replace",
                "replace file /etc/nginx/nginx.conf from ./nginx.conf",
            ),
            BlockStatement::action("validate", "validate config nginx"),
            BlockStatement::action("restart", "restart service nginx"),
            BlockStatement::action("verify", "verify service nginx state == running"),
        ])
        .expect("§5.2 permits a sequence of action descriptions");
        assert_eq!(block.len(), 4);
        assert!(!block.is_empty());
        assert_eq!(block.actions()[0].verb(), "replace");
    }

    #[test]
    fn should_refuse_a_loop_in_a_plan_block() {
        let refusal = BlockPlan::of(vec![
            BlockStatement::action("restart", "restart service nginx"),
            BlockStatement::Loop {
                source: Arc::from("for unit in $units { restart service $unit }"),
            },
        ])
        .expect_err("§5.2 forbids loops");
        assert_eq!(refusal.code(), ErrorCode::ChangeActionNotPlannable);
        assert!(
            refusal.help().unwrap_or_default().contains("loops"),
            "§5.2's refusal names the construct it refused"
        );
    }

    #[test]
    fn should_refuse_a_function_definition_in_a_plan_block() {
        let refusal = BlockPlan::of(vec![BlockStatement::Function {
            source: Arc::from("def bounce(unit) { restart service $unit }"),
        }])
        .expect_err("§5.2 forbids arbitrary functions");
        assert_eq!(refusal.code(), ErrorCode::ChangeActionNotPlannable);
    }

    #[test]
    fn should_refuse_a_background_job_in_a_plan_block() {
        let refusal = BlockPlan::of(vec![BlockStatement::BackgroundJob {
            source: Arc::from("restart service nginx &"),
        }])
        .expect_err("§5.2 forbids background jobs");
        assert_eq!(refusal.code(), ErrorCode::ChangeActionNotPlannable);
    }

    #[test]
    fn should_refuse_runtime_control_flow_in_a_plan_block() {
        let refusal = BlockPlan::of(vec![BlockStatement::ControlFlow {
            source: Arc::from("if $failed { restart service nginx }"),
        }])
        .expect_err("§5.2 forbids unbounded runtime control flow");
        assert_eq!(refusal.code(), ErrorCode::ChangeActionNotPlannable);
    }

    #[test]
    fn should_refuse_an_empty_plan_block() {
        let refusal = BlockPlan::of(Vec::new()).expect_err("§5.2 describes actions");
        assert_eq!(refusal.code(), ErrorCode::ChangeActionNotPlannable);
    }

    #[test]
    fn should_refuse_a_block_longer_than_the_bound() {
        let statements: Vec<BlockStatement> = (0..=MAX_BLOCK_ACTIONS)
            .map(|index| BlockStatement::action("restart", format!("restart service s{index}")))
            .collect();
        let refusal = BlockPlan::of(statements).expect_err("§5.2 bounds the block");
        assert_eq!(refusal.code(), ErrorCode::ChangeActionNotPlannable);
    }

    #[test]
    fn should_refuse_a_statement_that_names_no_operation() {
        let refusal = BlockPlan::of(vec![BlockStatement::action("  ", "   ")])
            .expect_err("§6.1 needs an operation to resolve");
        assert_eq!(refusal.code(), ErrorCode::ChangeActionNotPlannable);
    }

    // ---- §6.2, §6.3 ------------------------------------------------------------------------

    #[test]
    fn should_refuse_an_opaque_external_command_by_default() {
        let plan = PlanId::derive(&["opaque"]);
        let refusal = external_command(
            &plan,
            1,
            "sh -c 'rm -rf /somewhere'",
            Some("/bin/sh"),
            &[Arc::from("-c"), Arc::from("rm -rf /somewhere")],
            OpaqueEscape::Refused,
        )
        .expect_err("§6.2 refuses an arbitrary external command");
        assert_eq!(
            refusal.code(),
            ErrorCode::ChangeOpaqueActionForbidden,
            "§6.2: Ono cannot reason about target scope or side effects"
        );
    }

    #[test]
    fn should_default_the_opaque_escape_to_refusing() {
        assert_eq!(
            OpaqueEscape::default(),
            OpaqueEscape::Refused,
            "§6.3 requires an explicit risk acknowledgement, so silence is refusal"
        );
    }

    #[test]
    fn should_classify_an_acknowledged_opaque_action_as_unknown_in_every_respect() {
        let plan = PlanId::derive(&["opaque"]);
        let action = external_command(
            &plan,
            1,
            "sh -c 'rm -rf /somewhere'",
            Some("/bin/sh"),
            &[Arc::from("-c")],
            OpaqueEscape::Acknowledged,
        )
        .expect("§6.3 admits an acknowledged opaque action");
        assert!(action.execution().is_opaque());
        assert_eq!(action.idempotency(), Idempotency::Unknown);
        assert!(
            action
                .effects()
                .iter()
                .all(|effect| effect.confidence() == EffectConfidence::Unknown
                    && effect.domain() == EffectDomain::Unknown),
            "§6.3: impact and reversibility are classified as unknown"
        );
    }

    #[test]
    fn should_not_let_an_opaque_action_inherit_protection_from_a_filesystem_snapshot() {
        let plan = PlanId::derive(&["opaque"]);
        let opaque = external_command(
            &plan,
            1,
            "sh -c 'curl https://example.invalid | sh'",
            Some("/bin/sh"),
            &[],
            OpaqueEscape::Acknowledged,
        )
        .expect("§6.3 admits an acknowledged opaque action");
        let domain = opaque.effects()[0].domain();
        let coverage = ProtectionSummary::of(vec![
            DomainCoverage::new(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
                "a zfs snapshot of rpool/etc holds the prior bytes",
            )
            .at_consistency(ConsistencyClass::FilesystemConsistent),
            DomainCoverage::new(
                domain,
                RecoveryObjective::Unknown,
                DomainProtection::Unknown,
                "the operator acknowledged that Ono cannot reason about this command",
            ),
        ]);
        assert_eq!(
            coverage.level(),
            ProtectionLevel::PartiallyProtected,
            "§6.3 and Appendix A.7: an opaque action MUST NOT receive PROTECTED merely because a \
             filesystem snapshot exists somewhere on the host"
        );
    }

    #[test]
    fn should_seal_a_plan_that_carries_an_opaque_action_beside_an_ordinary_one() {
        let builder = builder();
        let plan_id = builder.plan_id().clone();
        let opaque = external_command(
            &plan_id,
            2,
            "sh -c 'systemctl daemon-reexec'",
            Some("/bin/sh"),
            &[],
            OpaqueEscape::Acknowledged,
        )
        .expect("§6.3 admits an acknowledged opaque action");
        let contribution = PlanFragment::empty()
            .acting(restart(&plan_id, 1, "nginx.service"))
            .acting(opaque)
            .verifying(contract(&plan_id, "systemd:nginx.service"));
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        assert!(
            plan.has_opaque_action(),
            "§6.3: the plan records that it carries an action Ono cannot reason about"
        );
    }

    // ---- §36.3 ------------------------------------------------------------------------------

    #[test]
    fn should_seal_over_the_handle_when_an_action_carries_a_secret() {
        let builder = builder();
        let plan_id = builder.plan_id().clone();
        let action = PlanAction::new(
            &plan_id,
            1,
            ActionRole::Mutate,
            "set the database password",
            Execution::ProviderAction {
                provider: Arc::from("linux.users"),
                operation: Arc::from("ono.user.set-password"),
                arguments: vec![(Arc::from("password"), Value::string("hunter2"))],
            },
        );
        let contribution = PlanFragment::empty()
            .acting(action)
            .verifying(contract(&plan_id, "systemd:nginx.service"));
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        assert!(
            !plan.actions()[0]
                .execution()
                .digest_text()
                .contains("hunter2"),
            "§36.3: the secret leaves the plan before §4.4's digest is taken over it"
        );
        assert!(plan.digest_holds(), "§4.4: the seal describes the plan");
    }

    #[test]
    fn should_leave_an_ordinary_argument_alone_while_redacting_a_secret() {
        let builder = builder();
        let plan_id = builder.plan_id().clone();
        let action = PlanAction::new(
            &plan_id,
            1,
            ActionRole::Mutate,
            "set the database password",
            Execution::ProviderAction {
                provider: Arc::from("linux.users"),
                operation: Arc::from("ono.user.set-password"),
                arguments: vec![
                    (Arc::from("user"), Value::string("alice")),
                    (Arc::from("password"), Value::string("hunter2")),
                ],
            },
        );
        let contribution = PlanFragment::empty()
            .acting(action)
            .verifying(contract(&plan_id, "systemd:nginx.service"));
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        assert!(
            plan.actions()[0]
                .execution()
                .digest_text()
                .contains("alice"),
            "§36.3 redacts secrets, and the rest of the plan stays readable"
        );
    }

    #[test]
    fn should_keep_a_secret_out_of_the_plan_only_where_a_name_declares_one() {
        let builder = builder().redacting(SecretRedaction::declaring_nothing());
        let plan_id = builder.plan_id().clone();
        let action = PlanAction::new(
            &plan_id,
            1,
            ActionRole::Mutate,
            "set the database password",
            Execution::ProviderAction {
                provider: Arc::from("linux.users"),
                operation: Arc::from("ono.user.set-password"),
                arguments: vec![(Arc::from("password"), Value::string("hunter2"))],
            },
        );
        let contribution = PlanFragment::empty()
            .acting(action)
            .verifying(contract(&plan_id, "systemd:nginx.service"));
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        assert!(
            plan.actions()[0]
                .execution()
                .digest_text()
                .contains("hunter2"),
            "§36.3 is driven by a declared set, and a caller that declares nothing gets nothing"
        );
    }

    // ---- §52.1 ------------------------------------------------------------------------------

    #[test]
    fn should_create_a_single_service_plan_well_inside_the_budget() {
        // §52.1's budget is 150 ms for a typical single-host single-service plan, excluding
        // explicitly slow provider discovery — which is exactly what this measures, since the
        // fragment is already in hand.
        let start = std::time::Instant::now();
        let builder = builder();
        let contribution = fragment(builder.plan_id(), &["nginx.service"]);
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve(vec![service("nginx.service")])
            .expect("one target resolves")
            .seal(later())
            .expect("a plan seals");
        let elapsed = start.elapsed();
        assert!(plan.digest_holds());
        assert!(
            elapsed < std::time::Duration::from_millis(150),
            "§52.1: single-host single-service plan creation SHOULD complete in less than 150 ms \
             excluding provider discovery; took {elapsed:?}"
        );
    }

    #[test]
    fn should_seal_a_plan_over_thousands_of_targets_within_the_budget() {
        let builder = builder();
        let contribution = fragment(builder.plan_id(), &["a.service"]);
        let units: Vec<String> = (0..5_000)
            .map(|index| format!("s{index}.service"))
            .collect();
        let start = std::time::Instant::now();
        let plan = builder
            .contributing(&contribution)
            .expect("a fragment is accepted")
            .resolve_streaming(units.iter().map(|unit| Ok(service(unit))))
            .expect("five thousand targets resolve")
            .seal(later())
            .expect("a plan seals");
        let elapsed = start.elapsed();
        assert_eq!(
            plan.targets().len(),
            5_000,
            "§52.5: the final sealed target identity list is durable"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "§52.5: thousands of targets must not make sealing quadratic; took {elapsed:?}"
        );
    }
}
