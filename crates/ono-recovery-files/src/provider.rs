//! The first-party file and configuration recovery provider (spec v0.6 §15).
//!
//! §15 opens with the reason it exists: *"Not every system uses snapshot-capable storage."* A
//! machine on ext4 has no dataset to roll back and no subvolume to replace, and the change it is
//! about to make to `/etc/nginx/nginx.conf` is exactly the change §31's workflow is written
//! around. This provider is the answer — it copies the objects a plan will touch into a private
//! store, and puts them back one at a time.
//!
//! Appendix A.4 is why that narrowness is a strength rather than a fallback: *"A small
//! configuration-file backup may dominate a root-dataset rollback for one file because it has a
//! smaller recovery blast radius."* A candidate from here carries
//! [`RestoreMethod::SelectiveFileRestore`], the least destructive method above a provider's own
//! native restore, and a scope exactly as wide as the objects it holds. The coverage algorithm
//! then prefers it over a dataset rollback for a single file without knowing anything about files.
//!
//! Three refusals shape everything below, and each is a spec sentence rather than a policy:
//!
//! - §15.2 excludes sockets, devices, pseudo-filesystems and arbitrarily large trees, and the
//!   refusals name the limit that was crossed. A silently truncated archive is §62.1's snapshot
//!   theatre with a file extension.
//! - §15.5 and §44.2 forbid rendering recovery content by default. There is no method on this
//!   provider, or on anything it returns, that hands file bytes to a caller: an asset carries
//!   paths, sizes, digests and modes, and the bytes stay in the store behind mode `0600`.
//! - §43.5 forbids protecting one object and restoring through another. Identity is recorded when
//!   the copy is made and re-stated before it is written back.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::{SignedDuration, Timestamp};
use ono_change_core::error::{
    asset_create_failed, asset_invalid, cleanup_blocked, recovery_plan_incomplete, scope_mismatch,
};
use ono_change_core::{
    ActionRole, AssetState, ChangePlan, ConsistencyClass, DirectoryRestorePolicy, EffectDomain,
    Execution, FilesystemKind, Idempotency, MetadataCoverage, NewerStateClass, NewerStateImpact,
    NewerStateItem, NonPersistentReason, PersistenceDomain, PlanAction, PlanId, ProtectionAction,
    ProtectionMode, ProviderAvailability, ProviderCapabilities, RecoveryAsset, RecoveryAssetType,
    RecoveryCandidate, RecoveryCapability, RecoveryCost, RecoveryExclusion, RecoveryGoal,
    RecoveryObjective, RecoveryPlanFragment, RecoveryProvider, RecoveryScope, RecoveryValidation,
    ResolvedMount, RestoreMethod, RetentionPolicy, UnrecoverableEffect, VerificationClass,
    VerificationContract,
};
use ono_change_core::{EquivalenceDomain, RecoveryAssetId};
use ono_value::{ByteSize, ErrorValue, Value};

use crate::capture::{Capture, ScanMode, scan};
use crate::coverage;
use crate::identity::{FilesystemFacts, ObjectKind, filesystem_of, lstat};
use crate::limits::FileProtectionLimits;
use crate::manifest::{ArchiveEntry, Manifest, digest_of};
use crate::restore::{RestoreReport, restore_objects};
use crate::store::{FileRecoveryStore, is_writable};

/// The provider's id, as it appears in every asset, candidate and action it produces.
pub const PROVIDER_ID: &str = "ono.recovery.file-copy";

/// Copies files and configuration into a private store, and puts them back (§15).
#[derive(Debug, Clone)]
pub struct FileRecoveryProvider {
    store: FileRecoveryStore,
    limits: FileProtectionLimits,
    host: Arc<str>,
    now: Timestamp,
    directory_policy: DirectoryRestorePolicy,
    retention: RetentionPolicy,
}

impl FileRecoveryProvider {
    /// A provider writing into `store`, whose sense of now is `now`.
    ///
    /// `now` is a parameter because a recovery asset's identity, its expiry and the timestamps in
    /// its manifest are all derived from it, and a provider that read the clock could not be
    /// tested twice with the same result (AGENTS.md §11).
    #[must_use]
    pub fn new(store: FileRecoveryStore, now: Timestamp) -> Self {
        Self {
            store,
            limits: FileProtectionLimits::default(),
            host: Arc::from("localhost"),
            now,
            directory_policy: DirectoryRestorePolicy::KeepExtraFiles,
            retention: RetentionPolicy::default(),
        }
    }

