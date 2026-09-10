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
//!
//! # A recursive mutation reaches every mount beneath it
//!
//! §32.3: *"Recursive path operations MUST NOT assume mounted filesystems or nested subvolumes
//! belong to the same recovery scope."* When a request carries a mount table
//! ([`CoverageRequest::within`]), a removal of a directory — or any mutation marked
//! [`MutationDomain::recursive`] — gets one row per filesystem mounted beneath it: a child
//! dataset, a nested subvolume, an ext4 disk or an NFS export is a persistence domain of its own,
//! and the plan is protected only where something captures each one. A tmpfs or a pseudo
//! filesystem beneath it holds no persistent state and stays visible as an exclusion (§32.4). A
//! candidate that captures the mounts beneath outranks one that does not, because for a tree the
//! objective of A.4's first key spans the whole tree.
//!
//! # Assets made one after another share no point in time
//!
//! Appendix D.7 forbids inventing atomicity. The actions a provider plans from one candidate are
//! one creation — `zfs snapshot -r` over a dataset tree is one atomic operation — while assets
//! from different candidates are created one after another. Where the rows of one effect domain
//! rest on more than one such creation, each of those rows is crash-consistent at best, and its
//! note says so.

use std::path::Path;
use std::sync::Arc;

use ono_change_core::{
    ConsistencyClass, CoverageExclusion, DomainCoverage, DomainProtection, EffectDomain,
    EffectKind, NonPersistentReason, PersistenceDomain, PlanAction, ProposedEffect,
    ProtectionAction, ProtectionLevel, ProtectionMode, ProtectionSummary, RecoveryAssetType,
    RecoveryCandidate, RecoveryCost, RecoveryObjective,
};

