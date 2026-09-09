//! The provider itself: identity, protection, recovery, cleanup and cost (§14, Appendix D.6–D.9).
//!
//! Everything the provider says about a filesystem comes from a `btrfs` invocation or from the
//! mount table, and everything it refuses to say comes from one of those failing. The shape of
//! each method follows from that: there are no defaults for facts, so a query that did not answer
//! leaves a [`SafetyFact`] outstanding and §56.3 blocks.
//!
//! # Where the calls go
//!
//! Each method makes a fixed sequence of `btrfs` calls, in a fixed order, so a recorded script
//! replays deterministically and a reader can see what a method costs:
//!
//! | method | calls |
//! |---|---|
//! | `availability` | `--version` |
//! | `resolve_domain` | `filesystem show`, `subvolume show`, `subvolume list` |
//! | `discover` | `subvolume list`, `filesystem usage` |
//! | `plan_protection` | none — §2.1 keeps planning side-effect free |
//! | `create` | `subvolume snapshot -r`, `subvolume show`, `property get … ro` |
//! | `validate` | `subvolume show` (snapshot), `property get … ro`, `subvolume show` (source) |
//! | `plan_recovery` | `subvolume show` (snapshot), `property get … ro`, `filesystem show`, `subvolume show` (source), `subvolume list`, `subvolume get-default` |
//! | `restore` | none, or `subvolume set-default` |
//! | `cleanup` | `subvolume delete` |
//! | `estimate_cost` | `filesystem usage` |
//!
//! # What it will not say
//!
//! §14.4 forbids presenting Btrfs as having a generic in-place rollback primitive, so no method
//! here produces [`RestoreMethod::DatasetRollback`] and no sentence it composes uses that word for
//! what Btrfs would do. The four methods it does produce — selective file restore, clone and copy,
//! subvolume replacement and offline root recovery — are §14.4's own list, and every fragment
//! declares which one it is.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ActionRole, ChangePlan, ConsistencyClass, EffectDomain, EquivalenceDomain, Execution,
    NewerStateClass, NewerStateImpact, NewerStateItem, PersistenceDomain, PlanAction, PlanId,
    ProtectionAction, ProtectionMode, ProviderAvailability, ProviderCapabilities, RecoveryAsset,
    RecoveryAssetType, RecoveryCandidate, RecoveryCapability, RecoveryCost, RecoveryExclusion,
    RecoveryGoal, RecoveryObjective, RecoveryPlanFragment, RecoveryProvider, RecoveryScope,
    RecoveryValidation, ResolvedMount, RestoreMethod, ToolOutput, ToolRunner, UnrecoverableEffect,
    VerificationClass, VerificationContract, error as core_error,
};
use ono_value::{ErrorValue, Value};

use crate::assets::{ProtectionShortfall, RecoveryAssetSet};
use crate::boundary::{RequiredProtection, SubvolumeBoundary, SubvolumeLayout};
use crate::config::{BtrfsConfig, RootRecovery, snapshot_name};
use crate::error::{
    PROVIDER_ID, command_failed, fact_not_established, no_btrfs_mount, recursive_snapshot_location,
    snapshot_failed,
};
use crate::files::{FileStore, SystemFiles};
use crate::mount::{BtrfsMount, BtrfsMounts};
use crate::newer::{classify_object, classify_subvolume};
use crate::parse::{
    BtrfsVersion, DefaultSubvolume, FilesystemInfo, FilesystemUsage, ShowOutcome, SubvolumeShow,
    parse_filesystem_show, parse_filesystem_usage, parse_get_default, parse_read_only_property,
    parse_subvolume_list, parse_version, read_subvolume_show, spoken_text,
};
use crate::safety::{SafetyChecklist, SafetyFact};
use crate::subvolume::SubvolumeRef;

/// The program every Btrfs operation runs through (§12.3).
pub const BTRFS: &str = "btrfs";

/// The `btrfs-progs` series this provider has been validated against (Appendix G.4).
///
/// Appendix G.4 requires a provider to degrade rather than execute semantics it has not tested,
/// so the list holds the versions the recorded fixtures were taken from and the ones the gated
/// real-filesystem suite has run against — nothing else. A version outside it makes the provider
/// [`ProviderAvailability::Unsupported`], which is a refusal a person can read and act on.
pub const VALIDATED_VERSIONS: &[&str] = &["6.16", "6.17"];

/// The persistence-object kind a Btrfs recovery scope carries (§11.2).
pub const SCOPE_KIND: &str = "btrfs-subvolume";

/// The argument naming the operation a recovery action performs.
pub const ARG_OPERATION: &str = "operation";

/// The argument naming what a recovery action reads from.
pub const ARG_SOURCE: &str = "source";

/// The argument naming what a recovery action writes to.
pub const ARG_DESTINATION: &str = "destination";

/// The argument naming the subvolume a recovery action acts on.
pub const ARG_SUBVOLUME: &str = "subvolume";

/// The argument naming the subvolume id a recovery action acts on.
pub const ARG_SUBVOLUME_ID: &str = "subvolume-id";

/// The argument naming the mount a recovery action acts through.
pub const ARG_MOUNT: &str = "mount";

/// Copy one object out of the read-only snapshot into the live subvolume (§13.5).
pub const OP_RESTORE_FILE: &str = "restore-file";

/// Create a writable subvolume from the read-only snapshot (§14.5, Appendix D.9).
pub const OP_DERIVE_WRITABLE: &str = "derive-writable-subvolume";

/// Point the next boot at a subvolume (Appendix D.9).
pub const OP_SET_DEFAULT: &str = "set-default-subvolume";

/// Put a derived subvolume in place of the live one (§14.4).
pub const OP_REPLACE_SUBVOLUME: &str = "replace-subvolume";

/// The suffix a derived writable subvolume's name carries (§14.5).
pub const DERIVED_SUFFIX: &str = "-rw";

/// The suffix the subvolume being replaced is moved aside under (§14.4).
///
/// The live subvolume is renamed rather than deleted, so a replacement that turns out to have
/// been the wrong idea still has the state it displaced. §2.15 and Appendix F both prefer keeping
/// the evidence over a tidy tree.
pub const SUPERSEDED_SUFFIX: &str = ".ono-superseded";

/// The first-party Btrfs recovery provider (§14).
#[derive(Debug, Clone)]
pub struct BtrfsProvider {
    runner: Arc<dyn ToolRunner>,
    files: Arc<dyn FileStore>,
    mounts: Option<BtrfsMounts>,
    config: BtrfsConfig,
    host: Arc<str>,
    program: Arc<str>,
    plan: Option<PlanId>,
    instant: Timestamp,
}

impl BtrfsProvider {
    /// A provider that runs `btrfs` through `runner` (§12.3).
    ///
    /// The mount table is read from `/proc/self/mountinfo` on first use unless
    /// [`BtrfsProvider::with_mounts`] supplied one, and file content is read and copied through
    /// the real filesystem unless [`BtrfsProvider::with_files`] supplied a store.
    #[must_use]
    pub fn new(runner: Arc<dyn ToolRunner>) -> Self {
        Self {
            runner,
            files: Arc::new(SystemFiles),
            mounts: None,
            config: BtrfsConfig::default(),
            host: Arc::from("localhost"),
            program: Arc::from(BTRFS),
            plan: None,
            instant: Timestamp::UNIX_EPOCH,
        }
    }

    /// Resolves paths against `mounts` rather than against `/proc/self/mountinfo`.
    #[must_use]
    pub fn with_mounts(mut self, mounts: BtrfsMounts) -> Self {
        self.mounts = Some(mounts);
        self
    }

    /// Reads and copies file content through `files` (§13.5).
    #[must_use]
    pub fn with_files(mut self, files: Arc<dyn FileStore>) -> Self {
        self.files = files;
        self
    }

    /// Applies the `[recovery.btrfs]` settings of §53.
    #[must_use]
    pub fn with_config(mut self, config: BtrfsConfig) -> Self {
        self.config = config;
        self
    }

    /// Names the host the scopes belong to (§11.2).
    #[must_use]
    pub fn on_host(mut self, host: impl Into<Arc<str>>) -> Self {
        self.host = host.into();
        self
    }

    /// Runs a program other than plain `btrfs` — an absolute path, or a test double.
    #[must_use]
    pub fn with_program(mut self, program: impl Into<Arc<str>>) -> Self {
        self.program = program.into();
        self
    }

    /// Attributes the assets this provider proposes to `plan` (§11.1's `source_plan`).
    ///
    /// The plan id is also what makes a snapshot name predictable and attributable: the fixtures'
    /// `ono-a82f-var` is plan `a82f`'s snapshot of `@var` (§37, §43.6, Appendix D.8).
    #[must_use]
    pub fn for_plan(mut self, plan: PlanId) -> Self {
        self.plan = Some(plan);
        self
    }

    /// Sets the instant the provider stamps proposals and validations with.
    ///
    /// The provider takes its clock from the caller rather than reading one, so a plan is a
    /// deterministic function of its inputs. The instant a snapshot *exists* at is never this one:
    /// that is read back from the filesystem's own record of when it was created (Appendix D.7).
    #[must_use]
    pub const fn at_instant(mut self, instant: Timestamp) -> Self {
        self.instant = instant;
        self
    }

    /// The settings in force (§53).
    #[must_use]
    pub const fn config(&self) -> &BtrfsConfig {
        &self.config
    }

