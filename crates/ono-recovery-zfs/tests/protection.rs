//! Planning and creating protection (§13.3, §17.2, Appendix D.1, D.3, §2.14).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_change_core::{
    AssetState, ConsistencyClass, NonPersistentReason, PersistenceDomain, ProtectionAction,
    ProtectionMode, RecoveryAssetType, RecoveryCandidate, RecoveryObjective, RecoveryProvider,
    ResolvedMount, RestoreMethod, ToolOutput,
};
use ono_recovery_zfs::{FreeSpaceFloor, GUID_FINGERPRINT, ZFS, ZPOOL};
use ono_value::ByteSize;

use support::{CHILD_DATASET, PARENT_DATASET, Script, code, out, provider, runner};

fn bare(path: &str) -> PersistenceDomain {
    PersistenceDomain::refused(
        path,
        ResolvedMount::new("0:0", "/", "zfs", "-", "/"),
        NonPersistentReason::Unresolved,
        "the caller supplies the path only",
    )
}

/// The exact and recursive candidates the recorded pool offers for `/tank/data`.
fn candidates_for(path: &str) -> Vec<RecoveryCandidate> {
    let tools = Script::survey().runner();
    provider(&tools)
        .discover(&bare(path), RecoveryObjective::PreserveExact)
        .expect("the recorded pool is discoverable")
}

/// Whether a candidate protects more than one dataset at one point (§13.3).
fn spans_datasets(candidate: &RecoveryCandidate) -> bool {
    candidate
        .scope()
        .covers()
        .iter()
        .filter(|covered| !covered.starts_with('/'))
        .count()
        > 1
}

fn recursive_candidate() -> RecoveryCandidate {
    candidates_for("/tank/data")
        .into_iter()
        .find(spans_datasets)
        .expect("§13.3: the recorded layout has a child dataset inside the tree")
}

fn exact_candidate() -> RecoveryCandidate {
    candidates_for("/tank/data/customer/db.sqlite")
        .into_iter()
        .next()
        .expect("a candidate over the child dataset")
}

#[test]
fn should_create_nothing_when_the_policy_is_off() {
    let tools = runner(Vec::new());
    let actions = provider(&tools)
        .plan_protection(&[exact_candidate()], ProtectionMode::Off)
        .expect("`off` is a policy, not a failure");
    assert!(
        actions.is_empty(),
        "§17.2: `off` creates no automatic recovery assets"
    );
}

#[test]
fn should_make_a_protection_action_required_under_prefer() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let actions = provider(&tools)
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect("the recorded pool has room");
    assert!(
        actions.first().expect("one action").is_required(),
        "§2.3: if a required recovery asset cannot be created, mutation MUST NOT begin"
    );
}

#[test]
fn should_make_extra_coverage_optional_under_maximize() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let actions = provider(&tools)
        .plan_protection(&[exact_candidate()], ProtectionMode::Maximize)
        .expect("the recorded pool has room");
    assert!(
        !actions.first().expect("one action").is_required(),
        "§17.2: `maximize` adds coverage whose failure degrades the matrix, not the plan"
    );
}

#[test]
fn should_record_each_dataset_of_one_recursive_creation_individually() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let actions = provider(&tools)
        .plan_protection(&[recursive_candidate()], ProtectionMode::Prefer)
        .expect("the recorded pool has room");
    let references: Vec<&str> = actions
        .iter()
        .map(|action| action.proposed_asset().reference())
        .collect();
    assert_eq!(
        references.len(),
        2,
        "§13.3 and Appendix D.1: one concrete snapshot reference per dataset covered, got {references:?}"
    );
    assert!(
        references
            .iter()
            .any(|reference| reference.starts_with(&format!("{PARENT_DATASET}@")))
    );
    assert!(
        references
            .iter()
            .any(|reference| reference.starts_with(&format!("{CHILD_DATASET}@")))
    );
}

#[test]
fn should_give_each_dataset_of_a_recursive_creation_its_own_asset_identity() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let actions = provider(&tools)
        .plan_protection(&[recursive_candidate()], ProtectionMode::Prefer)
        .expect("the recorded pool has room");
    assert_ne!(
        actions[0].proposed_asset().id(),
        actions[1].proposed_asset().id(),
        "§13.3: each dataset/snapshot identity is recorded individually"
    );
    assert_eq!(actions[0].proposed_asset().scope().domain(), PARENT_DATASET);
    assert_eq!(actions[1].proposed_asset().scope().domain(), CHILD_DATASET);
}

