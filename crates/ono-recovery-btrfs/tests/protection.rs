#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! Protection: what is created, where it goes, and what the set of them may claim.
//!
//! §14.2, §14.5, §43.6, Appendix D.6, D.7, D.8 and F.1.

mod support;

use std::path::Path;
use std::sync::Arc;

use ono_change_core::{
    AssetState, ConsistencyClass, ProtectionMode, RecoveryAssetType, RecoveryObjective,
    RecoveryProvider, ToolOutput,
};
use ono_recovery_btrfs::{
    BtrfsConfig, BtrfsProvider, RecoveryAssetSet, SubvolumeRef, snapshot_name,
};
use support::{FILESYSTEM, ROOT_ID, VAR_ID, fixture, mounts, provider, runner};

/// The three calls `create` makes.
fn creation_script() -> Vec<ToolOutput> {
    vec![
        fixture("subvolume-create"),
        fixture("snapshot-create"),
        fixture("subvol-show-snapshot"),
        fixture("snapshot-ro-flag"),
    ]
}

/// A candidate over `path`, discovered from the recorded filesystem.
fn candidate(provider: &BtrfsProvider, path: &str) -> ono_change_core::RecoveryCandidate {
    let domain = provider
        .resolve_domain(path)
        .expect("the resolution runs")
        .expect("the path is on Btrfs");
    provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs")
        .into_iter()
        .next()
        .expect("one subvolume, one candidate")
}

#[test]
fn should_propose_one_read_only_snapshot_per_subvolume_the_plan_changes() {
    let provider =
        provider(support::discovery_script("subvol-show-root")).for_plan(support::plan_id());
    let candidate = candidate(&provider, support::NGINX_CONF);
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning protection runs");
    assert_eq!(actions.len(), 1);
    let action = &actions[0];
    assert!(
        action.summary().contains("read-only"),
        "§14.5 and §53's `prefer_read_only_snapshots`: recovery snapshots are read-only by default"
    );
    assert_eq!(
        action.proposed_asset().state(),
        AssetState::Proposed,
        "§2.1: planning protection creates nothing; the PREPARE action does"
    );
    assert_eq!(
        action.proposed_asset().asset_type(),
        RecoveryAssetType::BtrfsSnapshot
    );
    assert!(
        action.is_required(),
        "§2.3: protection that fails stops the apply"
    );
}

#[test]
fn should_place_the_snapshot_in_the_recovery_namespace_on_the_same_filesystem() {
    let provider =
        provider(support::discovery_script("subvol-show-root")).for_plan(support::plan_id());
    let candidate = candidate(&provider, support::NGINX_CONF);
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning protection runs");
    let reference = actions[0].proposed_asset().reference();
    assert!(
        reference.starts_with("/mnt/top/@snapshots/"),
        "Appendix D.8: snapshots go in a predictable recovery namespace on the same Btrfs \
         filesystem, and `{reference}` is where the recorded layout keeps it"
    );
    assert!(
        reference.contains(&snapshot_name(support::plan_id().short(), "@")),
        "§37: the name carries the plan, so one plan's assets are tellable from another's"
    );
}

#[test]
fn should_refuse_a_snapshot_location_nested_inside_the_subvolume_being_snapshotted() {
    let provider = provider(support::discovery_script("subvol-show-root"))
        .with_config(BtrfsConfig::default().snapshots_in("@/.snapshots"));
    let candidate = candidate(&provider, support::NGINX_CONF);
    let error = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect_err("Appendix D.8 forbids a recovery namespace under a source subvolume");
    assert_eq!(error.code().name(), "recovery.asset_create_failed");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("nested boundary")),
        "the refusal says why: each snapshot would become a nested boundary inside the next one, \
         and §14.3 makes every one of those a hole in the coverage"
    );
}

#[test]
fn should_accept_a_recovery_namespace_beside_the_subvolumes_it_protects() {
    let config = BtrfsConfig::default().snapshots_in("@snapshots");
    assert!(
        !config.is_nested_in("@"),
        "Appendix D.8: `@snapshots` is a sibling of `@`, not something underneath it"
    );
    assert!(
        !config.is_nested_in("@snap"),
        "and the comparison is by path component, so `@snapshots` is not read as living in `@snap`"
    );
    assert!(
        BtrfsConfig::default()
            .snapshots_in("@/snaps")
            .is_nested_in("@")
    );
}

