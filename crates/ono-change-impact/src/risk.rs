//! The risk rules of §19, as a registry of named rules.
//!
//! §19.2 is the sentence this module is built around: *"These classes are rule-based, not
//! AI-generated."* §62.11 says the same thing from the other side — a model "may explain or
//! propose", and "may not upgrade unknown to guaranteed". So risk here is a fold over findings
//! that named rules produced, every finding carries [`RiskFinding::rule`] and a sentence naming
//! the actual reason, and there is no entry point through which anything else contributes one.
//!
//! The sentence matters as much as the class. §40.2 requires a high-risk gate to "summarize the
//! actual risk reason, not display generic 'Are you sure?'", and the only way a gate can do that
//! is if the rule that raised the class already said why, in words, at the moment it fired.
//!
//! Risk is not protection. §19.1 and §10.4 make the two axes independent — "a strongly protected
//! plan may still be risky" — so nothing in this module reads a protection level, and the §19.3
//! examples this crate's tests reproduce assert the risk class alone.

use std::collections::BTreeSet;
use std::sync::Arc;

use ono_change_core::{
    EffectConfidence, EffectDomain, EffectKind, FrozenTarget, ImpactGraph, PlanAction,
    ProposedEffect, RestoreMethod, RiskAssessment, RiskClass, RiskDimension, RiskFinding,
};
use ono_spatial_core::{SpatialId, SpatialType, relation};
use ono_spatial_index::{IndexEntry, SpatialIndex};

/// The object a provider names on a reboot **requirement** effect (§30.5).
///
/// §30.5 lets a provider "mark reboot requirement or recommendation as a ProposedEffect" and
/// requires it to "distinguish requirement from suggestion". The distinction has to be
/// machine-readable for a rule to act on it, so it is the effect's object: a requirement names
/// [`REBOOT_REQUIRED`] and a recommendation names [`REBOOT_SUGGESTED`], and the difference
/// between "you will have to reboot" and "you might want to" survives into the risk class.
pub const REBOOT_REQUIRED: &str = "ono.reboot/required";

/// The object a provider names on a reboot **suggestion** effect (§30.5).
///
/// A suggestion raises no risk of its own: nothing in the plan stops working if the operator
/// ignores it, which is precisely why §30.5 insists the two are distinguishable.
pub const REBOOT_SUGGESTED: &str = "ono.reboot/suggested";

/// The relations that prove shared role membership in the v0.4 topology (§28.3).
///
/// §28.3 is careful: availability risk is identified "where topology proves shared role
/// membership". Two services whose names begin `frontend-` prove nothing — a name is a string,
/// and §2.8 of v0.4 spends a whole section on why an identifier is not an identity. A control
/// group that contains both, or a container that runs both, is a fact a provider observed.
///
/// The boolean says which end of the edge is the group: `true` when the group is the edge's
/// target (a service is *in* a cgroup), `false` when it is the source (a container *contains* a
/// process).
const ROLE_MEMBERSHIP: &[(&str, bool)] = &[
    ("service.in_cgroup", true),
    ("process.member_of_cgroup", true),
    ("container.contains_process", false),
];

/// The relations that make an object a listener, for the downtime rule (§33.3, v0.4 §14.3).
const LISTENER_RELATIONS: &[&str] = &["service.listens_on", "process.owns_socket"];

/// The bulk thresholds of §53's reference configuration (`[change.bulk]`).
///
/// §53 declares `warn_targets = 10` and `high_risk_targets = 50`, and they are the defaults here.
/// The host-fanout rule reuses `warn_targets`: §53 declares no separate key for it, and a plan
/// reaching more hosts than the count at which target bulk becomes noteworthy is at least as
/// noteworthy (§29.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulkThresholds {
    /// The target count at which §28.3's bulk risk becomes worth saying out loud.
    pub warn_targets: usize,
    /// The target count at which §28.3's bulk risk is `HIGH`.
    pub high_risk_targets: usize,
}

impl Default for BulkThresholds {
    fn default() -> Self {
        Self {
            warn_targets: 10,
            high_risk_targets: 50,
        }
    }
}

/// The network path this Ono session's own remote link depends on (§34.2, Appendix I.3).
///
/// §34.2 makes a plan that may remove this path a CRITICAL risk landmark, and a rule can only
/// find that out if it knows what the path is. The route and the interface are identities the
/// session already holds — the remote transport resolved them when the link was established — so
/// they are an input rather than something this crate goes and looks up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLink {
    host: Arc<str>,
    route: Option<Arc<str>>,
    interface: Option<Arc<str>>,
}

