//! Recovery assets: what exists, what it protects, and what it does not (spec v0.6 §11).
//!
//! §11.2 requires the scope to be concrete — *"A path MUST be mapped to its containing
//! persistence domain before protection is claimed"* — so [`RecoveryScope`] holds a resolved
//! domain and never a path on its own. §11.5 forbids calling a copy-on-write snapshot a backup,
//! and [`RecoveryAssetType::shares_failure_domain`] is where that distinction is a fact the
//! renderer reads rather than a sentence somebody remembered to write.
//!
//! §11.4 says creating an asset is not enough. An asset arrives [`AssetState::Creating`], and only
//! a [`RecoveryValidation`] that actually checked existence, identity, scope and restore
//! availability moves it to [`AssetState::Ready`]. Nothing else can: [`RecoveryAsset::validated`]
//! is the only path to that state, and it takes the validation as evidence.

use std::sync::Arc;

use jiff::Timestamp;
use ono_value::ByteSize;

use crate::id::{PlanId, RecoveryAssetId};
use crate::protection::ConsistencyClass;
use crate::vocab::vocabulary;

vocabulary! {
    /// The mechanism behind an asset (§3.6).
    RecoveryAssetType {
        ZfsSnapshot => "zfs-snapshot", "§13.2: a point-in-time read-only ZFS dataset state.";
        BtrfsSnapshot => "btrfs-snapshot", "§14.2: a Btrfs subvolume sharing extents with its source.";
        LvmSnapshot => "lvm-snapshot", "§16.1: an LVM snapshot, which can become invalid if its space is exhausted (Appendix D.10).";
        FileArchive => "file-archive", "§15: a copy of files and their metadata in the Ono recovery store.";
        ConfigurationBackup => "configuration-backup", "§15: a configuration object captured before replacement.";
        VmSnapshot => "vm-snapshot", "§16.2: a virtual machine snapshot, whose memory inclusion must be stated.";
        ContainerCheckpoint => "container-checkpoint", "§16.3: a container checkpoint, which is not guaranteed process rollback.";
        DatabaseCheckpoint => "database-checkpoint", "§16.4: an application-owned checkpoint contributed by a KUANG/11 provider.";
        TransactionSavepoint => "transaction-savepoint", "§27.1: a savepoint inside a provider's own transaction.";
        TimedReversion => "timed-reversion", "§34.3: a leased automatic reversion, which is an asset only once it is concrete and validated.";
        PackageRecoveryMetadata => "package-recovery-metadata", "§30.1: package-manager metadata sufficient to restore a prior version.";
    }
}

impl RecoveryAssetType {
    /// Whether the asset lives on the same storage that would fail with the thing it protects.
    ///
    /// §11.5 and §14.7 both require this to be said out loud: a local snapshot is a recovery
    /// point, not a backup, and a renderer that cannot tell the difference will eventually print
    /// one word for the other.
    #[must_use]
    pub const fn shares_failure_domain(self) -> bool {
        matches!(
            self,
            RecoveryAssetType::ZfsSnapshot
                | RecoveryAssetType::BtrfsSnapshot
                | RecoveryAssetType::LvmSnapshot
                | RecoveryAssetType::TransactionSavepoint
        )
    }

    /// Whether the asset captures a copy of the data rather than a reference to shared extents.
    #[must_use]
    pub const fn is_independent_copy(self) -> bool {
        matches!(
            self,
            RecoveryAssetType::FileArchive | RecoveryAssetType::ConfigurationBackup
        )
    }
}

vocabulary! {
    /// The lifecycle of one asset (§11.1).
    AssetState {
        Proposed => "proposed", "§11.1: the plan says this asset will be created. Nothing exists yet, and §2.1 keeps it that way until apply.";
        Creating => "creating", "§11.1: creation is in flight.";
        Ready => "ready", "§11.1: the asset exists and a validation confirmed it (§11.4).";
        Invalid => "invalid", "§11.1 and Appendix D.10: the asset exists and can no longer satisfy protection.";
        Expired => "expired", "§11.1 and §37.1: retention passed and the asset is no longer available.";
        Removed => "removed", "§11.1: the asset was deleted.";
        Failed => "failed", "§11.1: creation did not succeed.";
    }
}