    /// The mount table resolutions are performed against (Appendix B.1).
    ///
    /// # Errors
    ///
    /// A structured error when no table was supplied and `/proc/self/mountinfo` cannot be read.
    pub fn mounts(&self) -> Result<Cow<'_, BtrfsMounts>, ErrorValue> {
        match &self.mounts {
            Some(mounts) => Ok(Cow::Borrowed(mounts)),
            None => BtrfsMounts::from_proc().map(Cow::Owned),
        }
    }

    /// Runs one `btrfs` subcommand (§12.3).
    ///
    /// # Errors
    ///
    /// A structured error when the program could not be run at all. A program that ran and failed
    /// comes back as a [`ToolOutput`], because what it printed is usually the answer.
    pub fn btrfs(&self, argv: &[&str]) -> Result<ToolOutput, ErrorValue> {
        self.runner.run(&self.program, argv)
    }

    /// The version of `btrfs-progs` in use (Appendix G.4).
    ///
    /// # Errors
    ///
    /// A structured error when `btrfs --version` could not be run or could not be read.
    pub fn version(&self) -> Result<BtrfsVersion, ErrorValue> {
        let output = self.btrfs(&["--version"])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs --version",
                spoken_text(&output).trim(),
            ));
        }
        parse_version(spoken_text(&output))
    }

    /// Every subvolume of the filesystem `mount` belongs to, as boundaries (§14.3).
    ///
    /// This is the query §14.3 turns on. Each entry is a place a snapshot stops, and the flags
    /// asked for — `-a -p -u -q -R` — are what make the answer complete: every subvolume rather
    /// than only those below the mount, with the parent id, the uuid and the lineage that ties a
    /// snapshot to what it was taken from.
    ///
    /// # Errors
    ///
    /// A structured error when the listing failed — including the unprivileged case, where
    /// `btrfs` answers `ERROR: can't perform the search: Operation not permitted`. §56.3 makes
    /// that a refusal: a boundary list nobody could read is not an empty boundary list.
    pub fn boundaries(&self, mount: &Path) -> Result<Vec<SubvolumeBoundary>, ErrorValue> {
        let mount_text = mount.to_string_lossy().into_owned();
        let output = self.btrfs(&[
            "subvolume",
            "list",
            "-a",
            "-p",
            "-u",
            "-q",
            "-R",
            &mount_text,
        ])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs subvolume list",
                spoken_text(&output).trim(),
            ));
        }
        Ok(parse_subvolume_list(spoken_text(&output))?
            .iter()
            .map(SubvolumeBoundary::from_entry)
            .collect())
    }

    /// The subvolumes and the mounts that reach them, together (§14.3, Appendix B.9).
    ///
    /// # Errors
    ///
    /// A structured error when either half could not be established.
    pub fn layout(&self, mount: &Path) -> Result<SubvolumeLayout, ErrorValue> {
        let boundaries = self.boundaries(mount)?;
        Ok(SubvolumeLayout::new(
            boundaries,
            self.mounts()?.into_owned(),
        ))
    }

    /// The subvolumes a plan changing `paths` must snapshot, each separately (§14.3, §59.3).
    ///
    /// # Errors
    ///
    /// A structured error when the boundaries could not be listed.
    pub fn required_protection(
        &self,
        mount: &Path,
        paths: &[&Path],
    ) -> Result<RequiredProtection, ErrorValue> {
        Ok(self.layout(mount)?.required_for(paths))
    }

    /// What `btrfs filesystem show` says about the filesystem at `mount` (Appendix D.6).
    ///
    /// # Errors
    ///
    /// A structured error when the command failed or its output carried no filesystem UUID.
    pub fn filesystem_info(&self, mount: &Path) -> Result<FilesystemInfo, ErrorValue> {
        let mount_text = mount.to_string_lossy().into_owned();
        let output = self.btrfs(&["filesystem", "show", &mount_text])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs filesystem show",
                spoken_text(&output).trim(),
            ));
        }
        parse_filesystem_show(spoken_text(&output))
    }

    /// What `btrfs filesystem usage` says about space at `mount` (§38).
    ///
    /// # Errors
    ///
    /// A structured error when the command failed or its output could not be read.
    pub fn filesystem_usage(&self, mount: &Path) -> Result<FilesystemUsage, ErrorValue> {
        let mount_text = mount.to_string_lossy().into_owned();
        let output = self.btrfs(&["filesystem", "usage", &mount_text])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs filesystem usage",
                spoken_text(&output).trim(),
            ));
        }
        parse_filesystem_usage(spoken_text(&output))
    }

    /// What `btrfs subvolume show` says about `path` (§14.1, Appendix B.9).
    ///
    /// # Errors
    ///
    /// A structured error when the command could not be run or printed unreadable output. A path
    /// that is a plain directory, or is not there at all, is an outcome rather than an error.
    pub fn show(&self, path: &Path) -> Result<ShowOutcome, ErrorValue> {
        let path_text = path.to_string_lossy().into_owned();
        let output = self.btrfs(&["subvolume", "show", &path_text])?;
        read_subvolume_show(&output)
    }

    /// Whether the subvolume at `path` carries the read-only flag (§14.5).
    ///
    /// # Errors
    ///
    /// A structured error when the property could not be read. §14.5 asks for the flag to be
    /// verified rather than assumed, and an unread flag is not `true`.
    pub fn is_read_only(&self, path: &Path) -> Result<bool, ErrorValue> {
        let path_text = path.to_string_lossy().into_owned();
        let output = self.btrfs(&["property", "get", &path_text, "ro"])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs property get",
                spoken_text(&output).trim(),
            ));
        }
        parse_read_only_property(spoken_text(&output))
    }

    /// Which subvolume the filesystem at `mount` mounts by default (§56.2, Appendix D.9).
    ///
    /// # Errors
    ///
    /// A structured error when the default could not be read, which leaves §56.2's
    /// default-subvolume and boot impact fact outstanding.
    pub fn default_subvolume(&self, mount: &Path) -> Result<DefaultSubvolume, ErrorValue> {
        let mount_text = mount.to_string_lossy().into_owned();
        let output = self.btrfs(&["subvolume", "get-default", &mount_text])?;
        if !output.succeeded() {
            return Err(command_failed(
                "btrfs subvolume get-default",
                spoken_text(&output).trim(),
            ));
        }
        parse_get_default(spoken_text(&output))
    }

    /// Creates a writable subvolume from a read-only snapshot, tracked as its own asset (§14.5).
    ///
    /// §14.5 is explicit: *"If recovery requires a writable clone/subvolume derived from them,
    /// that derived object MUST be tracked separately."* So this returns a second
    /// [`RecoveryAsset`] that depends on the first rather than changing it, and the read-only
    /// snapshot it came from stays read-only and stays retained.
    ///
    /// # Errors
    ///
    /// A structured error when the derived subvolume could not be created or read back.
    pub fn derive_writable(
        &self,
        asset: &RecoveryAsset,
        destination: &Path,
    ) -> Result<RecoveryAsset, ErrorValue> {
        let destination_text = destination.to_string_lossy().into_owned();
        let output = self.btrfs(&["subvolume", "snapshot", asset.reference(), &destination_text])?;
        if !output.succeeded() {
            return Err(snapshot_failed(
                asset.scope().domain(),
                spoken_text(&output).trim(),
            ));
        }
        let derived = match self.show(destination)? {
            ShowOutcome::Subvolume(show) => show,
            other => {
                return Err(snapshot_failed(
                    asset.scope().domain(),
                    &format!(
                        "the derived subvolume at {destination_text} could not be read back: \
                         {other:?}"
                    ),
                ));
            }
        };
        let source = SubvolumeRef::parse(asset.scope().domain());
        let filesystem = source.as_ref().map_or("unknown", SubvolumeRef::filesystem);
        let scope = RecoveryScope::new(
            SCOPE_KIND,
            SubvolumeRef::new(filesystem, derived.id(), derived.tree_path()).reference(),
            Arc::clone(&self.host),
        )
        .covering(destination_text.clone());
        let mut writable = RecoveryAsset::proposed(
            PROVIDER_ID,
            RecoveryAssetType::BtrfsSnapshot,
            destination_text,
            scope,
            derived.created_at().unwrap_or(self.instant),
        );
        if let Some(plan) = asset.source_plan() {
            writable = writable.for_plan(plan.clone());
        }
        Ok(writable
            .depending_on(asset.id().clone())
            .at_consistency(asset.consistency())
            .restored_by(RestoreMethod::CloneAndCopy)
            .costing(snapshot_cost())
            .creating()
            .excluding(RecoveryExclusion::new(
                format!("the read-only snapshot {} it came from", asset.reference()),
                "§14.5: a writable subvolume derived from a retained snapshot is tracked as its \
                 own asset, because what it holds stops being the captured state the moment \
                 anything writes to it",
            )))
    }

    /// Creates every snapshot a plan's protection needs, in order (Appendix D.7, F.1).
    ///
    /// # Errors
    ///
    /// A [`ProtectionShortfall`] when one of them fails. It carries the snapshots that were
    /// already created, because Appendix F.1 keeps them until a cleanup decision is made — and
    /// §2.3 forbids mutating any plan target either way.
    pub fn create_set(
        &self,
        actions: &[ProtectionAction],
    ) -> Result<RecoveryAssetSet, ProtectionShortfall> {
        let mut created = Vec::new();
        for action in actions {
            match self.create(action) {
                Ok(asset) => created.push(asset),
                Err(error) => {
                    return Err(ProtectionShortfall::new(
                        created,
                        action.candidate().scope().domain(),
                        error,
                    ));
                }
            }
        }
        Ok(RecoveryAssetSet::sequential(created))
    }

    /// Turns candidates into protection actions for `plan`, stamped at `at` (§12.1, Appendix D.6).
    ///
    /// # Errors
    ///
    /// A structured error when Appendix D.8's location rule is violated, when the recovery
    /// namespace is not reachable, or when a candidate's scope does not name a subvolume.
    pub fn plan_protection_for(
        &self,
        candidates: &[RecoveryCandidate],
        mode: ProtectionMode,
        plan: Option<&PlanId>,
        at: Timestamp,
    ) -> Result<Vec<ProtectionAction>, ErrorValue> {
        if !mode.creates_assets() {
            return Ok(Vec::new());
        }
        let mounts = self.mounts()?;
        let mut actions = Vec::new();
        for candidate in candidates {
            let Some(reference) = SubvolumeRef::parse(candidate.scope().domain()) else {
                return Err(fact_not_established(
                    SafetyFact::FilesystemAndSubvolumeId,
                    &format!(
                        "the candidate scope `{}` does not name a Btrfs subvolume, so nothing \
                         could be snapshotted from it",
                        candidate.scope().domain()
                    ),
                ));
            };
            if self.config.is_nested_in(reference.tree_path()) {
                return Err(recursive_snapshot_location(
                    self.config.snapshot_location(),
                    reference.tree_path(),
                ));
            }
            let destination = self.snapshot_destination(&mounts, plan, &reference)?;
            let destination_text = destination.to_string_lossy().into_owned();
            let mut asset = RecoveryAsset::proposed(
                PROVIDER_ID,
                RecoveryAssetType::BtrfsSnapshot,
                destination_text.clone(),
                candidate.scope().clone(),
                at,
            );
            if let Some(plan) = plan {
                asset = asset.for_plan(plan.clone());
            }
            for exclusion in candidate.exclusions() {
                asset = asset.excluding(exclusion.clone());
            }
            let asset = asset
                .at_consistency(candidate.consistency())
                .restored_by(candidate.restore_method())
                .costing(candidate.cost().clone());
            let summary = format!(
                "snapshot {} to {destination_text} ({})",
                reference.describe(),
                if self.config.prefers_read_only_snapshots() {
                    "read-only"
                } else {
                    "writable"
                }
            );
            actions.push(ProtectionAction::new(
                PROVIDER_ID,
                summary,
                candidate.clone(),
                asset,
            ));
        }
        Ok(actions)
    }

    /// Reads §56.2's ten facts, and blocks where one is missing (§56.3).
    ///
    /// The checklist comes back beside the fragment so a caller can show what was proven before
    /// executing anything (§14.6's "It MUST be shown before execution").
    ///
    /// # Errors
    ///
    /// [`ono_change_core::error::recovery_plan_incomplete`], naming the first fact that could not
    /// be established.
    pub fn plan_recovery_with_checklist(
        &self,
        asset: &RecoveryAsset,
        source: Option<&ChangePlan>,
        goal: RecoveryGoal,
    ) -> Result<(RecoveryPlanFragment, SafetyChecklist), ErrorValue> {
        let mut checklist = SafetyChecklist::outstanding();
        let mounts = self.mounts()?;
        let snapshot_path = PathBuf::from(asset.reference());

        let Some(reference) = SubvolumeRef::parse(asset.scope().domain()) else {
            checklist.block(
                SafetyFact::FilesystemAndSubvolumeId,
                format!(
                    "the asset's scope `{}` does not name a filesystem and a subvolume id",
                    asset.scope().domain()
                ),
            );
            return Err(refusal(&checklist));
        };
        let Some((live_path, live_mount)) = self.live_path(&mounts, &reference) else {
            checklist.block(
                SafetyFact::MountAndRebootRequirement,
                format!(
                    "{} is not mounted anywhere this process can see, so whether recovery needs a \
                     remount, an offline window or a reboot could not be established",
                    reference.describe()
                ),
            );
            return Err(refusal(&checklist));
        };
        let query_mount = PathBuf::from(live_mount.mount_point());

        // §56.2's second and ninth facts: the snapshot exists, and it is still read-only.
        let snapshot = match self.show(&snapshot_path)? {
            ShowOutcome::Subvolume(show) => {
                checklist.establish(
                    SafetyFact::SnapshotExistsAndValid,
                    format!(
                        "the recovery point at {} is subvolume {} with uuid {}, created {}",
                        asset.reference(),
                        show.id(),
                        show.uuid(),
                        show.created_at()
                            .map_or_else(|| "at an unrecorded time".to_owned(), |at| at.to_string())
                    ),
                );
                show
            }
            outcome => {
                checklist.block(
                    SafetyFact::SnapshotExistsAndValid,
                    format!(
                        "`btrfs subvolume show {}` reports no subvolume ({}), so the recovery \
                         point could not be confirmed to exist",
                        asset.reference(),
                        describe_outcome(&outcome)
                    ),
                );
                return Err(refusal(&checklist));
            }
        };
        self.check_read_only(&mut checklist, &snapshot_path);

        self.plan_recovery_from(
            checklist,
            asset,
            source,
            goal,
            &reference,
            &snapshot,
            &live_path,
            &live_mount,
            &query_mount,
            &mounts,
        )
    }

    /// Which of §14.4's methods a recovery would be (§14.4, §14.6, Appendix C.1).
    ///
    /// Appendix C.1's least-destructive principle decides the first branch: where the goal is to
    /// put named objects back and they can be read out of the snapshot, that is what happens,
    /// whatever the root policy says, because it touches nothing else and needs no reboot. The
    /// root policy of §53 governs the case where a whole subvolume has to come back, and it
    /// selects between §14.6's three: an online selective restore, an offline subvolume
    /// replacement, or a next-boot switch.
    ///
    /// `None` means no method this provider has can achieve the goal, which §56.3 makes a block
    /// rather than a fall back to the biggest hammer.
    #[must_use]
    pub fn method_for(
        &self,
        goal: RecoveryGoal,
        is_root: bool,
        restore_set: &[PathBuf],
    ) -> Option<RestoreMethod> {
        let selective = !restore_set.is_empty();
        match goal {
            RecoveryGoal::RestoreChangedObjects | RecoveryGoal::RestoreServiceHealth => {
                selective.then_some(RestoreMethod::SelectiveFileRestore)
            }
            RecoveryGoal::CompensateSemantics => None,
            RecoveryGoal::RestoreDomain if is_root => match self.config.root_recovery() {
                RootRecovery::NextBoot => Some(RestoreMethod::OfflineRootRecovery),
                RootRecovery::OfflineSubvolumeReplacement => {
                    Some(RestoreMethod::SubvolumeReplacement)
                }
                RootRecovery::OnlineSelectiveRestore => {
                    selective.then_some(RestoreMethod::CloneAndCopy)
                }
            },
            RecoveryGoal::RestoreDomain if self.config.permits_offline_replacement() => {
                Some(RestoreMethod::SubvolumeReplacement)
            }
            RecoveryGoal::RestoreDomain => selective.then_some(RestoreMethod::CloneAndCopy),
        }
    }

    /// Which of §14.6's three root workflows `method` is, where the subvolume is the root.
    #[must_use]
    pub const fn root_workflow(method: RestoreMethod) -> Option<RootRecovery> {
        match method {
            RestoreMethod::SelectiveFileRestore | RestoreMethod::CloneAndCopy => {
                Some(RootRecovery::OnlineSelectiveRestore)
            }
            RestoreMethod::SubvolumeReplacement => {
                Some(RootRecovery::OfflineSubvolumeReplacement)
            }
            RestoreMethod::OfflineRootRecovery => Some(RootRecovery::NextBoot),
            _ => None,
        }
    }

    /// Where the snapshot of `reference` goes (Appendix D.8).
    fn snapshot_destination(
        &self,
        mounts: &BtrfsMounts,
        plan: Option<&PlanId>,
        reference: &SubvolumeRef,
    ) -> Result<PathBuf, ErrorValue> {
        let name = snapshot_name(
            plan.map_or("ono", |plan| plan.short()),
            reference.tree_path(),
        );
        Ok(self.recovery_namespace(mounts, reference.tree_path())?.join(name))
    }

    /// Where the recovery namespace is visible in this mount namespace (Appendix D.8).
    fn recovery_namespace(
        &self,
        mounts: &BtrfsMounts,
        subject: &str,
    ) -> Result<PathBuf, ErrorValue> {
        let location = self.config.snapshot_location();
        mounts
            .mounts()
            .iter()
            .filter(|mount| !mount.is_read_only())
            .find_map(|mount| mount.visible_path(location))
            .ok_or_else(|| {
                core_error::asset_create_failed(
                    PROVIDER_ID,
                    subject,
                    &format!(
                        "the recovery namespace {location} is not reachable through any writable \
                         mount of this filesystem, so a snapshot could not be placed on the same \
                         Btrfs filesystem as its source (Appendix D.8). Nothing was changed"
                    ),
                )
            })
    }

    /// Where a subvolume is visible now, and through which mount (§14.6, §56.2).
    fn live_path(
        &self,
        mounts: &BtrfsMounts,
        reference: &SubvolumeRef,
    ) -> Option<(PathBuf, BtrfsMount)> {
        mounts
            .mounts()
            .iter()
            .find(|mount| {
                mount.subvolume_id() == Some(reference.id())
                    || mount.tree_path() == reference.tree_path()
            })
            .and_then(|mount| {
                mount
                    .visible_path(reference.tree_path())
                    .map(|path| (path, mount.clone()))
            })
    }

    /// The candidate a snapshot of `boundary` would be (§11.1, §14.3, §14.7).
    fn candidate_for(
        &self,
        layout: &SubvolumeLayout,
        filesystem: &str,
        boundary: &SubvolumeBoundary,
        covers: &[&str],
        objective: RecoveryObjective,
        usage: FilesystemUsage,
    ) -> RecoveryCandidate {
        let reference = SubvolumeRef::new(filesystem, boundary.id(), boundary.tree_path());
        let mut scope =
            RecoveryScope::new(SCOPE_KIND, reference.reference(), Arc::clone(&self.host));
        for object in covers {
            scope = scope.covering(*object);
        }
        let nested = layout.nested_within(boundary);
        let is_root = layout
            .mount_for(boundary)
            .is_some_and(|mount| layout.mounts().is_root_subvolume(mount));
        let detail = format!(
            "a read-only Btrfs snapshot of {}. It shares extents with the live subvolume and lives \
             on the same filesystem and the same devices, so it is a local recovery point and \
             shares the storage failure domain of what it protects — it is not a backup \
             (§14.7){}",
            reference.describe(),
            if nested.is_empty() {
                String::new()
            } else {
                format!(
                    ". It does not contain the {} nested inside it, each of which needs its own \
                     snapshot (§14.3)",
                    nested
                        .iter()
                        .map(|nested| format!("{} ({})", nested.tree_path(), nested.id()))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        );
        let mut cost = snapshot_cost();
        if is_root && self.config.root_recovery().requires_reboot() {
            cost = cost.needing_reboot();
        }
        if is_root && self.config.root_recovery().requires_offline() {
            cost = cost.needing_offline();
        }
        let mut candidate = RecoveryCandidate::new(
            PROVIDER_ID,
            scope,
            EffectDomain::FilesystemPersistent,
            objective,
            detail,
        )
        .at_consistency(ConsistencyClass::FilesystemConsistent)
        .restored_by(RestoreMethod::SelectiveFileRestore)
        .costing(cost)
        .excluding(RecoveryExclusion::new(
            "recovery from the loss of the filesystem or its devices",
            "§14.7: the snapshot shares the same filesystem and storage failure domain as what it \
             protects. Losing the devices loses both, and only a separate backup provider covers \
             that",
        ));
        for nested in nested {
            candidate = candidate.excluding(nested_exclusion(nested));
        }
        candidate = candidate
            .needing_to_create(
                "the privilege to create a subvolume snapshot; an unprivileged caller is refused \
                 with `Operation not permitted` (§43.4)",
            )
            .needing_to_restore(restore_requirement(is_root, self.config.root_recovery()));
        if let Some(free) = usage.free_estimated() {
            candidate = candidate.needing_to_create(format!(
                "room on the filesystem, which reports {free} free — Btrfs's own estimate rather \
                 than an exact figure (§37.5)"
            ));
        }
        candidate
    }

    /// §56.2's ninth fact (§14.5).
    fn check_read_only(&self, checklist: &mut SafetyChecklist, snapshot_path: &Path) {
        match self.is_read_only(snapshot_path) {
            Ok(true) => checklist.establish(
                SafetyFact::ReadOnlySnapshotTreatment,
                format!(
                    "`btrfs property get {} ro` answers ro=true, so the retained snapshot is \
                     unchanged since it was taken; anything writable derived from it is tracked \
                     as its own asset (§14.5)",
                    snapshot_path.display()
                ),
            ),
            Ok(false) => checklist.block(
                SafetyFact::ReadOnlySnapshotTreatment,
                format!(
                    "the snapshot at {} is writable, so what it holds is no longer provably the \
                     state that was captured (§14.5)",
                    snapshot_path.display()
                ),
            ),
            Err(error) => checklist.block(
                SafetyFact::ReadOnlySnapshotTreatment,
                format!(
                    "the read-only flag of {} could not be read: {}",
                    snapshot_path.display(),
                    error.message()
                ),
            ),
        }
    }

    /// The rest of §56.2's checklist, and the fragment it permits (§14.4, §14.6).
    #[allow(clippy::too_many_arguments)]
    fn plan_recovery_from(
        &self,
        mut checklist: SafetyChecklist,
        asset: &RecoveryAsset,
        source: Option<&ChangePlan>,
        goal: RecoveryGoal,
        reference: &SubvolumeRef,
        snapshot: &SubvolumeShow,
        live_path: &Path,
        live_mount: &BtrfsMount,
        query_mount: &Path,
        mounts: &BtrfsMounts,
    ) -> Result<(RecoveryPlanFragment, SafetyChecklist), ErrorValue> {
        // §56.2's first fact: the exact filesystem, and the subvolume id from real metadata.
        let live_show = self.check_identity(&mut checklist, reference, live_path, query_mount)?;

        // §56.2's third and tenth facts: the nested boundaries, and no assumption about them.
        let layout = self.check_boundaries(&mut checklist, asset, reference, query_mount, mounts);

        // §56.2's fourth fact: whether a selected restore is possible.
        let restore_set = self.restore_set(asset, live_path, layout.as_ref(), &mut checklist);

        // §56.2's fifth fact: whether replacement is required, and whether the snapshot could do
        // it — which is a question about lineage, not about the operator's preference.
        let is_root = mounts.is_root_subvolume(live_mount);
        let method = self.method_for(goal, is_root, &restore_set);
        self.check_replacement(
            &mut checklist,
            asset,
            reference,
            snapshot,
            live_show.as_deref(),
            method,
            goal,
        );

        // §56.2's sixth fact: the default subvolume and the boot impact.
        match self.default_subvolume(query_mount) {
            Ok(default) => checklist.establish(
                SafetyFact::DefaultSubvolumeAndBootImpact,
                default_subvolume_evidence(&default, reference, method),
            ),
            Err(error) => checklist.block(
                SafetyFact::DefaultSubvolumeAndBootImpact,
                format!(
                    "the default subvolume of {} could not be read, so the boot impact of this \
                     recovery is unknown: {}",
                    query_mount.display(),
                    error.message()
                ),
            ),
        }

        // §56.2's seventh fact: the mount and reboot requirement.
        checklist.establish(
            SafetyFact::MountAndRebootRequirement,
            mount_evidence(
                live_mount,
                live_path,
                is_root,
                method,
                self.config.root_recovery(),
            ),
        );

        // §56.2's eighth fact: the later state this method would discard.
        let newer = self.newer_state(
            &mut checklist,
            method,
            &restore_set,
            reference,
            asset.reference(),
            live_path,
            snapshot,
            live_show.as_deref(),
        );

        let Some(method) = method else {
            return Err(refusal(&checklist));
        };
        if !checklist.is_complete() {
            return Err(refusal(&checklist));
        }

        let plan_id = source.map_or_else(
            || PlanId::of(PROVIDER_ID, asset.reference(), goal.as_str()),
            |plan| plan.id().clone(),
        );
        let fragment = self.fragment(
            &plan_id,
            asset,
            method,
            is_root,
            &restore_set,
            reference,
            live_path,
            live_mount,
            layout.as_ref(),
            mounts,
            newer,
        );
        Ok((fragment, checklist))
    }

    /// §56.2's first fact (§14.1, Appendix B.9).
    fn check_identity(
        &self,
        checklist: &mut SafetyChecklist,
        reference: &SubvolumeRef,
        live_path: &Path,
        query_mount: &Path,
    ) -> Result<Option<Box<SubvolumeShow>>, ErrorValue> {
        let info = match self.filesystem_info(query_mount) {
            Ok(info) => info,
            Err(error) => {
                checklist.block(
                    SafetyFact::FilesystemAndSubvolumeId,
                    format!(
                        "the filesystem at {} could not be identified: {}",
                        query_mount.display(),
                        error.message()
                    ),
                );
                return Ok(None);
            }
        };
        if info.uuid() != reference.filesystem() {
            checklist.block(
                SafetyFact::FilesystemAndSubvolumeId,
                format!(
                    "the filesystem at {} is {} and the asset names {}, so the subvolume id \
                     belongs to a different filesystem",
                    query_mount.display(),
                    info.uuid(),
                    reference.filesystem()
                ),
            );
            return Ok(None);
        }
        match self.show(live_path)? {
            ShowOutcome::Subvolume(show) if show.id() == reference.id() => {
                checklist.establish(
                    SafetyFact::FilesystemAndSubvolumeId,
                    format!(
                        "filesystem {} carries subvolume {} at {}, both read from `btrfs \
                         filesystem show` and `btrfs subvolume show` rather than from the shape \
                         of the path (Appendix B.9)",
                        info.uuid(),
                        show.id(),
                        live_path.display()
                    ),
                );
                Ok(Some(show))
            }
            outcome => {
                checklist.block(
                    SafetyFact::FilesystemAndSubvolumeId,
                    format!(
                        "{} does not report subvolume {}: {}",
                        live_path.display(),
                        reference.id(),
                        describe_outcome(&outcome)
                    ),
                );
                Ok(None)
            }
        }
    }

    /// §56.2's third and tenth facts (§14.3).
    fn check_boundaries(
        &self,
        checklist: &mut SafetyChecklist,
        asset: &RecoveryAsset,
        reference: &SubvolumeRef,
        query_mount: &Path,
        mounts: &BtrfsMounts,
    ) -> Option<SubvolumeLayout> {
        let boundaries = match self.boundaries(query_mount) {
            Ok(boundaries) => boundaries,
            Err(error) => {
                checklist.block(
                    SafetyFact::NestedBoundaries,
                    format!(
                        "the subvolumes of {} could not be listed, so where a snapshot of {} stops \
                         is unknown: {}",
                        query_mount.display(),
                        reference.tree_path(),
                        error.message()
                    ),
                );
                return None;
            }
        };
        let layout = SubvolumeLayout::new(boundaries, mounts.clone());
        let Some(boundary) = layout.by_id(reference.id()) else {
            checklist.block(
                SafetyFact::NestedBoundaries,
                format!(
                    "subvolume {} does not appear in `btrfs subvolume list` for {}, so the \
                     boundaries around it could not be established",
                    reference.id(),
                    query_mount.display()
                ),
            );
            return None;
        };
        let nested = layout.nested_within(boundary);
        checklist.establish(
            SafetyFact::NestedBoundaries,
            if nested.is_empty() {
                format!(
                    "no subvolume is nested inside {}, so a snapshot of it stops at the subvolume \
                     itself",
                    reference.tree_path()
                )
            } else {
                format!(
                    "{} nested inside {}, and a snapshot of the parent holds an empty directory \
                     where each one is mounted (§14.3)",
                    nested
                        .iter()
                        .map(|nested| format!("{} {}", nested.id(), nested.tree_path()))
                        .collect::<Vec<_>>()
                        .join(", "),
                    reference.tree_path()
                )
            },
        );
        let unstated: Vec<&str> = nested
            .iter()
            .filter(|nested| {
                !asset
                    .exclusions()
                    .iter()
                    .any(|exclusion| exclusion.subject().contains(nested.tree_path()))
            })
            .map(|nested| nested.tree_path())
            .collect();
        if unstated.is_empty() {
            checklist.establish(
                SafetyFact::NoRecursiveCoverageAssumption,
                format!(
                    "every subvolume nested inside {} is recorded on the asset as something it \
                     does not hold, so nothing treats this snapshot as recursive (§14.3)",
                    reference.tree_path()
                ),
            );
        } else {
            checklist.block(
                SafetyFact::NoRecursiveCoverageAssumption,
                format!(
                    "the asset says nothing about {}, which is nested inside {} and is therefore \
                     an empty directory inside the snapshot; treating the snapshot as covering it \
                     is the recursive assumption §14.3 forbids",
                    unstated.join(", "),
                    reference.tree_path()
                ),
            );
        }
        Some(layout)
    }

    /// §56.2's fifth fact (§14.4).
    #[allow(clippy::too_many_arguments)]
    fn check_replacement(
        &self,
        checklist: &mut SafetyChecklist,
        asset: &RecoveryAsset,
        reference: &SubvolumeRef,
        snapshot: &SubvolumeShow,
        live: Option<&SubvolumeShow>,
        method: Option<RestoreMethod>,
        goal: RecoveryGoal,
    ) {
        let Some(method) = method else {
            checklist.block(
                SafetyFact::SubvolumeReplacementRequired,
                format!(
                    "no method this provider offers achieves the goal `{}` for {} under the \
                     configured root policy `{}`, so whether the subvolume would have to be \
                     replaced could not be answered",
                    goal.as_str(),
                    reference.tree_path(),
                    self.config.root_recovery().token()
                ),
            );
            return;
        };
        match (snapshot.parent_uuid(), live) {
            (Some(parent), Some(live)) if parent == live.uuid() => checklist.establish(
                SafetyFact::SubvolumeReplacementRequired,
                format!(
                    "the recovery point's parent uuid {parent} is subvolume {}'s own uuid, so it \
                     is a snapshot of it and could stand in its place; {}",
                    reference.id(),
                    replacement_sentence(method)
                ),
            ),
            (Some(parent), _) => checklist.block(
                SafetyFact::SubvolumeReplacementRequired,
                format!(
                    "the recovery point's parent uuid is {parent} and subvolume {}'s own uuid \
                     could not be matched against it, so what putting it in place would produce \
                     is unknown",
                    reference.id()
                ),
            ),
            (None, _) => checklist.block(
                SafetyFact::SubvolumeReplacementRequired,
                format!(
                    "the object at {} carries no parent uuid, so it is not a snapshot of \
                     subvolume {} and could not stand in its place",
                    asset.reference(),
                    reference.id()
                ),
            ),
        }
    }

    /// §56.2's fourth fact: the objects a selective restore would put back (Appendix C.1).
    fn restore_set(
        &self,
        asset: &RecoveryAsset,
        live_path: &Path,
        layout: Option<&SubvolumeLayout>,
        checklist: &mut SafetyChecklist,
    ) -> Vec<PathBuf> {
        let covered: Vec<PathBuf> = asset
            .scope()
            .covers()
            .iter()
            .map(|object| PathBuf::from(object.as_ref()))
            .collect();
        if covered.is_empty() {
            checklist.block(
                SafetyFact::SelectiveRestorePossible,
                "the asset names no object it covers, so whether the wanted state could be put \
                 back one object at a time could not be established"
                    .to_owned(),
            );
            return covered;
        }
        let mut beyond: Vec<String> = Vec::new();
        for object in &covered {
            if !object.starts_with(live_path) {
                beyond.push(format!(
                    "{} is outside {}, which is what this snapshot holds",
                    object.display(),
                    live_path.display()
                ));
                continue;
            }
            if let Some(layout) = layout
                && let Some(boundary) = layout.containing(object)
                && layout
                    .visible_path(boundary)
                    .is_some_and(|path| path != live_path)
            {
                beyond.push(format!(
                    "{} lies in the nested subvolume {} {}, which a snapshot of the parent does \
                     not contain — inside the snapshot that path is an empty directory (§14.3)",
                    object.display(),
                    boundary.id(),
                    boundary.tree_path()
                ));
            }
        }
        if beyond.is_empty() {
            checklist.establish(
                SafetyFact::SelectiveRestorePossible,
                format!(
                    "all {} covered object(s) lie inside the snapshotted subvolume and can be read \
                     out of the snapshot one at a time",
                    covered.len()
                ),
            );
        } else {
            checklist.block(SafetyFact::SelectiveRestorePossible, beyond.join("; "));
        }
        covered
    }

    /// §56.2's eighth fact (Appendix C.3, C.4).
    #[allow(clippy::too_many_arguments)]
    fn newer_state(
        &self,
        checklist: &mut SafetyChecklist,
        method: Option<RestoreMethod>,
        restore_set: &[PathBuf],
        reference: &SubvolumeRef,
        snapshot_root: &str,
        live_path: &Path,
        snapshot: &SubvolumeShow,
        live: Option<&SubvolumeShow>,
    ) -> NewerStateImpact {
        let Some(method) = method else {
            checklist.block(
                SafetyFact::LaterStateDiscarded,
                "no recovery method was selected, so what it would discard could not be computed"
                    .to_owned(),
            );
            return NewerStateImpact::unanalysed();
        };
        let mut items: Vec<NewerStateItem> = Vec::new();
        if method.discards_newer_state() {
            if let Some(item) = classify_subvolume(
                reference.tree_path(),
                live.and_then(SubvolumeShow::generation),
                snapshot.generation(),
            ) {
                items.push(item);
            }
        } else {
            for object in restore_set {
                let Ok(relative) = object.strip_prefix(live_path) else {
                    continue;
                };
                let inside = PathBuf::from(snapshot_root).join(relative);
                let before = match self.files.read(&inside) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        checklist.block(
                            SafetyFact::LaterStateDiscarded,
                            format!(
                                "the snapshot's copy of {} could not be read: {}",
                                object.display(),
                                error.message()
                            ),
                        );
                        return NewerStateImpact::unanalysed();
                    }
                };
                let after = match self.files.read(object) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        checklist.block(
                            SafetyFact::LaterStateDiscarded,
                            format!(
                                "the live {} could not be read: {}",
                                object.display(),
                                error.message()
                            ),
                        );
                        return NewerStateImpact::unanalysed();
                    }
                };
                items.push(classify_object(
                    object,
                    true,
                    before.as_deref(),
                    after.as_deref(),
                ));
            }
        }
        if let Some(unknown) = items
            .iter()
            .find(|item| item.class() == NewerStateClass::Unknown)
        {
            checklist.block(
                SafetyFact::LaterStateDiscarded,
                format!("{}: {}", unknown.object(), unknown.detail()),
            );
            return NewerStateImpact::unanalysed();
        }
        let losses = items.iter().filter(|item| item.class().is_loss()).count();
        checklist.establish(
            SafetyFact::LaterStateDiscarded,
            if losses == 0 {
                format!(
                    "nothing written since the recovery point would be taken away by a {}",
                    method.as_str()
                )
            } else {
                format!(
                    "{losses} object(s) have changed since the recovery point, and a {} would \
                     discard those changes (Appendix C.4)",
                    method.as_str()
                )
            },
        );
        NewerStateImpact::analysed(items)
    }

    /// Builds the fragment for `method` (§12.1, §14.4).
    #[allow(clippy::too_many_arguments)]
    fn fragment(
        &self,
        plan_id: &PlanId,
        asset: &RecoveryAsset,
        method: RestoreMethod,
        is_root: bool,
        restore_set: &[PathBuf],
        reference: &SubvolumeRef,
        live_path: &Path,
        live_mount: &BtrfsMount,
        layout: Option<&SubvolumeLayout>,
        mounts: &BtrfsMounts,
        newer: NewerStateImpact,
    ) -> RecoveryPlanFragment {
        let mut fragment = RecoveryPlanFragment::new(PROVIDER_ID, method).with_newer_state(newer);
        let derived = self
            .recovery_namespace(mounts, reference.tree_path())
            .map(|namespace| {
                namespace.join(format!(
                    "{}{DERIVED_SUFFIX}",
                    PathBuf::from(asset.reference())
                        .file_name()
                        .map_or_else(|| "ono-derived".to_owned(), |name| name
                            .to_string_lossy()
                            .into_owned())
                ))
            })
            .unwrap_or_else(|_| PathBuf::from(format!("{}{DERIVED_SUFFIX}", asset.reference())));
        let mut ordinal = 0usize;
        if matches!(
            method,
            RestoreMethod::CloneAndCopy
                | RestoreMethod::SubvolumeReplacement
                | RestoreMethod::OfflineRootRecovery
        ) {
            fragment = fragment.acting(
                PlanAction::new(
                    plan_id,
                    ordinal,
                    ActionRole::Prepare,
                    format!(
                        "create the writable subvolume {} from the read-only recovery point {}, \
                         tracked as its own recovery asset (§14.5)",
                        derived.display(),
                        asset.reference()
                    ),
                    recovery_operation(
                        RecoveryCapability::Prepare,
                        OP_DERIVE_WRITABLE,
                        vec![
                            (ARG_SOURCE, Value::string(asset.reference())),
                            (ARG_DESTINATION, Value::string(&derived.to_string_lossy())),
                        ],
                    ),
                )
                .on(asset.reference().to_owned())
                .privileged(),
            );
            ordinal += 1;
        }
        match method {
            RestoreMethod::SelectiveFileRestore | RestoreMethod::CloneAndCopy => {
                let root = if method == RestoreMethod::SelectiveFileRestore {
                    PathBuf::from(asset.reference())
                } else {
                    derived.clone()
                };
                for object in restore_set {
                    let Ok(relative) = object.strip_prefix(live_path) else {
                        continue;
                    };
                    let from = root.join(relative);
                    fragment = fragment.acting(
                        PlanAction::new(
                            plan_id,
                            ordinal,
                            ActionRole::Recover,
                            format!(
                                "restore {} from {}",
                                object.display(),
                                from.display()
                            ),
                            recovery_operation(
                                RecoveryCapability::Restore,
                                OP_RESTORE_FILE,
                                vec![
                                    (ARG_SOURCE, Value::string(&from.to_string_lossy())),
                                    (
                                        ARG_DESTINATION,
                                        Value::string(&object.to_string_lossy()),
                                    ),
                                ],
                            ),
                        )
                        .on(object.to_string_lossy().into_owned())
                        .privileged(),
                    );
                    ordinal += 1;
                }
            }
            RestoreMethod::SubvolumeReplacement => {
                fragment = fragment
                    .acting(
                        PlanAction::new(
                            plan_id,
                            ordinal,
                            ActionRole::Recover,
                            format!(
                                "with {} unmounted, move it aside and put {} in its place (§14.4)",
                                live_path.display(),
                                derived.display()
                            ),
                            recovery_operation(
                                RecoveryCapability::Restore,
                                OP_REPLACE_SUBVOLUME,
                                vec![
                                    (ARG_SOURCE, Value::string(&derived.to_string_lossy())),
                                    (
                                        ARG_DESTINATION,
                                        Value::string(&live_path.to_string_lossy()),
                                    ),
                                    (ARG_SUBVOLUME, Value::string(reference.tree_path())),
                                ],
                            ),
                        )
                        .on(live_path.to_string_lossy().into_owned())
                        .privileged(),
                    )
                    .needing_offline();
            }
            RestoreMethod::OfflineRootRecovery => {
                fragment = fragment
                    .acting(
                        PlanAction::new(
                            plan_id,
                            ordinal,
                            ActionRole::Recover,
                            format!(
                                "point the next boot at {} by making it the default subvolume of \
                                 {} (Appendix D.9)",
                                derived.display(),
                                live_mount.mount_point()
                            ),
                            recovery_operation(
                                RecoveryCapability::Restore,
                                OP_SET_DEFAULT,
                                vec![
                                    (ARG_SOURCE, Value::string(&derived.to_string_lossy())),
                                    (ARG_MOUNT, Value::string(live_mount.mount_point())),
                                ],
                            ),
                        )
                        .on(live_mount.mount_point().to_owned())
                        .privileged(),
                    )
                    .needing_reboot();
            }
            _ => {}
        }
        if let Some(layout) = layout
            && let Some(boundary) = layout.by_id(reference.id())
        {
            for nested in layout.nested_within(boundary) {
                fragment = fragment.leaving(
                    UnrecoverableEffect::new(
                        nested.tree_path().to_owned(),
                        EffectDomain::FilesystemPersistent,
                        "§14.3: this subvolume is nested inside the one being recovered, and the \
                         recovery point holds an empty directory where it is mounted. Recovering \
                         it needs its own recovery asset",
                    )
                    .compensated_by(format!(
                        "recover subvolume {} from an asset of its own",
                        nested.tree_path()
                    )),
                );
            }
        }
        fragment = fragment.verifying(
            VerificationContract::new(
                plan_id,
                VerificationClass::Required,
                reference.tree_path().to_owned(),
                format!(
                    "the recovered state of {} matches the recovery point {}",
                    reference.tree_path(),
                    asset.reference()
                ),
            )
            .about(EquivalenceDomain::PersistentState),
        );
        if is_root && method == RestoreMethod::OfflineRootRecovery {
            fragment = fragment.verifying(
                VerificationContract::new(
                    plan_id,
                    VerificationClass::Required,
                    live_mount.mount_point().to_owned(),
                    "after the reboot, the running root is the subvolume the recovery selected",
                )
                .about(EquivalenceDomain::PersistentState),
            );
        }
        fragment
    }
}