impl ActiveLink {
    /// The link this session reaches `host` over.
    #[must_use]
    pub fn new(host: impl Into<Arc<str>>) -> Self {
        Self {
            host: host.into(),
            route: None,
            interface: None,
        }
    }

    /// Records the route the link's traffic takes (Appendix I.3's default route).
    #[must_use]
    pub fn via_route(mut self, route: impl Into<Arc<str>>) -> Self {
        self.route = Some(route.into());
        self
    }

    /// Records the interface the link's traffic leaves through.
    #[must_use]
    pub fn via_interface(mut self, interface: impl Into<Arc<str>>) -> Self {
        self.interface = Some(interface.into());
        self
    }

    /// The host at the far end.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The route the link depends on, where the session resolved one.
    #[must_use]
    pub fn route(&self) -> Option<&str> {
        self.route.as_deref()
    }

    /// The interface the link depends on, where the session resolved one.
    #[must_use]
    pub fn interface(&self) -> Option<&str> {
        self.interface.as_deref()
    }

    /// Whether `object` is one of the identities the link depends on.
    fn depends_on(&self, object: &str) -> bool {
        self.route.as_deref() == Some(object) || self.interface.as_deref() == Some(object)
    }
}

/// One plan, and everything the rules of §19 need to judge it.
///
/// Everything beyond the actions is optional, and a rule with no input answers with no finding
/// rather than with a guess. That is what keeps the assessment honest when it is made early:
/// §4.2 lets a DRAFT plan be inspected, and a draft has no impact graph yet.
#[derive(Debug, Clone)]
pub struct RiskRequest<'a> {
    actions: &'a [PlanAction],
    targets: &'a [FrozenTarget],
    impact: Option<&'a ImpactGraph>,
    index: Option<&'a SpatialIndex>,
    link: Option<&'a ActiveLink>,
    recovery: &'a [RestoreMethod],
    thresholds: BulkThresholds,
}

impl<'a> RiskRequest<'a> {
    /// Asks what `actions` over `targets` risk.
    #[must_use]
    pub const fn new(actions: &'a [PlanAction], targets: &'a [FrozenTarget]) -> Self {
        Self {
            actions,
            targets,
            impact: None,
            index: None,
            link: None,
            recovery: &[],
            thresholds: BulkThresholds {
                warn_targets: 10,
                high_risk_targets: 50,
            },
        }
    }

    /// Reads the plan's impact graph, which is where §9.6's boundaries come from.
    #[must_use]
    pub const fn over_impact(mut self, impact: &'a ImpactGraph) -> Self {
        self.impact = Some(impact);
        self
    }

    /// Reads the v0.4 topology, which is what proves shared role membership (§28.3).
    #[must_use]
    pub const fn over_topology(mut self, index: &'a SpatialIndex) -> Self {
        self.index = Some(index);
        self
    }

    /// Declares the remote link this session depends on (§34.2).
    #[must_use]
    pub const fn over_link(mut self, link: &'a ActiveLink) -> Self {
        self.link = Some(link);
        self
    }

    /// Declares the recovery methods this plan's protection would rest on (Appendix C.1).
    #[must_use]
    pub const fn with_recovery(mut self, methods: &'a [RestoreMethod]) -> Self {
        self.recovery = methods;
        self
    }