impl AssetState {
    /// Whether an asset in this state can be used for recovery right now (§11.4).
    #[must_use]
    pub const fn is_usable(self) -> bool {
        matches!(self, AssetState::Ready)
    }

    /// Whether an asset in this state still occupies storage that cleanup could reclaim (§37).
    #[must_use]
    pub const fn occupies_storage(self) -> bool {
        matches!(
            self,
            AssetState::Creating | AssetState::Ready | AssetState::Invalid | AssetState::Expired
        )
    }
}

vocabulary! {
    /// How an asset would actually be used to restore state (§13.5, §14.4, Appendix C.1).
    ///
    /// Appendix C.1 orders these least-destructive first, and [`RestoreMethod::destructiveness`]
    /// is that order made comparable so recovery planning can prefer the top of the list.
    RestoreMethod {
        ProviderNativeRestore => "provider-native-restore", "Appendix C.1: the owning provider restores its own object — a package downgrade, a database point-in-time restore.";
        SelectiveFileRestore => "selective-file-restore", "Appendix C.1 and §13.5: read the wanted objects out of the asset and put them back, leaving everything else alone.";
        CloneAndCopy => "clone-and-copy", "Appendix C.1 and §13.5: materialise the asset somewhere else, then copy the wanted state out of it.";
        SubvolumeReplacement => "subvolume-replacement", "Appendix C.1 and §14.4: replace or rename a subvolume. Offline or next-boot depending on layout.";
        DatasetRollback => "dataset-rollback", "Appendix C.1 and §13.6: roll the whole dataset back, discarding everything written since.";
        OfflineRootRecovery => "offline-root-recovery", "Appendix C.1, §13.7 and §14.6: recovery that needs the filesystem unmounted, a boot-environment switch or a reboot.";
        Compensation => "compensation", "§27.4: an inverse action. Not rollback, and never described as one.";
    }
}

impl RestoreMethod {
    /// How much unrelated state the method risks, least first (Appendix C.1).
    #[must_use]
    pub const fn destructiveness(self) -> u8 {
        match self {
            RestoreMethod::ProviderNativeRestore => 0,
            RestoreMethod::SelectiveFileRestore => 1,
            RestoreMethod::CloneAndCopy => 2,
            RestoreMethod::Compensation => 3,
            RestoreMethod::SubvolumeReplacement => 4,
            RestoreMethod::DatasetRollback => 5,
            RestoreMethod::OfflineRootRecovery => 6,
        }
    }

    /// Whether the method discards state written after the asset was captured (§24.2).
    #[must_use]
    pub const fn discards_newer_state(self) -> bool {
        matches!(
            self,
            RestoreMethod::DatasetRollback
                | RestoreMethod::SubvolumeReplacement
                | RestoreMethod::OfflineRootRecovery
        )
    }

    /// Whether the method restores prior state rather than approximating it (§27.4).
    #[must_use]
    pub const fn restores_prior_state(self) -> bool {
        !matches!(self, RestoreMethod::Compensation)
    }
}

/// What exactly an asset protects (§11.2).
///
/// The domain is a resolved persistence object — a dataset, a subvolume id, a database name —
/// and never a bare path. §11.2 makes that mapping a precondition of claiming protection at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryScope {
    domain: Arc<str>,
    domain_kind: Arc<str>,
    covers: Vec<Arc<str>>,
    host: Arc<str>,
}

