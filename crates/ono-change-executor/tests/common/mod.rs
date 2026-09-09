//! The fixtures every executor test shares: a plan, a store, and providers that fail on demand.
//!
//! Everything faked here is outside the shell — a recovery provider's storage, an application's
//! quiesce hook, the link to another host. No internal layer is mocked: `prepare`, `apply`,
//! `verify` and `resume` are called for real and asserted on their outcomes.

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use ono_change_core::{
    ActionRole, ChangePlan, ConsistencyClass, EffectDomain, Execution, FrozenTarget, Intent,
    PersistenceDomain, PlanAction, PlanId, PlanState, Precondition, PreconditionKind,
    ProtectionAction, ProviderAvailability, ProviderCapabilities, RecoveryAsset, RecoveryCandidate,
    RecoveryCapability, RecoveryCost, RecoveryGoal, RecoveryObjective, RecoveryPlanFragment,
    RecoveryProvider, RecoveryScope, RecoveryValidation, RestoreMethod, RiskAssessment, RiskClass,
    RiskDimension, RiskFinding, Strategy, VerificationClass, VerificationContract, VerificationSet,
};
use ono_change_executor::execute::Quiesce;
use ono_change_plan::PlanStore;
use ono_change_protection::ProviderRegistry;
use ono_value::{ErrorValue, Value};
use tempfile::TempDir;

/// A deterministic instant. §39.2 keeps the clock a parameter, so the tests name their own.
#[must_use]
pub fn instant(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).expect("a valid instant")
}

/// An open plan store in a temporary directory the test owns.
#[must_use]
pub fn store() -> (TempDir, PlanStore) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let store =
        PlanStore::open(&directory.path().join("plans.sqlite3")).expect("a fresh store opens");
    (directory, store)
}

/// The provider id the fake recovery provider answers to.
pub const PROVIDER: &str = "ono.recovery.fake";

/// A structured execution that names an operation and nothing interpolated (§2.17).
#[must_use]
pub fn execution(operation: &str) -> Execution {
    Execution::ProviderAction {
        provider: Arc::from("test.provider"),
        operation: Arc::from(operation),
        arguments: vec![(Arc::from("subject"), Value::string(operation))],
    }
}

/// One mutating action on `target`.
#[must_use]
pub fn mutate(plan: &PlanId, ordinal: usize, target: &str) -> PlanAction {
    PlanAction::new(
        plan,
        ordinal,
        ActionRole::Mutate,
        format!("restart {target}"),
        execution(&format!("ono.service.restart.{target}")),
    )
    .on(target)
    .with_idempotency(ono_change_core::Idempotency::Idempotent)
}

/// A required verification contract over `subject`.
#[must_use]
pub fn required(plan: &PlanId, subject: &str) -> VerificationContract {
    VerificationContract::new(
        plan,
        VerificationClass::Required,
        subject,
        "state == running",
    )
}

/// How a test wants its plan shaped.
#[derive(Debug, Clone)]
pub struct PlanSpec {
    pub targets: Vec<String>,
    pub hosts: Vec<Option<String>>,
    pub strategy: Strategy,
    pub verification: Vec<(VerificationClass, String)>,
    pub risk: RiskAssessment,
    pub privileged: bool,
    pub chained: bool,
    pub idempotency: ono_change_core::Idempotency,
    pub recovery: bool,
    pub protection_mode: ono_change_core::ProtectionMode,
}

impl Default for PlanSpec {
    fn default() -> Self {
        Self {
            targets: vec!["nginx.service".to_owned()],
            hosts: vec![None],
            strategy: Strategy::Sequential,
            verification: vec![(VerificationClass::Required, "nginx.service".to_owned())],
            risk: RiskAssessment::empty(),
            privileged: false,
            chained: false,
            idempotency: ono_change_core::Idempotency::Idempotent,
            recovery: false,
            protection_mode: ono_change_core::ProtectionMode::Prefer,
        }
    }
}

