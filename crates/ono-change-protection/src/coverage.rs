//! Appendix A, as an explicit pipeline (spec Appendix A, §10, §17.2).
//!
//! Appendix A is normative and it has seven steps. A.1 derives the mutation domains, A.2 gives
//! each one a recovery objective, A.3 asks the providers what they could offer, A.4 chooses among
//! the offers, A.5 composes the coverage, A.6 is the worked example and A.7 is the cap an unknown
//! domain puts on the whole plan. [`analyse`] runs the first four and hands the fifth to
//! `ProtectionSummary::level`, which already composes A.5 and A.7 and has no setter.
//!
//! # A.4 is not "strongest snapshot wins"
//!
//! The preference order is written out in Appendix A.4 and implemented in [`preference_key`]:
//!
//! 1. satisfies the required objective;
//! 2. smallest affected recovery scope;
//! 3. strongest useful consistency;
//! 4. lowest recovery destructiveness;
//! 5. lowest creation cost;
//! 6. lowest retained cost;
//! 7. lowest downtime.
//!
//! The order matters more than any single key. Appendix A.4's own example is a small
//! configuration-file backup dominating a root-dataset rollback for one file, because the
//! rollback's recovery blast radius takes everything else on the dataset with it. Scope and
//! destructiveness are decided before cost, so the cheap-to-create snapshot loses to the archive
//! that puts one file back.
//!
//! # Unknown never wins
//!
//! A candidate whose consistency the provider could not establish maps to
//! `DomainProtection::Unknown` in [`candidate_protection`], and an unknown protection satisfies no
//! objective. It therefore fails A.4's *first* key and can only be chosen when nothing else
//! exists — at which point the row says `UNKNOWN` and §55.6 case 29 holds: unknown provider
//! recovery semantics remain unknown.

use std::sync::Arc;

use ono_change_core::{
    ConsistencyClass, CoverageExclusion, DomainCoverage, DomainProtection, EffectDomain,
    EffectKind, PersistenceDomain, ProposedEffect, ProtectionAction, ProtectionLevel,
    ProtectionMode, ProtectionSummary, RecoveryAssetId, RecoveryCandidate, RecoveryCost,
    RecoveryObjective,
};

use crate::cost;
use crate::policy::ProtectionPolicy;
use crate::registry::{ProviderRefusal, ProviderRegistry};

/// One thing a plan proposes to change, in the domain it changes it in (Appendix A.1).
///
/// The domains come from the plan's actions' proposed effects; [`MutationDomain::from_effect`]
/// makes one out of a `ProposedEffect` so a caller derives them once and passes them in.
/// [`analyse`] never reads a `ChangePlan`, which keeps the coverage algorithm a pure function of
/// what it was told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationDomain {
    domain: EffectDomain,
    kind: EffectKind,
    subject: Arc<str>,
    compensation: Option<Arc<str>>,
    detail: Arc<str>,
}

impl MutationDomain {
    /// Declares that the plan changes `subject` in `domain`, in the way `kind` describes.
    #[must_use]
    pub fn new(
        domain: EffectDomain,
        kind: EffectKind,
        subject: impl Into<Arc<str>>,
        detail: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            domain,
            kind,
            subject: subject.into(),
            compensation: None,
            detail: detail.into(),
        }
    }

    /// Records the inverse action that would restore an acceptable semantic state (§27.4).
    ///
    /// Appendix A.6's `process-runtime` row is covered this way: restarting the service again is
    /// compensation, and §27.4 forbids calling it rollback.
    #[must_use]
    pub fn compensated_by(mut self, action: impl Into<Arc<str>>) -> Self {
        self.compensation = Some(action.into());
        self
    }

    /// Reads a mutation domain out of one proposed effect (Appendix A.1).
    #[must_use]
    pub fn from_effect(effect: &ProposedEffect) -> Self {
        Self {
            domain: effect.domain(),
            kind: effect.kind(),
            subject: Arc::from(effect.object().unwrap_or(effect.explanation())),
            compensation: effect.compensation().map(Arc::from),
            detail: Arc::from(effect.explanation()),
        }
    }

    /// The domain (Appendix A.1).
    #[must_use]
    pub const fn domain(&self) -> EffectDomain {
        self.domain
    }

    /// What the plan does to the subject.
    #[must_use]
    pub const fn kind(&self) -> EffectKind {
        self.kind
    }

    /// The object that changes — a path, a unit name, an endpoint.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The declared inverse action, where there is one (§27.4).
    #[must_use]
    pub fn compensation(&self) -> Option<&str> {
        self.compensation.as_deref()
    }

    /// The sentence the coverage row carries.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// What recovery this domain would need (Appendix A.2).
    #[must_use]
    pub fn objective(&self) -> RecoveryObjective {
        objective_for(self.domain, self.kind)
    }
}