#[test]
fn should_make_the_recovery_namespace_before_the_first_snapshot_on_a_filesystem() {
    // Appendix D.8: the namespace is a subvolume at the top level of the source's filesystem, and
    // a filesystem Ono has never protected has none. The first protection makes it, before the
    // snapshot that needs it, rather than failing on the missing directory.
    let script = [
        support::discovery_script("subvol-show-root"),
        creation_script(),
    ]
    .concat();
    let runner = runner(script);
    let provider = BtrfsProvider::new(Arc::clone(&runner) as Arc<dyn ono_change_core::ToolRunner>)
        .with_mounts(mounts())
        .for_plan(support::plan_id());
    let candidate = candidate(&provider, support::NGINX_CONF);
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");

    let asset = provider.create(&actions[0]).expect("creation runs");

    let namespace = std::path::Path::new(asset.reference())
        .parent()
        .expect("a snapshot lives in its namespace")
        .display()
        .to_string();
    let calls: Vec<Vec<String>> = runner.calls().into_iter().map(|(_, argv)| argv).collect();
    let made = calls.iter().position(|argv| {
        argv == &vec![
            "subvolume".to_owned(),
            "create".to_owned(),
            namespace.clone(),
        ]
    });
    let taken = calls
        .iter()
        .position(|argv| argv.get(1).is_some_and(|verb| verb == "snapshot"));
    assert!(
        matches!((made, taken), (Some(made), Some(taken)) if made < taken),
        "Appendix D.8: the namespace {namespace} is made before the snapshot that needs it: \
         {calls:?}"
    );
}

#[test]
fn should_name_a_snapshot_by_its_instant_when_no_plan_asked_for_it() {
    // §37 tells one recovery point from another by its name. A session's provider is asked by no
    // single plan, so the instant a snapshot is taken at keeps two of one subvolume apart — the
    // shape the ZFS provider's names have too.
    let runner = runner(support::discovery_script("subvol-show-root"));
    let provider = BtrfsProvider::new(Arc::clone(&runner) as Arc<dyn ono_change_core::ToolRunner>)
        .with_mounts(mounts())
        .at_instant(jiff::Timestamp::from_second(1_767_225_600).expect("a valid instant"));
    let candidate = candidate(&provider, support::NGINX_CONF);

    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");

    assert!(
        actions[0]
            .proposed_asset()
            .reference()
            .ends_with("/ono-manual-20260101T000000Z-root"),
        "§37: the snapshot is named by the instant it was proposed at: {}",
        actions[0].proposed_asset().reference()
    );
}

#[test]
fn should_create_the_snapshot_with_the_read_only_flag_and_no_shell() {
    let script = [
        support::discovery_script("subvol-show-root"),
        creation_script(),
    ]
    .concat();
    let runner = runner(script);
    let provider = BtrfsProvider::new(Arc::clone(&runner) as Arc<dyn ono_change_core::ToolRunner>)
        .with_mounts(mounts())
        .for_plan(support::plan_id());
    let candidate = candidate(&provider, support::NGINX_CONF);
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");
    let asset = provider.create(&actions[0]).expect("creation runs");
    let calls = runner.calls();
    let snapshot = calls
        .iter()
        .find(|(_, argv)| {
            argv.first().is_some_and(|first| first == "subvolume")
                && argv.get(1).is_some_and(|second| second == "snapshot")
        })
        .expect("a snapshot was taken");
    assert_eq!(
        snapshot.1,
        vec![
            "subvolume".to_owned(),
            "snapshot".to_owned(),
            "-r".to_owned(),
            "/mnt/root".to_owned(),
            asset.reference().to_owned(),
        ],
        "§14.5 creates a read-only snapshot, and §12.3 passes an argument vector with no shell \
         between the provider and the tool"
    );
    assert_eq!(asset.state(), AssetState::Creating);
    assert_eq!(
        asset.consistency(),
        ConsistencyClass::FilesystemConsistent,
        "§11.3: one `btrfs subvolume snapshot` is atomic for its own subvolume"
    );
}

#[test]
fn should_verify_the_read_only_flag_after_creation_rather_than_assuming_it() {
    let script = [
        support::discovery_script("subvol-show-root"),
        vec![
            fixture("subvolume-create"),
            fixture("snapshot-create"),
            fixture("subvol-show-snapshot"),
            ToolOutput::ok("ro=false\n"),
        ],
    ]
    .concat();
    let provider = provider(script).for_plan(support::plan_id());
    let candidate = candidate(&provider, support::NGINX_CONF);
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");
    let error = provider.create(&actions[0]).expect_err(
        "§14.5: a snapshot asked to be read-only and found writable is not a recovery point",
    );
    assert_eq!(error.code().name(), "recovery.asset_create_failed");
    assert!(error.help().is_some_and(|help| help.contains("ro=false")));
}