impl RecoveryScope {
    /// Declares that `domain` of kind `domain_kind` on `host` is what the asset holds.
    #[must_use]
    pub fn new(
        domain_kind: impl Into<Arc<str>>,
        domain: impl Into<Arc<str>>,
        host: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            domain: domain.into(),
            domain_kind: domain_kind.into(),
            covers: Vec::new(),
            host: host.into(),
        }
    }

    /// Names one object the scope actually covers.
    #[must_use]
    pub fn covering(mut self, object: impl Into<Arc<str>>) -> Self {
        self.covers.push(object.into());
        self
    }

    /// The resolved persistence object.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// What kind of persistence object it is — `zfs-dataset`, `btrfs-subvolume`, `file`.
    #[must_use]
    pub fn domain_kind(&self) -> &str {
        &self.domain_kind
    }

    /// The objects the scope covers.
    #[must_use]
    pub fn covers(&self) -> &[Arc<str>] {
        &self.covers
    }

    /// The host the scope lives on.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Whether this scope actually covers `object`.
    ///
    /// The answer is exact membership, never a path-prefix guess: §13.4's whole point is that
    /// `/data/customer.db` living under `/` does not make the root dataset's snapshot cover it.
    #[must_use]
    pub fn covers_object(&self, object: &str) -> bool {
        self.covers.iter().any(|covered| covered.as_ref() == object)
    }
}

/// What a provider checked before an asset was called usable (§11.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryValidation {
    exists: bool,
    identity_matches: bool,
    scope_matches: bool,
    restore_available: bool,
    permissions_present: bool,
    at: Timestamp,
    detail: Arc<str>,
}

impl RecoveryValidation {
    /// Records a validation in which every §11.4 check was made and passed.
    #[must_use]
    pub fn complete(at: Timestamp, detail: impl Into<Arc<str>>) -> Self {
        Self {
            exists: true,
            identity_matches: true,
            scope_matches: true,
            restore_available: true,
            permissions_present: true,
            at,
            detail: detail.into(),
        }
    }

    /// Records a validation in which nothing passed yet.
    #[must_use]
    pub fn none(at: Timestamp, detail: impl Into<Arc<str>>) -> Self {
        Self {
            exists: false,
            identity_matches: false,
            scope_matches: false,
            restore_available: false,
            permissions_present: false,
            at,
            detail: detail.into(),
        }
    }

    /// Records that the asset exists.
    #[must_use]
    pub const fn existing(mut self, exists: bool) -> Self {
        self.exists = exists;
        self
    }

    /// Records whether the asset's identity matches the planned source (§11.4).
    #[must_use]
    pub const fn identity(mut self, matches: bool) -> Self {
        self.identity_matches = matches;
        self
    }

    /// Records whether the asset's scope matches the expected target (§11.4).
    #[must_use]
    pub const fn scope(mut self, matches: bool) -> Self {
        self.scope_matches = matches;
        self
    }

    /// Records whether a restore path is available (§11.4).
    #[must_use]
    pub const fn restore(mut self, available: bool) -> Self {
        self.restore_available = available;
        self
    }

    /// Records whether the permissions recovery needs are held (§11.4, §43.4).
    #[must_use]
    pub const fn permissions(mut self, present: bool) -> Self {
        self.permissions_present = present;
        self
    }

    /// Whether the asset was found to exist (§11.4).
    #[must_use]
    pub const fn exists(&self) -> bool {
        self.exists
    }

    /// Whether the asset's identity matched the planned source (§11.4).
    #[must_use]
    pub const fn identity_matches(&self) -> bool {
        self.identity_matches
    }

    /// Whether the asset's scope matched the expected target (§11.4).
    #[must_use]
    pub const fn scope_matches(&self) -> bool {
        self.scope_matches
    }

    /// Whether a restore path was available (§11.4).
    #[must_use]
    pub const fn restore_available(&self) -> bool {
        self.restore_available
    }

    /// Whether the permissions recovery needs were held (§11.4, §43.4).
    #[must_use]
    pub const fn permissions_present(&self) -> bool {
        self.permissions_present
    }

    /// Whether every §11.4 check passed.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.exists
            && self.identity_matches
            && self.scope_matches
            && self.restore_available
            && self.permissions_present
    }

    /// The checks that did not pass, named so a refusal can say which.
    #[must_use]
    pub fn failures(&self) -> Vec<&'static str> {
        let mut failures = Vec::new();
        if !self.exists {
            failures.push("the asset does not exist");
        }
        if !self.identity_matches {
            failures.push("the asset's identity does not match the planned source");
        }
        if !self.scope_matches {
            failures.push("the asset's scope does not match the expected target");
        }
        if !self.restore_available {
            failures.push("no restore path is available");
        }
        if !self.permissions_present {
            failures.push("the permissions recovery needs are not held");
        }
        failures
    }

    /// When the validation was made.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }

    /// What the provider said about it.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// What retaining an asset costs, in the dimensions §38.1 lists.