    /// Uses `thresholds` in place of §53's defaults.
    #[must_use]
    pub const fn with_thresholds(mut self, thresholds: BulkThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// The plan's actions.
    #[must_use]
    pub const fn actions(&self) -> &[PlanAction] {
        self.actions
    }

    /// The plan's frozen targets (§7.1).
    #[must_use]
    pub const fn targets(&self) -> &[FrozenTarget] {
        self.targets
    }

    /// The thresholds in force (§53).
    #[must_use]
    pub const fn thresholds(&self) -> BulkThresholds {
        self.thresholds
    }

    /// The actions that change the target system (§3.3).
    fn mutating(&self) -> impl Iterator<Item = &PlanAction> {
        self.actions
            .iter()
            .filter(|action| action.role().mutates_target())
    }

    /// Every effect the plan declares, with the action that declared it.
    fn effects(&self) -> impl Iterator<Item = (&PlanAction, &ProposedEffect)> {
        self.actions
            .iter()
            .flat_map(|action| action.effects().iter().map(move |effect| (action, effect)))
    }

    /// The distinct objects the plan changes, in the order the plan names them.
    ///
    /// Frozen targets first, because §2.6 makes them the plan's membership; a mutating action
    /// that names a target no frozen record covers still counts, because it still changes it.
    fn changed_objects(&self) -> Vec<&str> {
        let mut objects: Vec<&str> = Vec::new();
        for target in self.targets {
            let identity = target.identity();
            if !objects.contains(&identity) {
                objects.push(identity);
            }
        }
        for action in self.mutating() {
            if let Some(target) = action.target()
                && !objects.contains(&target)
                && !self
                    .targets
                    .iter()
                    .any(|frozen| frozen.spatial_id() == Some(target))
            {
                objects.push(target);
            }
        }
        objects
    }

    /// Every identity the plan's targets answer to, for comparing against the topology.
    fn targeted_identities(&self) -> BTreeSet<&str> {
        let mut identities: BTreeSet<&str> = BTreeSet::new();
        for target in self.targets {
            identities.insert(target.identity());
            if let Some(spatial) = target.spatial_id() {
                identities.insert(spatial);
            }
        }
        for action in self.mutating() {
            if let Some(target) = action.target() {
                identities.insert(target);
            }
        }
        identities
    }

    /// The hosts the plan reaches (§29.1).
    fn hosts(&self) -> BTreeSet<&str> {
        self.targets.iter().filter_map(FrozenTarget::host).collect()
    }
}

/// One declared risk rule: its identity, the §19.1 dimension it speaks to, and what it looks for.
///
/// The evaluation function is private, so the registry is inspectable and closed at once: `explain`
/// can list every rule that could fire, and nothing outside this crate can add one. §19.2's
/// "rule-based, not AI-generated" is a property of the type rather than a convention.
#[derive(Debug, Clone, Copy)]
pub struct RiskRuleSpec {
    id: &'static str,
    dimension: RiskDimension,
    doc: &'static str,
    evaluate: fn(&RiskRuleSpec, &RiskRequest<'_>) -> Option<RiskFinding>,
}

impl RiskRuleSpec {
    /// The rule's id, which every finding it makes carries (§19.2).
    #[must_use]
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// The §19.1 dimension the rule speaks to.
    #[must_use]
    pub const fn dimension(&self) -> RiskDimension {
        self.dimension
    }