impl RecoveryProvider for BtrfsProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        let capabilities = RecoveryCapability::REQUIRED.iter().fold(
            ProviderCapabilities::new(PROVIDER_ID),
            |carry, capability| carry.recovering(*capability),
        );
        VALIDATED_VERSIONS
            .iter()
            .fold(capabilities, |carry, version| {
                carry.tested_against("btrfs-progs", *version)
            })
    }

    fn availability(&self) -> ProviderAvailability {
        if !self.runner.is_available(&self.program) {
            return ProviderAvailability::Unavailable {
                reason: Arc::from(format!(
                    "`{}` is not installed, so no Btrfs subvolume can be inspected or snapshotted",
                    self.program
                )),
            };
        }
        match self.version() {
            Ok(version) if VALIDATED_VERSIONS.contains(&version.series().as_str()) => {
                ProviderAvailability::Available {
                    version: Arc::from(version.raw()),
                }
            }
            Ok(version) => ProviderAvailability::Unsupported {
                version: Arc::from(version.raw()),
                reason: Arc::from(format!(
                    "this provider has been validated against btrfs-progs {} and not against {}. \
                     Appendix G.4: it degrades rather than executing semantics it has not tested",
                    VALIDATED_VERSIONS.join(", "),
                    version.series()
                )),
            },
            Err(error) => ProviderAvailability::Unavailable {
                reason: Arc::from(format!(
                    "the version of `{}` could not be established: {}",
                    self.program,
                    error.message()
                )),
            },
        }
    }

    fn resolve_domain(&self, path: &str) -> Result<Option<PersistenceDomain>, ErrorValue> {
        let target = PathBuf::from(path);
        let mounts = self.mounts()?;
        let Some(mount) = mounts.covering(&target) else {
            return Ok(None);
        };
        let mount_point = PathBuf::from(mount.mount_point());
        let info = self.filesystem_info(&mount_point)?;
        let outcome = self.show(&target)?;
        let layout = SubvolumeLayout::new(self.boundaries(&mount_point)?, mounts.clone().into_owned());
        let (boundary, evidence) = match &outcome {
            ShowOutcome::Subvolume(show) => match layout.by_id(show.id()) {
                Some(boundary) => (
                    boundary,
                    format!(
                        "`btrfs subvolume show` reports the path as subvolume {} itself",
                        show.id()
                    ),
                ),
                None => {
                    return Err(fact_not_established(
                        SafetyFact::FilesystemAndSubvolumeId,
                        &format!(
                            "`btrfs subvolume show {path}` reports subvolume {}, which does not \
                             appear in `btrfs subvolume list` for the same filesystem",
                            show.id()
                        ),
                    ));
                }
            },
            ShowOutcome::PlainDirectory => match layout.containing(&target) {
                Some(boundary) => (
                    boundary,
                    "`btrfs subvolume show` answers `Not a Btrfs subvolume`, so the path is an \
                     ordinary directory and its state lives in the subvolume containing it — a \
                     name that resembles a subvolume is not evidence of one (Appendix B.9)"
                        .to_owned(),
                ),
                None => return Err(no_btrfs_mount(path)),
            },
            ShowOutcome::Missing => match layout.containing(&target) {
                Some(boundary) => (
                    boundary,
                    "the path does not exist yet, so its state will land in the subvolume that \
                     contains it"
                        .to_owned(),
                ),
                None => return Err(no_btrfs_mount(path)),
            },
            ShowOutcome::Refused(reason) => {
                return Err(command_failed("btrfs subvolume show", reason));
            }
        };
        let reference = SubvolumeRef::new(info.uuid(), boundary.id(), boundary.tree_path());
        let nested = layout.nested_within(boundary).len();
        let detail = format!(
            "{}; {evidence}{}",
            reference.describe(),
            if nested == 0 {
                String::new()
            } else {
                format!(
                    ". {nested} subvolume(s) are nested inside it, and a snapshot of it holds \
                     none of them (§14.3)"
                )
            }
        );
        Ok(Some(
            PersistenceDomain::resolved(
                path,
                resolved_mount(mount),
                SCOPE_KIND,
                reference.reference(),
                detail,
            )
            .with_boundary(boundary.tree_path()),
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
        let Some(reference) = domain.object().and_then(SubvolumeRef::parse) else {
            return Ok(Vec::new());
        };
        let mount_point = PathBuf::from(domain.mount().mount_point());
        let layout = self.layout(&mount_point)?;
        let usage = self.filesystem_usage(&mount_point)?;
        let Some(boundary) = layout.by_id(reference.id()) else {
            return Err(fact_not_established(
                SafetyFact::FilesystemAndSubvolumeId,
                &format!(
                    "subvolume {} does not appear in `btrfs subvolume list` for {}, so there is \
                     no metadata to protect it from",
                    reference.id(),
                    mount_point.display()
                ),
            ));
        };
        Ok(vec![self.candidate_for(
            &layout,
            reference.filesystem(),
            boundary,
            &[domain.path()],
            objective,
            usage,
        )])
    }

    fn plan_protection(
        &self,
        candidates: &[RecoveryCandidate],
        mode: ProtectionMode,
    ) -> Result<Vec<ProtectionAction>, ErrorValue> {
        self.plan_protection_for(candidates, mode, self.plan.as_ref(), self.instant)
    }

    fn create(&self, action: &ProtectionAction) -> Result<RecoveryAsset, ErrorValue> {
        let proposed = action.proposed_asset();
        let Some(reference) = SubvolumeRef::parse(proposed.scope().domain()) else {
            return Err(snapshot_failed(
                proposed.scope().domain(),
                "the action's scope does not name a Btrfs subvolume",
            ));
        };
        let mounts = self.mounts()?;
        let Some((source_path, _)) = self.live_path(&mounts, &reference) else {
            return Err(snapshot_failed(
                proposed.scope().domain(),
                &format!(
                    "{} is not mounted anywhere this process can see, so there is nothing to \
                     snapshot from. Nothing was changed",
                    reference.describe()
                ),
            ));
        };
        let source_text = source_path.to_string_lossy().into_owned();
        let destination = proposed.reference().to_owned();
        let output = if self.config.prefers_read_only_snapshots() {
            self.btrfs(&["subvolume", "snapshot", "-r", &source_text, &destination])?
        } else {
            self.btrfs(&["subvolume", "snapshot", &source_text, &destination])?
        };
        if !output.succeeded() {
            return Err(snapshot_failed(
                proposed.scope().domain(),
                spoken_text(&output).trim(),
            ));
        }
        let created = match self.show(Path::new(&destination))? {
            ShowOutcome::Subvolume(show) => show,
            outcome => {
                return Err(snapshot_failed(
                    proposed.scope().domain(),
                    &format!(
                        "`btrfs subvolume snapshot` reported success and {destination} is not a \
                         subvolume: {}",
                        describe_outcome(&outcome)
                    ),
                ));
            }
        };
        // §14.5: the read-only flag is verified rather than assumed. A snapshot that was asked to
        // be read-only and is not would be retained as a recovery point whose contents anything
        // could change, which is the one thing a recovery point may not be.
        let read_only = self.is_read_only(Path::new(&destination))?;
        if self.config.prefers_read_only_snapshots() && !read_only {
            return Err(snapshot_failed(
                proposed.scope().domain(),
                &format!(
                    "the snapshot at {destination} was created and `btrfs property get \
                     {destination} ro` answers ro=false, so it is not the read-only recovery \
                     point §14.5 asks for"
                ),
            ));
        }
        let mut asset = RecoveryAsset::proposed(
            PROVIDER_ID,
            RecoveryAssetType::BtrfsSnapshot,
            destination,
            proposed.scope().clone(),
            created.created_at().unwrap_or(self.instant),
        );
        if let Some(plan) = proposed.source_plan() {
            asset = asset.for_plan(plan.clone());
        }
        for exclusion in proposed.exclusions() {
            asset = asset.excluding(exclusion.clone());
        }
        Ok(asset
            .at_consistency(ConsistencyClass::FilesystemConsistent)
            .restored_by(RestoreMethod::SelectiveFileRestore)
            .costing(proposed.cost().clone())
            .retained_for(proposed.retention())
            .capturing(format!("btrfs-subvolume-uuid:{}", created.uuid()))
            .creating())
    }

    fn validate(&self, asset: &RecoveryAsset) -> Result<RecoveryValidation, ErrorValue> {
        let snapshot_path = PathBuf::from(asset.reference());
        let detail_prefix = format!("recovery point {}", asset.reference());
        let outcome = self.show(&snapshot_path)?;
        let snapshot = match &outcome {
            ShowOutcome::Subvolume(show) => show,
            ShowOutcome::Refused(reason) => {
                return Err(command_failed("btrfs subvolume show", reason));
            }
            other => {
                return Ok(RecoveryValidation::none(
                    self.instant,
                    format!(
                        "{detail_prefix} is not there: {} (§11.4)",
                        describe_outcome(other)
                    ),
                ));
            }
        };
        let read_only = self.is_read_only(&snapshot_path)?;
        let mounts = self.mounts()?;
        let reference = SubvolumeRef::parse(asset.scope().domain());
        let live = match &reference {
            Some(reference) => match self.live_path(&mounts, reference) {
                Some((path, mount)) => match self.show(&path)? {
                    ShowOutcome::Subvolume(show) => Some((show, mount)),
                    _ => None,
                },
                None => None,
            },
            None => None,
        };
        let identity = snapshot.parent_uuid().is_some_and(|parent| {
            live.as_ref()
                .is_some_and(|(source, _)| parent == source.uuid())
        }) && (read_only || !self.config.prefers_read_only_snapshots());
        let scope_matches = reference
            .as_ref()
            .zip(live.as_ref())
            .is_some_and(|(reference, (source, _))| source.id() == reference.id());
        let restore_available = live
            .as_ref()
            .is_some_and(|(_, mount)| !mount.is_read_only());
        Ok(RecoveryValidation::complete(
            self.instant,
            format!(
                "{detail_prefix} is subvolume {} (ro={read_only}), taken from {}",
                snapshot.id(),
                snapshot
                    .parent_uuid()
                    .unwrap_or("no subvolume it could name"),
            ),
        )
        .existing(true)
        .identity(identity)
        .scope(scope_matches)
        .restore(restore_available)
        .permissions(true))
    }

    fn plan_recovery(
        &self,
        asset: &RecoveryAsset,
        source: Option<&ChangePlan>,
        goal: RecoveryGoal,
    ) -> Result<RecoveryPlanFragment, ErrorValue> {
        self.plan_recovery_with_checklist(asset, source, goal)
            .map(|(fragment, _)| fragment)
    }

    fn restore(&self, action: &PlanAction, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        let Execution::RecoveryOperation { arguments, .. } = action.execution() else {
            return Err(core_error::recovery_apply_failed(
                action.summary(),
                "this action is not a Btrfs recovery operation",
            ));
        };
        let operation = argument(arguments, ARG_OPERATION).unwrap_or_default();
        let source = argument(arguments, ARG_SOURCE).unwrap_or_default();
        let destination = argument(arguments, ARG_DESTINATION).unwrap_or_default();
        match operation.as_str() {
            OP_RESTORE_FILE => self
                .files
                .copy(Path::new(&source), Path::new(&destination)),
            OP_SET_DEFAULT => {
                let mount = argument(arguments, ARG_MOUNT).unwrap_or_default();
                let id = match self.show(Path::new(&source))? {
                    ShowOutcome::Subvolume(show) => show.id().to_string(),
                    outcome => {
                        return Err(core_error::recovery_apply_failed(
                            action.summary(),
                            &format!(
                                "{source} is not a subvolume, so the next boot could not be \
                                 pointed at it: {}",
                                describe_outcome(&outcome)
                            ),
                        ));
                    }
                };
                let output = self.btrfs(&["subvolume", "set-default", &id, &mount])?;
                if output.succeeded() {
                    Ok(())
                } else {
                    Err(core_error::recovery_apply_failed(
                        action.summary(),
                        spoken_text(&output).trim(),
                    ))
                }
            }
            OP_REPLACE_SUBVOLUME => {
                let mounts = self.mounts()?;
                if mounts
                    .mounts()
                    .iter()
                    .any(|mount| mount.mount_point() == destination)
                {
                    return Err(core_error::requires_offline(
                        &destination,
                        RestoreMethod::SubvolumeReplacement.as_str(),
                    ));
                }
                let aside = PathBuf::from(format!("{destination}{SUPERSEDED_SUFFIX}"));
                self.files.rename(Path::new(&destination), &aside)?;
                self.files
                    .rename(Path::new(&source), Path::new(&destination))
            }
            OP_DERIVE_WRITABLE => Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!(
                    "a writable subvolume derived from {} is a recovery asset in its own right \
                     (§14.5), and this call cannot hand one back. Create it with \
                     `BtrfsProvider::derive_writable`, which records it and its dependency on the \
                     read-only snapshot it came from",
                    asset.reference()
                ),
            )),
            other => Err(core_error::recovery_apply_failed(
                action.summary(),
                &format!("`{other}` is not an operation this provider performs"),
            )),
        }
    }

    fn cleanup(&self, asset: &RecoveryAsset) -> Result<(), ErrorValue> {
        let reference = PathBuf::from(asset.reference());
        let mounts = self.mounts()?;
        let namespace = self.recovery_namespace(&mounts, asset.scope().domain())?;
        if reference.parent() != Some(namespace.as_path()) {
            return Err(core_error::asset_invalid(
                asset.id(),
                &[
                    "the asset is not inside this provider's recovery namespace, and §37 removes \
                     the objects this provider created rather than any subvolume it is pointed at",
                ],
            ));
        }
        // Exactly one path, and no recursive flag: `btrfs subvolume delete` takes what it is
        // given, and a recursive removal here would take the nested subvolumes §14.3 keeps
        // separate with it.
        let output = self.btrfs(&["subvolume", "delete", asset.reference()])?;
        if output.succeeded() {
            Ok(())
        } else {
            Err(command_failed(
                "btrfs subvolume delete",
                spoken_text(&output).trim(),
            ))
        }
    }

    fn estimate_cost(&self, asset: &RecoveryAsset) -> Result<RecoveryCost, ErrorValue> {
        let mounts = self.mounts()?;
        let reference = SubvolumeRef::parse(asset.scope().domain());
        let query_mount = reference
            .as_ref()
            .and_then(|reference| self.live_path(&mounts, reference))
            .map(|(_, mount)| PathBuf::from(mount.mount_point()))
            .or_else(|| {
                mounts
                    .covering(Path::new(asset.reference()))
                    .map(|mount| PathBuf::from(mount.mount_point()))
            })
            .ok_or_else(|| no_btrfs_mount(asset.reference()))?;
        // The figure is read even though neither space field can be filled from it: a filesystem
        // that no longer answers is a cost question that has changed its answer, and §37.5 would
        // rather refuse than report yesterday's number.
        let _usage = self.filesystem_usage(&query_mount)?;
        let mut cost = snapshot_cost();
        let is_root = reference
            .as_ref()
            .and_then(|reference| self.live_path(&mounts, reference))
            .is_some_and(|(_, mount)| mounts.is_root_subvolume(&mount));
        if is_root && self.config.root_recovery().requires_reboot() {
            cost = cost.needing_reboot();
        }
        if is_root && self.config.root_recovery().requires_offline() {
            cost = cost.needing_offline();
        }
        Ok(cost)
    }
}