///
/// Every figure is optional and every figure is labelled estimated by
/// [`RecoveryCost::is_estimated`], because §37.5 requires it wherever filesystem accounting is not
/// exact, and §38.2 forbids displaying "free" for a copy-on-write snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoveryCost {
    initial_bytes: Option<ByteSize>,
    retained_bytes: Option<ByteSize>,
    estimated: bool,
    creation_latency: Option<std::time::Duration>,
    quiesce: Option<std::time::Duration>,
    requires_reboot: bool,
    requires_offline: bool,
}

impl RecoveryCost {
    /// A cost nobody has measured yet.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            initial_bytes: None,
            retained_bytes: None,
            estimated: true,
            creation_latency: None,
            quiesce: None,
            requires_reboot: false,
            requires_offline: false,
        }
    }

    /// Records the space the asset occupies now, and whether the figure is exact (§37.5).
    #[must_use]
    pub const fn with_space(
        mut self,
        initial: Option<ByteSize>,
        retained: Option<ByteSize>,
        estimated: bool,
    ) -> Self {
        self.initial_bytes = initial;
        self.retained_bytes = retained;
        self.estimated = estimated;
        self
    }

    /// Records how long creation took or is expected to take (§38.1).
    #[must_use]
    pub const fn with_latency(mut self, latency: std::time::Duration) -> Self {
        self.creation_latency = Some(latency);
        self
    }

    /// Records how long an application must be quiesced for this asset (§18.4, §38.1).
    #[must_use]
    pub const fn with_quiesce(mut self, quiesce: std::time::Duration) -> Self {
        self.quiesce = Some(quiesce);
        self
    }

    /// Records that restoring from this asset needs a reboot (§13.7, §38.1).
    #[must_use]
    pub const fn needing_reboot(mut self) -> Self {
        self.requires_reboot = true;
        self
    }

    /// Records that restoring from this asset needs the filesystem offline (§14.6, §38.1).
    #[must_use]
    pub const fn needing_offline(mut self) -> Self {
        self.requires_offline = true;
        self
    }

    /// The space the asset occupied when it was created.
    #[must_use]
    pub const fn initial_bytes(&self) -> Option<ByteSize> {
        self.initial_bytes
    }

    /// The space it occupies now.
    #[must_use]
    pub const fn retained_bytes(&self) -> Option<ByteSize> {
        self.retained_bytes
    }

    /// Whether the space figures are estimated rather than exact (§37.5).
    #[must_use]
    pub const fn is_estimated(&self) -> bool {
        self.estimated
    }

    /// How long creation took or is expected to take.
    #[must_use]
    pub const fn creation_latency(&self) -> Option<std::time::Duration> {
        self.creation_latency
    }

    /// How long an application must be quiesced.
    #[must_use]
    pub const fn quiesce(&self) -> Option<std::time::Duration> {
        self.quiesce
    }

    /// Whether restoring needs a reboot.
    #[must_use]
    pub const fn requires_reboot(&self) -> bool {
        self.requires_reboot
    }

    /// Whether restoring needs the filesystem offline.
    #[must_use]
    pub const fn requires_offline(&self) -> bool {
        self.requires_offline
    }
}

/// How long an asset is kept (§37.1, §37.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    after_verification: std::time::Duration,
    hold: bool,
}

/// The default retention of §37.1: twenty-four hours after successful verification.
pub const DEFAULT_RETENTION: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            after_verification: DEFAULT_RETENTION,
            hold: false,
        }
    }
}

impl RetentionPolicy {
    /// A retention of `after_verification` past a successful verification (§37.1).
    #[must_use]
    pub const fn of(after_verification: std::time::Duration) -> Self {
        Self {
            after_verification,
            hold: false,
        }
    }