    /// Sets what the provider will archive before it refuses (§15.2).
    #[must_use]
    pub const fn with_limits(mut self, limits: FileProtectionLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Names the host every scope belongs to (§29.1).
    #[must_use]
    pub fn with_host(mut self, host: impl Into<Arc<str>>) -> Self {
        self.host = host.into();
        self
    }

    /// Sets what a directory restore does with files the archive never held (Appendix C.6).
    ///
    /// The default is [`DirectoryRestorePolicy::KeepExtraFiles`], which Appendix C.6 requires:
    /// *"Default selective directory restore MUST NOT delete newer extra files unless the
    /// recovery objective explicitly requires exact-tree equivalence."*
    #[must_use]
    pub const fn with_directory_policy(mut self, policy: DirectoryRestorePolicy) -> Self {
        self.directory_policy = policy;
        self
    }

    /// Sets how long an asset is kept (§37.1).
    #[must_use]
    pub const fn with_retention(mut self, retention: RetentionPolicy) -> Self {
        self.retention = retention;
        self
    }

    /// The store the copies live in (§15.3).
    #[must_use]
    pub const fn store(&self) -> &FileRecoveryStore {
        &self.store
    }

    /// The configured limits (§15.2).
    #[must_use]
    pub const fn limits(&self) -> &FileProtectionLimits {
        &self.limits
    }

    /// What a directory restore does with newer files (Appendix C.6).
    #[must_use]
    pub const fn directory_policy(&self) -> DirectoryRestorePolicy {
        self.directory_policy
    }

    /// The instant the provider works from.
    #[must_use]
    pub const fn now(&self) -> Timestamp {
        self.now
    }

    /// What a restore of `path` would actually put back, measured now (Appendix C.7).
    ///
    /// # Errors
    ///
    /// `change.store_unavailable` when the probe could not be made, which leaves the coverage
    /// unestablished rather than assumed (§56.3).
    pub fn metadata_coverage(&self, path: &Path) -> Result<MetadataCoverage, ErrorValue> {
        coverage::measure_for(path, self.store.root())
    }

    /// Restores what `action` names, and reports what came back with it (Appendix C.6, C.7).
    ///
    /// [`RecoveryProvider::restore`] is this method with the report dropped. The report is where
    /// Appendix C.7's *"missing metadata support MUST be visible"* is answered for one restore:
    /// the extended attribute a filesystem refused is named here rather than silently absent.
    ///
    /// # Errors
    ///
    /// As [`RecoveryProvider::restore`].
    pub fn restore_reporting(
        &self,
        action: &PlanAction,
        asset: &RecoveryAsset,
    ) -> Result<RestoreReport, ErrorValue> {
        self.owns(asset)?;
        if !asset.is_usable() {
            return Err(asset_invalid(
                asset.id(),
                &["the asset is not in a state recovery may use (§11.4)"],
            ));
        }
        let manifest = self.store.read_manifest(asset.id())?;
        let selection = action.target().map(Path::new);
        restore_objects(
            &self.store,
            asset.id(),
            &manifest,
            selection,
            self.directory_policy,
        )
    }

    /// Refuses an asset another provider owns (§12.1).
    fn owns(&self, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        if asset.provider() == PROVIDER_ID {
            return Ok(());
        }
        Err(ono_change_core::error::provider_unavailable(
            PROVIDER_ID,
            &format!(
                "asset {} belongs to {} and only its own provider can answer for it",
                asset.id().short(),
                asset.provider()
            ),
        ))
    }

    /// The asset a candidate proposes, whose reference is where the copy will live (§15.3).
    fn proposed_asset(&self, candidate: &RecoveryCandidate) -> RecoveryAsset {
        let created = self.now.as_nanosecond().to_string();
        let id = RecoveryAssetId::of(PROVIDER_ID, None, candidate.scope().domain(), &created);
        let reference = self.store.asset_directory(&id).display().to_string();
        let asset = RecoveryAsset::proposed(
            PROVIDER_ID,
            RecoveryAssetType::FileArchive,
            reference,
            candidate.scope().clone(),
            self.now,
        )
        .at_consistency(ConsistencyClass::ByteConsistent)
        .restored_by(RestoreMethod::SelectiveFileRestore)
        .retained_for(self.retention)
        .costing(candidate.cost().clone())
        .expiring_at(self.expiry());
        candidate
            .exclusions()
            .iter()
            .fold(asset, |asset, exclusion| asset.excluding(exclusion.clone()))
    }

    /// When retention ends for an asset created now (§37.1).
    fn expiry(&self) -> Timestamp {
        let window =
            SignedDuration::try_from(self.retention.window()).unwrap_or(SignedDuration::ZERO);
        self.now.checked_add(window).unwrap_or(self.now)
    }

    /// The filesystem holding `path`, asked through the directory when the path is a symlink.
    fn filesystem_for(&self, path: &Path) -> Result<FilesystemFacts, ErrorValue> {
        let metadata = lstat(path)?;
        if ObjectKind::of(&metadata) == ObjectKind::Symlink {
            return filesystem_of(path.parent().unwrap_or(Path::new("/")));
        }
        filesystem_of(path)
    }

    /// The mount Appendix B.1's pipeline resolved, as far as `statfs` can state it.
    fn mount_of(&self, facts: &FilesystemFacts, path: &Path) -> ResolvedMount {
        let device = format!("dev:{}", facts.device());
        let mount = ResolvedMount::new(
            device.clone(),
            facts.mount_point(),
            facts.name(),
            device,
            "unknown",
        );
        let mut options = vec![Arc::from("rw")];
        if read_only_mount(path) {
            options = vec![Arc::from("ro")];
        }
        mount.with_options(options)
    }

    /// The five §11.4 checks, each answered against the filesystem rather than assumed.
    fn check(&self, asset: &RecoveryAsset) -> Result<RecoveryValidation, ErrorValue> {
        let Ok(manifest) = self.store.read_manifest(asset.id()) else {
            return Ok(RecoveryValidation::none(
                self.now,
                format!(
                    "the recovery copy is not in the store at `{}`",
                    asset.reference()
                ),
            ));
        };
        let mut notes = Vec::new();
        let identity = self.identity_intact(asset, &manifest, &mut notes);
        let scope = self.scope_intact(asset, &manifest, &mut notes);
        let root = manifest.root();
        let parent = root.parent().unwrap_or(Path::new("/"));
        let restore_available = is_writable(parent);
        if !restore_available {
            notes.push(format!(
                "`{}` is not writable by this process",
                parent.display()
            ));
        }
        let permissions = self.permissions_held(&manifest, &mut notes)?;
        if notes.is_empty() {
            notes.push(format!(
                "the copy of `{}` is present, matches its recorded digest and can be written back",
                root.display()
            ));
        }
        Ok(RecoveryValidation::none(self.now, notes.join("; "))
            .existing(true)
            .identity(identity)
            .scope(scope)
            .restore(restore_available)
            .permissions(permissions))
    }

    /// Whether the stored copy is still the bytes that were copied (§11.4's identity check).
    fn identity_intact(
        &self,
        asset: &RecoveryAsset,
        manifest: &Manifest,
        notes: &mut Vec<String>,
    ) -> bool {
        let Ok(fingerprint) = self.store.manifest_fingerprint(asset.id()) else {
            notes.push("the manifest could not be read back".to_owned());
            return false;
        };
        if asset.captured_state() != Some(fingerprint.as_str()) {
            notes.push(
                "the manifest does not match the fingerprint recorded when the asset was created"
                    .to_owned(),
            );
            return false;
        }
        for entry in manifest.entries() {
            let Some(blob) = entry.blob() else {
                continue;
            };
            match self.store.read_blob(asset.id(), blob) {
                Ok(bytes) if digest_of(&bytes) == entry.digest() => {}
                Ok(_) => {
                    notes.push(format!(
                        "the stored copy of `{}` no longer matches its recorded digest",
                        entry.destination(manifest.root()).display()
                    ));
                    return false;
                }
                Err(_) => {
                    notes.push(format!(
                        "the stored copy of `{}` is missing",
                        entry.destination(manifest.root()).display()
                    ));
                    return false;
                }
            }
        }
        true
    }

    /// Whether the destination still resolves to the domain the copy was taken from (§11.4).
    fn scope_intact(
        &self,
        asset: &RecoveryAsset,
        manifest: &Manifest,
        notes: &mut Vec<String>,
    ) -> bool {
        let root = manifest.root();
        if asset.scope().domain() != root.display().to_string() {
            notes.push("the asset's scope names a different object from its manifest".to_owned());
            return false;
        }
        let parent = root.parent().unwrap_or(Path::new("/"));
        let Ok(metadata) = std::fs::symlink_metadata(parent) else {
            notes.push(format!(
                "`{}` no longer exists, so the destination has no persistence domain",
                parent.display()
            ));
            return false;
        };
        if ObjectKind::of(&metadata) != ObjectKind::Directory {
            notes.push(format!(
                "`{}` is a {} now, so the destination is not the domain the copy came from",
                parent.display(),
                ObjectKind::of(&metadata).as_str()
            ));
            return false;
        }
        let Ok(facts) = filesystem_of(parent) else {
            notes.push(format!(
                "the filesystem holding `{}` could not be read",
                parent.display()
            ));
            return false;
        };
        if facts.device() != manifest.root_parent().device() {
            notes.push(format!(
                "`{}` is on a different filesystem from the one the copy was taken from",
                parent.display()
            ));
            return false;
        }
        true
    }

    /// Whether this process holds the privilege the restore needs (§11.4, §43.4).
    fn permissions_held(
        &self,
        manifest: &Manifest,
        notes: &mut Vec<String>,
    ) -> Result<bool, ErrorValue> {
        let uid = rustix::process::geteuid().as_raw();
        let gid = rustix::process::getegid().as_raw();
        let foreign: Vec<&ArchiveEntry> = manifest
            .entries()
            .iter()
            .filter(|entry| entry.uid() != uid || entry.gid() != gid)
            .collect();
        if foreign.is_empty() {
            return Ok(true);
        }
        let coverage = coverage::measure(
            manifest.root().parent().unwrap_or(Path::new("/")),
            self.store.root(),
        )?;
        if !coverage.owner {
            notes.push(format!(
                "{} object(s) are owned by another user and this process cannot restore ownership",
                foreign.len()
            ));
        }
        Ok(coverage.owner)
    }

    /// The measured cost of a walk, which this provider knows exactly (§37.5, §38.1).
    fn cost_of(&self, capture: &Capture) -> RecoveryCost {
        let bytes = ByteSize::from_bytes(u128::from(capture.total_bytes()));
        RecoveryCost::unknown().with_space(Some(bytes), Some(bytes), false)
    }

    /// The exclusions §C.7 and §15.2 require to be visible on every candidate and asset.
    fn exclusions_for(&self, path: &Path, capture: &Capture) -> Vec<RecoveryExclusion> {
        let mut exclusions = capture.exclusions().to_vec();
        if let Ok(coverage) = self.metadata_coverage(path) {
            let gaps = coverage.gaps();
            if !gaps.is_empty() {
                exclusions.push(RecoveryExclusion::new(
                    "metadata coverage",
                    format!(
                        "Appendix C.7: a restore from this archive does not put back {}",
                        gaps.join(", ")
                    ),
                ));
            }
        }
        match capture.filesystem().kind() {
            FilesystemKind::Network => exclusions.push(RecoveryExclusion::new(
                capture.filesystem().mount_point().to_owned(),
                "Appendix B.6: the bytes live on another machine. The copy is local and \
                 independent, and restoring it needs the export to be writable",
            )),
            FilesystemKind::Overlay => exclusions.push(RecoveryExclusion::new(
                capture.filesystem().mount_point().to_owned(),
                "Appendix B.4: the writable layer of this overlay is elsewhere, so the restore \
                 writes into the upper layer rather than into the image beneath it",
            )),
            _ => {}
        }
        exclusions
    }
}

impl RecoveryProvider for FileRecoveryProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        RecoveryCapability::REQUIRED
            .iter()
            .fold(
                ProviderCapabilities::new(PROVIDER_ID),
                |carry, capability| carry.recovering(*capability),
            )
            .tested_against("ono.recovery.file-copy", env!("CARGO_PKG_VERSION"))
    }

    fn availability(&self) -> ProviderAvailability {
        self.store.unusable_reason().map_or_else(
            || ProviderAvailability::Available {
                version: Arc::from(env!("CARGO_PKG_VERSION")),
            },
            |reason| ProviderAvailability::Unavailable {
                reason: Arc::from(reason),
            },
        )
    }

    fn resolve_domain(&self, path: &str) -> Result<Option<PersistenceDomain>, ErrorValue> {
        let path = Path::new(path);
        if self.store.contains(path) {
            return Ok(None);
        }
        let facts = self.filesystem_for(path)?;
        let mount = self.mount_of(&facts, path);
        let kind = ObjectKind::of(&lstat(path)?);
        let display = path.display().to_string();
        if !facts.kind().is_persistent() {
            let reason = match facts.kind() {
                FilesystemKind::Volatile => NonPersistentReason::Volatile,
                _ => NonPersistentReason::Pseudo,
            };
            return Ok(Some(PersistenceDomain::refused(
                display,
                mount,
                reason,
                format!(
                    "v0.6 Appendix B.7 and §15.2: `{}` is a {} and holds runtime state rather \
                     than state a copy could return",
                    facts.mount_point(),
                    facts.name()
                ),
            )));
        }
        if let Some(exclusion) = kind.exclusion() {
            return Ok(Some(PersistenceDomain::refused(
                display,
                mount,
                NonPersistentReason::NoProvider,
                format!("v0.6 §15.2: {exclusion}"),
            )));
        }
        Ok(Some(
            PersistenceDomain::resolved(
                display.clone(),
                mount,
                kind.as_str(),
                display,
                format!(
                    "v0.6 §15: a copy of the object into the Ono recovery store on {}",
                    facts.name()
                ),
            )
            .with_boundary(facts.mount_point()),
        ))
    }

    fn discover(
        &self,
        domain: &PersistenceDomain,
        objective: RecoveryObjective,
    ) -> Result<Vec<RecoveryCandidate>, ErrorValue> {
        if !domain.is_protectable() {
            return Ok(Vec::new());
        }
        if !matches!(
            objective,
            RecoveryObjective::PreserveExact | RecoveryObjective::RestoreSemantic
        ) {
            return Ok(Vec::new());
        }
        let path = Path::new(domain.path());
        if self.store.contains(path) {
            return Ok(Vec::new());
        }
        let capture = scan(
            path,
            &self.limits,
            ScanMode::Measure,
            self.now.as_nanosecond(),
        )?;
        let manifest = capture.manifest();
        let kind = manifest
            .entries()
            .first()
            .map_or(ObjectKind::RegularFile, ArchiveEntry::kind);
        let scope = manifest.covered_objects().into_iter().fold(
            RecoveryScope::new(kind.as_str(), domain.path(), self.host.clone()),
            RecoveryScope::covering,
        );
        let candidate = RecoveryCandidate::new(
            PROVIDER_ID,
            scope,
            EffectDomain::FilesystemPersistent,
            objective,
            format!(
                "copy {} object(s) totalling {} bytes into the Ono recovery store",
                capture.object_count(),
                capture.total_bytes()
            ),
        )
        .at_consistency(ConsistencyClass::ByteConsistent)
        .restored_by(RestoreMethod::SelectiveFileRestore)
        .costing(self.cost_of(&capture))
        .needing_to_create("read access to every object in the scope")
        .needing_to_restore("write access to the directory that holds the object");
        let candidate = self
            .exclusions_for(path, &capture)
            .into_iter()
            .fold(candidate, RecoveryCandidate::excluding);
        let candidate = if capture.owners_beyond_process() {
            candidate.needing_to_restore(
                "privilege to restore ownership, because objects here belong to another user",
            )
        } else {
            candidate
        };
        Ok(vec![candidate])
    }

    fn plan_protection(
        &self,
        candidates: &[RecoveryCandidate],
        mode: ProtectionMode,
    ) -> Result<Vec<ProtectionAction>, ErrorValue> {
        if !mode.creates_assets() {
            return Ok(Vec::new());
        }
        Ok(candidates
            .iter()
            .filter(|candidate| candidate.provider() == PROVIDER_ID)
            .map(|candidate| {
                let action = ProtectionAction::new(
                    PROVIDER_ID,
                    format!(
                        "copy {} into the Ono recovery store",
                        candidate.scope().domain()
                    ),
                    candidate.clone(),
                    self.proposed_asset(candidate),
                );
                if mode == ProtectionMode::Maximize {
                    action.optional()
                } else {
                    action
                }
            })
            .collect())
    }

    fn create(&self, action: &ProtectionAction) -> Result<RecoveryAsset, ErrorValue> {
        let scope = action.candidate().scope().clone();
        if action.provider() != PROVIDER_ID {
            return Err(asset_create_failed(
                PROVIDER_ID,
                scope.domain(),
                &format!(
                    "this protection action belongs to {} and this provider will not act for it",
                    action.provider()
                ),
            ));
        }
        let path = Path::new(scope.domain());
        let capture = scan(
            path,
            &self.limits,
            ScanMode::Capture,
            self.now.as_nanosecond(),
        )?;
        let covered = capture.manifest().covered_objects();
        let planned: Vec<String> = scope
            .covers()
            .iter()
            .map(|object| object.to_string())
            .collect();
        if covered != planned {
            return Err(scope_mismatch(
                action.proposed_asset().id(),
                &planned.join(", "),
                &covered.join(", "),
            ));
        }
        self.store.write_archive(
            action.proposed_asset().id(),
            capture.manifest(),
            capture.contents(),
        )?;
        let fingerprint = self
            .store
            .manifest_fingerprint(action.proposed_asset().id())?;
        let asset = action
            .proposed_asset()
            .clone()
            .creating()
            .capturing(fingerprint)
            .costing(self.cost_of(&capture))
            .expiring_at(self.expiry());
        let validation = self.check(&asset)?;
        let failures = validation.failures();
        if !failures.is_empty() {
            let error = asset_invalid(asset.id(), &failures);
            let _ = self.store.remove_archive(asset.id());
            return Err(error);
        }
        Ok(asset.validated(validation))
    }

    fn validate(&self, asset: &RecoveryAsset) -> Result<RecoveryValidation, ErrorValue> {
        self.owns(asset)?;
        self.check(asset)
    }

    fn plan_recovery(
        &self,
        asset: &RecoveryAsset,
        source: Option<&ChangePlan>,
        goal: RecoveryGoal,
    ) -> Result<RecoveryPlanFragment, ErrorValue> {
        self.owns(asset)?;
        if asset.state() != AssetState::Ready {
            return Err(asset_invalid(
                asset.id(),
                &["the asset has not been validated, so no recovery can be planned from it"],
            ));
        }
        if goal != RecoveryGoal::RestoreChangedObjects {
            return Err(recovery_plan_incomplete(
                "a way to satisfy this recovery goal from a file archive",
                &format!(
                    "v0.6 Appendix C.1: a file archive restores the objects it holds, which is \
                     `restore-changed-objects`. `{}` asks for something a copy of files cannot \
                     give, and a provider that claimed it would be describing a snapshot it never \
                     took",
                    goal.as_str()
                ),
            ));
        }
        let manifest = self.store.read_manifest(asset.id())?;
        let root = manifest.root();
        let selected = restore_set(&manifest, source);
        if selected.is_empty() {
            return Err(scope_mismatch(
                asset.id(),
                source.map_or("the plan's targets", |plan| plan.intent().text()),
                &manifest.covered_objects().join(", "),
            ));
        }
        let plan_id = PlanId::derive(&[PROVIDER_ID, asset.id().as_str(), "recovery"]);
        let mut fragment =
            RecoveryPlanFragment::new(PROVIDER_ID, RestoreMethod::SelectiveFileRestore);
        for (ordinal, entry) in selected.iter().enumerate() {
            let destination = entry.destination(root);
            let object = destination.display().to_string();
            fragment = fragment.acting(
                PlanAction::new(
                    &plan_id,
                    ordinal,
                    ActionRole::Recover,
                    format!("restore {object} from the Ono recovery store"),
                    Execution::RecoveryOperation {
                        provider: Arc::from(PROVIDER_ID),
                        capability: Arc::from(RecoveryCapability::Restore.as_str()),
                        arguments: vec![
                            (Arc::from("asset"), Value::string(asset.id().as_str())),
                            (Arc::from("object"), Value::string(&object)),
                        ],
                    },
                )
                .on(object.clone())
                .with_idempotency(Idempotency::Idempotent)
                .recovery_semantics(
                    "v0.6 §15.4: the copy is written beside the live object and renamed over it, \
                     so the object is either the old one or the new one and never half of either",
                ),
            );
            if entry.kind() != ObjectKind::Directory {
                fragment = fragment.verifying(
                    VerificationContract::new(
                        &plan_id,
                        VerificationClass::Required,
                        object.clone(),
                        format!("the SHA-256 of `{object}` is the digest the archive recorded"),
                    )
                    .expecting(Value::string(entry.digest()))
                    .about(EquivalenceDomain::PersistentState),
                );
            }
        }
        for effect in self.unrecoverable_metadata(&manifest, &selected) {
            fragment = fragment.leaving(effect);
        }
        Ok(fragment.with_newer_state(self.newer_state(&manifest, &selected)))
    }

    fn restore(&self, action: &PlanAction, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        self.restore_reporting(action, asset).map(|_| ())
    }

    fn cleanup(&self, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        self.owns(asset)?;
        if asset.retention().is_held() {
            return Err(cleanup_blocked(
                asset.id(),
                &[asset
                    .source_plan()
                    .map_or_else(|| "the plan that created it".to_owned(), PlanId::to_string)],
            ));
        }
        self.store.remove_archive(asset.id())
    }

    fn estimate_cost(&self, asset: &RecoveryAsset) -> Result<RecoveryCost, ErrorValue> {
        self.owns(asset)?;
        let retained = self.store.occupied_bytes(asset.id())?;
        let manifest = self.store.read_manifest(asset.id())?;
        Ok(RecoveryCost::unknown().with_space(
            Some(ByteSize::from_bytes(u128::from(manifest.total_bytes()))),
            Some(ByteSize::from_bytes(u128::from(retained))),
            false,
        ))
    }
}