use crate::cost;
use crate::domain::{DomainReach, MountBoundary, MountTable, contains};
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
    recursive: bool,
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
            recursive: false,
        }
    }

    /// Declares that the mutation reaches everything beneath its subject (§32.2, §32.3).
    ///
    /// A removal already does, because removing a directory removes what it holds; this is for
    /// any other mutation a provider knows descends — a recursive ownership or mode change.
    #[must_use]
    pub const fn recursive(mut self) -> Self {
        self.recursive = true;
        self
    }

    /// Whether the mutation reaches everything beneath its subject (§32.3).
    ///
    /// A removal does: `remove file <dir> --recursive` and `remove dir` both delete whatever is
    /// mounted beneath, and a removal of a plain file reaches nothing beneath it anyway.
    #[must_use]
    pub const fn is_recursive(&self) -> bool {
        self.recursive || matches!(self.kind, EffectKind::Remove)
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
            recursive: false,
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

/// Every mutation domain a plan's actions declare (Appendix A.1).
///
/// Appendix A.1 is normative and short: *"For each MUTATE action, Ono MUST derive one or more
/// `MutationDomain` records."* The domains come from the effects the provider declared, which is
/// what makes Appendix A.1's own two worked examples fall out rather than being special-cased.
/// `restart service nginx` touches `process-runtime` and `network-runtime` and no persistent
/// domain, because a restart declares no persistent effect; the same plan with a configuration
/// replacement touches `filesystem-persistent`, because that action does.
///
/// A mutating action that declares no effect at all yields one [`EffectDomain::Unknown`] record.
/// That is deliberate, and it is Appendix A.7's hook: an action nobody could describe caps the
/// plan's protection, so §6.3's opaque action cannot inherit safety from a snapshot taken for
/// something else.
#[must_use]
pub fn mutation_domains(actions: &[PlanAction]) -> Vec<MutationDomain> {
    let mut domains: Vec<MutationDomain> = Vec::new();
    for action in actions
        .iter()
        .filter(|action| action.role().mutates_target())
    {
        if action.effects().is_empty() {
            push_unique(
                &mut domains,
                MutationDomain::new(
                    EffectDomain::Unknown,
                    EffectKind::Unknown,
                    action.target().unwrap_or_else(|| action.summary()),
                    format!(
                        "`{}` declares no effect, so what it changes could not be established",
                        action.summary()
                    ),
                ),
            );
            continue;
        }
        for effect in action.effects() {
            push_unique(&mut domains, MutationDomain::from_effect(effect));
        }
    }
    // Appendix A.1 lists the domains in a fixed order, and §4.4 seals the plan: two derivations of
    // one plan must compare equal, so the answer is ordered rather than merely observed.
    domains.sort_by_key(|mutation| {
        (
            EffectDomain::ALL
                .iter()
                .position(|domain| *domain == mutation.domain())
                .unwrap_or(usize::MAX),
            mutation.subject().to_owned(),
        )
    });
    domains
}

/// Records one mutation domain unless an identical one is already there.
///
/// A plan that touches one object twice in one domain has one coverage answer for it, and two
/// rows saying the same thing would be two rows an operator has to reconcile. Two *different*
/// objects in one domain stay separate rows, and §13.4 is why: `/etc/nginx/nginx.conf` on the root
/// dataset and `/data/customer.db` on `tank/data` are both `filesystem-persistent`, and merging
/// them into one row is exactly the claim that appendix forbids.
fn push_unique(domains: &mut Vec<MutationDomain>, mutation: MutationDomain) {
    let duplicate = domains.iter().any(|existing| {
        existing.domain() == mutation.domain() && existing.subject() == mutation.subject()
    });
    if !duplicate {
        domains.push(mutation);
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
    /// Appendix B.4: the domain is an overlay whose writable layer cannot be followed, and the
    /// candidate is not a copy of the visible bytes, so its protection would be the claim that
    /// snapshotting the merged mount protects data that lives elsewhere.
    SnapshotOfMergedView,
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
            RejectionReason::SnapshotOfMergedView => "snapshot-of-merged-view",
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
    mounts: Option<&'a MountTable>,
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
            mounts: None,
        }
    }

    /// Resolves the plan's paths against `mounts` (Appendix B.1, §32.2).
    ///
    /// With a table, a subject beneath a resolved directory is resolved through the mount
    /// boundaries between them rather than inherited from the directory (§13.4), and a recursive
    /// mutation is analysed over every mount beneath its subject (§32.3). Without one, the only
    /// boundaries known are the ones the resolved domains name.
    #[must_use]
    pub const fn within(mut self, mounts: &'a MountTable) -> Self {
        self.mounts = Some(mounts);
        self
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
    /// An exact match first. Then, where the request carries a mount table, `subject` is
    /// resolved through it (Appendix B.1): a file beneath `/srv` that lives on the child dataset
    /// mounted at `/srv/data` belongs to that dataset, and §13.4 is the rule that inheriting the
    /// directory's dataset would break. Without a table, the deepest resolved domain whose path
    /// contains the subject answers — but only when no mount any resolved domain names lies
    /// between the two, because a boundary between them means the subject is on another
    /// filesystem.
    #[must_use]
    pub fn persistence_for(&self, subject: &str) -> Option<PersistenceDomain> {
        if let Some(exact) = self
            .persistence
            .iter()
            .find(|domain| domain.path() == subject)
        {
            return Some(exact.clone());
        }
        if let Some(mounts) = self.mounts
            && subject.starts_with('/')
        {
            return Some(mounts.resolve(Path::new(subject)));
        }
        let serving = self
            .persistence
            .iter()
            .map(|domain| domain.mount().mount_point())
            .filter(|point| !point.is_empty() && contains(point, subject))
            .max_by_key(|point| point.trim_end_matches('/').len())?;
        self.persistence
            .iter()
            .filter(|domain| {
                let path = domain.path().trim_end_matches('/');
                !path.is_empty() && subject.starts_with(&format!("{path}/"))
            })
            .max_by_key(|domain| domain.path().len())
            .filter(|domain| domain.mount().mount_point() == serving)
            .cloned()
    }

    /// The filesystems a mutation reaches beneath its subject (§32.3).
    fn boundaries_reached_by(&self, mutation: &MutationDomain) -> Vec<MountBoundary> {
        match self.mounts {
            Some(mounts) if mutation.is_recursive() && mutation.subject().starts_with('/') => {
                mounts.boundaries_beneath(Path::new(mutation.subject()))
            }
            _ => Vec::new(),
        }
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
    let mut analysis = CoverageAnalysis::default();
    let mut drafts: Vec<RowDraft> = Vec::new();

    for mutation in request.mutations() {
        // A.2: what recovery this domain would need.
        let objective = mutation.objective();
        let irrelevant = policy.is_irrelevant(mutation.domain());
        let boundaries = if objective.is_required() {
            request.boundaries_reached_by(mutation)
        } else {
            Vec::new()
        };
        let mut found = if objective.is_required() {
            let persistence = request.persistence_for(mutation.subject());
            cover(
                request,
                &mut analysis,
                mutation,
                persistence.as_ref(),
                &boundaries,
            )
        } else {
            Cover::nothing(format!(
                "no recovery is required of {}, and it stays visible as an exclusion \
                 (Appendix A.5)",
                mutation.domain()
            ))
        };
        if !boundaries.is_empty() {
            found.note.push_str(&format!(
                "; the mutation reaches {} filesystem(s) mounted beneath {}, and each is a \
                 persistence domain of its own (§32.3)",
                boundaries.len(),
                mutation.subject()
            ));
        }
        let mut tree: Vec<ProtectionAction> = found.actions.clone();
        analysis.actions.extend(found.actions.iter().cloned());
        let mut parent = RowDraft::of(mutation, found, irrelevant);
        let mut children: Vec<RowDraft> = Vec::new();
        for boundary in &boundaries {
            let beneath: Vec<MountBoundary> = boundaries
                .iter()
                .filter(|other| {
                    other.mount_point() != boundary.mount_point()
                        && contains(boundary.mount_point(), other.mount_point())
                })
                .cloned()
                .collect();
            if let Some(child) = boundary_row(
                request,
                &mut analysis,
                mutation,
                boundary,
                &beneath,
                &mut tree,
                &mut parent,
                irrelevant,
            ) {
                children.push(child);
            }
        }
        drafts.push(parent);
        drafts.extend(children);
    }

    compose_sequential(&mut drafts);
    analysis.summary = ProtectionSummary::of(drafts.into_iter().map(RowDraft::build).collect());
    analysis
}

/// What A.3 and A.4 produced for one persistence domain: the actions, and what the row says.
struct Cover {
    actions: Vec<ProtectionAction>,
    note: String,
    unknown_semantics: bool,
    exclusions: Vec<CoverageExclusion>,
}

impl Cover {
    fn nothing(note: String) -> Self {
        Self {
            actions: Vec::new(),
            note,
            unknown_semantics: false,
            exclusions: Vec::new(),
        }
    }
}

/// A.3 and A.4 for one mutation over one persistence domain.
///
/// `boundaries` are the filesystems beneath the subject the mutation also reaches. They do not
/// get candidates here — each has a row of its own — but they order the candidates: for a tree,
/// one that captures the mounts beneath satisfies more of the objective than one that stops at
/// the top, so it is preferred before A.4's scope key would pick the narrower one.
fn cover(
    request: &CoverageRequest<'_>,
    analysis: &mut CoverageAnalysis,
    mutation: &MutationDomain,
    persistence: Option<&PersistenceDomain>,
    boundaries: &[MountBoundary],
) -> Cover {
    let objective = mutation.objective();
    let mode = request.policy().mode();
    // A.3: ask every provider that can run here, and remember the ones that could not.
    let (candidates, mut note, exclusions) = discover(request, mutation, persistence, analysis);
    let copy_only =
        persistence.is_some_and(|domain| DomainReach::of(domain) == DomainReach::CopyOnly);
    let candidates = if copy_only {
        copies_only(request, candidates, analysis)
    } else {
        candidates
    };
    let planned = analysis.actions.len();
    let fitting = within_limits(request, &candidates, planned, analysis);
    let mut ordered = ranked(&fitting, objective);
    if !boundaries.is_empty() {
        ordered.sort_by_key(|candidate| {
            boundaries
                .iter()
                .filter(|boundary| {
                    boundary.domain().is_protectable() && !captures(candidate, boundary)
                })
                .count()
        });
    }
    let (usable, unusable): (Vec<RecoveryCandidate>, Vec<RecoveryCandidate>) = ordered
        .into_iter()
        .partition(|candidate| satisfies(candidate, objective));
    let mut unknown_semantics = false;
    for candidate in unusable {
        // §39.1 and §55.6 case 29: a candidate whose consistency the provider could not
        // establish satisfies nothing, and it makes the domain's recovery properties unknown
        // rather than absent. The two are different answers and the row says so.
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
            analysis,
            candidate,
            RejectionReason::ObjectiveUnsupported,
            detail,
        );
    }
    let mut chosen: Vec<RecoveryCandidate> = Vec::new();
    if mode.creates_assets() {
        chosen = choose(analysis, usable, mode);
    } else {
        for candidate in usable {
            let detail = format!(
                "protection mode is off; {} would have protected {} (§17.2)",
                candidate.provider(),
                candidate.scope().domain()
            );
            reject(analysis, candidate, RejectionReason::ProtectionOff, detail);
        }
        "protection is off, and the opportunities that were available are listed rather than \
         taken (§17.2)"
            .clone_into(&mut note);
    }
    // A.3 continued: a candidate is a description until a provider turns it into a PREPARE
    // action, and a provider that cannot do that has not protected anything (§12.1, §2.1).
    let actions = plan_actions(request, &chosen, mode, analysis);
    if copy_only && !actions.is_empty() {
        // Appendix B.5 and §11.5: the copy is kept inside the container, on the same overlay as
        // what it protects, and a writable layer that is discarded takes both with it.
        note.push_str(
            "; the protection is container-local: the recovery copy lives on the same overlay as \
             the target, whose writable layer may be ephemeral, so it shares the target's \
             failure domain (Appendix B.5, §11.5)",
        );
    }
    Cover {
        actions,
        note,
        unknown_semantics,
        exclusions,
    }
}

/// The row of one filesystem mounted beneath a mutated directory (§32.2, §32.3).
///
/// A tmpfs or pseudo filesystem holds no persistent state, so it becomes an exclusion on the
/// directory's row rather than a row of its own (§32.4, Appendix B.7). Everything else is a
/// persistence domain the mutation reaches: captured by an action already planned for the tree
/// where that action's scope names it, otherwise offered to the providers in its own right, and
/// named as an exclusion when nothing captures it.
#[allow(
    clippy::too_many_arguments,
    reason = "one boundary of one tree, and every argument is a fact the row is made of"
)]
fn boundary_row(
    request: &CoverageRequest<'_>,
    analysis: &mut CoverageAnalysis,
    mutation: &MutationDomain,
    boundary: &MountBoundary,
    beneath: &[MountBoundary],
    tree: &mut Vec<ProtectionAction>,
    parent: &mut RowDraft,
    irrelevant: bool,
) -> Option<RowDraft> {
    let point = boundary.mount_point();
    let domain = boundary.domain();
    if matches!(
        domain.refusal(),
        Some(NonPersistentReason::Volatile | NonPersistentReason::Pseudo)
    ) {
        parent.exclusions.push(CoverageExclusion::new(
            mutation.domain(),
            point,
            format!(
                "§32.3: the mutation of {} reaches {point}, and {}",
                mutation.subject(),
                domain.detail()
            ),
        ));
        return None;
    }
    let child = MutationDomain::new(
        mutation.domain(),
        mutation.kind(),
        point,
        format!(
            "{point} is mounted beneath {} and the mutation reaches it",
            mutation.subject()
        ),
    );
    if domain.is_protectable() {
        let covering: Vec<ProtectionAction> = tree
            .iter()
            .filter(|action| captures(action.candidate(), boundary))
            .cloned()
            .collect();
        if let Some(first) = covering.first() {
            let note = format!(
                "{point} is the separate {} {} beneath {}, captured by {} in the same operation \
                 that captures the tree above it (§32.3)",
                domain.object_kind(),
                domain.object().unwrap_or(point),
                mutation.subject(),
                first.summary()
            );
            let mut draft = RowDraft::of(
                &child,
                Cover {
                    actions: covering,
                    note,
                    unknown_semantics: false,
                    exclusions: Vec::new(),
                },
                irrelevant,
            );
            // The candidate's own exclusions are on the directory's row already.
            draft.exclusions.clear();
            return Some(draft);
        }
    }
    let mut found = cover(request, analysis, &child, Some(domain), beneath);
    if found.actions.is_empty() && found.exclusions.is_empty() {
        found.exclusions.push(CoverageExclusion::new(
            mutation.domain(),
            point,
            format!(
                "§32.3: the mutation of {} reaches {point}, a separate filesystem nothing \
                 captures: {}",
                mutation.subject(),
                found.note
            ),
        ));
    }
    found.note = format!(
        "{point} is a separate filesystem beneath {} that the mutation reaches: {}",
        mutation.subject(),
        found.note
    );
    tree.extend(found.actions.iter().cloned());
    analysis.actions.extend(found.actions.iter().cloned());
    Some(RowDraft::of(&child, found, irrelevant))
}

