//! How long a recovery asset is kept, and what removing it takes away (spec §37).
//!
//! §37.1 sets the default at twenty-four hours after successful verification. §37.2 takes the
//! assets of a failed plan out of that rule entirely — `PlanState::retains_assets_indefinitely`
//! is the predicate, and a plan that failed is a plan somebody may still need to recover from.
//!
//! §37.3 and §2.15 are the safety invariant this module exists for, and it is worth naming:
//! **cleanup MUST never silently invalidate a recovery guarantee that is still presented as
//! available.** A plan that says `recovery available` in `get change` is a promise, and deleting
//! the asset behind it turns that line into a lie at the exact moment somebody needs it. So
//! [`cleanup_preview`] answers, for each asset, which plans its removal would strand, and
//! [`CleanupPreview::refusal_for`] turns a removal that would strand one into §2.15's refusal.
//!
//! §37.4 is the last rule: storage pressure surfaces a landmark and recommends cleanup, and MUST
//! NOT delete anything early without policy authorisation. [`storage_pressure`] therefore returns
//! a recommendation and never a decision.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::error;
use ono_change_core::{AssetState, PlanId, PlanState, RecoveryAsset, RecoveryAssetId};
use ono_value::{ByteSize, ErrorValue};

use crate::policy::{FreeSpaceFloor, ProtectionPolicy};

/// What a plan still says about its own recoverability (§37.2, §37.3).
///
/// The store owns plans; this is the slice of one that retention needs, so nothing here has to
/// read a `ChangePlan` and §37's rules can be tested against states rather than against a store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRetention {
    plan: PlanId,
    state: PlanState,
    verified_at: Option<Timestamp>,
    assets: Vec<RecoveryAssetId>,
    advertises_recovery: bool,
}

impl PlanRetention {
    /// Records that `plan` is in `state`.
    #[must_use]
    pub fn new(plan: PlanId, state: PlanState) -> Self {
        Self {
            plan,
            state,
            verified_at: None,
            assets: Vec::new(),
            advertises_recovery: state.is_recoverable(),
        }
    }

    /// Records when the plan's verification succeeded, which starts §37.1's clock.
    #[must_use]
    pub const fn verified_at(mut self, at: Timestamp) -> Self {
        self.verified_at = Some(at);
        self
    }

    /// Names an asset the plan's recovery rests on (§11.1).
    #[must_use]
    pub fn resting_on(mut self, asset: RecoveryAssetId) -> Self {
        self.assets.push(asset);
        self
    }

    /// States whether the plan still shows recovery as available (§2.15).
    #[must_use]
    pub const fn advertising_recovery(mut self, advertises: bool) -> Self {
        self.advertises_recovery = advertises;
        self
    }

    /// The plan.
    #[must_use]
    pub const fn plan(&self) -> &PlanId {
        &self.plan
    }

    /// Its state.
    #[must_use]
    pub const fn state(&self) -> PlanState {
        self.state
    }

    /// When its verification succeeded.
    #[must_use]
    pub const fn verification(&self) -> Option<Timestamp> {
        self.verified_at
    }

    /// The assets it rests on.
    #[must_use]
    pub fn assets(&self) -> &[RecoveryAssetId] {
        &self.assets
    }

    /// Whether it still offers recovery to an operator reading it (§2.15).
    #[must_use]
    pub const fn advertises_recovery(&self) -> bool {
        self.advertises_recovery
    }

    /// Whether §37.2 keeps its assets out of ordinary success retention.
    #[must_use]
    pub const fn retains_assets_indefinitely(&self) -> bool {
        self.state.retains_assets_indefinitely()
    }
}

/// When retention ends for an asset a plan verified at `verified_at` (§37.1).
///
/// `None` means nothing automatic ends it: either the retention policy holds the asset (§37.2) or
/// no verification has succeeded yet, and §37.1 starts the clock at a successful verification
/// rather than at creation.
#[must_use]
pub fn expires_at(asset: &RecoveryAsset, verified_at: Option<Timestamp>) -> Option<Timestamp> {
    if asset.retention().is_held() {
        return None;
    }
    let verified = verified_at?;
    let window = jiff::SignedDuration::try_from(asset.retention().window()).ok()?;
    verified.checked_add(window).ok()
}

/// What §37 says should happen to one asset now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupVerdict {
    /// Retention has passed and no plan depends on it: ordinary cleanup may remove it (§37.1).
    Removable,
    /// Retention has not passed yet (§37.1).
    WithinRetention,
    /// The plan failed, and §37.2 keeps its assets out of ordinary success retention.
    RetainedAfterFailure,
    /// An explicit hold prevents automatic removal (§37.2).
    Held,
    /// Removing it would take away recovery a retained plan still offers (§2.15, §37.3).
    Blocked,
    /// The asset no longer occupies storage, so there is nothing to clean up (§11.1).
    AlreadyGone,
}