/// The recovery objective a domain and a kind imply (Appendix A.2).
///
/// Persistent state gets `PRESERVE_EXACT`, because the operator who replaces a configuration file
/// expects the prior bytes back. Process and kernel runtime get `RESTORE_SEMANTIC`, because a
/// restarted service legitimately has new worker PIDs. Network sessions and anything emitted get
/// `NO_RECOVERY_REQUIRED` and stay visible as exclusions (Appendix A.5) — an HTTP POST has been
/// served by the time Ono could reconsider (§35.1). An effect in a domain Ono cannot classify gets
/// `UNKNOWN`, which nothing satisfies, and that is what caps the plan in A.7.
#[must_use]
pub const fn objective_for(domain: EffectDomain, kind: EffectKind) -> RecoveryObjective {
    if matches!(kind, EffectKind::Emit) {
        return RecoveryObjective::NoRecoveryRequired;
    }
    match domain {
        EffectDomain::FilesystemPersistent
        | EffectDomain::BlockStoragePersistent
        | EffectDomain::ApplicationPersistent
        | EffectDomain::IdentitySecurityState => RecoveryObjective::PreserveExact,
        EffectDomain::ProcessRuntime
        | EffectDomain::KernelRuntime
        | EffectDomain::ProviderTransactionState => RecoveryObjective::RestoreSemantic,
        EffectDomain::NetworkRuntime | EffectDomain::ExternalSideEffect => {
            RecoveryObjective::NoRecoveryRequired
        }
        EffectDomain::RemoteSystem => RecoveryObjective::Compensate,
        EffectDomain::Unknown => RecoveryObjective::Unknown,
    }
}

/// The exclusion a domain carries when no recovery is required of it (Appendix A.5).
///
/// A.5 is explicit that runtime and external effects stay visible even when the persistent part
/// of the plan is protected, and §2.13 makes an emitted request irreversible rather than merely
/// uncovered. §62.6 turns on this: protection is never rendered without what it leaves out.
#[must_use]
pub fn exclusion_for(mutation: &MutationDomain) -> Option<CoverageExclusion> {
    match (mutation.domain(), mutation.kind()) {
        (EffectDomain::ExternalSideEffect, _) | (_, EffectKind::Emit) => Some(
            CoverageExclusion::new(
                mutation.domain(),
                mutation.subject(),
                "§35.1: the call has left the system by the time recovery could reconsider, and no \
                 local recovery asset touches it",
            )
            .irreversible(),
        ),
        (EffectDomain::NetworkRuntime, _) => Some(CoverageExclusion::new(
            mutation.domain(),
            mutation.subject(),
            "§34: live sessions and in-flight connections are not restored by any recovery asset",
        )),
        (EffectDomain::Unknown, _) => Some(CoverageExclusion::new(
            mutation.domain(),
            mutation.subject(),
            "Appendix A.7 and §6.3: the provider declared a side-effect domain Ono cannot \
             classify, so no snapshot elsewhere on the host says anything about it",
        )),
        _ => None,
    }
}