/// Whether `candidate` captures the filesystem mounted at `boundary` (§13.4, §14.3, §32.3).
///
/// Either its scope is that persistence object or names it — `zfs snapshot -r` lists the child
/// datasets it takes — or it covers something strictly inside the mount, which only a copy that
/// crossed into that filesystem can. Listing the mountpoint directory itself is not enough: that
/// entry lives on the filesystem above.
fn captures(candidate: &RecoveryCandidate, boundary: &MountBoundary) -> bool {
    let scope = candidate.scope();
    let point = boundary.mount_point();
    let by_object = boundary
        .domain()
        .object()
        .is_some_and(|object| scope.domain() == object || scope.covers_object(object));
    by_object
        || scope
            .covers()
            .iter()
            .any(|covered| covered.as_ref() != point && contains(point, covered))
}

/// One row of the matrix before Appendix D.7's composition is applied to it.
struct RowDraft {
    domain: EffectDomain,
    objective: RecoveryObjective,
    protection: DomainProtection,
    note: String,
    actions: Vec<ProtectionAction>,
    consistency: Option<ConsistencyClass>,
    exclusions: Vec<CoverageExclusion>,
    not_protected_by: Vec<std::sync::Arc<str>>,
    irrelevant: bool,
}

impl RowDraft {
    fn of(mutation: &MutationDomain, cover: Cover, irrelevant: bool) -> Self {
        let Cover {
            actions,
            mut note,
            unknown_semantics,
            exclusions: refused,
        } = cover;
        let protection = protection_of(&actions, mutation, unknown_semantics, &mut note);
        let mut exclusions: Vec<CoverageExclusion> = exclusion_for(mutation).into_iter().collect();
        for action in &actions {
            for exclusion in action.candidate().exclusions() {
                exclusions.push(CoverageExclusion::new(
                    mutation.domain(),
                    exclusion.subject(),
                    exclusion.reason(),
                ));
            }
        }
        exclusions.extend(refused);
        // §13.4: what encloses the target without reaching it, as the providers of the chosen
        // candidates named it — less anything one of those candidates captures after all.
        let mut not_protected_by: Vec<std::sync::Arc<str>> = Vec::new();
        for object in actions
            .iter()
            .flat_map(|action| action.candidate().not_protecting())
        {
            let captured = actions.iter().any(|action| {
                let scope = action.candidate().scope();
                scope.domain() == object.as_ref() || scope.covers_object(object)
            });
            if !captured && !not_protected_by.contains(object) {
                not_protected_by.push(std::sync::Arc::clone(object));
            }
        }
        Self {
            domain: mutation.domain(),
            objective: mutation.objective(),
            protection,
            note,
            consistency: weakest_consistency(&actions),
            actions,
            exclusions,
            not_protected_by,
            irrelevant,
        }
    }