impl FileRecoveryProvider {
    /// What a selective restore would do to everything written since the copy (Appendix C.3, C.4).
    fn newer_state(&self, manifest: &Manifest, selected: &[&ArchiveEntry]) -> NewerStateImpact {
        let root = manifest.root();
        let mut items = Vec::new();
        for entry in manifest.entries() {
            let destination = entry.destination(root);
            let object = destination.display().to_string();
            let inside = selected
                .iter()
                .any(|chosen| chosen.destination(root) == destination);
            match live_digest(&destination, entry) {
                LiveState::Same | LiveState::Absent => {}
                LiveState::Different if inside => items.push(
                    NewerStateItem::new(
                        object,
                        NewerStateClass::Conflicting,
                        "Appendix C.4: the object was changed again after the recovery point, and \
                         restoring the copy would discard that change",
                    )
                    .changed_at(self.now),
                ),
                LiveState::Different => items.push(NewerStateItem::new(
                    object,
                    NewerStateClass::PreservedByMethod,
                    "Appendix C.3: a selective file restore does not touch this object, so the \
                     newer state stays",
                )),
                LiveState::Unknown => items.push(NewerStateItem::new(
                    object,
                    NewerStateClass::Unknown,
                    "§56.3: the object could not be read, so what recovery would do to it could \
                     not be established",
                )),
            }
        }
        items.extend(self.extra_files(manifest, selected));
        NewerStateImpact::analysed(items)
    }