#[test]
fn should_take_the_creation_instant_from_the_filesystem_rather_than_from_a_clock() {
    let script = [
        support::discovery_script("subvol-show-root"),
        creation_script(),
    ]
    .concat();
    let provider = provider(script).for_plan(support::plan_id());
    let candidate = candidate(&provider, support::NGINX_CONF);
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");
    let asset = provider.create(&actions[0]).expect("creation runs");
    assert_eq!(
        asset.created_at().to_string(),
        "2026-09-09T20:33:06Z",
        "Appendix D.7: each member carries its own creation instant, and the filesystem's record \
         of when the snapshot was made is the one fact about it that no clock here can improve on"
    );
}

#[test]
fn should_carry_the_nested_subvolume_exclusions_onto_the_created_asset() {
    let script = [
        support::discovery_script("subvol-show-var"),
        creation_script(),
    ]
    .concat();
    let provider = provider(script).for_plan(support::plan_id());
    let candidate = candidate(&provider, "/mnt/root/var/log/syslog");
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");
    let asset = provider.create(&actions[0]).expect("creation runs");
    assert!(
        asset
            .exclusions()
            .iter()
            .any(|exclusion| exclusion.subject().contains("@var/lib-app")),
        "§14.3: what the candidate said it would not hold, the asset says it does not hold"
    );
    assert!(
        asset.is_local_recovery_point(),
        "§14.7 and §11.5: a Btrfs snapshot is a local recovery point, not a backup"
    );
}

#[test]
fn should_refuse_to_snapshot_a_subvolume_that_is_not_mounted() {
    let provider =
        provider(support::discovery_script("subvol-show-root")).for_plan(support::plan_id());
    let mut candidate = candidate(&provider, support::NGINX_CONF);
    candidate = ono_change_core::RecoveryCandidate::new(
        ono_recovery_btrfs::PROVIDER_ID,
        support::scope(999, "@absent", &["/mnt/root/x"]),
        candidate.domain(),
        RecoveryObjective::PreserveExact,
        "a subvolume nothing shows",
    );
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");
    let error = provider
        .create(&actions[0])
        .expect_err("§56.3: a source nobody can see is a refusal rather than a guessed path");
    assert!(
        error
            .message()
            .contains("could not create a recovery point")
    );
}

#[test]
fn should_create_no_assets_when_protection_is_off() {
    let provider = provider(support::discovery_script("subvol-show-root"));
    let candidate = candidate(&provider, support::NGINX_CONF);
    assert!(
        provider
            .plan_protection(&[candidate], ProtectionMode::Off)
            .expect("planning runs")
            .is_empty(),
        "§17.2: `off` creates no automatic recovery assets"
    );
}

#[test]
fn should_sanitise_a_plan_name_that_carries_command_syntax() {
    let name = snapshot_name("a82f; rm -rf /", "@var");
    assert_eq!(
        name, "ono-a82f--rm--rf---var",
        "§43.6: provider-generated asset names are sanitised and carry no command syntax"
    );
    assert!(!snapshot_name("../..", "@").starts_with('.'));
    assert_eq!(
        snapshot_name("a82f", "@"),
        "ono-a82f-root",
        "and the recorded layout's own name for a snapshot of `@` is what it produces"
    );
}

#[test]
fn should_not_claim_filesystem_consistency_across_two_subvolumes_snapshotted_in_turn() {
    let script = [
        support::discovery_script("subvol-show-root"),
        creation_script(),
        support::discovery_script("subvol-show-var"),
        vec![
            fixture("subvolume-create"),
            fixture("snapshot-create"),
            ToolOutput::ok(
                fixture("subvol-show-snapshot")
                    .stdout()
                    .replace("20:33:06", "20:33:09"),
            ),
            fixture("snapshot-ro-flag"),
        ],
    ]
    .concat();
    let provider = provider(script).for_plan(support::plan_id());
    let root = candidate(&provider, support::NGINX_CONF);
    let first = provider
        .plan_protection(&[root], ProtectionMode::Prefer)
        .expect("planning runs");
    let root_asset = provider.create(&first[0]).expect("the first snapshot");
    let var = candidate(&provider, "/mnt/root/var/log/syslog");
    let second = provider
        .plan_protection(&[var], ProtectionMode::Prefer)
        .expect("planning runs");
    let var_asset = provider.create(&second[0]).expect("the second snapshot");

    let set = RecoveryAssetSet::sequential(vec![root_asset, var_asset]);
    assert!(
        set.is_sequential(),
        "Appendix D.7: the set MUST record that its members were created sequentially"
    );
    assert_eq!(
        set.composed_consistency(),
        ConsistencyClass::CrashConsistent,
        "Appendix D.7: Ono MUST NOT invent cross-subvolume atomicity. Each snapshot is \
         filesystem-consistent for its own subvolume; taken one after another they are, together, \
         no better than crash-consistent"
    );
    let instants = set.creation_instants();
    assert_eq!(instants.len(), 2);
    assert_ne!(
        instants[0], instants[1],
        "and each member carries its own creation instant, which is what makes the gap between \
         them visible rather than deniable"
    );
    assert!(
        set.detail().contains("no mechanism proves a common point"),
        "the sentence the plan view shows says so in words too"
    );
}