    /// The candidate whose creation this row rests on.
    fn creation(&self) -> Option<&RecoveryCandidate> {
        self.actions.first().map(ProtectionAction::candidate)
    }

    fn build(self) -> DomainCoverage {
        let mut row = DomainCoverage::new(self.domain, self.objective, self.protection, self.note);
        if self.irrelevant {
            row = row.declared_irrelevant();
        }
        for action in &self.actions {
            row = row.by_asset(action.proposed_asset().id().clone());
        }
        if let Some(consistency) = self.consistency {
            row = row.at_consistency(consistency);
        }
        if self.protection == DomainProtection::Transactional
            && let Some(action) = self.actions.first()
        {
            row = row.within_transaction(action.provider());
        }
        for exclusion in self.exclusions {
            row = row.excluding(exclusion);
        }
        for object in self.not_protected_by {
            row = row.outside_of(object);
        }
        row
    }
}

/// Appendix D.7: rows of one domain that rest on separate creations share no point in time.
///
/// The actions a provider plans from one candidate are one creation — `zfs snapshot -r` is the
/// case that matters — and assets from different candidates are made one after another. Where the
/// rows of one effect domain rest on more than one creation, restoring them together yields a
/// state that never existed as a whole, which is what crash-consistent means; each row is lowered
/// to that at best and says why.
fn compose_sequential(drafts: &mut [RowDraft]) {
    for domain in EffectDomain::ALL {
        let mut creations: Vec<&RecoveryCandidate> = Vec::new();
        for draft in drafts.iter().filter(|draft| draft.domain == *domain) {
            if let Some(creation) = draft.creation()
                && !creations.contains(&creation)
            {
                creations.push(creation);
            }
        }
        let count = creations.len();
        if count < 2 {
            continue;
        }
        for draft in drafts
            .iter_mut()
            .filter(|draft| draft.domain == *domain && !draft.actions.is_empty())
        {
            let composed = draft
                .consistency
                .map_or(ConsistencyClass::CrashConsistent, |class| {
                    class.weakest_of(ConsistencyClass::CrashConsistent)
                });
            draft.consistency = Some(composed);
            draft.note.push_str(&format!(
                "; {domain} rests on {count} assets created one after another in separate \
                 operations, so together they hold no common point in time and are {composed} at \
                 best (Appendix D.7)"
            ));
        }
    }
}

