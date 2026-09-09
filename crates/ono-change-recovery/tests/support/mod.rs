//! What the suites need from outside: recovery providers under test control, and the plans and
//! assets a recovery is planned against.
//!
//! AGENTS.md section 16 permits faking the outside world and forbids mocking an internal layer.
//! A [`TestProvider`] is the outside world — it stands in for ZFS, Btrfs, a file archive or a
//! package manager, so Appendix C.4's worked example and Appendix I.5's acceptance scenario can be
//! written down instead of staged on a live filesystem. It also counts the calls that would change
//! something, which is how `should_not_touch_the_target_when_planning_recovery` can assert §24.1.

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a shared test fixture is used by some suites and not by others (AGENTS.md section 16)"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use jiff::Timestamp;
use ono_change_core::{
    ActionRole, ChangePlan, ConsistencyClass, DirectoryRestorePolicy, DomainCoverage,
    DomainProtection, EffectConfidence, EffectDomain, EffectKind, EquivalenceDomain, Execution,
    Idempotency, Intent, MetadataCoverage, PersistenceDomain, PlanAction, PlanId, ProposedEffect,
    ProtectionAction, ProtectionMode, ProtectionSummary, ProviderAvailability,
    ProviderCapabilities, RecoveryAsset, RecoveryAssetType, RecoveryCandidate, RecoveryCapability,
    RecoveryCost, RecoveryGoal, RecoveryObjective, RecoveryPlanFragment, RecoveryProvider,
    RecoveryScope, RecoveryValidation, RestoreMethod, VerificationClass, VerificationContract,
    VerificationSet,
};
use ono_change_protection::ProviderRegistry;
use ono_value::ErrorValue;

/// Every fixture is dated from this instant, so nothing depends on a wall clock.
pub const EPOCH: Timestamp = Timestamp::UNIX_EPOCH;

/// `hh:mm` on the fixture's day, as a timestamp a test can name in prose.
#[must_use]
pub fn at(hour: i64, minute: i64) -> Timestamp {
    Timestamp::from_second(hour * 3600 + minute * 60).expect("a valid instant")
}

/// How often a provider was asked to change something (§24.1, §55.8 case 35).
#[derive(Debug, Default)]
pub struct Calls {
    pub created: AtomicUsize,
    pub restored: AtomicUsize,
    pub cleaned: AtomicUsize,
    pub planned: AtomicUsize,
}

impl Calls {
    /// Every call that would have changed the target system.
    #[must_use]
    pub fn mutating(&self) -> usize {
        self.created.load(Ordering::SeqCst)
            + self.restored.load(Ordering::SeqCst)
            + self.cleaned.load(Ordering::SeqCst)
    }

    /// How often the planning half was asked (§12.1).
    #[must_use]
    pub fn planning(&self) -> usize {
        self.planned.load(Ordering::SeqCst)
    }
}

/// A recovery provider whose whole answer is declared by the test that builds it.
#[derive(Debug)]
pub struct TestProvider {
    id: Arc<str>,
    capabilities: ProviderCapabilities,
    availability: ProviderAvailability,
    method: RestoreMethod,
    metadata: MetadataCoverage,
    directory_policy: DirectoryRestorePolicy,
    actions: Vec<(Arc<str>, Arc<str>)>,
    contracts: Vec<(Arc<str>, Arc<str>, VerificationClass, EquivalenceDomain)>,
    requires_reboot: bool,
    requires_offline: bool,
    failure: Option<ErrorValue>,
    calls: Arc<Calls>,
}

impl TestProvider {
    /// A fully capable, available provider that restores one object by selective file restore.
    #[must_use]
    pub fn new(id: &str) -> Self {
        let mut capabilities = ProviderCapabilities::new(id);
        for capability in RecoveryCapability::REQUIRED {
            capabilities = capabilities.recovering(*capability);
        }
        Self {
            id: Arc::from(id),
            capabilities,
            availability: ProviderAvailability::Available {
                version: Arc::from("2.2.2"),
            },
            method: RestoreMethod::SelectiveFileRestore,
            metadata: MetadataCoverage::content_only(),
            directory_policy: DirectoryRestorePolicy::KeepExtraFiles,
            actions: vec![(
                Arc::from("restore the captured object"),
                Arc::from("recovery.restore"),
            )],
            contracts: Vec::new(),
            requires_reboot: false,
            requires_offline: false,
            failure: None,
            calls: Arc::new(Calls::default()),
        }
    }

    /// The method its fragment offers (Appendix C.1).
    #[must_use]
    pub const fn restoring_by(mut self, method: RestoreMethod) -> Self {
        self.method = method;
        self
    }

    /// The metadata its restore actually puts back (Appendix C.7).
    #[must_use]
    pub const fn restoring_metadata(mut self, metadata: MetadataCoverage) -> Self {
        self.metadata = metadata;
        self
    }