#[test]
fn should_keep_a_single_subvolume_set_at_the_consistency_its_member_has() {
    let script = [
        support::discovery_script("subvol-show-root"),
        creation_script(),
    ]
    .concat();
    let provider = provider(script).for_plan(support::plan_id());
    let candidate = candidate(&provider, support::NGINX_CONF);
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");
    let set = provider
        .create_set(&actions)
        .expect("one subvolume, one snapshot");
    assert_eq!(
        set.composed_consistency(),
        ConsistencyClass::FilesystemConsistent,
        "§11.3: a single `btrfs subvolume snapshot` is atomic for its subvolume, and one member \
         is not weakened by a gap that does not exist"
    );
    assert_eq!(set.span(), 1);
}

#[test]
fn should_retain_the_first_snapshot_when_the_second_one_fails() {
    let script = [
        support::discovery_script("subvol-show-root"),
        support::discovery_script("subvol-show-var"),
        creation_script(),
        vec![fixture("snapshot-exists")],
    ]
    .concat();
    let runner = runner(script);
    let provider = BtrfsProvider::new(Arc::clone(&runner) as Arc<dyn ono_change_core::ToolRunner>)
        .with_mounts(mounts())
        .for_plan(support::plan_id());
    let root = candidate(&provider, support::NGINX_CONF);
    let var = candidate(&provider, "/mnt/root/var/log/syslog");
    let actions = provider
        .plan_protection(&[root, var], ProtectionMode::Prefer)
        .expect("planning runs");
    let shortfall = provider
        .create_set(&actions)
        .expect_err("Appendix F.1: the second snapshot failed");
    assert_eq!(
        shortfall.retained().len(),
        1,
        "Appendix F.1: the snapshot that was created is retained until a cleanup decision is made"
    );
    assert_eq!(
        shortfall.retained()[0].state(),
        AssetState::Creating,
        "and it is still there, not quietly removed"
    );
    assert!(
        shortfall.failed_scope().contains(&VAR_ID.to_string()),
        "the failure names the subvolume it could not protect"
    );
    assert_eq!(
        shortfall.error().code().name(),
        "recovery.asset_create_failed",
        "Appendix F.1: the original prepare failure is what the caller sees"
    );
    assert!(
        !runner
            .calls()
            .iter()
            .any(|(_, argv)| argv.iter().any(|argument| argument == "delete")),
        "§2.3 and Appendix F.1: no plan target is mutated and nothing is deleted on the way out — \
         cleaning up is a decision, not a reflex"
    );
    assert!(
        shortfall
            .error()
            .help()
            .is_some_and(|help| help.contains("Nothing was changed")),
        "§2.3: the refusal says that mutation never began"
    );
}

#[test]
fn should_track_a_writable_subvolume_derived_from_a_snapshot_as_its_own_asset() {
    let provider = provider(vec![
        fixture("snapshot-create"),
        ToolOutput::ok(
            fixture("subvol-show-snapshot")
                .stdout()
                .replace("Flags: \t\t\treadonly", "Flags: \t\t\t-"),
        ),
    ]);
    let read_only = support::root_asset();
    let derived = provider
        .derive_writable(
            &read_only,
            Path::new("/mnt/top/@snapshots/ono-a82f-root-rw"),
        )
        .expect("the derived subvolume is created");
    assert_ne!(
        derived.id(),
        read_only.id(),
        "§14.5: a writable subvolume derived from a retained snapshot MUST be tracked separately"
    );
    assert_eq!(
        derived.dependencies(),
        &[read_only.id().clone()],
        "and it records which recovery point it came from"
    );
    assert_eq!(derived.reference(), "/mnt/top/@snapshots/ono-a82f-root-rw");
    assert!(
        derived.exclusions().iter().any(|exclusion| exclusion
            .reason()
            .contains("stops being the captured state")),
        "§14.5: what a writable derivative holds is no longer provably the captured state"
    );
}

