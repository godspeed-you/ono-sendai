//! Protection as coverage rather than a boolean (spec v0.6 §10, Appendix A).
//!
//! §10.1 forbids `reversible: true/false`, and this module is the shape of that refusal. A plan
//! carries a [`ProtectionSummary`]: one [`DomainCoverage`] row per mutation domain, each with its
//! own recovery objective, its own level and its own exclusions. The plan-level word in §10.2 is
//! **computed** from those rows by [`ProtectionSummary::level`] and can never be set, so the
//! invariant §2.3 and §4.6 both depend on — *a protection level never overstates actual
//! coverage* — is a property of the type rather than a rule reviewers have to remember.
//!
//! The composition is Appendix A.5 with A.7's cap, and the two rules worth stating in full:
//!
//! - A plan is [`ProtectionLevel::Protected`] only when every required persistent domain is
//!   covered by a validated recovery path. Runtime and external domains stay visible as
//!   exclusions and do not earn the word (Appendix A.6).
//! - A domain whose recovery properties are unknown never satisfies anything, and an unknown
//!   *domain* caps the plan at [`ProtectionLevel::PartiallyProtected`] unless policy has declared
//!   it irrelevant (Appendix A.7). §6.3's opaque action therefore cannot inherit safety from a
//!   snapshot taken for something else.

use std::sync::Arc;

use crate::effect::EffectDomain;
use crate::id::RecoveryAssetId;
use crate::vocab::vocabulary;

vocabulary! {
    /// The plan-level protection status of §10.2.
    ProtectionLevel {
        Unprotected => "unprotected", "§10.2: no usable recovery or compensation mechanism covers the required persistent effects.";
        Compensatable => "compensatable", "§10.2: no prior state image exists, but declared inverse actions can restore an acceptable semantic state. §27.4 forbids calling this rollback.";
        PartiallyProtected => "partially-protected", "§10.2: some important effects are covered and others are not. The matrix says which.";
        Protected => "protected", "§10.2: every known persistent state mutation the plan requires is covered by validated recovery assets or provider restore contracts. Runtime and external effects may still be excluded, and §4.6 forbids using the word for partial coverage.";
        Transactional => "transactional", "§10.2: one provider guarantees atomic commit and rollback for the entire protected scope. §27.2 forbids the word for a multi-provider plan.";
        Unknown => "unknown", "§10.2: Ono cannot establish the recovery properties at all.";
    }
}

impl ProtectionLevel {
    /// Whether this level may be shown as covering the persistent state a plan changes.
    #[must_use]
    pub const fn covers_persistent_state(self) -> bool {
        matches!(
            self,
            ProtectionLevel::Protected | ProtectionLevel::Transactional
        )
    }

    /// The v0.6 §20.3 symbol for this level, in the ASCII fallback every terminal has.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            ProtectionLevel::Protected | ProtectionLevel::Transactional => "<->",
            ProtectionLevel::PartiallyProtected => "1/2",
            ProtectionLevel::Compensatable => "~>",
            ProtectionLevel::Unprotected => "!",
            ProtectionLevel::Unknown => "?",
        }
    }
}

vocabulary! {
    /// What one domain is covered by, before the plan-level word is composed (§10.3).
    ///
    /// `PARTIALLY_PROTECTED` is deliberately absent: a domain is atomic, and partial coverage is
    /// a statement about a plan spanning several domains, not about one of them.
    DomainProtection {
        Unprotected => "unprotected", "Nothing covers this domain.";
        Compensatable => "compensatable", "An inverse action restores an acceptable semantic state, without a prior state image (§27.4).";
        Protected => "protected", "A validated recovery asset or provider restore contract covers the domain (§11.4).";
        Transactional => "transactional", "The provider guarantees atomic commit and rollback within its own boundary (§27.1).";
        Unknown => "unknown", "The recovery properties of this domain could not be established. §2.4 forbids promoting this.";
    }
}