impl PlanSpec {
    /// A plan over `count` targets named `svc-1` upwards.
    #[must_use]
    pub fn over(count: usize) -> Self {
        Self {
            targets: (1..=count).map(|index| format!("svc-{index}")).collect(),
            hosts: vec![None; count],
            verification: vec![(VerificationClass::Required, "svc-1".to_owned())],
            ..Self::default()
        }
    }

    /// The same plan, running the named strategy.
    #[must_use]
    pub fn with_strategy(mut self, strategy: Strategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// The same plan, with every action after the first depending on the one before it.
    #[must_use]
    pub fn chained(mut self) -> Self {
        self.chained = true;
        self
    }

    /// The same plan, with every action declaring `idempotency` (§41.1).
    #[must_use]
    pub fn declaring(mut self, idempotency: ono_change_core::Idempotency) -> Self {
        self.idempotency = idempotency;
        self
    }

    /// The same plan, with every action needing elevated privilege (§43.3).
    #[must_use]
    pub fn privileged(mut self) -> Self {
        self.privileged = true;
        self
    }

    /// The same plan, as a recovery plan (§3.8).
    #[must_use]
    pub fn recovering(mut self) -> Self {
        self.recovery = true;
        self
    }

    /// The same plan, carrying an unacknowledged risk finding (§19.4).
    #[must_use]
    pub fn risky(mut self, class: RiskClass, dimension: RiskDimension) -> Self {
        self.risk = RiskAssessment::empty().with(RiskFinding::new(
            dimension,
            class,
            "test.rule",
            "the whole serving group would restart at once",
        ));
        self
    }

    /// The same plan, with its targets on `host` (§7.1).
    #[must_use]
    pub fn on_hosts(mut self, hosts: &[&str]) -> Self {
        self.hosts = hosts.iter().map(|host| Some((*host).to_owned())).collect();
        self
    }

    /// The same plan, under a protection policy mode (§17.2).
    #[must_use]
    pub fn under(mut self, mode: ono_change_core::ProtectionMode) -> Self {
        self.protection_mode = mode;
        self
    }

    /// The same plan, with `class` verification over `subject`.
    #[must_use]
    pub fn checking(mut self, class: VerificationClass, subject: &str) -> Self {
        self.verification.push((class, subject.to_owned()));
        self
    }

    /// The same plan, with no verification at all beyond the one §23.1 requires.
    #[must_use]
    pub fn only_required(mut self, subject: &str) -> Self {
        self.verification = vec![(VerificationClass::Required, subject.to_owned())];
        self
    }

    /// Builds and seals the plan (§4.4).
    #[must_use]
    pub fn seal(&self, now: Timestamp) -> ChangePlan {
        let mut plan = ChangePlan::draft(
            Intent::new("restart the serving group", "plan restart service"),
            "session-under-test",
            now,
        );
        if self.recovery {
            plan = plan.as_recovery();
        }
        let targets: Vec<FrozenTarget> = self
            .targets
            .iter()
            .enumerate()
            .map(|(index, identity)| {
                let target =
                    FrozenTarget::new("ono.service/1", identity.as_str(), identity.as_str());
                match self.hosts.get(index).and_then(Option::as_ref) {
                    Some(host) => target.on_host(host.as_str()),
                    None => target,
                }
            })
            .collect();
        let plan = plan.resolve(targets).expect("a draft resolves");
        let id = plan.id().clone();
        let mut actions: Vec<PlanAction> = Vec::new();
        for (ordinal, identity) in self.targets.iter().enumerate() {
            let mut action = PlanAction::new(
                &id,
                ordinal + 1,
                if self.recovery {
                    ActionRole::Recover
                } else {
                    ActionRole::Mutate
                },
                format!("restart {identity}"),
                execution(&format!("ono.service.restart.{identity}")),
            )
            .on(identity.as_str())
            .with_idempotency(self.idempotency)
            .requiring(
                Precondition::new(
                    PreconditionKind::Generation,
                    identity.as_str(),
                    "generation",
                    Value::Int(1),
                )
                .explained("the unit's generation was frozen at resolution"),
            );
            if self.privileged {
                action = action.privileged();
            }
            if self.chained
                && let Some(previous) = actions.last()
            {
                action = action.after(previous.id().clone());
            }
            actions.push(action);
        }
        let mut plan = plan;
        for action in actions {
            plan = plan.with_action(action).expect("a draft accepts an action");
        }
        let mut verification = VerificationSet::empty();
        for (class, subject) in &self.verification {
            verification = verification.with(VerificationContract::new(
                &id,
                *class,
                subject.as_str(),
                "state == running",
            ));
        }
        plan.with_verification(verification)
            .with_strategy(self.strategy)
            .with_risk(self.risk.clone())
            .with_protection_mode(self.protection_mode)
            .seal(now)
            .expect("a plan with a required contract seals")
    }
}

/// A sealed one-target plan, which is the shape most Appendix F rows need.
#[must_use]
pub fn sealed_plan(now: Timestamp) -> ChangePlan {
    PlanSpec::default().seal(now)
}

/// A sealed plan that is already stored, so §41.2's action records have somewhere to go.
#[must_use]
pub fn stored(spec: &PlanSpec, store: &PlanStore, now: Timestamp) -> ChangePlan {
    let plan = spec.seal(now);
    store.put(&plan).expect("a sealed plan is stored");
    plan
}

/// What the fake recovery provider does when it is asked (§54.5's injection points).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderScript {
    /// Everything works.
    #[default]
    Healthy,
    /// `create` refuses — §54.5's "snapshot creation fails".
    CreateFails,
    /// `create` refuses because the filesystem is full — §54.5's "storage fills".
    StorageFills,
    /// `validate` reports a scope that is not the one the plan expected — §54.5's "snapshot
    /// validates wrong scope".
    ValidatesWrongScope,
    /// `validate` cannot be carried out at all, which §11.4 separates from a check that failed.
    ValidationUnavailable,
    /// `cleanup` refuses — §54.5's "cleanup fails".
    CleanupFails,
    /// `restore` refuses — §54.5's "recovery fails halfway".
    RestoreFails,
}