    /// The line `explain` prints for the rule.
    #[must_use]
    pub const fn doc(&self) -> &'static str {
        self.doc
    }
}

/// Every rule, in the order they are evaluated (§19.1's dimensions).
const RULES: &[RiskRuleSpec] = &[
    RiskRuleSpec {
        id: "risk.scope.single-object",
        dimension: RiskDimension::Scope,
        doc: "A plan that changes exactly one object. §19.3 rates replacing one configuration \
              file MODERATE, so a real change to one object is where the scale starts.",
        evaluate: single_object,
    },
    RiskRuleSpec {
        id: "risk.scope.multi-object",
        dimension: RiskDimension::Scope,
        doc: "A plan that changes more than one object (§19.1's scope dimension).",
        evaluate: multi_object,
    },
    RiskRuleSpec {
        id: "risk.bulk.warn-threshold",
        dimension: RiskDimension::BulkCount,
        doc: "The plan reaches the configured `change.bulk.warn_targets` count (§28.3, §53).",
        evaluate: bulk_warn,
    },
    RiskRuleSpec {
        id: "risk.bulk.high-threshold",
        dimension: RiskDimension::BulkCount,
        doc: "The plan reaches the configured `change.bulk.high_risk_targets` count (§28.3, §53).",
        evaluate: bulk_high,
    },
    RiskRuleSpec {
        id: "risk.bulk.whole-role",
        dimension: RiskDimension::Downtime,
        doc: "The v0.4 topology proves every member of a service group is targeted, so no serving \
              member is excluded (§28.3, §19.3).",
        evaluate: whole_role,
    },
    RiskRuleSpec {
        id: "risk.privilege.elevation",
        dimension: RiskDimension::Privilege,
        doc: "An action needs elevated privilege, and §43.3 requires the plan to show which.",
        evaluate: privilege,
    },
    RiskRuleSpec {
        id: "risk.irreversibility.effect",
        dimension: RiskDimension::Irreversibility,
        doc: "An effect nothing can undo (§19.1). §19.4 gates on this independently of the class.",
        evaluate: irreversibility,
    },
    RiskRuleSpec {
        id: "risk.external.side-effect",
        dimension: RiskDimension::ExternalSideEffects,
        doc: "An effect that leaves the machine, which no local snapshot reaches (§35.1, §35.2).",
        evaluate: external_side_effect,
    },
    RiskRuleSpec {
        id: "risk.downtime.service-restart",
        dimension: RiskDimension::Downtime,
        doc: "A service restart that interrupts listeners or connections (§33.1, §33.3).",
        evaluate: service_restart,
    },
    RiskRuleSpec {
        id: "risk.unknown.impact",
        dimension: RiskDimension::UnknownImpact,
        doc: "The impact graph ends at a boundary, the plan carries an UNKNOWN effect, or an \
              action is opaque (§9.6, §8.1, §6.3). §19.2's UNKNOWN class.",
        evaluate: unknown_impact,
    },
    RiskRuleSpec {
        id: "risk.reboot.required",
        dimension: RiskDimension::RebootRequirement,
        doc: "A provider declared a reboot requirement rather than a recommendation (§30.5).",
        evaluate: reboot_required,
    },
    RiskRuleSpec {
        id: "risk.remote.fanout",
        dimension: RiskDimension::RemoteFanout,
        doc: "The plan reaches more than one host, and §29.1 forbids implying global atomicity.",
        evaluate: remote_fanout,
    },
    RiskRuleSpec {
        id: "risk.remote.link-loss",
        dimension: RiskDimension::RemoteFanout,
        doc: "The plan may remove the transport path this Ono link depends on (§34.2). CRITICAL.",
        evaluate: link_loss,
    },
    RiskRuleSpec {
        id: "risk.recovery.complexity",
        dimension: RiskDimension::RecoveryComplexity,
        doc: "Recovery would need an offline or next-boot method (§19.1, Appendix C.1).",
        evaluate: recovery_complexity,
    },
];

/// Every declared risk rule, so `explain` can list what could fire (§19.2).
#[must_use]
pub const fn rules() -> &'static [RiskRuleSpec] {
    RULES
}

/// The rule with this id, or `None`.
#[must_use]
pub fn rule(id: &str) -> Option<&'static RiskRuleSpec> {
    RULES.iter().find(|rule| rule.id == id)
}

/// The plan's risk: every rule run, composed by [`RiskAssessment::classify`] (§19.2).
///
/// Composition is the strongest class any rule found, which is [`RiskAssessment::classify`]'s
/// own fold. It is deliberately not an average or a score: §8.3 rules out probability theatre,
/// and a plan with one critical finding among twenty low ones is a critical plan.
#[must_use]
pub fn assess(request: &RiskRequest<'_>) -> RiskAssessment {
    RiskAssessment::of(
        RULES
            .iter()
            .filter_map(|rule| (rule.evaluate)(rule, request))
            .collect(),
    )
}

/// The finding `rule` makes, carrying the rule's own id and dimension (§19.2, §40.2).
fn finding(rule: &RiskRuleSpec, class: RiskClass, reason: String) -> Option<RiskFinding> {
    Some(RiskFinding::new(rule.dimension, class, rule.id, reason))
}

/// §19.3's first example: replacing one configuration file is a MODERATE change.
fn single_object(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let objects = request.changed_objects();
    if objects.len() != 1 {
        return None;
    }
    let object = objects.first()?;
    finding(
        rule,
        RiskClass::Moderate,
        format!("the plan changes one object: `{object}`"),
    )
}

/// §19.1's scope dimension: more than one object is more than one thing to be wrong about.
fn multi_object(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let objects = request.changed_objects();
    if objects.len() < 2 {
        return None;
    }
    finding(
        rule,
        RiskClass::Moderate,
        format!(
            "the plan changes {} objects, beginning with `{}`",
            objects.len(),
            objects.first().copied().unwrap_or("?")
        ),
    )
}

/// §28.3 and §53's `change.bulk.warn_targets`.
fn bulk_warn(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let count = request.changed_objects().len();
    let threshold = request.thresholds.warn_targets;
    if count < threshold {
        return None;
    }
    finding(
        rule,
        RiskClass::Moderate,
        format!(
            "{count} targets is at or beyond the configured bulk warning threshold of {threshold} \
             (§53 `change.bulk.warn_targets`)"
        ),
    )
}