impl DomainProtection {
    /// The plan-level word this domain level would carry on its own.
    #[must_use]
    pub const fn level(self) -> ProtectionLevel {
        match self {
            DomainProtection::Unprotected => ProtectionLevel::Unprotected,
            DomainProtection::Compensatable => ProtectionLevel::Compensatable,
            DomainProtection::Protected => ProtectionLevel::Protected,
            DomainProtection::Transactional => ProtectionLevel::Transactional,
            DomainProtection::Unknown => ProtectionLevel::Unknown,
        }
    }

    /// Whether this level rests on a captured image of the prior state.
    #[must_use]
    pub const fn is_state_image(self) -> bool {
        matches!(
            self,
            DomainProtection::Protected | DomainProtection::Transactional
        )
    }
}

vocabulary! {
    /// What recovery of a domain would have to achieve (Appendix A.2).
    RecoveryObjective {
        PreserveExact => "preserve-exact", "Appendix A.2: the operator expects the exact prior bytes and metadata back. Appropriate for file and configuration state.";
        RestoreSemantic => "restore-semantic", "Appendix A.2: identity may legitimately change — a restarted service's worker PIDs — provided the semantic state returns.";
        Compensate => "compensate", "Appendix A.2: an inverse action is acceptable and is not equivalent to the prior state.";
        NoRecoveryRequired => "no-recovery-required", "Appendix A.2: the plan does not ask for this domain to come back. It stays visible as an exclusion (Appendix A.5).";
        Unknown => "unknown", "Appendix A.2: the objective itself could not be established, which nothing can satisfy.";
    }
}

impl RecoveryObjective {
    /// Whether a domain carrying this objective must be covered before a plan may be protected.
    #[must_use]
    pub const fn is_required(self) -> bool {
        !matches!(self, RecoveryObjective::NoRecoveryRequired)
    }

    /// Whether `protection` satisfies this objective (Appendix A.5).
    ///
    /// `PRESERVE_EXACT` is the strict one: only a captured state image satisfies it, because a
    /// compensating action cannot promise the same bytes back. `RESTORE_SEMANTIC` and
    /// `COMPENSATE` accept compensation, which is what lets Appendix A.6's example call a plan
    /// protected while its process runtime is merely compensatable.
    #[must_use]
    pub const fn satisfied_by(self, protection: DomainProtection) -> bool {
        match self {
            RecoveryObjective::PreserveExact => protection.is_state_image(),
            RecoveryObjective::RestoreSemantic | RecoveryObjective::Compensate => matches!(
                protection,
                DomainProtection::Compensatable
                    | DomainProtection::Protected
                    | DomainProtection::Transactional
            ),
            RecoveryObjective::NoRecoveryRequired => true,
            RecoveryObjective::Unknown => false,
        }
    }
}

vocabulary! {
    /// How consistent the state a recovery asset captures actually is (§11.3).
    ///
    /// §39.1 makes the distinction between the third and fourth members the one that must be
    /// visible everywhere: a filesystem snapshot of a database is not an application-consistent
    /// backup of it, and only a database-aware provider may say otherwise.
    ConsistencyClass {
        ByteConsistent => "byte-consistent", "§11.3: the provider can restore the captured bytes and files, and claims nothing filesystem- or application-wide.";
        FilesystemConsistent => "filesystem-consistent", "§11.3: the storage mechanism captured a filesystem, subvolume or dataset point in time under its own atomicity semantics.";
        CrashConsistent => "crash-consistent", "§11.3: the state is what abrupt interruption would have left, usable only by applications that recover from that.";
        ApplicationConsistent => "application-consistent", "§11.3: the application participated in quiesce or checkpoint semantics and owns the claim (§39.2).";
        TransactionConsistent => "transaction-consistent", "§11.3: a transaction-capable provider supplies its own atomic guarantee.";
        Unknown => "unknown", "§11.3: the consistency of the captured state could not be established.";
    }
}