/// Why a candidate was not used (Appendix A.4, §17.2, §38.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    /// Appendix A.4's first key: it cannot satisfy the objective the domain requires.
    ObjectiveUnsupported,
    /// Appendix A.4: another candidate was preferred, and the detail says which.
    Dominated,
    /// §38.3: it exceeds a configured cost limit.
    CostLimit,
    /// §17.2: the mode is `off`, so nothing is created and the opportunity is only reported.
    ProtectionOff,
    /// §17.2's `maximize`: it conflicts with a mechanism already chosen for this domain.
    Conflicting,
    /// The provider could not turn the candidate into a protection action (§12.1).
    NotPlannable,
}

impl RejectionReason {
    /// The word a rendering shows for the reason.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RejectionReason::ObjectiveUnsupported => "objective-unsupported",
            RejectionReason::Dominated => "dominated",
            RejectionReason::CostLimit => "cost-limit",
            RejectionReason::ProtectionOff => "protection-off",
            RejectionReason::Conflicting => "conflicting",
            RejectionReason::NotPlannable => "not-plannable",
        }
    }
}

/// A candidate that lost, and what it lost to (Appendix A.4).
///
/// The losers are kept because `inspect plan` has to be able to answer "why this asset and not
/// that one" (Appendix B.10, §20.1). A choice nobody can see the alternatives to is not
/// inspectable.
#[derive(Debug, Clone, PartialEq)]
pub struct RejectedCandidate {
    candidate: RecoveryCandidate,
    reason: RejectionReason,
    detail: Arc<str>,
}

impl RejectedCandidate {
    /// The candidate.
    #[must_use]
    pub const fn candidate(&self) -> &RecoveryCandidate {
        &self.candidate
    }

    /// Why it lost.
    #[must_use]
    pub const fn reason(&self) -> RejectionReason {
        self.reason
    }

    /// The sentence that says so.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Everything [`analyse`] needs, and nothing it could read for itself (Appendix A).
#[derive(Debug, Clone)]
pub struct CoverageRequest<'a> {
    registry: &'a ProviderRegistry,
    policy: &'a ProtectionPolicy,
    mutations: Vec<MutationDomain>,
    persistence: Vec<PersistenceDomain>,
}

impl<'a> CoverageRequest<'a> {
    /// A request against `registry` under `policy`.
    #[must_use]
    pub const fn new(registry: &'a ProviderRegistry, policy: &'a ProtectionPolicy) -> Self {
        Self {
            registry,
            policy,
            mutations: Vec::new(),
            persistence: Vec::new(),
        }
    }

    /// Adds one of the plan's mutation domains (Appendix A.1).
    #[must_use]
    pub fn mutating(mut self, mutation: MutationDomain) -> Self {
        self.mutations.push(mutation);
        self
    }

    /// Adds a resolved persistence domain the plan's targets map to (Appendix B).
    #[must_use]
    pub fn over(mut self, persistence: PersistenceDomain) -> Self {
        self.persistence.push(persistence);
        self
    }

    /// The mutation domains.
    #[must_use]
    pub fn mutations(&self) -> &[MutationDomain] {
        &self.mutations
    }

    /// The resolved persistence domains.
    #[must_use]
    pub fn persistence(&self) -> &[PersistenceDomain] {
        &self.persistence
    }

    /// The policy (§17).
    #[must_use]
    pub const fn policy(&self) -> &ProtectionPolicy {
        self.policy
    }

    /// The registry (§12.2).
    #[must_use]
    pub const fn registry(&self) -> &ProviderRegistry {
        self.registry
    }