/// §28.3 and §53's `change.bulk.high_risk_targets`.
fn bulk_high(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let count = request.changed_objects().len();
    let threshold = request.thresholds.high_risk_targets;
    if count < threshold {
        return None;
    }
    finding(
        rule,
        RiskClass::High,
        format!(
            "{count} targets is at or beyond the configured high-risk threshold of {threshold} \
             (§53 `change.bulk.high_risk_targets`)"
        ),
    )
}

/// §28.3: "If all members of a service group are targeted, Ono SHOULD identify availability risk
/// where topology proves shared role membership."
///
/// The reason sentence is written the way §40.2's own example writes it — the count, and the
/// fact that no member is left serving — because that is the sentence the gate prints.
fn whole_role(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let index = request.index?;
    let targeted = request.targeted_identities();
    if targeted.is_empty() {
        return None;
    }
    let mut covered: Vec<(String, usize)> = Vec::new();
    for entry in index.entries() {
        // Only a place a membership relation can lead *to* can be a group, and skipping the rest
        // keeps the rule linear in the objects that could matter rather than in the whole index
        // (§52.2).
        if !is_group_end(entry.object().object_type()) {
            continue;
        }
        let members = members_of(index, entry);
        if members.len() < 2 {
            continue;
        }
        if members
            .iter()
            .all(|member| targeted.contains(member.as_str()))
        {
            covered.push((entry.object().display_name().to_owned(), members.len()));
        }
    }
    covered.sort();
    let (group, count) = covered.first()?;
    finding(
        rule,
        RiskClass::Critical,
        format!(
            "all {count} members of `{group}` are targeted; no serving member of that role is \
             excluded (§28.3)"
        ),
    )
}

/// Whether a place of this type could be the group end of a membership relation (§28.3).
fn is_group_end(object_type: SpatialType) -> bool {
    ROLE_MEMBERSHIP.iter().any(|(id, group_is_target)| {
        relation::spec(id).is_some_and(|spec| {
            let end = if *group_is_target {
                spec.target
            } else {
                spec.source
            };
            object_type.is_a(end)
        })
    })
}

/// The members the v0.4 topology puts inside one group object (§28.3).
fn members_of(index: &SpatialIndex, entry: &IndexEntry) -> Vec<String> {
    let id = entry.object().spatial_id();
    let mut members: Vec<String> = Vec::new();
    for edge in entry.edges() {
        let Some((_, group_is_target)) = ROLE_MEMBERSHIP
            .iter()
            .find(|(relation, _)| *relation == edge.relation().as_str())
        else {
            continue;
        };
        let group_end: &SpatialId = if *group_is_target {
            edge.target()
        } else {
            edge.source()
        };
        if group_end != id {
            continue;
        }
        let Some(member) = edge.other_end(id) else {
            continue;
        };
        if index.get(member).is_none() {
            continue;
        }
        let member = member.as_str().to_owned();
        if !members.contains(&member) {
            members.push(member);
        }
    }
    members.sort();
    members
}

/// §43.3: "The plan MUST show which actions require privilege and when elevation will occur."
fn privilege(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let elevated: Vec<&PlanAction> = request
        .actions
        .iter()
        .filter(|action| action.needs_privilege())
        .collect();
    let first = elevated.first()?;
    finding(
        rule,
        RiskClass::Moderate,
        format!(
            "{} of {} actions run with elevated privilege, beginning with `{}`",
            elevated.len(),
            request.actions.len(),
            first.summary()
        ),
    )
}

/// §19.1's irreversibility dimension, which §19.4 gates on independently of the class.
fn irreversibility(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let irreversible: Vec<&ProposedEffect> = request
        .effects()
        .map(|(_, effect)| effect)
        .filter(|effect| effect.is_irreversible())
        .collect();
    let first = irreversible.first()?;
    finding(
        rule,
        RiskClass::High,
        format!(
            "{} effect(s) cannot be undone by any recovery asset: {}",
            irreversible.len(),
            first.explanation()
        ),
    )
}