impl ConsistencyClass {
    /// The weaker of two consistency claims.
    ///
    /// A recovery asset set spanning two mechanisms is only as consistent as its weakest member,
    /// and Appendix D.7 forbids inventing cross-subvolume atomicity where none was proven. Like
    /// [`crate::EffectConfidence::weakest_of`], there is no counterpart that strengthens.
    #[must_use]
    pub fn weakest_of(self, other: Self) -> Self {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }

    const fn rank(self) -> u8 {
        match self {
            ConsistencyClass::TransactionConsistent => 0,
            ConsistencyClass::ApplicationConsistent => 1,
            ConsistencyClass::FilesystemConsistent => 2,
            ConsistencyClass::CrashConsistent => 3,
            ConsistencyClass::ByteConsistent => 4,
            ConsistencyClass::Unknown => 5,
        }
    }
}

vocabulary! {
    /// The protection policy modes of §17.2.
    ProtectionMode {
        Off => "off", "§17.2: create no automatic recovery assets, and still show the protection that was available.";
        Prefer => "prefer", "§17.2 and §17.1: use protection for persistent mutations where a provider can create a bounded-cost asset without materially changing the operation. The interactive default.";
        Require => "require", "§17.2: refuse to apply when a required mutation domain cannot reach the plan's required protection class.";
        Maximize => "maximize", "§17.2: attempt every non-conflicting mechanism that improves coverage inside the configured cost limits. Never 'snapshot everything on the host'.";
    }
}

impl ProtectionMode {
    /// Whether the mode creates recovery assets during PREPARE at all.
    #[must_use]
    pub const fn creates_assets(self) -> bool {
        !matches!(self, ProtectionMode::Off)
    }

    /// Whether unmet coverage must refuse the apply rather than proceed (§17.2 `require`).
    #[must_use]
    pub const fn refuses_shortfall(self) -> bool {
        matches!(self, ProtectionMode::Require)
    }
}

/// Something a plan's protection does not cover, stated where the coverage is stated (§10.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageExclusion {
    domain: EffectDomain,
    subject: Arc<str>,
    reason: Arc<str>,
    irreversible: bool,
}

impl CoverageExclusion {
    /// Records that `subject` in `domain` is outside the protection, and why.
    #[must_use]
    pub fn new(
        domain: EffectDomain,
        subject: impl Into<Arc<str>>,
        reason: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            domain,
            subject: subject.into(),
            reason: reason.into(),
            irreversible: false,
        }
    }

    /// Marks the excluded subject as one nothing can restore (§2.13).
    #[must_use]
    pub const fn irreversible(mut self) -> Self {
        self.irreversible = true;
        self
    }

    /// The domain the excluded subject belongs to.
    #[must_use]
    pub const fn domain(&self) -> EffectDomain {
        self.domain
    }

    /// What is excluded.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Why it is excluded.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Whether the excluded subject is irreversible rather than merely uncovered.
    #[must_use]
    pub const fn is_irreversible(&self) -> bool {
        self.irreversible
    }
}

/// One row of the coverage matrix: a domain, what recovery it needs, and what it has (§10.3).
#[derive(Debug, Clone, PartialEq)]
pub struct DomainCoverage {
    domain: EffectDomain,
    objective: RecoveryObjective,
    protection: DomainProtection,
    assets: Vec<RecoveryAssetId>,
    consistency: Option<ConsistencyClass>,
    transaction_scope: Option<Arc<str>>,
    exclusions: Vec<CoverageExclusion>,
    note: Arc<str>,
    policy_irrelevant: bool,
}