/// The cost of a Btrfs snapshot: never zero, never exact (§37.5, §38.2).
///
/// A snapshot shares every extent with its source at creation, and what deleting it would free
/// depends on what has been rewritten since. Without quota groups the filesystem cannot answer
/// that at all — the recorded `quota-disabled.txt` is what it says when asked — so both figures
/// are null and the estimate flag is set. §38.2 forbids the alternative: showing a
/// copy-on-write snapshot as free.
#[must_use]
pub fn snapshot_cost() -> RecoveryCost {
    RecoveryCost::unknown().with_space(None, None, true)
}

/// The exclusion a nested subvolume produces (§14.3, §55.4 case 18).
#[must_use]
pub fn nested_exclusion(boundary: &SubvolumeBoundary) -> RecoveryExclusion {
    RecoveryExclusion::new(
        format!(
            "the nested subvolume {} ({})",
            boundary.tree_path(),
            boundary.id()
        ),
        "§14.3: a snapshot of a parent subvolume does not contain the live contents of a nested \
         subvolume. Inside the snapshot this path is an empty directory — which is what the \
         recorded `nested-live.txt` and `nested-in-snapshot.txt` pair shows — so protecting it \
         needs a snapshot of its own",
    )
}

/// §56.3's refusal, from a checklist with something outstanding.
fn refusal(checklist: &SafetyChecklist) -> ErrorValue {
    checklist.refusal().unwrap_or_else(|| {
        fact_not_established(
            SafetyFact::SelectiveRestorePossible,
            "the recovery could not be planned",
        )
    })
}