/// A.3: what the providers offer for this mutation domain, what they could not be asked, and
/// what the resolution itself refused (§11.2, Appendix B, ADR-0807).
fn discover(
    request: &CoverageRequest<'_>,
    mutation: &MutationDomain,
    persistence: Option<&PersistenceDomain>,
    analysis: &mut CoverageAnalysis,
) -> (Vec<RecoveryCandidate>, String, Vec<CoverageExclusion>) {
    let subject = mutation.subject();
    let unresolved = || {
        format!(
            "no persistence domain was resolved for {subject}, and §11.2 requires the mapping \
             before protection is claimed"
        )
    };
    if let Some(persistence) = persistence
        && !persistence.is_protectable()
    {
        // Appendix B.6, B.7 and §32.4: a refusal is part of the coverage answer, and an exclusion
        // is where the matrix states what the protection leaves out and why.
        return (
            Vec::new(),
            format!(
                "{subject} has no persistence domain a local provider may protect: {}",
                persistence.detail()
            ),
            vec![CoverageExclusion::new(
                mutation.domain(),
                subject,
                persistence.detail(),
            )],
        );
    }
    if persistence.is_none() && !subject.starts_with('/') {
        return (Vec::new(), unresolved(), Vec::new());
    }
    let outcome = request
        .registry()
        .discover_at(subject, persistence, mutation.objective());
    for refusal in outcome.refusals() {
        if !analysis
            .refusals
            .iter()
            .any(|kept| kept.provider() == refusal.provider())
        {
            analysis.refusals.push(refusal.clone());
        }
    }
    let resolutions = outcome.resolutions();
    let Some(named) = persistence.or_else(|| resolutions.first().map(|(_, domain)| *domain)) else {
        return (Vec::new(), unresolved(), Vec::new());
    };
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
                named.object().unwrap_or(subject)
            )
        } else {
            let refused: Vec<String> = outcome
                .refusals()
                .iter()
                .map(|refusal| format!("{} ({})", refusal.provider(), refusal.reason()))
                .collect();
            format!(
                "{} could not be asked about {subject}: {}. The absence of a candidate \
                 establishes nothing (§55.6 case 29)",
                if refused.len() == 1 {
                    "1 provider".to_owned()
                } else {
                    format!("{} providers", refused.len())
                },
                refused.join("; ")
            )
        }
    } else {
        format!(
            "{} covers {}",
            named.object_kind(),
            named.object().unwrap_or(subject)
        )
    };
    (candidates, note, Vec::new())
}