    /// The persistence domain holding `subject`, where one was resolved (§11.2).
    ///
    /// An exact match first, then the deepest resolved domain whose path contains the subject:
    /// a plan that resolved `/etc/nginx` covers an action on `/etc/nginx/nginx.conf`, and §11.2
    /// still requires the mapping to have happened rather than being assumed here.
    #[must_use]
    pub fn persistence_for(&self, subject: &str) -> Option<&PersistenceDomain> {
        if let Some(exact) = self
            .persistence
            .iter()
            .find(|domain| domain.path() == subject)
        {
            return Some(exact);
        }
        self.persistence
            .iter()
            .filter(|domain| {
                let path = domain.path().trim_end_matches('/');
                !path.is_empty() && subject.starts_with(&format!("{path}/"))
            })
            .max_by_key(|domain| domain.path().len())
    }
}

/// The answer: the coverage matrix, what would be created, and what lost (Appendix A.5).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CoverageAnalysis {
    summary: ProtectionSummary,
    actions: Vec<ProtectionAction>,
    rejected: Vec<RejectedCandidate>,
    available: Vec<RecoveryCandidate>,
    refusals: Vec<ProviderRefusal>,
}

impl CoverageAnalysis {
    /// The coverage matrix (§10.3).
    #[must_use]
    pub const fn summary(&self) -> &ProtectionSummary {
        &self.summary
    }

    /// The plan-level word, composed by the summary (Appendix A.5, A.7).
    #[must_use]
    pub fn level(&self) -> ProtectionLevel {
        self.summary.level()
    }

    /// The protection actions the plan would run during PREPARE (§17.1).
    #[must_use]
    pub fn actions(&self) -> &[ProtectionAction] {
        &self.actions
    }

    /// The candidates that were not used, and why (Appendix A.4).
    #[must_use]
    pub fn rejected(&self) -> &[RejectedCandidate] {
        &self.rejected
    }

    /// Every candidate any available provider offered (§17.2's `off`).
    #[must_use]
    pub fn available(&self) -> &[RecoveryCandidate] {
        &self.available
    }

    /// The providers that could not be asked, and why (§55.6 case 29).
    #[must_use]
    pub fn provider_refusals(&self) -> &[ProviderRefusal] {
        &self.refusals
    }

    /// The domains that need coverage and do not have it (§10.3).
    #[must_use]
    pub fn shortfall(&self) -> Vec<&DomainCoverage> {
        self.summary.shortfall()
    }

    /// What the chosen protection would cost together (§38.1).
    #[must_use]
    pub fn estimated_cost(&self) -> RecoveryCost {
        cost::compose(
            self.actions
                .iter()
                .map(|action| action.candidate().cost().clone())
                .collect::<Vec<RecoveryCost>>()
                .as_slice(),
        )
    }
}

/// The protection one candidate would actually provide (§10.3, §11.3).
///
/// A provider that cannot say how consistent its capture is has not established the recovery
/// property, and §2.4 forbids promoting that to anything. §39.1 is the same rule from the other
/// side: a filesystem snapshot of a database is not an application-consistent backup of it, and
/// only the provider may say what it is.
#[must_use]
pub fn candidate_protection(candidate: &RecoveryCandidate) -> DomainProtection {
    match candidate.consistency() {
        ConsistencyClass::Unknown => DomainProtection::Unknown,
        ConsistencyClass::TransactionConsistent => DomainProtection::Transactional,
        _ => {
            if candidate.restore_method().restores_prior_state() {
                DomainProtection::Protected
            } else {
                DomainProtection::Compensatable
            }
        }
    }
}

/// Whether `candidate` satisfies `objective` (Appendix A.4's first key, A.5).
#[must_use]
pub fn satisfies(candidate: &RecoveryCandidate, objective: RecoveryObjective) -> bool {
    objective.satisfied_by(candidate_protection(candidate))
        && covers(candidate.objective(), objective)
}

