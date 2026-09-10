//! Property tests for cleanup (v0.6 §54.2, §37.1, §37.2, §37.3, §2.15).
//!
//! §54.2: *cleanup never removes required assets before policy permits.* The example suite in
//! `retention.rs` shows each rule once. This holds them against generated stores — assets in
//! every lifecycle state, with every retention, attributed or not to a plan; plans in every
//! lifecycle state, verified or not, resting on any subset of the assets, advertising recovery
//! or not — at a generated instant. Each case is fixed by its seed, and a failure names the seed
//! that reproduces it.
//!
//! [`cleanup_preview`] takes no override, and §37.4 forbids early deletion without policy
//! authorisation, so "without the override" is every call here: an asset the preview lists as
//! removable is one ordinary cleanup would delete.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use ono_change_core::{
    PlanId, PlanState, RecoveryAsset, RecoveryAssetType, RecoveryScope, RecoveryValidation,
    RetentionPolicy,
};
use ono_change_protection::retention::{
    CleanupPreview, CleanupVerdict, PlanRetention, cleanup_preview,
};
use ono_testkit::Rng;

/// How many generated stores each property is held against.
const CASES: u64 = 256;

fn hours(count: usize) -> Timestamp {
    Timestamp::UNIX_EPOCH
        .checked_add(SignedDuration::from_hours(
            i64::try_from(count).expect("a small count"),
        ))
        .expect("a fixture instant")
}

/// A generated store and the instant cleanup runs at.
struct World {
    assets: Vec<RecoveryAsset>,
    plans: Vec<PlanRetention>,
    now: Timestamp,
}

impl World {
    fn generate(seed: u64) -> Self {
        let mut rng = Rng::seeded(seed);
        let plan_ids: Vec<PlanId> = (0..rng.below(5))
            .map(|index| PlanId::of("session-1", &index.to_string(), &format!("case {seed}")))
            .collect();
        let assets: Vec<RecoveryAsset> = (0..1 + rng.below(6))
            .map(|index| asset(&mut rng, index, &plan_ids))
            .collect();
        let plans = plan_ids
            .into_iter()
            .map(|id| plan(&mut rng, id, &assets))
            .collect();
        let now = hours(rng.below(200));
        Self { assets, plans, now }
    }

    fn preview(&self) -> CleanupPreview {
        cleanup_preview(&self.assets, &self.plans, self.now)
    }

    /// The plans whose recovery rests on `asset`, by reference or by attribution (§11.1).
    fn dependents(&self, asset: &RecoveryAsset) -> Vec<&PlanRetention> {
        self.plans
            .iter()
            .filter(|plan| {
                plan.assets().contains(asset.id()) || asset.source_plan() == Some(plan.plan())
            })
            .collect()
    }
}

fn asset(rng: &mut Rng, index: usize, plans: &[PlanId]) -> RecoveryAsset {
    let created = hours(rng.below(48));
    let scope = RecoveryScope::new("zfs-dataset", format!("tank/data-{index}"), "localhost")
        .covering(format!("/srv/data-{index}"));
    let mut asset = RecoveryAsset::proposed(
        "ono.recovery.zfs",
        RecoveryAssetType::ZfsSnapshot,
        format!("tank/data-{index}@ono-{index}"),
        scope,
        created,
    );
    if rng.chance(3)
        && let Some(plan) = rng.pick(plans)
    {
        asset = asset.for_plan(plan.clone());
    }
    asset = asset.retained_for(match rng.below(4) {
        0 => RetentionPolicy::held(),
        1 => RetentionPolicy::of(Duration::from_secs(3600 * rng.below(72) as u64)),
        _ => RetentionPolicy::default(),
    });
    let ready = |asset: RecoveryAsset| {
        asset.creating().validated(RecoveryValidation::complete(
            created,
            "the fixture checked it",
        ))
    };
    match rng.below(8) {
        0..=2 => ready(asset),
        3 => asset.creating().validated(RecoveryValidation::none(
            created,
            "the scope no longer matches",
        )),
        4 => asset.creating().failed(),
        5 => ready(asset).removed(),
        6 => ready(asset).expired(),
        _ => asset.creating(),
    }
}

fn plan(rng: &mut Rng, id: PlanId, assets: &[RecoveryAsset]) -> PlanRetention {
    let state = *rng.pick(PlanState::ALL).expect("a closed vocabulary");
    let mut plan = PlanRetention::new(id, state);
    if rng.chance(2) {
        plan = plan.verified_at(hours(rng.below(96)));
    }
    for asset in assets {
        if rng.chance(3) {
            plan = plan.resting_on(asset.id().clone());
        }
    }
    if rng.chance(5) {
        plan = plan.advertising_recovery(rng.chance(2));
    }
    plan
}