impl DomainCoverage {
    /// Declares that `domain` needs `objective` and currently has `protection`.
    #[must_use]
    pub fn new(
        domain: EffectDomain,
        objective: RecoveryObjective,
        protection: DomainProtection,
        note: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            domain,
            objective,
            protection,
            assets: Vec::new(),
            consistency: None,
            transaction_scope: None,
            exclusions: Vec::new(),
            note: note.into(),
            policy_irrelevant: false,
        }
    }

    /// Names a validated asset that covers this domain (§11.4).
    #[must_use]
    pub fn by_asset(mut self, asset: RecoveryAssetId) -> Self {
        self.assets.push(asset);
        self
    }

    /// Records the consistency class the covering mechanism achieves.
    #[must_use]
    pub const fn at_consistency(mut self, consistency: ConsistencyClass) -> Self {
        self.consistency = Some(consistency);
        self
    }

    /// Names the provider whose transaction boundary this domain sits inside (§10.2, §27.1).
    #[must_use]
    pub fn within_transaction(mut self, provider: impl Into<Arc<str>>) -> Self {
        self.transaction_scope = Some(provider.into());
        self
    }

    /// Records something this row does not cover.
    #[must_use]
    pub fn excluding(mut self, exclusion: CoverageExclusion) -> Self {
        self.exclusions.push(exclusion);
        self
    }

    /// Marks the domain as one policy has explicitly declared irrelevant (Appendix A.7).
    ///
    /// This is the only way an unknown domain stops capping the plan, and it is an operator
    /// decision recorded in the sealed plan rather than a default.
    #[must_use]
    pub const fn declared_irrelevant(mut self) -> Self {
        self.policy_irrelevant = true;
        self
    }

    /// The domain this row is about.
    #[must_use]
    pub const fn domain(&self) -> EffectDomain {
        self.domain
    }

    /// What recovery this domain needs.
    #[must_use]
    pub const fn objective(&self) -> RecoveryObjective {
        self.objective
    }

    /// What covers it.
    #[must_use]
    pub const fn protection(&self) -> DomainProtection {
        self.protection
    }

    /// The assets that cover it.
    #[must_use]
    pub fn assets(&self) -> &[RecoveryAssetId] {
        &self.assets
    }

    /// The consistency the covering mechanism achieves, where one is claimed.
    #[must_use]
    pub const fn consistency(&self) -> Option<ConsistencyClass> {
        self.consistency
    }

    /// The provider transaction this row sits inside, where it does.
    #[must_use]
    pub fn transaction_scope(&self) -> Option<&str> {
        self.transaction_scope.as_deref()
    }

    /// What this row does not cover.
    #[must_use]
    pub fn exclusions(&self) -> &[CoverageExclusion] {
        &self.exclusions
    }

    /// The sentence a person reads beside the row.
    #[must_use]
    pub fn note(&self) -> &str {
        &self.note
    }

    /// Whether policy has declared this domain irrelevant to the requested recovery.
    #[must_use]
    pub const fn is_declared_irrelevant(&self) -> bool {
        self.policy_irrelevant
    }

    /// Whether the plan must cover this domain before it may be called protected.
    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.objective.is_required() && !self.policy_irrelevant
    }

    /// Whether what covers this domain satisfies what it needs (Appendix A.5).
    #[must_use]
    pub const fn is_satisfied(&self) -> bool {
        self.objective.satisfied_by(self.protection)
    }
}

/// The whole coverage matrix of one plan, and the single word §20.4 puts at the top of it.
///
/// The word is computed. There is no constructor that takes one, and no setter, because §2.3 and
/// §62.1 both fail the moment a level can be asserted independently of the rows beneath it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProtectionSummary {
    rows: Vec<DomainCoverage>,
    exclusions: Vec<CoverageExclusion>,
}