#[test]
fn should_leave_a_planned_asset_proposed_until_something_creates_it() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let actions = provider(&tools)
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect("the recorded pool has room");
    let asset = actions.first().expect("one action").proposed_asset();
    assert_eq!(
        asset.state(),
        AssetState::Proposed,
        "§2.1: creating or inspecting a plan MUST NOT mutate the target system"
    );
    assert_eq!(asset.asset_type(), RecoveryAssetType::ZfsSnapshot);
    assert_eq!(asset.consistency(), ConsistencyClass::FilesystemConsistent);
    assert_eq!(asset.restore_method(), RestoreMethod::SelectiveFileRestore);
}

#[test]
fn should_carry_the_candidates_exclusions_onto_the_asset_it_would_create() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let actions = provider(&tools)
        .plan_protection(
            &[candidates_for("/tank/data/notes.txt")[0].clone()],
            ProtectionMode::Prefer,
        )
        .expect("the recorded pool has room");
    let asset = actions.first().expect("one action").proposed_asset();
    assert!(
        asset
            .exclusions()
            .iter()
            .any(|exclusion| exclusion.subject() == CHILD_DATASET),
        "§13.4 and §11.1: what an asset does not protect travels with the asset"
    );
}

fn created() -> (
    std::sync::Arc<ono_change_core::ScriptedRunner>,
    ono_change_core::RecoveryAsset,
) {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZFS, ToolOutput::ok("")),
        (ZFS, out("list-snapshots")),
    ]);
    let provider = provider(&tools);
    let candidate = RecoveryCandidate::new(
        ono_recovery_zfs::PROVIDER_ID,
        ono_change_core::RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost")
            .covering("rpool/ROOT/debian"),
        ono_change_core::EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        "snapshot rpool/ROOT/debian",
    );
    let asset = ono_change_core::RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        support::ROOT_SNAPSHOT,
        ono_change_core::RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost")
            .covering("rpool/ROOT/debian"),
        support::instant(),
    );
    let action = ProtectionAction::new(
        ono_recovery_zfs::PROVIDER_ID,
        "snapshot rpool/ROOT/debian",
        candidate,
        asset,
    );
    let created = provider
        .create(&action)
        .expect("the recorded pool creates it");
    (tools, created)
}

#[test]
fn should_record_the_snapshot_guid_at_creation_so_identity_can_be_rechecked() {
    let (_, asset) = created();
    assert_eq!(
        asset.captured_state(),
        Some(format!("{GUID_FINGERPRINT}{}", support::ROOT_SNAPSHOT_GUID).as_str()),
        "§56.1 and §11.4: exact snapshot identity is the GUID, recorded when the asset was made"
    );
    assert_eq!(asset.state(), AssetState::Creating);
}

#[test]
fn should_run_zfs_snapshot_with_the_exact_name_and_nothing_else() {
    let (tools, _) = created();
    let call = tools
        .calls()
        .into_iter()
        .find(|(program, argv)| {
            program == ZFS && argv.first().is_some_and(|word| word == "snapshot")
        })
        .expect("create ran `zfs snapshot`");
    assert_eq!(
        call.1,
        vec!["snapshot".to_owned(), support::ROOT_SNAPSHOT.to_owned()],
        "§12.3: a program and an argument vector, with no flag nobody asked for"
    );
}