/// A recovery provider whose every answer is scripted (§54.5).
#[derive(Debug)]
pub struct FakeRecoveryProvider {
    id: Arc<str>,
    script: ProviderScript,
    capabilities: Vec<RecoveryCapability>,
    availability: ProviderAvailability,
    created: Mutex<Vec<String>>,
    removed: Mutex<Vec<String>>,
}

impl FakeRecoveryProvider {
    /// A healthy provider declaring everything §12.2 requires.
    #[must_use]
    pub fn healthy() -> Self {
        Self::with_script(ProviderScript::Healthy)
    }

    /// A provider running `script`.
    #[must_use]
    pub fn with_script(script: ProviderScript) -> Self {
        Self {
            id: Arc::from(PROVIDER),
            script,
            capabilities: RecoveryCapability::REQUIRED.to_vec(),
            availability: ProviderAvailability::Available {
                version: Arc::from("1.0"),
            },
            created: Mutex::new(Vec::new()),
            removed: Mutex::new(Vec::new()),
        }
    }

    /// The same provider under another id, for a plan naming two of them.
    #[must_use]
    pub fn named(mut self, id: &str) -> Self {
        self.id = Arc::from(id);
        self
    }

    /// The same provider without `capability`, for Appendix F.1's "provider-declared" condition.
    #[must_use]
    pub fn without(mut self, capability: RecoveryCapability) -> Self {
        self.capabilities.retain(|held| *held != capability);
        self
    }