    /// Files that exist now and were never in the archive (Appendix C.6).
    fn extra_files(&self, manifest: &Manifest, selected: &[&ArchiveEntry]) -> Vec<NewerStateItem> {
        let root = manifest.root();
        let known: Vec<PathBuf> = manifest
            .entries()
            .iter()
            .map(|entry| entry.destination(root))
            .collect();
        let mut extras = Vec::new();
        for entry in selected
            .iter()
            .filter(|entry| entry.kind() == ObjectKind::Directory)
        {
            let Ok(children) = std::fs::read_dir(entry.destination(root)) else {
                continue;
            };
            for child in children.flatten() {
                let path = child.path();
                if known.contains(&path) {
                    continue;
                }
                extras.push(match self.directory_policy {
                    DirectoryRestorePolicy::KeepExtraFiles => NewerStateItem::new(
                        path.display().to_string(),
                        NewerStateClass::PreservedByMethod,
                        "Appendix C.6: the archive never held this file, and the default \
                         directory restore does not delete it",
                    ),
                    DirectoryRestorePolicy::ExactTree => NewerStateItem::new(
                        path.display().to_string(),
                        NewerStateClass::DiscardedByMethod,
                        "Appendix C.6: an exact-tree restore removes files the archive never held",
                    ),
                });
            }
        }
        extras.sort_by(|left, right| left.object().cmp(right.object()));
        extras
    }