#[test]
fn should_snapshot_exactly_the_datasets_of_a_multi_dataset_candidate_in_one_creation() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZFS, ToolOutput::ok("")),
        (ZFS, out("list-snapshots")),
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZFS, out("snapshot-exists")),
        (ZFS, out("list-snapshots")),
    ]);
    let provider = provider(&tools);
    // The recorded `list-snapshots.txt` holds exactly this case: `tank/data@ono-b91c-...` and
    // `tank/data/customer@ono-b91c-...`, both made by one `zfs snapshot -r` (§13.3).
    let candidate = RecoveryCandidate::new(
        ono_recovery_zfs::PROVIDER_ID,
        ono_change_core::RecoveryScope::new("zfs-dataset", PARENT_DATASET, "localhost")
            .covering(PARENT_DATASET)
            .covering(CHILD_DATASET),
        ono_change_core::EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        "snapshot -r tank/data, covering 2 dataset(s) at one point",
    );
    for (dataset, reference) in [
        (PARENT_DATASET, support::RECURSIVE_SNAPSHOT),
        (CHILD_DATASET, support::RECURSIVE_CHILD_SNAPSHOT),
    ] {
        let asset = ono_change_core::RecoveryAsset::proposed(
            ono_recovery_zfs::PROVIDER_ID,
            RecoveryAssetType::ZfsSnapshot,
            reference,
            ono_change_core::RecoveryScope::new("zfs-dataset", dataset, "localhost")
                .covering(dataset),
            support::instant(),
        );
        let action = ProtectionAction::new(
            ono_recovery_zfs::PROVIDER_ID,
            format!("snapshot {reference}"),
            candidate.clone(),
            asset,
        );
        provider
            .create(&action)
            .expect("both halves of the recursive creation are recorded");
    }
    let snapshot_calls: Vec<Vec<String>> = tools
        .calls()
        .into_iter()
        .filter(|(program, argv)| {
            program == ZFS && argv.first().is_some_and(|word| word == "snapshot")
        })
        .map(|(_, argv)| argv)
        .collect();
    assert_eq!(snapshot_calls.len(), 2);
    assert_eq!(
        snapshot_calls[0],
        vec![
            "snapshot".to_owned(),
            support::RECURSIVE_SNAPSHOT.to_owned(),
            support::RECURSIVE_CHILD_SNAPSHOT.to_owned()
        ],
        "§13.3: one atomic creation naming exactly the datasets that get an asset — `-r` would \
         also snapshot every out-of-tree child and zvol, which no asset records and no cleanup \
         removes"
    );
    for call in &snapshot_calls {
        assert!(
            !call.contains(&"-r".to_owned()),
            "no recursive flag in any creation, got {call:?}"
        );
    }
}

#[test]
fn should_accept_a_snapshot_the_recursive_sibling_already_made() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZFS, out("snapshot-exists")),
        (ZFS, out("list-snapshots")),
    ]);
    let candidate = RecoveryCandidate::new(
        ono_recovery_zfs::PROVIDER_ID,
        ono_change_core::RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost")
            .covering("rpool/ROOT/debian"),
        ono_change_core::EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        "snapshot rpool/ROOT/debian",
    );
    let asset = ono_change_core::RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        support::ROOT_SNAPSHOT,
        ono_change_core::RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost")
            .covering("rpool/ROOT/debian"),
        support::instant(),
    );
    let action = ProtectionAction::new(
        ono_recovery_zfs::PROVIDER_ID,
        "snapshot rpool/ROOT/debian",
        candidate,
        asset,
    );
    let created = provider(&tools)
        .create(&action)
        .expect("§13.3: one recursive operation made it, and its identity is still recorded");
    assert_eq!(created.state(), AssetState::Creating);
}

#[test]
fn should_refuse_to_create_when_zfs_denies_the_privilege() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZFS, out("unprivileged-snapshot")),
    ]);
    let (_, action) = protection_action();
    let error = provider(&tools)
        .create(&action)
        .expect_err("§43.4: a refusal for want of privilege is named as one");
    assert_eq!(code(&error), "change.privilege_required");
}

#[test]
fn should_refuse_to_create_when_the_dataset_does_not_exist() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZFS, out("snapshot-missing")),
    ]);
    let (_, action) = protection_action();
    let error = provider(&tools)
        .create(&action)
        .expect_err("§2.3: mutation MUST NOT begin without the protection the plan required");
    assert_eq!(code(&error), "recovery.asset_create_failed");
}

#[test]
fn should_refuse_to_claim_protection_a_listing_does_not_confirm() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZFS, ToolOutput::ok("")),
        (ZFS, ToolOutput::ok("")),
    ]);
    let (_, action) = protection_action();
    let error = provider(&tools)
        .create(&action)
        .expect_err("§2.14: exiting zero is not evidence the snapshot exists");
    assert_eq!(code(&error), "recovery.asset_create_failed");
}

fn protection_action() -> (RecoveryCandidate, ProtectionAction) {
    let candidate = RecoveryCandidate::new(
        ono_recovery_zfs::PROVIDER_ID,
        ono_change_core::RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost")
            .covering("rpool/ROOT/debian"),
        ono_change_core::EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        "snapshot rpool/ROOT/debian",
    );
    let asset = ono_change_core::RecoveryAsset::proposed(
        ono_recovery_zfs::PROVIDER_ID,
        RecoveryAssetType::ZfsSnapshot,
        support::ROOT_SNAPSHOT,
        ono_change_core::RecoveryScope::new("zfs-dataset", "rpool/ROOT/debian", "localhost")
            .covering("rpool/ROOT/debian"),
        support::instant(),
    );
    let action = ProtectionAction::new(
        ono_recovery_zfs::PROVIDER_ID,
        "snapshot rpool/ROOT/debian",
        candidate.clone(),
        asset,
    );
    (candidate, action)
}