/// §35.1's external side effects, which §35.2 keeps separately visible.
fn external_side_effect(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let external: Vec<&ProposedEffect> = request
        .effects()
        .map(|(_, effect)| effect)
        .filter(|effect| effect.domain() == EffectDomain::ExternalSideEffect)
        .collect();
    let first = external.first()?;
    finding(
        rule,
        RiskClass::High,
        format!(
            "{} effect(s) leave this machine and no local snapshot reaches them: {}",
            external.len(),
            first.explanation()
        ),
    )
}

/// A restart interrupts what the restarted thing was serving (§33.1, §33.3).
///
/// §33.1 makes runtime state generally unrecoverable, and a listener is where that stops being
/// abstract: the connections it was serving end, whatever the filesystem protection says.
fn service_restart(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    for (action, effect) in request.effects() {
        if effect.domain() != EffectDomain::ProcessRuntime
            || !matches!(effect.kind(), EffectKind::Replace | EffectKind::Interrupt)
        {
            continue;
        }
        let object = effect.object().or_else(|| action.target()).unwrap_or("");
        let listeners = request.index.map_or(0, |index| listeners_of(index, object));
        let declared = request.effects().any(|(_, other)| {
            other.action() == effect.action()
                && other.domain() == EffectDomain::NetworkRuntime
                && other.kind() == EffectKind::Interrupt
        });
        if listeners > 0 {
            return finding(
                rule,
                RiskClass::Moderate,
                format!(
                    "restarting `{object}` interrupts {listeners} known listener(s); §33.1 makes \
                     that runtime state unrecoverable"
                ),
            );
        }
        if declared {
            return finding(
                rule,
                RiskClass::Moderate,
                format!(
                    "restarting `{object}` interrupts what it was serving; §33.1 makes that \
                     runtime state unrecoverable"
                ),
            );
        }
    }
    None
}

/// How many listeners the topology gives an object (v0.4 §14.3).
fn listeners_of(index: &SpatialIndex, object: &str) -> usize {
    let Some(id) = SpatialId::parse(object) else {
        return 0;
    };
    let Some(entry) = index.get(&id) else {
        return 0;
    };
    entry
        .edges()
        .iter()
        .filter(|edge| LISTENER_RELATIONS.contains(&edge.relation().as_str()))
        .filter(|edge| edge.source() == &id)
        .count()
}

/// §19.2's `UNKNOWN`, which outranks `MODERATE` and is outranked by `HIGH`.
///
/// Three shapes reach it, and §1.3's truth rule makes all three the same statement: Ono does not
/// know what this plan touches. A graph that ended at a boundary (§9.6), an effect the provider
/// could not classify (§8.1), and an action the operator declared opaque (§6.3).
fn unknown_impact(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let mut reasons: Vec<String> = Vec::new();
    if let Some(impact) = request.impact {
        let boundaries = impact.blast_radius().boundaries;
        if boundaries > 0 {
            reasons.push(format!(
                "the impact graph ends at {boundaries} boundary/boundaries Ono cannot see past"
            ));
        }
        if let Some(truncation) = impact.truncation() {
            reasons.push(format!("the impact graph is incomplete: {truncation}"));
        }
    }
    let unknown_effects = request
        .effects()
        .filter(|(_, effect)| {
            effect.confidence() == EffectConfidence::Unknown
                || effect.domain() == EffectDomain::Unknown
                || effect.kind() == EffectKind::Unknown
        })
        .count();
    if unknown_effects > 0 {
        reasons.push(format!(
            "{unknown_effects} declared effect(s) are UNKNOWN and §8.1 forbids promoting them"
        ));
    }
    let opaque = request
        .actions
        .iter()
        .filter(|action| action.execution().is_opaque())
        .count();
    if opaque > 0 {
        reasons.push(format!(
            "{opaque} action(s) are opaque, so their scope is outside what Ono can reason about \
             (§6.3)"
        ));
    }
    if reasons.is_empty() {
        return None;
    }
    finding(rule, RiskClass::Unknown, reasons.join("; "))
}

/// §30.5's reboot requirement, which is not the same thing as its recommendation.
fn reboot_required(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let (action, effect) = request
        .effects()
        .find(|(_, effect)| effect.object() == Some(REBOOT_REQUIRED))?;
    finding(
        rule,
        RiskClass::High,
        format!(
            "`{}` requires a reboot to take effect, which ends every process on the host: {}",
            action.summary(),
            effect.explanation()
        ),
    )
}