    /// A retention nothing automatic may end (§37.2).
    #[must_use]
    pub const fn held() -> Self {
        Self {
            after_verification: DEFAULT_RETENTION,
            hold: true,
        }
    }

    /// Places an explicit hold on the asset (§37.2, §37.4).
    #[must_use]
    pub const fn holding(mut self) -> Self {
        self.hold = true;
        self
    }

    /// How long the asset is kept after a successful verification.
    #[must_use]
    pub const fn window(&self) -> std::time::Duration {
        self.after_verification
    }

    /// Whether an explicit hold prevents automatic removal (§37.2).
    #[must_use]
    pub const fn is_held(&self) -> bool {
        self.hold
    }
}

/// Something an asset explicitly does not protect (§11.1's `exclusions`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryExclusion {
    subject: Arc<str>,
    reason: Arc<str>,
}

impl RecoveryExclusion {
    /// Records that `subject` is outside the asset, and why.
    #[must_use]
    pub fn new(subject: impl Into<Arc<str>>, reason: impl Into<Arc<str>>) -> Self {
        Self {
            subject: subject.into(),
            reason: reason.into(),
        }
    }

    /// What is excluded.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Why.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// One concrete resource that can contribute to restoring state (§3.6, §11.1).
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryAsset {
    id: RecoveryAssetId,
    provider: Arc<str>,
    asset_type: RecoveryAssetType,
    reference: Arc<str>,
    scope: RecoveryScope,
    created_at: Timestamp,
    source_plan: Option<PlanId>,
    state: AssetState,
    consistency: ConsistencyClass,
    restore_method: RestoreMethod,
    validation: Option<RecoveryValidation>,
    retention: RetentionPolicy,
    cost: RecoveryCost,
    dependencies: Vec<RecoveryAssetId>,
    exclusions: Vec<RecoveryExclusion>,
    captured_state: Option<Arc<str>>,
    expires_at: Option<Timestamp>,
}

impl RecoveryAsset {
    /// Declares an asset `provider` proposes to create over `scope`.
    ///
    /// The asset starts [`AssetState::Proposed`]: §2.1 makes planning side-effect free, so a plan
    /// that mentions an asset has not created one.
    #[must_use]
    pub fn proposed(
        provider: impl Into<Arc<str>>,
        asset_type: RecoveryAssetType,
        reference: impl Into<Arc<str>>,
        scope: RecoveryScope,
        created_at: Timestamp,
    ) -> Self {
        let provider = provider.into();
        let reference = reference.into();
        Self {
            id: RecoveryAssetId::of(
                &provider,
                None,
                scope.domain(),
                &created_at.as_nanosecond().to_string(),
            ),
            provider,
            asset_type,
            reference,
            scope,
            created_at,
            source_plan: None,
            state: AssetState::Proposed,
            consistency: ConsistencyClass::Unknown,
            restore_method: RestoreMethod::SelectiveFileRestore,
            validation: None,
            retention: RetentionPolicy::default(),
            cost: RecoveryCost::unknown(),
            dependencies: Vec::new(),
            exclusions: Vec::new(),
            captured_state: None,
            expires_at: None,
        }
    }

    /// Rebuilds an asset out of the fields a store read back (§37.5).
    ///
    /// `pub(crate)`, and reached only through [`crate::value`]. An asset's identity is what a plan
    /// references and what `remove recovery` names, so re-deriving it on read would break every
    /// reference that had already been printed.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub(crate) fn restore(
        id: RecoveryAssetId,
        provider: Arc<str>,
        asset_type: RecoveryAssetType,
        reference: Arc<str>,
        scope: RecoveryScope,
        created_at: Timestamp,
        source_plan: Option<PlanId>,
        state: AssetState,
        consistency: ConsistencyClass,
        restore_method: RestoreMethod,
        validation: Option<RecoveryValidation>,
        retention: RetentionPolicy,
        cost: RecoveryCost,
        dependencies: Vec<RecoveryAssetId>,
        exclusions: Vec<RecoveryExclusion>,
        captured_state: Option<Arc<str>>,
        expires_at: Option<Timestamp>,
    ) -> Self {
        Self {
            id,
            provider,
            asset_type,
            reference,
            scope,
            created_at,
            source_plan,
            state,
            consistency,
            restore_method,
            validation,
            retention,
            cost,
            dependencies,
            exclusions,
            captured_state,
            expires_at,
        }
    }