/// A recovery operation, as the structured execution §2.17 requires.
fn recovery_operation(
    capability: RecoveryCapability,
    operation: &str,
    arguments: Vec<(&str, Value)>,
) -> Execution {
    let mut typed: Vec<(Arc<str>, Value)> =
        vec![(Arc::from(ARG_OPERATION), Value::string(operation))];
    for (name, value) in arguments {
        typed.push((Arc::from(name), value));
    }
    Execution::RecoveryOperation {
        provider: Arc::from(PROVIDER_ID),
        capability: Arc::from(capability.as_str()),
        arguments: typed,
    }
}

/// The text of one argument of a recovery operation.
fn argument(arguments: &[(Arc<str>, Value)], name: &str) -> Option<String> {
    arguments
        .iter()
        .find(|(key, _)| key.as_ref() == name)
        .and_then(|(_, value)| match value {
            Value::String(text) => Some(text.to_string()),
            _ => None,
        })
}

/// A [`ShowOutcome`] in a sentence, for a refusal a person reads.
fn describe_outcome(outcome: &ShowOutcome) -> String {
    match outcome {
        ShowOutcome::Subvolume(show) => format!("it is subvolume {}", show.id()),
        ShowOutcome::PlainDirectory => {
            "`btrfs` answers `Not a Btrfs subvolume`, so it is an ordinary directory".to_owned()
        }
        ShowOutcome::Missing => "the path does not exist".to_owned(),
        ShowOutcome::Refused(reason) => format!("`btrfs` refused: {reason}"),
    }
}