    /// The assets it was asked to remove.
    #[must_use]
    pub fn removed(&self) -> Vec<String> {
        self.removed
            .lock()
            .map(|log| log.clone())
            .unwrap_or_default()
    }

    /// The assets it created.
    #[must_use]
    pub fn created(&self) -> Vec<String> {
        self.created
            .lock()
            .map(|log| log.clone())
            .unwrap_or_default()
    }
}

impl RecoveryProvider for FakeRecoveryProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        let mut capabilities = ProviderCapabilities::new(self.id.as_ref());
        for capability in &self.capabilities {
            capabilities = capabilities.recovering(*capability);
        }
        capabilities
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
        _mode: ono_change_core::ProtectionMode,
    ) -> Result<Vec<ProtectionAction>, ErrorValue> {
        Ok(Vec::new())
    }

    fn create(&self, action: &ProtectionAction) -> Result<RecoveryAsset, ErrorValue> {
        match self.script {
            ProviderScript::CreateFails => Err(ono_change_core::error::asset_create_failed(
                &self.id,
                action.candidate().scope().domain(),
                "the snapshot could not be taken",
            )),
            ProviderScript::StorageFills => Err(ono_change_core::error::storage_pressure(
                action.candidate().scope().domain(),
                "2 MiB",
                "10 GiB",
            )),
            _ => {
                if let Ok(mut log) = self.created.lock() {
                    log.push(action.proposed_asset().reference().to_owned());
                }
                Ok(action.proposed_asset().clone().creating())
            }
        }
    }

    fn validate(&self, asset: &RecoveryAsset) -> Result<RecoveryValidation, ErrorValue> {
        match self.script {
            ProviderScript::ValidatesWrongScope => Ok(RecoveryValidation::complete(
                asset.created_at(),
                "the snapshot covers a different dataset from the one the plan named",
            )
            .scope(false)),
            ProviderScript::ValidationUnavailable => Err(ono_change_core::error::tool_failed(
                "/usr/sbin/fake",
                "the validation tool could not be run",
            )),
            _ => Ok(RecoveryValidation::complete(
                asset.created_at(),
                "existence, identity, scope, restore availability and permissions all checked",
            )),
        }
    }

    fn plan_recovery(
        &self,
        _asset: &RecoveryAsset,
        _source: Option<&ChangePlan>,
        _goal: RecoveryGoal,
    ) -> Result<RecoveryPlanFragment, ErrorValue> {
        Ok(RecoveryPlanFragment::new(
            self.id.as_ref(),
            RestoreMethod::SelectiveFileRestore,
        ))
    }

    fn restore(&self, action: &PlanAction, _asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        match self.script {
            ProviderScript::RestoreFails => Err(ono_change_core::error::recovery_apply_failed(
                action.summary(),
                "the restore stopped halfway",
            )),
            _ => Ok(()),
        }
    }

    fn cleanup(&self, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        match self.script {
            ProviderScript::CleanupFails => Err(ono_change_core::error::cleanup_blocked(
                asset.id(),
                &["plan/other".to_owned()],
            )),
            _ => {
                if let Ok(mut log) = self.removed.lock() {
                    log.push(asset.id().as_str().to_owned());
                }
                Ok(())
            }
        }
    }

    fn estimate_cost(&self, _asset: &RecoveryAsset) -> Result<RecoveryCost, ErrorValue> {
        Ok(RecoveryCost::unknown())
    }
}

/// A registry holding one scripted provider.
#[must_use]
pub fn registry(provider: FakeRecoveryProvider) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry
        .register(Arc::new(provider))
        .expect("the fake provider declares §12.2's five capabilities");
    registry
}

/// An empty registry, for a plan whose provider is not here.
#[must_use]
pub fn empty_registry() -> ProviderRegistry {
    ProviderRegistry::new()
}