impl ProtectionSummary {
    /// An empty summary — the honest state of a plan whose domains have not been analysed.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            rows: Vec::new(),
            exclusions: Vec::new(),
        }
    }

    /// Builds a summary over `rows`.
    #[must_use]
    pub fn of(rows: Vec<DomainCoverage>) -> Self {
        Self {
            rows,
            exclusions: Vec::new(),
        }
    }

    /// Adds a plan-level exclusion that belongs to no single domain row.
    #[must_use]
    pub fn excluding(mut self, exclusion: CoverageExclusion) -> Self {
        self.exclusions.push(exclusion);
        self
    }

    /// The coverage matrix.
    #[must_use]
    pub fn rows(&self) -> &[DomainCoverage] {
        &self.rows
    }

    /// Every exclusion, from the rows and from the plan (§10.3, Appendix A.5).
    ///
    /// §62.6 turns on this method: the summary is never rendered without them, so protection
    /// cannot read as permission to be reckless.
    #[must_use]
    pub fn exclusions(&self) -> Vec<&CoverageExclusion> {
        self.rows
            .iter()
            .flat_map(DomainCoverage::exclusions)
            .chain(self.exclusions.iter())
            .collect()
    }

    /// The rows a plan must cover before it may be called protected.
    pub fn required_rows(&self) -> impl Iterator<Item = &DomainCoverage> {
        self.rows.iter().filter(|row| row.is_required())
    }

    /// The required rows nothing covers (§10.3's honest half of the matrix).
    #[must_use]
    pub fn shortfall(&self) -> Vec<&DomainCoverage> {
        self.required_rows()
            .filter(|row| !row.is_satisfied())
            .collect()
    }

    /// The plan-level protection status, composed from the rows (Appendix A.5, A.7).
    #[must_use]
    pub fn level(&self) -> ProtectionLevel {
        let required: Vec<&DomainCoverage> = self.required_rows().collect();
        if required.is_empty() {
            return ProtectionLevel::Unprotected;
        }
        let satisfied = required.iter().filter(|row| row.is_satisfied()).count();
        if satisfied == 0 {
            return if required
                .iter()
                .all(|row| row.protection == DomainProtection::Unknown)
            {
                ProtectionLevel::Unknown
            } else {
                ProtectionLevel::Unprotected
            };
        }
        if satisfied < required.len() {
            return ProtectionLevel::PartiallyProtected;
        }
        // Everything required is satisfied. §27.1 lets one provider claim atomicity for its own
        // scope, and §27.2 forbids the word the moment a second boundary is involved.
        let scopes: Vec<&str> = required
            .iter()
            .filter_map(|row| row.transaction_scope())
            .collect();
        let transactional = required
            .iter()
            .all(|row| row.protection == DomainProtection::Transactional)
            && scopes.len() == required.len()
            && scopes.windows(2).all(|pair| pair[0] == pair[1]);
        if transactional {
            return ProtectionLevel::Transactional;
        }
        // Appendix A.7: an unknown domain caps the answer, even when everything else lines up.
        if required
            .iter()
            .any(|row| row.domain() == EffectDomain::Unknown)
        {
            return ProtectionLevel::PartiallyProtected;
        }
        let mut persistent = required
            .iter()
            .filter(|row| row.domain().is_persistent())
            .peekable();
        if persistent.peek().is_some() {
            return if persistent.all(|row| row.protection.is_state_image()) {
                ProtectionLevel::Protected
            } else {
                ProtectionLevel::PartiallyProtected
            };
        }
        ProtectionLevel::Compensatable
    }

    /// The weakest consistency any covering mechanism achieves, where any claims one.
    #[must_use]
    pub fn consistency(&self) -> Option<ConsistencyClass> {
        self.rows
            .iter()
            .filter(|row| row.is_required() && row.is_satisfied())
            .filter_map(DomainCoverage::consistency)
            .reduce(ConsistencyClass::weakest_of)
    }

    /// Whether any effect the plan carries is irreversible (§2.13, §19.4).
    #[must_use]
    pub fn has_irreversible(&self) -> bool {
        self.exclusions()
            .iter()
            .any(|exclusion| exclusion.is_irreversible())
    }

    /// The canonical text this summary contributes to a plan digest (§4.4).
    #[must_use]
    pub fn digest_text(&self) -> String {
        let mut text = String::new();
        for row in &self.rows {
            use std::fmt::Write as _;
            let _ = write!(
                text,
                "{}={}/{}/{}\u{1f}",
                row.domain().as_str(),
                row.objective().as_str(),
                row.protection().as_str(),
                u8::from(row.is_declared_irrelevant()),
            );
        }
        text
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    fn row(
        domain: EffectDomain,
        objective: RecoveryObjective,
        protection: DomainProtection,
    ) -> DomainCoverage {
        DomainCoverage::new(domain, objective, protection, "test row")
    }

    #[test]
    fn should_call_a_plan_protected_when_every_persistent_domain_has_a_state_image() {
        // Appendix A.6, verbatim: a config replacement with a ZFS snapshot, a service restart
        // compensated by restarting, and network sessions nobody claims to recover.
        let summary = ProtectionSummary::of(vec![
            row(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
            ),
            row(
                EffectDomain::ProcessRuntime,
                RecoveryObjective::RestoreSemantic,
                DomainProtection::Compensatable,
            ),
            row(
                EffectDomain::NetworkRuntime,
                RecoveryObjective::NoRecoveryRequired,
                DomainProtection::Unprotected,
            ),
        ]);
        assert_eq!(
            summary.level(),
            ProtectionLevel::Protected,
            "Appendix A.6 composes exactly this matrix to PROTECTED"
        );
    }

    #[test]
    fn should_never_call_a_plan_protected_when_a_persistent_domain_is_uncovered() {
        let summary = ProtectionSummary::of(vec![
            row(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
            ),
            row(
                EffectDomain::ApplicationPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Unprotected,
            ),
        ]);
        assert_eq!(
            summary.level(),
            ProtectionLevel::PartiallyProtected,
            "§4.6: the word must not be a marketing label for partial protection"
        );
    }

    #[test]
    fn should_refuse_to_let_compensation_satisfy_an_exact_objective() {
        let summary = ProtectionSummary::of(vec![row(
            EffectDomain::FilesystemPersistent,
            RecoveryObjective::PreserveExact,
            DomainProtection::Compensatable,
        )]);
        assert_eq!(
            summary.level(),
            ProtectionLevel::Unprotected,
            "an inverse action cannot promise the same bytes back (Appendix A.2)"
        );
    }

    #[test]
    fn should_stay_unprotected_for_a_signal_even_where_a_snapshot_exists() {
        // §59.4 and §33.1: even on ZFS, killing a process leaves runtime state unrecoverable.
        let summary = ProtectionSummary::of(vec![row(
            EffectDomain::ProcessRuntime,
            RecoveryObjective::PreserveExact,
            DomainProtection::Unprotected,
        )]);
        assert_eq!(
            summary.level(),
            ProtectionLevel::Unprotected,
            "§33.1: a filesystem snapshot does not change this classification"
        );
    }

    #[test]
    fn should_cap_at_partially_protected_when_a_domain_is_unknown() {
        // Appendix A.7: an opaque action must not inherit safety from an unrelated snapshot.
        let summary = ProtectionSummary::of(vec![
            row(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
            ),
            row(
                EffectDomain::Unknown,
                RecoveryObjective::RestoreSemantic,
                DomainProtection::Compensatable,
            ),
        ]);
        assert_eq!(
            summary.level(),
            ProtectionLevel::PartiallyProtected,
            "Appendix A.7 caps the plan while an unknown domain is required"
        );
    }

    #[test]
    fn should_lift_the_unknown_cap_only_when_policy_declares_the_domain_irrelevant() {
        let summary = ProtectionSummary::of(vec![
            row(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
            ),
            row(
                EffectDomain::Unknown,
                RecoveryObjective::RestoreSemantic,
                DomainProtection::Compensatable,
            )
            .declared_irrelevant(),
        ]);
        assert_eq!(
            summary.level(),
            ProtectionLevel::Protected,
            "Appendix A.7's one escape is an explicit policy declaration, not a default"
        );
    }

    #[test]
    fn should_answer_unknown_when_nothing_about_recovery_could_be_established() {
        let summary = ProtectionSummary::of(vec![row(
            EffectDomain::ApplicationPersistent,
            RecoveryObjective::PreserveExact,
            DomainProtection::Unknown,
        )]);
        assert_eq!(
            summary.level(),
            ProtectionLevel::Unknown,
            "§55.6 case 29: unknown provider recovery semantics remain UNKNOWN"
        );
    }

    #[test]
    fn should_call_a_plan_transactional_only_inside_one_provider_boundary() {
        let one = ProtectionSummary::of(vec![
            row(
                EffectDomain::ApplicationPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Transactional,
            )
            .within_transaction("kuang.postgresql"),
            row(
                EffectDomain::ProviderTransactionState,
                RecoveryObjective::RestoreSemantic,
                DomainProtection::Transactional,
            )
            .within_transaction("kuang.postgresql"),
        ]);
        assert_eq!(
            one.level(),
            ProtectionLevel::Transactional,
            "§27.1: a provider may state atomicity over its own resource scope"
        );

        let two = ProtectionSummary::of(vec![
            row(
                EffectDomain::ApplicationPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Transactional,
            )
            .within_transaction("kuang.postgresql"),
            row(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Transactional,
            )
            .within_transaction("ono.recovery.zfs"),
        ]);
        assert_eq!(
            two.level(),
            ProtectionLevel::Protected,
            "§27.2: two boundaries are not one transaction, whatever each of them guarantees"
        );
    }

    #[test]
    fn should_call_a_runtime_only_plan_compensatable_when_its_inverse_exists() {
        let summary = ProtectionSummary::of(vec![row(
            EffectDomain::ProcessRuntime,
            RecoveryObjective::RestoreSemantic,
            DomainProtection::Compensatable,
        )]);
        assert_eq!(
            summary.level(),
            ProtectionLevel::Compensatable,
            "§27.4: stopping a service is compensatable by starting it, and is not rollback"
        );
    }

    #[test]
    fn should_report_every_exclusion_the_matrix_carries() {
        let summary = ProtectionSummary::of(vec![
            row(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
            )
            .excluding(CoverageExclusion::new(
                EffectDomain::FilesystemPersistent,
                "/home",
                "separate dataset",
            )),
        ])
        .excluding(
            CoverageExclusion::new(
                EffectDomain::ExternalSideEffect,
                "requests already served externally",
                "the request has left the machine",
            )
            .irreversible(),
        );
        assert_eq!(
            summary.exclusions().len(),
            2,
            "§10.3: the plan-level summary must never hide the matrix"
        );
        assert!(
            summary.has_irreversible(),
            "§2.13: an irreversible effect stays visible after protection"
        );
    }

    #[test]
    fn should_take_the_weaker_consistency_when_a_set_spans_two_mechanisms() {
        assert_eq!(
            ConsistencyClass::FilesystemConsistent.weakest_of(ConsistencyClass::CrashConsistent),
            ConsistencyClass::CrashConsistent,
            "Appendix D.7 forbids inventing cross-mechanism atomicity"
        );
        assert_eq!(
            ConsistencyClass::ApplicationConsistent.weakest_of(ConsistencyClass::Unknown),
            ConsistencyClass::Unknown,
            "§39.1: an unknown half makes the whole unknown"
        );
    }

    #[test]
    fn should_report_the_shortfall_a_require_policy_would_refuse_on() {
        let summary = ProtectionSummary::of(vec![
            row(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
            ),
            row(
                EffectDomain::ApplicationPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Unprotected,
            ),
        ]);
        let shortfall = summary.shortfall();
        assert_eq!(shortfall.len(), 1, "one domain is uncovered");
        assert_eq!(
            shortfall[0].domain(),
            EffectDomain::ApplicationPersistent,
            "§17.2 `require` refuses on exactly the rows this reports"
        );
    }
}