#[test]
fn should_report_the_filesystem_and_subvolume_a_protection_action_names() {
    let provider =
        provider(support::discovery_script("subvol-show-root")).for_plan(support::plan_id());
    let candidate = candidate(&provider, support::NGINX_CONF);
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");
    let reference = SubvolumeRef::parse(actions[0].candidate().scope().domain())
        .expect("the scope names a subvolume");
    assert_eq!(reference.filesystem(), FILESYSTEM);
    assert_eq!(
        reference.id(),
        ROOT_ID,
        "Appendix D.6: the protection action carries `filesystem_uuid` and `source_subvol_id`"
    );
    assert!(
        actions[0].summary().contains("/mnt/top/@snapshots/"),
        "and `destination_path`, which is the other half of what D.6 asks it to name"
    );
}

#[test]
fn should_name_a_nested_subvolume_snapshot_after_its_whole_tree_path() {
    assert_eq!(snapshot_name("a82f", "@var/cache"), "ono-a82f-var-cache");
    assert_ne!(
        snapshot_name("a82f", "@var/cache"),
        snapshot_name("a82f", "@cache"),
        "§43.6 and Appendix D.8: `@var/cache` and `@cache` are two subvolumes, and one plan's \
         snapshots of both need two names"
    );
    assert_eq!(
        snapshot_name("a82f", "@var/lib-app"),
        "ono-a82f-var-lib-app"
    );
}

#[test]
fn should_describe_a_writable_snapshot_as_writable_when_read_only_is_not_preferred() {
    let provider = provider(support::discovery_script("subvol-show-root"))
        .with_config(BtrfsConfig::default().preferring_read_only(false));
    let candidate = candidate(&provider, support::NGINX_CONF);
    assert!(
        !candidate.detail().contains("read-only"),
        "§53's `prefer_read_only_snapshots = false` makes the snapshot writable, and the \
         candidate may not promise otherwise: {}",
        candidate.detail()
    );
    assert!(candidate.detail().contains("writable"));
}

#[test]
fn should_record_on_every_created_asset_that_the_plan_s_snapshots_share_no_common_point() {
    let script = [
        support::discovery_script("subvol-show-root"),
        creation_script(),
    ]
    .concat();
    let provider = provider(script).for_plan(support::plan_id());
    let candidate = candidate(&provider, support::NGINX_CONF);
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");
    let asset = provider.create(&actions[0]).expect("creation runs");
    let sequential = asset
        .exclusions()
        .iter()
        .find(|exclusion| exclusion.subject() == ono_recovery_btrfs::SEQUENTIAL_CREATION)
        .expect(
            "Appendix D.7: a member of a multi-subvolume set records that the snapshots were \
             created one after another, on the asset itself, so whoever composes the set cannot \
             miss it",
        );
    assert!(sequential.reason().contains("Appendix D.7"));
}

#[test]
fn should_snapshot_a_nested_subvolume_that_is_not_mounted_itself() {
    let script = [
        vec![
            fixture("fs-show-mount"),
            fixture("subvol-show-nested"),
            fixture("subvol-list-root"),
            fixture("subvol-list-root"),
            fixture("fs-usage"),
        ],
        vec![fixture("subvol-show-nested")],
        creation_script(),
    ]
    .concat();
    let runner = runner(script);
    let provider = BtrfsProvider::new(Arc::clone(&runner) as Arc<dyn ono_change_core::ToolRunner>)
        .with_mounts(mounts())
        .for_plan(support::plan_id());
    let candidate = candidate(&provider, "/mnt/root/var/lib-app");
    let actions = provider
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("planning runs");
    provider
        .create(&actions[0])
        .expect("§14.3: the nested subvolume gets a snapshot of its own");
    let snapshot = runner
        .calls()
        .into_iter()
        .find(|(_, argv)| argv.get(1).is_some_and(|second| second == "snapshot"))
        .expect("a snapshot was taken");
    assert_eq!(
        snapshot.1.get(3).map(String::as_str),
        Some("/mnt/root/var/lib-app"),
        "it is reached through the mount of the subvolume it is nested in, after `btrfs \
         subvolume show` confirmed the path is subvolume 260"
    );
}