/// The mount as `ono-change-core` models it (Appendix B.1).
fn resolved_mount(mount: &BtrfsMount) -> ResolvedMount {
    let mut options: Vec<Arc<str>> = vec![Arc::from(if mount.is_read_only() { "ro" } else { "rw" })];
    if let Some(id) = mount.subvolume_id() {
        options.push(Arc::from(format!("subvolid={id}")));
    }
    if let Some(subvolume) = mount.subvolume() {
        options.push(Arc::from(format!("subvol={subvolume}")));
    }
    ResolvedMount::new(
        mount.device().to_owned(),
        mount.mount_point().to_owned(),
        "btrfs",
        mount.source().to_owned(),
        mount.root().to_owned(),
    )
    .with_options(options)
}

/// What a method means for the question "must the subvolume be replaced?" (§56.2).
fn replacement_sentence(method: RestoreMethod) -> String {
    match method {
        RestoreMethod::SelectiveFileRestore => "replacing the subvolume is not required, because \
             the wanted objects can be read out of it one at a time"
            .to_owned(),
        RestoreMethod::CloneAndCopy => "replacing the subvolume is not required, because the \
             recovery point is materialised as a separate writable subvolume and the wanted state \
             copied out of it"
            .to_owned(),
        RestoreMethod::SubvolumeReplacement => "replacing the subvolume is required, and it needs \
             the filesystem offline (§14.4)"
            .to_owned(),
        RestoreMethod::OfflineRootRecovery => "replacing the running root subvolume is required, \
             and it takes effect on the next boot (§14.6, Appendix D.9)"
            .to_owned(),
        other => format!("the method is {}", other.as_str()),
    }
}