    /// What its directory restore does with files the asset never held (Appendix C.6).
    #[must_use]
    pub const fn restoring_directories(mut self, policy: DirectoryRestorePolicy) -> Self {
        self.directory_policy = policy;
        self
    }

    /// Replaces the actions its fragment contributes.
    #[must_use]
    pub fn acting(mut self, summary: &str, capability: &str) -> Self {
        self.actions = vec![(Arc::from(summary), Arc::from(capability))];
        self
    }

    /// A provider whose fragment carries no action at all (§56.3).
    #[must_use]
    pub fn acting_on_nothing(mut self) -> Self {
        self.actions = Vec::new();
        self
    }

    /// Adds a verification contract its fragment contributes (§25.1).
    #[must_use]
    pub fn verifying(
        mut self,
        subject: &str,
        expression: &str,
        class: VerificationClass,
        domain: EquivalenceDomain,
    ) -> Self {
        self.contracts
            .push((Arc::from(subject), Arc::from(expression), class, domain));
        self
    }

    /// A provider that cannot run here (§12.2, Appendix G.4).
    #[must_use]
    pub fn unavailable(mut self, reason: &str) -> Self {
        self.availability = ProviderAvailability::Unavailable {
            reason: Arc::from(reason),
        };
        self
    }

    /// A provider whose recovery planning itself fails (§56.3).
    #[must_use]
    pub fn failing(mut self, error: ErrorValue) -> Self {
        self.failure = Some(error);
        self
    }

    /// A restore that needs a reboot (§13.7).
    #[must_use]
    pub const fn needing_reboot(mut self) -> Self {
        self.requires_reboot = true;
        self
    }

    /// A restore that needs the filesystem offline (§14.6).
    #[must_use]
    pub const fn needing_offline(mut self) -> Self {
        self.requires_offline = true;
        self
    }

    /// The counter a suite reads to prove nothing was changed (§24.1).
    #[must_use]
    pub fn calls(&self) -> Arc<Calls> {
        Arc::clone(&self.calls)
    }

    /// The provider behind an `Arc`, as the registry holds it.
    #[must_use]
    pub fn shared(self) -> Arc<dyn RecoveryProvider> {
        Arc::new(self)
    }
}

impl RecoveryProvider for TestProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    fn availability(&self) -> ProviderAvailability {
        self.availability.clone()
    }

    fn resolve_domain(&self, _path: &str) -> Result<Option<PersistenceDomain>, ErrorValue> {
        Ok(None)
    }

    fn discover(
        &self,
        _domain: &PersistenceDomain,
        _objective: RecoveryObjective,
    ) -> Result<Vec<RecoveryCandidate>, ErrorValue> {
        Ok(Vec::new())
    }

    fn plan_protection(
        &self,
        _candidates: &[RecoveryCandidate],
        _mode: ProtectionMode,
    ) -> Result<Vec<ProtectionAction>, ErrorValue> {
        Ok(Vec::new())
    }

    fn create(&self, _action: &ProtectionAction) -> Result<RecoveryAsset, ErrorValue> {
        self.calls.created.fetch_add(1, Ordering::SeqCst);
        Err(ono_change_core::error::asset_create_failed(
            &self.id,
            "the fixture",
            "the fixture never creates anything",
        ))
    }

    fn validate(&self, _asset: &RecoveryAsset) -> Result<RecoveryValidation, ErrorValue> {
        Ok(RecoveryValidation::complete(EPOCH, "the fixture checked it"))
    }

    fn plan_recovery(
        &self,
        asset: &RecoveryAsset,
        source: Option<&ChangePlan>,
        _goal: RecoveryGoal,
    ) -> Result<RecoveryPlanFragment, ErrorValue> {
        self.calls.planned.fetch_add(1, Ordering::SeqCst);
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let plan = source
            .map_or_else(|| PlanId::of("fixture", "0", asset.reference()), |plan| plan.id().clone());
        let mut fragment = RecoveryPlanFragment::new(&*self.id, self.method)
            .restoring_metadata(self.metadata)
            .restoring_directories(self.directory_policy);
        for (ordinal, (summary, capability)) in self.actions.iter().enumerate() {
            fragment = fragment.acting(
                PlanAction::new(
                    &plan,
                    ordinal + 1,
                    ActionRole::Recover,
                    Arc::clone(summary),
                    Execution::RecoveryOperation {
                        provider: Arc::clone(&self.id),
                        capability: Arc::clone(capability),
                        arguments: Vec::new(),
                    },
                )
                .with_idempotency(Idempotency::Idempotent)
                .on(asset.reference()),
            );
        }
        for (subject, expression, class, domain) in &self.contracts {
            fragment = fragment.verifying(
                VerificationContract::new(&plan, *class, Arc::clone(subject), Arc::clone(expression))
                    .about(*domain),
            );
        }
        if self.requires_reboot {
            fragment = fragment.needing_reboot();
        }
        if self.requires_offline {
            fragment = fragment.needing_offline();
        }
        Ok(fragment)
    }

    fn restore(&self, _action: &PlanAction, _asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        self.calls.restored.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn cleanup(&self, _asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        self.calls.cleaned.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn estimate_cost(&self, asset: &RecoveryAsset) -> Result<RecoveryCost, ErrorValue> {
        Ok(asset.cost().clone())
    }
}

/// A registry holding exactly these providers.
#[must_use]
pub fn registry(providers: Vec<Arc<dyn RecoveryProvider>>) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    for provider in providers {
        registry
            .register(provider)
            .expect("the fixture declares every §12.2 capability");
    }
    registry
}