impl CleanupVerdict {
    /// The word a rendering shows.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CleanupVerdict::Removable => "removable",
            CleanupVerdict::WithinRetention => "within-retention",
            CleanupVerdict::RetainedAfterFailure => "retained-after-failure",
            CleanupVerdict::Held => "held",
            CleanupVerdict::Blocked => "blocked",
            CleanupVerdict::AlreadyGone => "already-gone",
        }
    }

    /// Whether ordinary automatic cleanup may remove the asset (§37.1, §37.4).
    #[must_use]
    pub const fn permits_automatic_removal(self) -> bool {
        matches!(self, CleanupVerdict::Removable)
    }
}

/// One asset's row in the cleanup preview (§37.3).
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupEntry {
    asset: RecoveryAssetId,
    verdict: CleanupVerdict,
    unrecoverable: Vec<PlanId>,
    expires_at: Option<Timestamp>,
    reclaimable: Option<ByteSize>,
    detail: Arc<str>,
}

impl CleanupEntry {
    /// The asset.
    #[must_use]
    pub const fn asset(&self) -> &RecoveryAssetId {
        &self.asset
    }

    /// What §37 says about removing it.
    #[must_use]
    pub const fn verdict(&self) -> CleanupVerdict {
        self.verdict
    }

    /// The plans that become unrecoverable if it is removed (§37.3).
    #[must_use]
    pub fn unrecoverable_plans(&self) -> &[PlanId] {
        &self.unrecoverable
    }

    /// When its retention ends, where anything ends it.
    #[must_use]
    pub const fn expires_at(&self) -> Option<Timestamp> {
        self.expires_at
    }

    /// The space removing it would reclaim, where the provider measured any (§37.5).
    #[must_use]
    pub const fn reclaimable(&self) -> Option<ByteSize> {
        self.reclaimable
    }

    /// The sentence shown beside the row.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// What cleanup would do, before it does anything (§37.3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CleanupPreview {
    entries: Vec<CleanupEntry>,
}

impl CleanupPreview {
    /// Every asset that was considered.
    #[must_use]
    pub fn entries(&self) -> &[CleanupEntry] {
        &self.entries
    }

    /// The row for one asset.
    #[must_use]
    pub fn entry_for(&self, asset: &RecoveryAssetId) -> Option<&CleanupEntry> {
        self.entries.iter().find(|entry| entry.asset() == asset)
    }

    /// The assets ordinary automatic cleanup may remove (§37.1).
    #[must_use]
    pub fn removable(&self) -> Vec<&CleanupEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.verdict().permits_automatic_removal())
            .collect()
    }

    /// The assets §37.2 and §2.15 keep.
    #[must_use]
    pub fn retained(&self) -> Vec<&CleanupEntry> {
        self.entries
            .iter()
            .filter(|entry| {
                !entry.verdict().permits_automatic_removal()
                    && entry.verdict() != CleanupVerdict::AlreadyGone
            })
            .collect()
    }

    /// The refusal for removing `asset`, where §2.15 forbids it.
    ///
    /// # Errors
    ///
    /// Never returns `Err`; the `Option` is the answer. `Some` is `recovery.cleanup_blocked`,
    /// naming the plans whose recovery the removal would take away.
    #[must_use]
    pub fn refusal_for(&self, asset: &RecoveryAssetId) -> Option<ErrorValue> {
        let entry = self.entry_for(asset)?;
        if entry.verdict() != CleanupVerdict::Blocked {
            return None;
        }
        let plans: Vec<String> = entry
            .unrecoverable_plans()
            .iter()
            .map(|plan| plan.as_str().to_owned())
            .collect();
        Some(error::cleanup_blocked(asset, &plans))
    }

    /// The space removing everything removable would reclaim (§37.4, §37.5).
    #[must_use]
    pub fn reclaimable(&self) -> ByteSize {
        ByteSize::from_bytes(
            self.removable()
                .iter()
                .filter_map(|entry| entry.reclaimable())
                .map(ByteSize::bytes)
                .sum(),
        )
    }
}

/// What removing each of `assets` would mean, given the plans that rest on them (§37.3).
///
/// The safety rule is applied here rather than at the point of deletion, so `remove recovery` and
/// automatic retention both see the same answer: an asset a retained plan still advertises
/// recovery from is [`CleanupVerdict::Blocked`], whatever its age.
#[must_use]
pub fn cleanup_preview(
    assets: &[RecoveryAsset],
    plans: &[PlanRetention],
    now: Timestamp,
) -> CleanupPreview {
    let entries = assets
        .iter()
        .map(|asset| entry_for(asset, plans, now))
        .collect();
    CleanupPreview { entries }
}