    /// Attributes the asset to the plan that asked for it (§11.1's `source_plan`).
    #[must_use]
    pub fn for_plan(mut self, plan: PlanId) -> Self {
        self.id = RecoveryAssetId::of(
            &self.provider,
            Some(&plan),
            self.scope.domain(),
            &self.created_at.as_nanosecond().to_string(),
        );
        self.source_plan = Some(plan);
        self
    }

    /// States the consistency the mechanism achieves (§11.3).
    #[must_use]
    pub const fn at_consistency(mut self, consistency: ConsistencyClass) -> Self {
        self.consistency = consistency;
        self
    }

    /// States how the asset would be used to restore (§13.5, Appendix C.1).
    #[must_use]
    pub const fn restored_by(mut self, method: RestoreMethod) -> Self {
        self.restore_method = method;
        self
    }

    /// Sets the retention policy (§37.1).
    #[must_use]
    pub const fn retained_for(mut self, retention: RetentionPolicy) -> Self {
        self.retention = retention;
        self
    }

    /// Records the cost (§38).
    #[must_use]
    pub fn costing(mut self, cost: RecoveryCost) -> Self {
        self.cost = cost;
        self
    }

    /// Names another asset this one needs (§11.1's `dependencies`).
    #[must_use]
    pub fn depending_on(mut self, other: RecoveryAssetId) -> Self {
        self.dependencies.push(other);
        self
    }

    /// Records something the asset does not protect (§11.1's `exclusions`).
    #[must_use]
    pub fn excluding(mut self, exclusion: RecoveryExclusion) -> Self {
        self.exclusions.push(exclusion);
        self
    }

    /// Records the fingerprint of the state the asset captured (§18.3's `captured_state_id`).
    #[must_use]
    pub fn capturing(mut self, fingerprint: impl Into<Arc<str>>) -> Self {
        self.captured_state = Some(fingerprint.into());
        self
    }

    /// Moves the asset into creation.
    #[must_use]
    pub const fn creating(mut self) -> Self {
        self.state = AssetState::Creating;
        self
    }

    /// Moves the asset to ready, or to invalid, on the strength of `validation` (§11.4).
    ///
    /// This is the only route to [`AssetState::Ready`]. §11.4's checks are not advisory: an asset
    /// whose scope does not match what the plan expected is `INVALID`, and the coverage algorithm
    /// then finds no validated path for that domain.
    #[must_use]
    pub fn validated(mut self, validation: RecoveryValidation) -> Self {
        self.state = if validation.is_complete() {
            AssetState::Ready
        } else {
            AssetState::Invalid
        };
        self.validation = Some(validation);
        self
    }

    /// Records that creation failed.
    #[must_use]
    pub const fn failed(mut self) -> Self {
        self.state = AssetState::Failed;
        self
    }

    /// Records that the asset became unusable — an LVM snapshot that overflowed (Appendix D.10).
    #[must_use]
    pub const fn invalidated(mut self) -> Self {
        self.state = AssetState::Invalid;
        self
    }

    /// Records that the asset was removed (§37).
    #[must_use]
    pub const fn removed(mut self) -> Self {
        self.state = AssetState::Removed;
        self
    }

    /// Records that retention passed (§37.1).
    #[must_use]
    pub const fn expired(mut self) -> Self {
        self.state = AssetState::Expired;
        self
    }

    /// Sets the instant retention ends (§37.1).
    #[must_use]
    pub const fn expiring_at(mut self, at: Timestamp) -> Self {
        self.expires_at = Some(at);
        self
    }

    /// The asset's identity.
    #[must_use]
    pub const fn id(&self) -> &RecoveryAssetId {
        &self.id
    }

    /// The provider that owns it.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The mechanism.
    #[must_use]
    pub const fn asset_type(&self) -> RecoveryAssetType {
        self.asset_type
    }