/// Appendix B.4: over an overlay whose writable layer cannot be followed, only a copy protects.
///
/// A candidate's mechanism is the asset its provider would create for it, so each provider is
/// asked to plan the candidate — side-effect free by §2.1 — and a candidate whose asset is not an
/// independent copy of the bytes is refused with B.4's reason before the ranking can prefer it. A
/// provider that cannot say what it would create has not shown it copies anything, and §56.3
/// makes that a refusal too.
fn copies_only(
    request: &CoverageRequest<'_>,
    candidates: Vec<RecoveryCandidate>,
    analysis: &mut CoverageAnalysis,
) -> Vec<RecoveryCandidate> {
    let mut kept = Vec::new();
    for candidate in candidates {
        let proposed: Option<Vec<RecoveryAssetType>> = request
            .registry()
            .get(candidate.provider())
            .and_then(|provider| {
                provider
                    .plan_protection(std::slice::from_ref(&candidate), ProtectionMode::Prefer)
                    .ok()
                    .map(|actions| {
                        actions
                            .iter()
                            .map(|action| action.proposed_asset().asset_type())
                            .collect()
                    })
            });
        match proposed {
            Some(types)
                if !types.is_empty() && types.iter().all(|kind| kind.is_independent_copy()) =>
            {
                kept.push(candidate);
            }
            proposed => {
                let mechanism = proposed
                    .and_then(|types| types.first().copied())
                    .map_or_else(
                        || "a mechanism it could not name".to_owned(),
                        |kind| format!("a {kind}"),
                    );
                let detail = format!(
                    "{} would protect {} with {mechanism}, and the writable layer of this overlay \
                     is not visible here: Appendix B.4 forbids claiming that snapshotting the \
                     merged mount protects data whose writable layer resides elsewhere. Only a \
                     copy of the visible bytes, written back through the same view, may",
                    candidate.provider(),
                    candidate.scope().domain()
                );
                reject(
                    analysis,
                    candidate,
                    RejectionReason::SnapshotOfMergedView,
                    detail,
                );
            }
        }
    }
    kept
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
/// Every domain the sealed plan showed as protected that `fresh` no longer protects (§2.3, §10.3).
///
/// A sealed plan carries its coverage matrix and not the actions that produce it, so `apply`
/// recomputes them. The operator approved the plan with the matrix in view, and a recomputation
/// that can no longer protect one of its rows would turn protected execution into unprotected
/// execution without saying so. The answer names each such domain so the refusal can.
///
/// A row the sealed plan did not show as satisfied is not a promise, so an unprotected plan
/// applies unprotected, as it said it would.
#[must_use]
pub fn protection_lost(sealed: &ProtectionSummary, fresh: &ProtectionSummary) -> Vec<String> {
    sealed
        .rows()
        .iter()
        .filter(|row| row.is_satisfied() && row.protection() != DomainProtection::Unknown)
        .filter(|row| {
            !fresh
                .rows()
                .iter()
                .any(|now| now.domain() == row.domain() && now.is_satisfied())
        })
        .map(|row| row.domain().as_str().to_owned())
        .collect()
}