    /// The metadata this restore cannot put back, named as an effect it leaves (§24.3, C.7).
    fn unrecoverable_metadata(
        &self,
        manifest: &Manifest,
        selected: &[&ArchiveEntry],
    ) -> Vec<UnrecoverableEffect> {
        let Ok(coverage) = self.metadata_coverage(manifest.root()) else {
            return Vec::new();
        };
        let uid = rustix::process::geteuid().as_raw();
        let mut effects = Vec::new();
        if !coverage.owner && selected.iter().any(|entry| entry.uid() != uid) {
            effects.push(UnrecoverableEffect::new(
                format!(
                    "the ownership of objects under {}",
                    manifest.root().display()
                ),
                EffectDomain::FilesystemPersistent,
                "Appendix C.7 and §43.4: this process cannot give a file to another user, so the \
                 objects come back owned by the process that restored them",
            ));
        }
        if !coverage.xattrs && selected.iter().any(|entry| !entry.xattrs().is_empty()) {
            effects.push(UnrecoverableEffect::new(
                format!(
                    "the extended attributes under {}",
                    manifest.root().display()
                ),
                EffectDomain::FilesystemPersistent,
                "Appendix C.7: the destination filesystem does not carry extended attributes",
            ));
        }
        effects
    }
}

/// The entries a recovery would restore: what the source plan touched, or the whole archive.
fn restore_set<'a>(manifest: &'a Manifest, source: Option<&ChangePlan>) -> Vec<&'a ArchiveEntry> {
    let root = manifest.root();
    let Some(plan) = source else {
        return manifest.entries().iter().collect();
    };
    let touched: Vec<&str> = plan
        .actions()
        .iter()
        .filter(|action| action.role().mutates_target())
        .filter_map(PlanAction::target)
        .collect();
    if touched.is_empty() {
        return manifest.entries().iter().collect();
    }
    manifest
        .entries()
        .iter()
        .filter(|entry| {
            let destination = entry.destination(root).display().to_string();
            touched.iter().any(|target| *target == destination)
        })
        .collect()
}