    /// The provider-native reference — `rpool/ROOT/debian@ono-a82f`, a store path, a savepoint.
    #[must_use]
    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// What it protects (§11.2).
    #[must_use]
    pub const fn scope(&self) -> &RecoveryScope {
        &self.scope
    }

    /// When it was created.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// The plan that asked for it.
    #[must_use]
    pub const fn source_plan(&self) -> Option<&PlanId> {
        self.source_plan.as_ref()
    }

    /// Where it is in its lifecycle.
    #[must_use]
    pub const fn state(&self) -> AssetState {
        self.state
    }

    /// The consistency of the captured state (§11.3).
    #[must_use]
    pub const fn consistency(&self) -> ConsistencyClass {
        self.consistency
    }

    /// How it would be used to restore.
    #[must_use]
    pub const fn restore_method(&self) -> RestoreMethod {
        self.restore_method
    }

    /// What was checked before it was called usable (§11.4).
    #[must_use]
    pub const fn validation(&self) -> Option<&RecoveryValidation> {
        self.validation.as_ref()
    }

    /// How long it is kept.
    #[must_use]
    pub const fn retention(&self) -> RetentionPolicy {
        self.retention
    }

    /// What it costs.
    #[must_use]
    pub const fn cost(&self) -> &RecoveryCost {
        &self.cost
    }

    /// The assets it needs.
    #[must_use]
    pub fn dependencies(&self) -> &[RecoveryAssetId] {
        &self.dependencies
    }

    /// What it does not protect.
    #[must_use]
    pub fn exclusions(&self) -> &[RecoveryExclusion] {
        &self.exclusions
    }

    /// The fingerprint of the state it captured (§18.3).
    #[must_use]
    pub fn captured_state(&self) -> Option<&str> {
        self.captured_state.as_deref()
    }

    /// When retention ends.
    #[must_use]
    pub const fn expires_at(&self) -> Option<Timestamp> {
        self.expires_at
    }

    /// Whether the asset can be used for recovery right now (§11.4).
    #[must_use]
    pub const fn is_usable(&self) -> bool {
        self.state.is_usable()
    }

    /// Whether the asset is a local recovery point rather than a backup (§11.5, §14.7).
    #[must_use]
    pub const fn is_local_recovery_point(&self) -> bool {
        self.asset_type.shares_failure_domain()
    }