/// §29.1: a plan across hosts has no global atomicity, and Ono must not imply one.
fn remote_fanout(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let hosts = request.hosts();
    if hosts.len() < 2 {
        return None;
    }
    let class = if hosts.len() >= request.thresholds.warn_targets {
        RiskClass::High
    } else {
        RiskClass::Moderate
    };
    finding(
        rule,
        class,
        format!(
            "the plan reaches {} hosts, and §29.1 decomposes it into per-host fragments with no \
             global atomicity",
            hosts.len()
        ),
    )
}

/// §34.2: "If a plan may remove the path used by the active remote Ono link, this MUST be a
/// CRITICAL risk landmark."
fn link_loss(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let link = request.link?;
    let touched = request
        .effects()
        .filter(|(_, effect)| {
            matches!(
                effect.domain(),
                EffectDomain::NetworkRuntime | EffectDomain::RemoteSystem
            )
        })
        .find_map(|(_, effect)| {
            effect
                .object()
                .filter(|object| link.depends_on(object))
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            request
                .targeted_identities()
                .into_iter()
                .find(|object| link.depends_on(object))
                .map(ToOwned::to_owned)
        })?;
    finding(
        rule,
        RiskClass::Critical,
        format!(
            "the proposed change to `{touched}` may remove the transport path this active Ono \
             link to `{}` depends on (§34.2)",
            link.host()
        ),
    )
}

/// §19.1's recovery-complexity dimension (Appendix C.1, §13.7, §14.6).
///
/// Two classes, because the two methods are different promises. An offline root recovery cannot
/// be performed from the running system at all; a subvolume replacement can be prepared while
/// the system runs and completes at the next boot.
fn recovery_complexity(rule: &RiskRuleSpec, request: &RiskRequest<'_>) -> Option<RiskFinding> {
    let offline = request
        .recovery
        .iter()
        .find(|method| **method == RestoreMethod::OfflineRootRecovery);
    if offline.is_some() {
        return finding(
            rule,
            RiskClass::High,
            "recovery would need an offline method — the filesystem unmounted or a \
             boot-environment switch — so it cannot be performed from the running system (§13.7, \
             §14.6)"
                .to_owned(),
        );
    }
    let next_boot = request
        .recovery
        .iter()
        .find(|method| **method == RestoreMethod::SubvolumeReplacement)?;
    finding(
        rule,
        RiskClass::Moderate,
        format!(
            "recovery would need `{}`, which completes at the next boot rather than in place \
             (§14.4)",
            next_boot.as_str()
        ),
    )
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_declare_every_rule_with_an_id_a_dimension_and_a_doc_line() {
        for rule in rules() {
            assert!(
                rule.id().starts_with("risk."),
                "a rule id names the family it belongs to"
            );
            assert!(
                !rule.doc().is_empty(),
                "§19.2's rules are inspectable, so `{}` says what it looks for",
                rule.id()
            );
            assert!(RiskDimension::ALL.contains(&rule.dimension()));
        }
    }

    #[test]
    fn should_give_every_rule_a_distinct_id() {
        let ids: BTreeSet<&str> = rules().iter().map(RiskRuleSpec::id).collect();
        assert_eq!(
            ids.len(),
            rules().len(),
            "a finding names the rule that made it, so two rules must not share an id (§19.2)"
        );
    }

    #[test]
    fn should_find_a_declared_rule_by_id() {
        let found = rule("risk.remote.link-loss").expect("§34.2's rule is declared");
        assert_eq!(found.dimension(), RiskDimension::RemoteFanout);
        assert!(rule("risk.invented.by.a.model").is_none(), "§62.11");
    }

    #[test]
    fn should_default_the_bulk_thresholds_to_the_reference_configuration() {
        let thresholds = BulkThresholds::default();
        assert_eq!(
            thresholds.warn_targets, 10,
            "§53 `change.bulk.warn_targets`"
        );
        assert_eq!(
            thresholds.high_risk_targets, 50,
            "§53 `change.bulk.high_risk_targets`"
        );
    }

    #[test]
    fn should_recognise_the_route_and_the_interface_the_link_depends_on() {
        let link = ActiveLink::new("prod-router")
            .via_route("route/default")
            .via_interface("interface/eth0");
        assert!(link.depends_on("route/default"));
        assert!(link.depends_on("interface/eth0"));
        assert!(
            !link.depends_on("route/10.0.0.0-8"),
            "§34.2 is about the path this link uses, not about routes in general"
        );
    }
}