/// Holds `property` for every asset of every generated store; `Some` is the violation.
fn for_every_asset(property: impl Fn(&World, &CleanupPreview, &RecoveryAsset) -> Option<String>) {
    for seed in 0..CASES {
        let world = World::generate(seed);
        let preview = world.preview();
        for asset in &world.assets {
            if let Some(violation) = property(&world, &preview, asset) {
                panic!(
                    "seed {seed}: {violation}\nasset {} ({}, retention {:?}), now {}\nplans: {:#?}",
                    asset.reference(),
                    asset.state(),
                    asset.retention(),
                    world.now,
                    world.dependents(asset)
                );
            }
        }
    }
}

fn removable(preview: &CleanupPreview, asset: &RecoveryAsset) -> bool {
    preview
        .removable()
        .iter()
        .any(|entry| entry.asset() == asset.id())
}

#[test]
fn should_never_let_cleanup_remove_an_asset_a_plan_still_offers_recovery_from() {
    for_every_asset(|world, preview, asset| {
        if !asset.state().occupies_storage() {
            return None;
        }
        let advertising: Vec<&PlanId> = world
            .dependents(asset)
            .into_iter()
            .filter(|plan| plan.advertises_recovery())
            .map(PlanRetention::plan)
            .collect();
        if advertising.is_empty() {
            return None;
        }
        if removable(preview, asset) {
            return Some(
                "§2.15: cleanup would silently remove an asset a plan still offers recovery from"
                    .to_owned(),
            );
        }
        if preview.refusal_for(asset.id()).is_none() {
            return Some(
                "§37.3: removing an asset a plan offers recovery from is not refused".to_owned(),
            );
        }
        let stranded = preview
            .entry_for(asset.id())
            .map(|entry| entry.unrecoverable_plans().to_vec())
            .unwrap_or_default();
        advertising
            .iter()
            .find(|plan| !stranded.contains(plan))
            .map(|plan| {
                format!(
                    "§37.3: the preview does not name {} among the plans the removal would \
                     strand",
                    plan.as_str()
                )
            })
    });
}

#[test]
fn should_keep_the_assets_of_a_failed_plan_out_of_ordinary_cleanup() {
    for_every_asset(|world, preview, asset| {
        let failed = world
            .dependents(asset)
            .into_iter()
            .find(|plan| plan.retains_assets_indefinitely())?;
        removable(preview, asset).then(|| {
            format!(
                "§37.2: the assets of a plan in {} must not be removed by ordinary retention",
                failed.state()
            )
        })
    });
}

#[test]
fn should_never_let_cleanup_remove_a_held_asset() {
    for_every_asset(|_, preview, asset| {
        (asset.retention().is_held() && removable(preview, asset))
            .then(|| "§37.2: an explicit hold was overridden by automatic cleanup".to_owned())
    });
}

#[test]
fn should_never_let_cleanup_remove_an_asset_before_its_retention_has_run() {
    for_every_asset(|world, preview, asset| {
        if !removable(preview, asset) {
            return None;
        }
        let window =
            SignedDuration::try_from(asset.retention().window()).expect("a generated window fits");
        let verifications: Vec<Timestamp> = world
            .dependents(asset)
            .into_iter()
            .filter_map(PlanRetention::verification)
            .collect();
        if verifications.is_empty() {
            return Some(
                "§37.1: the retention clock starts at a successful verification, and the asset \
                 was removable with none recorded"
                    .to_owned(),
            );
        }
        verifications
            .iter()
            .find(|verified| {
                verified
                    .checked_add(window)
                    .is_ok_and(|expiry| expiry > world.now)
            })
            .map(|verified| {
                format!(
                    "§37.1: a plan verified at {verified} keeps this asset until {}, and cleanup \
                     at {} would remove it",
                    verified.checked_add(window).expect("a fixture instant"),
                    world.now
                )
            })
    });
}

#[test]
fn should_answer_for_every_asset_it_was_asked_about() {
    // The preview is the only thing that stands between `remove recovery` and a deletion, so an
    // asset it silently drops is an asset nothing checked.
    for seed in 0..CASES {
        let world = World::generate(seed);
        let preview = world.preview();
        for asset in &world.assets {
            let entry = preview.entry_for(asset.id());
            assert!(
                entry.is_some(),
                "seed {seed}: the cleanup preview has no row for {}",
                asset.reference()
            );
            if !asset.state().occupies_storage() {
                assert_eq!(
                    entry.map(|entry| entry.verdict()),
                    Some(CleanupVerdict::AlreadyGone),
                    "seed {seed}: an asset that occupies no storage has nothing to remove"
                );
            }
        }
    }
}