    /// Whether the asset still reflects the state named by `fingerprint` (§18.3).
    ///
    /// An asset with no recorded fingerprint answers `false`, because §18.3 forbids pretending an
    /// asset of unknown vintage is a just-before-change recovery point.
    #[must_use]
    pub fn is_fresh_for(&self, fingerprint: &str) -> bool {
        self.captured_state
            .as_deref()
            .is_some_and(|captured| captured == fingerprint)
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

    fn instant() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    fn scope() -> RecoveryScope {
        RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost")
            .covering("/etc/nginx/nginx.conf")
    }

    fn asset() -> RecoveryAsset {
        RecoveryAsset::proposed(
            "ono.recovery.zfs",
            RecoveryAssetType::ZfsSnapshot,
            "rpool/ROOT/debian@ono-a82f",
            scope(),
            instant(),
        )
    }

    #[test]
    fn should_start_an_asset_proposed_because_planning_creates_nothing() {
        assert_eq!(
            asset().state(),
            AssetState::Proposed,
            "§2.1: creating or inspecting a plan MUST NOT mutate the target system"
        );
        assert!(!asset().is_usable());
    }

    #[test]
    fn should_reach_ready_only_through_a_validation_that_passed() {
        let ready = asset().validated(RecoveryValidation::complete(instant(), "checked"));
        assert_eq!(ready.state(), AssetState::Ready);
        assert!(ready.is_usable());
    }

    #[test]
    fn should_mark_an_asset_invalid_when_its_scope_does_not_match() {
        let checked = asset()
            .validated(RecoveryValidation::complete(instant(), "wrong dataset").scope(false));
        assert_eq!(
            checked.state(),
            AssetState::Invalid,
            "§11.4: a scope that does not match the expected target is not protection"
        );
        assert!(!checked.is_usable());
        assert_eq!(
            checked
                .validation()
                .map(RecoveryValidation::failures)
                .unwrap_or_default(),
            vec!["the asset's scope does not match the expected target"],
            "a refusal must say which check failed"
        );
    }

    #[test]
    fn should_refuse_to_call_a_snapshot_a_backup() {
        assert!(
            asset().is_local_recovery_point(),
            "§11.5 and §14.7: a copy-on-write snapshot shares the storage failure domain"
        );
        let archive = RecoveryAsset::proposed(
            "ono.recovery.file-copy",
            RecoveryAssetType::FileArchive,
            "/var/lib/ono/recovery/r-1",
            scope(),
            instant(),
        );
        assert!(
            !archive.is_local_recovery_point(),
            "a copy in the recovery store is an independent copy of the bytes"
        );
    }

    #[test]
    fn should_not_let_a_path_prefix_stand_in_for_coverage() {
        let root = RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost")
            .covering("/etc/nginx/nginx.conf");
        assert!(
            !root.covers_object("/data/customer.db"),
            "§13.4: a snapshot of the root dataset does not cover a separate child dataset"
        );
        assert!(root.covers_object("/etc/nginx/nginx.conf"));
    }

    #[test]
    fn should_order_restore_methods_least_destructive_first() {
        assert!(
            RestoreMethod::SelectiveFileRestore.destructiveness()
                < RestoreMethod::DatasetRollback.destructiveness(),
            "Appendix C.1: prefer the method that minimises unrelated state loss"
        );
        assert!(
            RestoreMethod::DatasetRollback.destructiveness()
                < RestoreMethod::OfflineRootRecovery.destructiveness()
        );
    }

    #[test]
    fn should_say_which_methods_discard_state_written_since_the_asset() {
        assert!(
            RestoreMethod::DatasetRollback.discards_newer_state(),
            "§13.6: rollback can discard all changes since the snapshot"
        );
        assert!(
            !RestoreMethod::SelectiveFileRestore.discards_newer_state(),
            "§59.6: selective restore preserves unrelated newer state"
        );
    }

    #[test]
    fn should_refuse_to_call_compensation_a_restore_of_prior_state() {
        assert!(
            !RestoreMethod::Compensation.restores_prior_state(),
            "§27.4: compensation MUST NOT be labelled rollback"
        );
    }

    #[test]
    fn should_treat_an_asset_of_unknown_vintage_as_not_fresh() {
        let stale = asset().validated(RecoveryValidation::complete(instant(), "ok"));
        assert!(
            !stale.is_fresh_for("sha256:abc"),
            "§18.3: without a captured-state fingerprint, nothing may claim the asset is current"
        );
        assert!(stale.capturing("sha256:abc").is_fresh_for("sha256:abc"));
    }

    #[test]
    fn should_never_report_a_snapshot_as_free() {
        let cost = RecoveryCost::unknown();
        assert!(
            cost.is_estimated(),
            "§38.2: Ono MUST NOT display 'free' for a copy-on-write snapshot"
        );
        assert!(cost.initial_bytes().is_none(), "unknown is null, not zero");
    }

    #[test]
    fn should_default_retention_to_the_twenty_four_hours_section_thirty_seven_names() {
        assert_eq!(RetentionPolicy::default().window(), DEFAULT_RETENTION);
        assert!(!RetentionPolicy::default().is_held());
        assert!(RetentionPolicy::held().is_held());
    }

    #[test]
    fn should_give_two_assets_over_different_domains_different_identities() {
        let etc = asset();
        let data = RecoveryAsset::proposed(
            "ono.recovery.zfs",
            RecoveryAssetType::ZfsSnapshot,
            "tank/data@ono-a82f",
            RecoveryScope::new("zfs-dataset", "tank/data", "localhost"),
            instant(),
        );
        assert_ne!(
            etc.id(),
            data.id(),
            "§13.3: each dataset's snapshot is recorded individually, even under one operation"
        );
    }
}