/// A validated, ready asset over `covers`, created at `created_at` (§11.4).
#[must_use]
pub fn ready_asset(
    provider: &str,
    reference: &str,
    domain: &str,
    covers: &[&str],
    created_at: Timestamp,
) -> RecoveryAsset {
    let mut scope = RecoveryScope::new("zfs-dataset", domain, "localhost");
    for object in covers {
        scope = scope.covering(*object);
    }
    RecoveryAsset::proposed(
        provider,
        RecoveryAssetType::ZfsSnapshot,
        reference,
        scope,
        created_at,
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore)
    .creating()
    .validated(RecoveryValidation::complete(
        created_at,
        "the fixture checked it",
    ))
}

/// The nginx configuration plan of §24.4: replace the file, restart the service.
#[must_use]
pub fn nginx_plan() -> ChangePlan {
    let plan = ChangePlan::draft(
        Intent::new(
            "replace nginx configuration and restart service",
            "plan { replace file /etc/nginx/nginx.conf from ./nginx.conf }",
        ),
        "session-1",
        EPOCH,
    );
    let write = PlanAction::new(
        plan.id(),
        1,
        ActionRole::Mutate,
        "replace /etc/nginx/nginx.conf",
        Execution::ProviderAction {
            provider: Arc::from("linux.files"),
            operation: Arc::from("ono.file.write"),
            arguments: Vec::new(),
        },
    )
    .effecting(
        ProposedEffect::new(
            ono_change_core::ActionId::of(plan.id(), 1, "replace /etc/nginx/nginx.conf"),
            EffectDomain::FilesystemPersistent,
            EffectKind::Modify,
            EffectConfidence::Guaranteed,
            "the file is replaced",
        )
        .on("/etc/nginx/nginx.conf"),
    );
    let restart = PlanAction::new(
        plan.id(),
        2,
        ActionRole::Mutate,
        "restart nginx.service",
        Execution::ProviderAction {
            provider: Arc::from("linux.systemd"),
            operation: Arc::from("ono.service.restart"),
            arguments: Vec::new(),
        },
    )
    .effecting(
        ProposedEffect::new(
            ono_change_core::ActionId::of(plan.id(), 2, "restart nginx.service"),
            EffectDomain::ProcessRuntime,
            EffectKind::Replace,
            EffectConfidence::Guaranteed,
            "the worker processes are replaced",
        )
        .on("nginx.service workers"),
    );
    let verification = VerificationSet::empty().with(VerificationContract::new(
        plan.id(),
        VerificationClass::Required,
        "nginx.service",
        "state == running",
    ));
    plan.with_action(write)
        .expect("a draft accepts an action")
        .with_action(restart)
        .expect("a draft accepts an action")
        .with_verification(verification)
        .with_protection(protected_filesystem())
        .seal(EPOCH)
        .expect("a plan with a required contract seals")
}

/// A coverage matrix in which the plan's persistent domain is PROTECTED (§10.3).
#[must_use]
pub fn protected_filesystem() -> ProtectionSummary {
    ProtectionSummary::of(vec![DomainCoverage::new(
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        DomainProtection::Protected,
        "a validated ZFS snapshot covers it",
    )])
}

/// A coverage matrix in which the plan's persistent domain is not covered (§10.2).
#[must_use]
pub fn unprotected_filesystem() -> ProtectionSummary {
    ProtectionSummary::of(vec![DomainCoverage::new(
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        DomainProtection::Unprotected,
        "no provider offered a recovery asset for it",
    )])
}

/// Every metadata piece a restore could put back (Appendix C.7).
#[must_use]
pub const fn full_metadata() -> MetadataCoverage {
    MetadataCoverage {
        content: true,
        mode: true,
        owner: true,
        acl: true,
        xattrs: true,
        capabilities: true,
        selinux: true,
        hardlinks: true,
    }
}

/// Content, mode and owner, and nothing else (Appendix C.7).
#[must_use]
pub const fn content_mode_owner() -> MetadataCoverage {
    MetadataCoverage {
        content: true,
        mode: true,
        owner: true,
        ..MetadataCoverage::none()
    }
}