fn entry_for(asset: &RecoveryAsset, plans: &[PlanRetention], now: Timestamp) -> CleanupEntry {
    let dependents: Vec<&PlanRetention> = plans
        .iter()
        .filter(|plan| {
            plan.assets().iter().any(|held| held == asset.id())
                || asset
                    .source_plan()
                    .is_some_and(|source| source == plan.plan())
        })
        .collect();
    let verification = dependents.iter().find_map(|plan| plan.verification());
    let expires = expires_at(asset, verification);
    let reclaimable = asset.cost().retained_bytes();

    let stranded: Vec<PlanId> = dependents
        .iter()
        .filter(|plan| plan.advertises_recovery())
        .map(|plan| plan.plan().clone())
        .collect();

    let (verdict, detail): (CleanupVerdict, String) = if !asset.state().occupies_storage() {
        (
            CleanupVerdict::AlreadyGone,
            format!(
                "the asset is {} and occupies no storage (§11.1)",
                asset.state()
            ),
        )
    } else if !stranded.is_empty() {
        (
            CleanupVerdict::Blocked,
            format!(
                "{} plan(s) still offer recovery from this asset, and §2.15 forbids removing it \
                 silently",
                stranded.len()
            ),
        )
    } else if asset.retention().is_held() {
        (
            CleanupVerdict::Held,
            "an explicit hold prevents automatic removal (§37.2)".to_owned(),
        )
    } else if dependents
        .iter()
        .any(|plan| plan.retains_assets_indefinitely())
    {
        (
            CleanupVerdict::RetainedAfterFailure,
            "§37.2: the assets of a failed, degraded or recovery-failed plan MUST NOT be removed \
             by ordinary success retention"
                .to_owned(),
        )
    } else {
        match expires {
            Some(expiry) if expiry <= now => (
                CleanupVerdict::Removable,
                format!("retention ended at {expiry} (§37.1)"),
            ),
            Some(expiry) => (
                CleanupVerdict::WithinRetention,
                format!("retention runs until {expiry} (§37.1)"),
            ),
            None => (
                CleanupVerdict::WithinRetention,
                "no successful verification has started §37.1's retention clock, so nothing \
                 automatic ends it"
                    .to_owned(),
            ),
        }
    };

    CleanupEntry {
        asset: asset.id().clone(),
        verdict,
        unrecoverable: stranded,
        expires_at: expires,
        reclaimable: if asset.state() == AssetState::Removed {
            None
        } else {
            reclaimable
        },
        detail: Arc::from(detail),
    }
}

/// A landmark raised when retained recovery assets crowd a filesystem (§37.4).
///
/// It recommends and never decides. §37.4 forbids deleting assets early without policy
/// authorisation, so [`StoragePressure::may_remove_early`] answers `false` unless the policy in
/// force says otherwise, and the recommendation stands as something an operator acts on.
#[derive(Debug, Clone, PartialEq)]
pub struct StoragePressure {
    scope: Arc<str>,
    free: ByteSize,
    capacity: ByteSize,
    floor: FreeSpaceFloor,
    recommended: Vec<RecoveryAssetId>,
    reclaimable: ByteSize,
    authorised: bool,
}

impl StoragePressure {
    /// The filesystem or pool under pressure.
    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// How much is free.
    #[must_use]
    pub const fn free(&self) -> ByteSize {
        self.free
    }

    /// How much there is in total.
    #[must_use]
    pub const fn capacity(&self) -> ByteSize {
        self.capacity
    }

    /// The floor it fell below (§53's `recovery.min_filesystem_free`).
    #[must_use]
    pub const fn floor(&self) -> FreeSpaceFloor {
        self.floor
    }

    /// The assets whose removal would help, in the order cleanup would take them (§37.4).
    #[must_use]
    pub fn recommended(&self) -> &[RecoveryAssetId] {
        &self.recommended
    }

    /// How much removing them would reclaim (§37.5's estimate).
    #[must_use]
    pub const fn reclaimable(&self) -> ByteSize {
        self.reclaimable
    }

    /// Whether policy authorises removing retained assets early (§37.4).
    #[must_use]
    pub const fn may_remove_early(&self) -> bool {
        self.authorised
    }

    /// The landmark line an operator sees (§37.4).
    #[must_use]
    pub fn landmark(&self) -> String {
        format!(
            "recovery assets on {} are under storage pressure: {} free of {}, below the \
             configured floor of {}. {} asset(s) could be cleaned up, reclaiming about {}",
            self.scope,
            self.free,
            self.capacity,
            self.floor.describe(),
            self.recommended.len(),
            self.reclaimable
        )
    }

    /// The refusal to raise when protection would make the pressure worse (Appendix D.3).
    #[must_use]
    pub fn refusal(&self) -> ErrorValue {
        error::storage_pressure(
            &self.scope,
            &format!("{}", self.free),
            &self.floor.describe(),
        )
    }
}

/// Raises a landmark when `scope` has fallen below the policy's free-space floor (§37.4).
///
/// Returns `None` when the floor is cleared: §37.4's landmark is a report of a condition, and
/// raising one where there is no pressure is noise that teaches an operator to ignore the next.
#[must_use]
pub fn storage_pressure(
    scope: &str,
    free: ByteSize,
    capacity: ByteSize,
    policy: &ProtectionPolicy,
    preview: &CleanupPreview,
) -> Option<StoragePressure> {
    let floor = policy.limits().min_filesystem_free();
    if floor.is_cleared_by(free, capacity) {
        return None;
    }
    Some(StoragePressure {
        scope: Arc::from(scope),
        free,
        capacity,
        floor,
        recommended: preview
            .removable()
            .iter()
            .map(|entry| entry.asset().clone())
            .collect(),
        reclaimable: preview.reclaimable(),
        authorised: policy.authorises_early_removal(),
    })
}