/// Whether an offered objective is at least as strong as a required one (Appendix A.2).
const fn covers(offered: RecoveryObjective, required: RecoveryObjective) -> bool {
    match required {
        RecoveryObjective::NoRecoveryRequired => true,
        RecoveryObjective::Unknown => false,
        RecoveryObjective::PreserveExact => matches!(offered, RecoveryObjective::PreserveExact),
        RecoveryObjective::RestoreSemantic => matches!(
            offered,
            RecoveryObjective::PreserveExact | RecoveryObjective::RestoreSemantic
        ),
        RecoveryObjective::Compensate => matches!(
            offered,
            RecoveryObjective::PreserveExact
                | RecoveryObjective::RestoreSemantic
                | RecoveryObjective::Compensate
        ),
    }
}

/// Appendix A.4's preference key, smallest first.
///
/// Every field is "less is better", so a plain tuple comparison is the preference order. The
/// unknown quantities sort last rather than first: a provider that did not state a cost has not
/// thereby offered a cheap one (§2.4).
#[must_use]
pub fn preference_key(
    candidate: &RecoveryCandidate,
    objective: RecoveryObjective,
) -> (u8, usize, u8, u8, u128, u128, u128, u128) {
    let cost = candidate.cost();
    (
        u8::from(!satisfies(candidate, objective)),
        match candidate.scope_width() {
            0 => usize::MAX,
            width => width,
        },
        consistency_rank(candidate.consistency()),
        candidate.restore_method().destructiveness(),
        cost.creation_latency()
            .map_or(u128::MAX, |latency| latency.as_micros()),
        cost.initial_bytes()
            .map_or(u128::MAX, ono_value::ByteSize::bytes),
        cost.retained_bytes()
            .map_or(u128::MAX, ono_value::ByteSize::bytes),
        downtime(candidate),
    )
}

/// The candidates in Appendix A.4's preference order, best first.
///
/// The order is total: candidates that tie on every key are ordered by provider and scope so the
/// same inputs produce the same plan every time (§29.3's determinism, carried into v0.6 by §4.4's
/// digest).
#[must_use]
pub fn ranked(
    candidates: &[RecoveryCandidate],
    objective: RecoveryObjective,
) -> Vec<RecoveryCandidate> {
    let mut ordered: Vec<RecoveryCandidate> = candidates.to_vec();
    ordered.sort_by(|left, right| {
        preference_key(left, objective)
            .cmp(&preference_key(right, objective))
            .then_with(|| left.provider().cmp(right.provider()))
            .then_with(|| left.scope().domain().cmp(right.scope().domain()))
    });
    ordered
}

/// How strong a consistency claim is, strongest first, derived from the core's own lattice.
fn consistency_rank(class: ConsistencyClass) -> u8 {
    let weaker = ConsistencyClass::ALL
        .iter()
        .filter(|other| **other != class && class.weakest_of(**other) == **other)
        .count();
    u8::try_from(ConsistencyClass::ALL.len() - weaker).unwrap_or(u8::MAX)
}

/// How much downtime restoring from a candidate would need (Appendix A.4's last key).
fn downtime(candidate: &RecoveryCandidate) -> u128 {
    let cost = candidate.cost();
    let quiesce = cost.quiesce().map_or(0, |window| window.as_micros());
    let reboot = if cost.requires_reboot() { 1 } else { 0 };
    let offline = if cost.requires_offline() { 1 } else { 0 };
    quiesce.saturating_add((reboot + offline) * 60 * 60 * 1_000_000)
}