/// One protection action over `domain`, proposing an asset owned by `plan`.
#[must_use]
pub fn protection(
    plan: &ChangePlan,
    provider: &str,
    domain: &str,
    now: Timestamp,
) -> ProtectionAction {
    protection_owned_by(plan.id(), provider, domain, now)
}

/// One protection action whose proposed asset belongs to `owner`.
///
/// Appendix F.1's first condition is that an asset was "created solely for this failed prepare",
/// and an asset an earlier `protect` made for another plan (§18.2) is the case that fails it.
#[must_use]
pub fn protection_owned_by(
    owner: &PlanId,
    provider: &str,
    domain: &str,
    now: Timestamp,
) -> ProtectionAction {
    let scope = RecoveryScope::new("zfs-dataset", domain, "localhost").covering(domain);
    let candidate = RecoveryCandidate::new(
        provider,
        scope.clone(),
        EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        format!("snapshot {domain} before the change"),
    )
    .at_consistency(ConsistencyClass::FilesystemConsistent)
    .restored_by(RestoreMethod::SelectiveFileRestore);
    let asset = RecoveryAsset::proposed(
        provider,
        ono_change_core::RecoveryAssetType::ZfsSnapshot,
        format!("{domain}@ono-test"),
        scope,
        now,
    )
    .for_plan(owner.clone())
    .at_consistency(ConsistencyClass::FilesystemConsistent);
    ProtectionAction::new(provider, format!("snapshot {domain}"), candidate, asset)
}

/// Five protection actions, which is Appendix F.1's own example.
#[must_use]
pub fn five_snapshots(plan: &ChangePlan, provider: &str, now: Timestamp) -> Vec<ProtectionAction> {
    (1..=5)
        .map(|index| protection(plan, provider, &format!("tank/data{index}"), now))
        .collect()
}

/// A quiesce hook whose pause and resume are scripted (§18.4, §54.5).
#[derive(Debug, Default)]
pub struct FakeQuiesce {
    pause_fails: bool,
    resume_fails: bool,
    log: Mutex<Vec<&'static str>>,
}

impl FakeQuiesce {
    /// A hook that pauses and resumes cleanly.
    #[must_use]
    pub fn healthy() -> Self {
        Self::default()
    }

    /// A hook that cannot pause the application (§18.4).
    #[must_use]
    pub fn failing_pause() -> Self {
        Self {
            pause_fails: true,
            ..Self::default()
        }
    }

    /// A hook that pauses and then cannot put the application back (§18.4's critical error).
    #[must_use]
    pub fn failing_resume() -> Self {
        Self {
            resume_fails: true,
            ..Self::default()
        }
    }

    /// What it was asked to do, in order.
    #[must_use]
    pub fn log(&self) -> Vec<&'static str> {
        self.log.lock().map(|log| log.clone()).unwrap_or_default()
    }
}

impl Quiesce for FakeQuiesce {
    fn pause(&self, application: &str) -> Result<(), ErrorValue> {
        if let Ok(mut log) = self.log.lock() {
            log.push("pause");
        }
        if self.pause_fails {
            return Err(ono_change_core::error::quiesce_failed(
                application,
                "the application did not acknowledge the freeze request",
            ));
        }
        Ok(())
    }

    fn resume(&self, application: &str) -> Result<(), ErrorValue> {
        if let Ok(mut log) = self.log.lock() {
            log.push("resume");
        }
        if self.resume_fails {
            return Err(ono_change_core::error::resume_failed(
                application,
                "the thaw request was refused",
            ));
        }
        Ok(())
    }
}

/// A revalidation that finds nothing moved (§7.3).
pub fn no_drift() -> impl Fn(&PlanAction) -> Result<Vec<ono_change_core::DriftFinding>, ErrorValue>
{
    |_action: &PlanAction| Ok(Vec::new())
}