/// A floor the recorded pool does not meet: it reports about 960 MiB free.
fn floor_above_the_pool() -> ByteSize {
    ByteSize::parse("2GiB").expect("a literal byte size")
}

/// A floor the recorded pool comfortably meets.
fn floor_below_the_pool() -> ByteSize {
    ByteSize::parse("256MiB").expect("a literal byte size")
}

#[test]
fn should_fail_closed_under_prefer_when_the_pool_is_below_the_configured_floor() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let error = provider(&tools)
        .with_free_space_floor(floor_above_the_pool())
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect_err("Appendix D.3: automatic protection fails closed rather than worsening it");
    assert_eq!(code(&error), "recovery.storage_pressure");
}

#[test]
fn should_plan_under_require_and_fail_the_apply_when_the_pool_is_below_the_floor() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let provider = provider(&tools).with_free_space_floor(floor_above_the_pool());
    let actions = provider
        .plan_protection(&[exact_candidate()], ProtectionMode::Require)
        .expect("Appendix D.3: `require` still produces the plan the operator has to see");
    let error = provider
        .create(actions.first().expect("one action"))
        .expect_err("Appendix D.3: `require` policy MUST fail apply");
    assert_eq!(code(&error), "recovery.storage_pressure");
}

#[test]
fn should_plan_normally_when_the_pool_meets_the_configured_floor() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let actions = provider(&tools)
        .with_free_space_floor(floor_below_the_pool())
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect("Appendix D.3: a pool above the floor is protected as usual");
    assert_eq!(actions.len(), 1);
}

#[test]
fn should_name_the_pool_and_the_floor_in_a_storage_pressure_refusal() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let error = provider(&tools)
        .with_free_space_floor(floor_above_the_pool())
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect_err("the pool is below the floor");
    assert_eq!(
        error.metadata().get("scope"),
        Some(&ono_value::Value::string("tank")),
        "Appendix D.3: the refusal names the pool it is about"
    );
}

#[test]
fn should_never_report_a_created_snapshot_as_costing_nothing() {
    let (_, asset) = created();
    assert!(
        asset.cost().is_estimated(),
        "§37.5 and §38.2: ZFS space accounting for a snapshot is not what deleting it frees"
    );
    assert_ne!(
        asset.cost().retained_bytes(),
        Some(ByteSize::ZERO),
        "§38.2: Ono MUST NOT display `free` for a copy-on-write snapshot"
    );
}