/// Runs Appendix A over `request` (Appendix A.1 to A.5, with A.7's cap).
#[must_use]
pub fn analyse(request: &CoverageRequest<'_>) -> CoverageAnalysis {
    let policy = request.policy();
    let mode = policy.mode();
    let mut analysis = CoverageAnalysis::default();
    let mut rows: Vec<DomainCoverage> = Vec::new();

    for mutation in request.mutations() {
        // A.2: what recovery this domain would need.
        let objective = mutation.objective();
        let irrelevant = policy.is_irrelevant(mutation.domain());
        let mut chosen: Vec<RecoveryCandidate> = Vec::new();
        let mut unknown_semantics = false;
        let mut note;

        if objective.is_required() {
            // A.3: ask every provider that can run here, and remember the ones that could not.
            let (candidates, mut note_from_discovery) = discover(request, mutation, &mut analysis);
            let planned = analysis.actions.len();
            let fitting = within_limits(request, &candidates, planned, &mut analysis);
            let ordered = ranked(&fitting, objective);
            let (usable, unusable): (Vec<RecoveryCandidate>, Vec<RecoveryCandidate>) = ordered
                .into_iter()
                .partition(|candidate| satisfies(candidate, objective));
            for candidate in unusable {
                // §39.1 and §55.6 case 29: a candidate whose consistency the provider could not
                // establish satisfies nothing, and it makes the domain's recovery properties
                // unknown rather than absent. The two are different answers and the row says so.
                let detail = if candidate.consistency() == ConsistencyClass::Unknown {
                    unknown_semantics = true;
                    format!(
                        "{} could not establish the consistency of what it would capture, so it \
                         satisfies no objective and the domain's recovery properties stay unknown \
                         (§39.1, §55.6 case 29)",
                        candidate.provider()
                    )
                } else {
                    format!(
                        "the candidate does not satisfy the {objective} this domain requires \
                         (Appendix A.4)"
                    )
                };
                reject(
                    &mut analysis,
                    candidate,
                    RejectionReason::ObjectiveUnsupported,
                    detail,
                );
            }
            if mode.creates_assets() {
                chosen = choose(&mut analysis, usable, mode);
            } else {
                for candidate in usable {
                    let detail = format!(
                        "protection mode is off; {} would have protected {} (§17.2)",
                        candidate.provider(),
                        candidate.scope().domain()
                    );
                    reject(
                        &mut analysis,
                        candidate,
                        RejectionReason::ProtectionOff,
                        detail,
                    );
                }
                note_from_discovery =
                    "protection is off, and the opportunities that were available are \
                     listed rather than taken (§17.2)"
                        .to_owned();
            }
            note = note_from_discovery;
        } else {
            note = format!(
                "no recovery is required of {}, and it stays visible as an exclusion \
                 (Appendix A.5)",
                mutation.domain()
            );
        }

        // A.3 continued: a candidate is a description until a provider turns it into a PREPARE
        // action, and a provider that cannot do that has not protected anything (§12.1, §2.1).
        let actions = plan_actions(request, &chosen, mode, &mut analysis);
        let protection = protection_of(&actions, mutation, unknown_semantics, &mut note);
        let mut row = DomainCoverage::new(mutation.domain(), objective, protection, note);
        if irrelevant {
            row = row.declared_irrelevant();
        }
        for action in &actions {
            row = row.by_asset(action.proposed_asset().id().clone());
        }
        if let Some(consistency) = weakest_consistency(&actions) {
            row = row.at_consistency(consistency);
        }
        if protection == DomainProtection::Transactional
            && let Some(action) = actions.first()
        {
            row = row.within_transaction(action.provider());
        }
        if let Some(exclusion) = exclusion_for(mutation) {
            row = row.excluding(exclusion);
        }
        for action in &actions {
            for exclusion in action.candidate().exclusions() {
                row = row.excluding(CoverageExclusion::new(
                    mutation.domain(),
                    exclusion.subject(),
                    exclusion.reason(),
                ));
            }
        }
        analysis.actions.extend(actions);
        rows.push(row);
    }

    analysis.summary = ProtectionSummary::of(rows);
    analysis
}