/// A revalidation that finds `subject`'s frozen fact moved materially (§7.3).
pub fn material_drift()
-> impl Fn(&PlanAction) -> Result<Vec<ono_change_core::DriftFinding>, ErrorValue> {
    |action: &PlanAction| {
        Ok(action
            .preconditions()
            .iter()
            .map(|precondition| {
                ono_change_core::DriftFinding::new(
                    precondition,
                    ono_change_core::DriftVerdict::Material,
                    Some(Value::Int(9)),
                )
            })
            .collect())
    }
}

/// A revalidation that finds a tolerated change, which §7.4 lets through.
pub fn tolerated_drift()
-> impl Fn(&PlanAction) -> Result<Vec<ono_change_core::DriftFinding>, ErrorValue> {
    |action: &PlanAction| {
        Ok(action
            .preconditions()
            .iter()
            .map(|precondition| {
                ono_change_core::DriftFinding::new(
                    precondition,
                    ono_change_core::DriftVerdict::Tolerated,
                    Some(Value::Int(9)),
                )
            })
            .collect())
    }
}

/// What the scripted execution function should do for a given action.
#[derive(Debug, Clone, Default)]
pub struct Script {
    fail_on: Vec<String>,
    unknown_on: Vec<String>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Script {
    /// Every action succeeds.
    #[must_use]
    pub fn healthy() -> Self {
        Self::default()
    }

    /// The action whose target is `target` fails.
    #[must_use]
    pub fn failing(mut self, target: &str) -> Self {
        self.fail_on.push(target.to_owned());
        self
    }

    /// The action whose target is `target` leaves its outcome unestablished (Appendix F.2).
    #[must_use]
    pub fn unknown(mut self, target: &str) -> Self {
        self.unknown_on.push(target.to_owned());
        self
    }

    /// The targets the executor actually asked for, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().map(|log| log.clone()).unwrap_or_default()
    }

    /// The execution function itself.
    pub fn execute(
        &self,
    ) -> impl Fn(&PlanAction) -> ono_change_executor::ExecutionOutcome + use<'_> {
        move |action: &PlanAction| {
            let target = action.target().unwrap_or(action.summary()).to_owned();
            if let Ok(mut log) = self.calls.lock() {
                log.push(target.clone());
            }
            if self.fail_on.contains(&target) {
                return ono_change_executor::ExecutionOutcome::Failed(
                    ono_change_core::error::tool_failed(
                        "/usr/bin/systemctl",
                        "the unit refused to restart",
                    ),
                );
            }
            if self.unknown_on.contains(&target) {
                return ono_change_executor::ExecutionOutcome::Unknown(
                    ono_change_core::error::remote_state_unknown(
                        action.target().unwrap_or("this host"),
                        action.summary(),
                    ),
                );
            }
            ono_change_executor::ExecutionOutcome::Succeeded
        }
    }
}

/// An observation function answering `status` for every contract.
pub fn observing(
    status: ono_change_core::VerificationStatus,
) -> impl Fn(&VerificationContract) -> ono_change_executor::Observation {
    move |_contract: &VerificationContract| ono_change_executor::Observation::Answered {
        status,
        observed: None,
    }
}

/// An observation function that answers `status` for `subject` and passes everything else.
pub fn observing_only(
    subject: &str,
    status: ono_change_core::VerificationStatus,
) -> impl Fn(&VerificationContract) -> ono_change_executor::Observation + use<'_> {
    move |contract: &VerificationContract| {
        if contract.subject() == subject {
            ono_change_executor::Observation::Answered {
                status,
                observed: None,
            }
        } else {
            ono_change_executor::Observation::passed()
        }
    }
}

/// An observation function whose every check exceeds its timeout (§23.5).
pub fn timing_out() -> impl Fn(&VerificationContract) -> ono_change_executor::Observation {
    |_contract: &VerificationContract| ono_change_executor::Observation::TimedOut
}

/// Whether the plan reached a state §4.1 says has touched the system.
#[must_use]
pub fn mutated(state: PlanState) -> bool {
    state.has_mutated()
}