#[test]
fn should_emit_no_destructive_flag_anywhere_in_protection() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
        (ZFS, ToolOutput::ok("")),
        (ZFS, out("list-snapshots")),
    ]);
    let provider = provider(&tools);
    let actions = provider
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect("the recorded pool has room");
    let _ = provider.create(actions.first().expect("one action"));
    for (_, argv) in tools.calls() {
        assert!(
            !argv.iter().any(|argument| argument == "-R"),
            "§13.6: Ono never adds a destructive flag, got {argv:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Appendix D.3 and §53: the floor is a share of the pool or a quantity.
// ---------------------------------------------------------------------------------------------

#[test]
fn should_read_the_floor_section_fifty_three_configures_as_a_share_or_a_quantity() {
    assert!(matches!(
        FreeSpaceFloor::parse("10%").expect("§53's default"),
        FreeSpaceFloor::Share(_)
    ));
    assert_eq!(
        FreeSpaceFloor::parse("2GiB").expect("a quantity"),
        FreeSpaceFloor::Bytes(ByteSize::parse("2GiB").expect("a literal"))
    );
    assert_eq!(
        FreeSpaceFloor::parse("0%").expect("no floor"),
        FreeSpaceFloor::None
    );
    assert!(
        FreeSpaceFloor::parse("150%").is_err(),
        "a pool has no more than all of itself free"
    );
    assert!(FreeSpaceFloor::parse("-5%").is_err());
    assert!(FreeSpaceFloor::parse("plenty").is_err());
}

#[test]
fn should_fail_closed_under_prefer_when_the_pool_is_below_a_share_floor() {
    // The recorded `tank` is 1006632960 bytes with 1006297600 free: 99.97% free.
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let error = provider(&tools)
        .with_minimum_free(FreeSpaceFloor::parse("99.99%").expect("a share"))
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect_err(
            "Appendix D.3: below the configured share of the pool, protection fails closed",
        );
    assert_eq!(code(&error), "recovery.storage_pressure");
    let floor = error
        .metadata()
        .get("floor")
        .map(ToString::to_string)
        .unwrap_or_default();
    assert!(
        floor.contains("99.99%"),
        "the refusal states the floor as configured, got {floor}"
    );
}

#[test]
fn should_plan_normally_when_the_pool_meets_the_default_share_floor() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-status")),
    ]);
    let actions = provider(&tools)
        .with_minimum_free(FreeSpaceFloor::parse("10%").expect("§53's default"))
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect("the recorded pool is almost empty");
    assert_eq!(actions.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// §13.1: pool health sufficient for the proposed protection operation.
// ---------------------------------------------------------------------------------------------

/// `zpool list` and `zpool status` with `tank` reported as `state`, composed from the recording by
/// replacing the one word; the recorded pools were healthy.
fn tank_in_state(state: &str) -> Vec<(&'static str, ToolOutput)> {
    let listed = out("zpool-list").stdout().replace(
        "1006297600\t0\t3\tONLINE",
        &format!("1006297600\t0\t3\t{state}"),
    );
    let status = out("zpool-status").stdout().replacen(
        "pool: tank\n state: ONLINE",
        &format!("pool: tank\n state: {state}"),
        1,
    );
    vec![
        (ZPOOL, ToolOutput::ok(listed)),
        (ZPOOL, ToolOutput::ok(status)),
    ]
}

#[test]
fn should_refuse_to_plan_protection_on_a_degraded_pool() {
    let tools = runner(tank_in_state("DEGRADED"));
    let error = provider(&tools)
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect_err("§13.1 and §56.3: no recovery point is made on a pool that is not healthy");
    assert_eq!(code(&error), "recovery.asset_create_failed");
    assert!(error.to_string().contains("DEGRADED") || format!("{error:?}").contains("DEGRADED"));
}

#[test]
fn should_refuse_to_plan_protection_on_a_pool_reporting_data_errors() {
    // Composed from the recording: `tank`'s own `errors:` line, and only that one, is replaced.
    let recorded = out("zpool-status").stdout().to_owned();
    let at = recorded
        .find("pool: tank")
        .expect("the recording has a tank section");
    let (before, tank) = recorded.split_at(at);
    let status = format!(
        "{before}{}",
        tank.replacen(
            "errors: No known data errors",
            "errors: 1 data errors, use '-v' for a list",
            1
        )
    );
    assert!(
        status.contains("1 data errors"),
        "the composition found the tank section"
    );
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, ToolOutput::ok(status)),
    ]);
    let error = provider(&tools)
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect_err("§13.1: a pool reporting data errors is not healthy");
    assert_eq!(code(&error), "recovery.asset_create_failed");
}

#[test]
fn should_refuse_to_plan_protection_when_pool_health_could_not_be_read() {
    let tools = runner(vec![
        (
            ZPOOL,
            ToolOutput::failed(1, "cannot open 'tank': no such pool\n"),
        ),
        (ZPOOL, out("zpool-status")),
    ]);
    let error = provider(&tools)
        .plan_protection(&[exact_candidate()], ProtectionMode::Prefer)
        .expect_err("§56.3: health that was not read is not assumed");
    assert_eq!(code(&error), "recovery.asset_create_failed");
}

#[test]
fn should_plan_under_require_and_fail_the_create_on_a_degraded_pool() {
    let mut answers = tank_in_state("DEGRADED");
    answers.extend(tank_in_state("DEGRADED"));
    let tools = runner(answers);
    let provider = provider(&tools);
    let actions = provider
        .plan_protection(&[exact_candidate()], ProtectionMode::Require)
        .expect("`require` shows the plan the operator has to decide about");
    let error = provider
        .create(actions.first().expect("one action"))
        .expect_err("and the apply fails rather than snapshotting onto a degraded pool");
    assert_eq!(code(&error), "recovery.asset_create_failed");
    assert!(
        !tools
            .calls()
            .iter()
            .any(|(_, argv)| argv.first().is_some_and(|word| word == "snapshot")),
        "nothing was created"
    );
}