/// A.3: what the providers offer for this mutation domain, and what they could not be asked.
fn discover(
    request: &CoverageRequest<'_>,
    mutation: &MutationDomain,
    analysis: &mut CoverageAnalysis,
) -> (Vec<RecoveryCandidate>, String) {
    let Some(persistence) = request.persistence_for(mutation.subject()) else {
        return (
            Vec::new(),
            format!(
                "no persistence domain was resolved for {}, and §11.2 requires the mapping before \
                 protection is claimed",
                mutation.subject()
            ),
        );
    };
    if !persistence.is_protectable() {
        return (
            Vec::new(),
            format!(
                "{} has no persistence domain a local provider may protect: {}",
                mutation.subject(),
                persistence.detail()
            ),
        );
    }
    let outcome = request
        .registry()
        .discover(persistence, mutation.objective());
    for refusal in outcome.refusals() {
        if !analysis
            .refusals
            .iter()
            .any(|kept| kept.provider() == refusal.provider())
        {
            analysis.refusals.push(refusal.clone());
        }
    }
    let candidates: Vec<RecoveryCandidate> = outcome
        .candidates()
        .iter()
        .filter(|candidate| candidate.domain() == mutation.domain())
        .cloned()
        .collect();
    for candidate in &candidates {
        if !analysis.available.contains(candidate) {
            analysis.available.push(candidate.clone());
        }
    }
    let note = if candidates.is_empty() {
        if outcome.is_conclusive() {
            format!(
                "no registered provider offers protection for {} (Appendix A.3)",
                persistence.object().unwrap_or(mutation.subject())
            )
        } else {
            format!(
                "{} provider(s) could not be asked about {}, so the absence of a candidate \
                 establishes nothing (§55.6 case 29)",
                outcome.refusals().len(),
                mutation.subject()
            )
        }
    } else {
        format!(
            "{} covers {}",
            persistence.object_kind(),
            persistence.object().unwrap_or(mutation.subject())
        )
    };
    (candidates, note)
}

/// §38.3: drops the candidates a configured limit forbids, recording each one.
fn within_limits(
    request: &CoverageRequest<'_>,
    candidates: &[RecoveryCandidate],
    planned: usize,
    analysis: &mut CoverageAnalysis,
) -> Vec<RecoveryCandidate> {
    let limits = request.policy().limits();
    let mut fitting = Vec::new();
    for candidate in candidates {
        let breaches = limits.breaches(candidate.cost(), candidate.scope_width(), planned);
        if breaches.is_empty() {
            fitting.push(candidate.clone());
            continue;
        }
        let detail = breaches
            .iter()
            .map(crate::policy::LimitBreach::describe)
            .collect::<Vec<String>>()
            .join("; ");
        reject(
            analysis,
            candidate.clone(),
            RejectionReason::CostLimit,
            detail,
        );
    }
    fitting
}

/// A.4: the dominant candidate, plus `maximize`'s non-conflicting extras (§17.2).
fn choose(
    analysis: &mut CoverageAnalysis,
    ordered: Vec<RecoveryCandidate>,
    mode: ProtectionMode,
) -> Vec<RecoveryCandidate> {
    let mut chosen: Vec<RecoveryCandidate> = Vec::new();
    for candidate in ordered {
        let Some(winner) = chosen.first() else {
            chosen.push(candidate);
            continue;
        };
        if mode != ProtectionMode::Maximize {
            let detail = format!(
                "{} over {} was preferred: Appendix A.4 orders by objective, then recovery scope, \
                 then consistency, then how destructive the restore is",
                winner.provider(),
                winner.scope().domain()
            );
            reject(analysis, candidate, RejectionReason::Dominated, detail);
            continue;
        }
        // §17.2's `maximize` adds mechanisms that improve coverage. A second asset from the same
        // provider over the same object improves nothing, and §62.7 forbids reading the mode as
        // "snapshot everything": the only candidates considered here are ones offered for the
        // domains this plan actually mutates.
        let conflicts = chosen.iter().any(|kept| {
            kept.provider() == candidate.provider()
                || kept.scope().domain() == candidate.scope().domain()
        });
        if conflicts {
            let detail = format!(
                "{} already protects {} for this domain, and a second asset over the same object \
                 improves no coverage (§17.2)",
                winner.provider(),
                winner.scope().domain()
            );
            reject(analysis, candidate, RejectionReason::Conflicting, detail);
        } else {
            chosen.push(candidate);
        }
    }
    chosen
}