/// §56.2's default-subvolume and boot impact fact, as evidence (Appendix D.9).
fn default_subvolume_evidence(
    default: &DefaultSubvolume,
    reference: &SubvolumeRef,
    method: Option<RestoreMethod>,
) -> String {
    let names_this = default.id() == reference.id();
    let changes_boot = method == Some(RestoreMethod::OfflineRootRecovery);
    let what = if default.is_filesystem_tree() {
        format!(
            "the filesystem's default is its top level (id {}), so which subvolume boots is \
             decided by the bootloader and `/etc/fstab` rather than by the default",
            default.id()
        )
    } else if names_this {
        format!(
            "the filesystem's default is subvolume {}, which is the one being recovered",
            default.id()
        )
    } else {
        format!(
            "the filesystem's default is subvolume {}{}, which is not the one being recovered",
            default.id(),
            default
                .tree_path()
                .map_or_else(String::new, |path| format!(" ({path})"))
        )
    };
    if changes_boot {
        format!("{what}. This recovery changes the default subvolume, so it changes what boots")
    } else {
        format!("{what}. This recovery does not change the default subvolume")
    }
}

/// §56.2's mount and reboot fact, as evidence (§14.6).
fn mount_evidence(
    mount: &BtrfsMount,
    live_path: &Path,
    is_root: bool,
    method: Option<RestoreMethod>,
    policy: RootRecovery,
) -> String {
    let where_it_is = format!(
        "{} is mounted at {} ({}), and the mount is {}",
        live_path.display(),
        mount.mount_point(),
        mount
            .subvolume()
            .map_or_else(|| "no subvol= option".to_owned(), |subvol| format!(
                "subvol={subvol}"
            )),
        if mount.is_read_only() {
            "read-only, so nothing could be written back into it (Appendix G.2)"
        } else {
            "writable"
        }
    );
    let requirement = match method {
        Some(RestoreMethod::SelectiveFileRestore) => {
            "recovery writes into the mounted subvolume and needs neither an unmount nor a reboot"
                .to_owned()
        }
        Some(RestoreMethod::CloneAndCopy) => "recovery materialises the recovery point beside the \
             live subvolume and copies out of it, needing neither an unmount nor a reboot"
            .to_owned(),
        Some(RestoreMethod::SubvolumeReplacement) => format!(
            "recovery replaces the subvolume, which requires it to be unmounted first{}",
            if is_root {
                " — and for the root subvolume that means an offline window under the \
                 `offline-subvolume-replacement` policy"
            } else {
                ""
            }
        ),
        Some(RestoreMethod::OfflineRootRecovery) => format!(
            "recovery selects the subvolume the machine boots into, so it takes effect on the \
             next reboot under the `{}` policy (§55.4 case 22)",
            policy.token()
        ),
        Some(other) => format!("the method is {}", other.as_str()),
        None => "no method was selected".to_owned(),
    };
    format!("{where_it_is}. {requirement}")
}

/// What restoring from a candidate would need (§43.4, §14.6).
fn restore_requirement(is_root: bool, policy: RootRecovery) -> String {
    if is_root {
        format!(
            "the live subvolume mounted read-write for a selective restore; a whole-subvolume \
             recovery of the root follows the `{}` policy of §14.6, which {}",
            policy.token(),
            if policy.requires_reboot() {
                "takes effect on the next boot"
            } else if policy.requires_offline() {
                "needs the filesystem offline"
            } else {
                "stays online and restores the named objects"
            }
        )
    } else {
        "the live subvolume mounted read-write; replacing it instead of restoring into it needs \
         it unmounted (§14.4)"
            .to_owned()
    }
}