/// How the object on disk compares with the copy in the archive.
enum LiveState {
    /// The object is byte-for-byte what was copied.
    Same,
    /// The object has changed since the copy was taken.
    Different,
    /// The object is not there at all.
    Absent,
    /// The object could not be read, which §56.3 keeps unknown.
    Unknown,
}

fn live_digest(path: &Path, entry: &ArchiveEntry) -> LiveState {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return LiveState::Absent;
    };
    let kind = ObjectKind::of(&metadata);
    if kind != entry.kind() {
        return LiveState::Different;
    }
    match kind {
        ObjectKind::Directory => LiveState::Same,
        ObjectKind::Symlink => match std::fs::read_link(path) {
            Ok(target) if Some(target.as_path()) == entry.link_target() => LiveState::Same,
            Ok(_) => LiveState::Different,
            Err(_) => LiveState::Unknown,
        },
        _ => match std::fs::read(path) {
            Ok(bytes) if digest_of(&bytes) == entry.digest() => LiveState::Same,
            Ok(_) => LiveState::Different,
            Err(_) => LiveState::Unknown,
        },
    }
}

/// Whether the mount holding `path` is read-only, which Appendix G.2 makes a restore blocker.
fn read_only_mount(path: &Path) -> bool {
    rustix::fs::statvfs(path).is_ok_and(|statvfs| {
        statvfs
            .f_flag
            .contains(rustix::fs::StatVfsMountFlags::RDONLY)
    })
}