/// Turns chosen candidates into PREPARE actions, provider by provider (§12.1).
fn plan_actions(
    request: &CoverageRequest<'_>,
    chosen: &[RecoveryCandidate],
    mode: ProtectionMode,
    analysis: &mut CoverageAnalysis,
) -> Vec<ProtectionAction> {
    let mut actions: Vec<ProtectionAction> = Vec::new();
    let mut providers: Vec<&str> = Vec::new();
    for candidate in chosen {
        if !providers.contains(&candidate.provider()) {
            providers.push(candidate.provider());
        }
    }
    for provider_id in providers {
        let group: Vec<RecoveryCandidate> = chosen
            .iter()
            .filter(|candidate| candidate.provider() == provider_id)
            .cloned()
            .collect();
        let Some(provider) = request.registry().get(provider_id) else {
            for candidate in group {
                reject(
                    analysis,
                    candidate,
                    RejectionReason::NotPlannable,
                    format!("{provider_id} offered a candidate and is not registered here"),
                );
            }
            continue;
        };
        match provider.plan_protection(&group, mode) {
            Ok(planned) => {
                for action in planned {
                    // §17.2: everything `prefer` and `require` plan is required, and §2.3 aborts
                    // before mutation when it fails. `maximize`'s extras degrade the matrix
                    // instead of the plan.
                    let extra = mode == ProtectionMode::Maximize && !actions.is_empty();
                    actions.push(if extra { action.optional() } else { action });
                }
            }
            Err(error) => {
                let message = error.message().to_owned();
                analysis
                    .refusals
                    .push(ProviderRefusal::new(provider_id, message.clone(), error));
                for candidate in group {
                    reject(
                        analysis,
                        candidate,
                        RejectionReason::NotPlannable,
                        message.clone(),
                    );
                }
            }
        }
    }
    actions
}

/// What the planned actions actually protect this domain with (§10.3, A.5).
fn protection_of(
    actions: &[ProtectionAction],
    mutation: &MutationDomain,
    unknown_semantics: bool,
    note: &mut String,
) -> DomainProtection {
    if let Some(action) = actions.first() {
        return candidate_protection(action.candidate());
    }
    if unknown_semantics {
        // §55.6 case 29: the only thing on offer had recovery semantics nobody could state, and
        // §2.4 forbids reading that as either protection or its absence.
        *note = "the only candidate offered had recovery semantics its provider could not \
                 establish, so this domain's recovery properties are unknown (§39.1, §55.6 \
                 case 29)"
            .to_owned();
        return DomainProtection::Unknown;
    }
    if let Some(compensation) = mutation.compensation() {
        // §27.4: an inverse action restores an acceptable semantic state, and calling it rollback
        // is exactly the confusion the word `compensatable` exists to prevent.
        *note = format!("compensated by {compensation}, which is not a rollback (§27.4)");
        return DomainProtection::Compensatable;
    }
    DomainProtection::Unprotected
}

/// The weakest consistency any planned action achieves (§11.3, Appendix D.7).
fn weakest_consistency(actions: &[ProtectionAction]) -> Option<ConsistencyClass> {
    actions
        .iter()
        .map(|action| action.candidate().consistency())
        .reduce(ConsistencyClass::weakest_of)
}

/// Records a rejection.
fn reject(
    analysis: &mut CoverageAnalysis,
    candidate: RecoveryCandidate,
    reason: RejectionReason,
    detail: impl Into<Arc<str>>,
) {
    analysis.rejected.push(RejectedCandidate {
        candidate,
        reason,
        detail: detail.into(),
    });
}

/// The asset ids a coverage row rests on (§11.4).
#[must_use]
pub fn asset_ids(actions: &[ProtectionAction]) -> Vec<RecoveryAssetId> {
    actions
        .iter()
        .map(|action| action.proposed_asset().id().clone())
        .collect()
}
