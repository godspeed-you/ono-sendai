//! Snapshot naming and sanitisation (Appendix D.2, §43.6, §12.3).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_change_core::{
    PlanId, ProtectionMode, RecoveryCandidate, RecoveryObjective, RecoveryProvider, RecoveryScope,
    ToolOutput,
};
use ono_recovery_zfs::naming::{
    MAX_NAME_LEN, NO_PLAN, full_name, is_valid_snapshot_name, is_valid_snapshot_part, sanitise,
    snapshot_part, timestamp,
};
use ono_recovery_zfs::{PROVIDER_ID, ZFS, ZPOOL};

use support::{instant, out, plan, provider, runner};

/// Every character OpenZFS refuses inside a name component, and every shell metacharacter.
const HOSTILE: &str = "a@b/c#d%e f\tg;h|i&j$k`l\"m'n(o)p<q>r*s?t[u]v{w}x\\y\nz!~^:.-_0";

#[test]
fn should_produce_a_name_zfs_accepts_from_a_plan_id_full_of_syntax() {
    let part = snapshot_part(Some(HOSTILE), instant());
    assert!(
        is_valid_snapshot_part(&part),
        "Appendix D.2 and §43.6: the generated name complies with ZFS naming rules, got `{part}`"
    );
}

#[test]
fn should_strip_the_characters_zfs_treats_as_syntax_from_a_generated_name() {
    let part = snapshot_part(Some(HOSTILE), instant());
    for forbidden in [
        '@', '/', '#', '%', ' ', '\t', '\n', ';', '|', '&', '$', '`', '"', '\'',
    ] {
        assert!(
            !part.contains(forbidden),
            "§43.6: a provider-generated name must not contain command syntax, found `{forbidden}`"
        );
    }
}

#[test]
fn should_shape_the_name_the_way_appendix_d_two_asks_for() {
    let part = snapshot_part(Some(plan().short()), instant());
    assert!(part.starts_with("ono-"), "Appendix D.2: `ono-<plan>-<utc>`");
    assert!(
        part.ends_with("20260909T203222Z"),
        "the UTC timestamp is basic ISO 8601 to the second, got `{part}`"
    );
    assert!(part.contains(plan().short()));
}

#[test]
fn should_name_an_unattributed_snapshot_so_it_is_distinguishable_from_a_plans() {
    let part = snapshot_part(None, instant());
    assert!(
        part.contains(NO_PLAN),
        "Appendix D.2: a snapshot no plan asked for says so rather than borrowing an id"
    );
    assert!(is_valid_snapshot_part(&part));
}

#[test]
fn should_never_begin_a_name_with_a_character_that_reads_as_an_option() {
    for hostile in ["--force", "-r", "___", "@@@"] {
        let part = snapshot_part(Some(hostile), instant());
        assert!(
            part.starts_with("ono-"),
            "§43.6: `{hostile}` must not survive into the front of a name"
        );
        assert!(is_valid_snapshot_part(&part));
    }
}

#[test]
fn should_keep_a_generated_name_inside_the_length_zfs_permits() {
    let part = snapshot_part(Some(&"z".repeat(4096)), instant());
    assert!(
        part.len() <= MAX_NAME_LEN,
        "OpenZFS bounds a name at {MAX_NAME_LEN} bytes, got {}",
        part.len()
    );
    assert!(is_valid_snapshot_part(&part));
}

#[test]
fn should_collapse_a_run_of_forbidden_characters_to_one_separator() {
    assert_eq!(sanitise("a///b"), "a_b");
    assert_eq!(sanitise("///"), "");
    assert_eq!(sanitise("keep-me.1:2_3"), "keep-me.1:2_3");
}

#[test]
fn should_render_the_timestamp_without_characters_a_listing_cannot_show() {
    let stamped = timestamp(instant());
    assert_eq!(stamped, "20260909T203222Z");
    assert!(is_valid_snapshot_part(&stamped));
}

#[test]
fn should_refuse_a_full_name_whose_snapshot_half_carries_zfs_syntax() {
    assert!(is_valid_snapshot_name(
        "tank/data@ono-a82f-20260909T194500Z"
    ));
    assert!(
        !is_valid_snapshot_name("tank/data@ono-a82f; rm -rf /"),
        "§43.6: a name containing command syntax is not a name this provider hands to ZFS"
    );
    assert!(!is_valid_snapshot_name("tank/data"));
    assert!(!is_valid_snapshot_name("tank//data@x"));
}

#[test]
fn should_join_a_dataset_and_a_snapshot_part_with_the_one_separator_zfs_uses() {
    assert_eq!(
        full_name("tank/data", "ono-a82f").as_ref(),
        "tank/data@ono-a82f"
    );
}

/// A candidate over a dataset whose name is full of syntax, to prove the argument vector holds.
fn hostile_candidate() -> RecoveryCandidate {
    let dataset = "tank/x; rm -rf / #and@more";
    RecoveryCandidate::new(
        PROVIDER_ID,
        RecoveryScope::new("zfs-dataset", dataset, "localhost").covering(dataset),
        ono_change_core::EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        "a dataset name nobody should have created",
    )
}

#[test]
fn should_keep_a_dataset_name_containing_shell_syntax_as_one_argument() {
    let tools = runner(vec![
        (ZPOOL, out("zpool-list")),
        (ZPOOL, out("zpool-list")),
        (ZFS, ToolOutput::ok("")),
        (ZFS, out("list-snapshots")),
    ]);
    let provider = provider(&tools);
    let actions = provider
        .plan_protection(&[hostile_candidate()], ProtectionMode::Prefer)
        .expect("planning a protection over a badly named dataset still plans");
    let action = actions.first().expect("one dataset, one action");
    let _ = provider.create(action);
    let snapshot_call = tools
        .calls()
        .into_iter()
        .find(|(program, argv)| {
            program == ZFS && argv.first().is_some_and(|word| word == "snapshot")
        })
        .expect("create ran `zfs snapshot`");
    assert_eq!(
        snapshot_call.1.len(),
        2,
        "§12.3: `snapshot` and one name — the semicolons never become a second command, got {:?}",
        snapshot_call.1
    );
    assert!(
        snapshot_call.1[1].contains("; rm -rf /"),
        "the name arrives at ZFS exactly as it is, inside one argument"
    );
}

#[test]
fn should_refuse_to_create_a_snapshot_whose_generated_name_zfs_would_not_accept() {
    let tools = runner(vec![(ZPOOL, out("zpool-list"))]);
    let candidate = hostile_candidate();
    let hand_built = ono_change_core::RecoveryAsset::proposed(
        PROVIDER_ID,
        ono_change_core::RecoveryAssetType::ZfsSnapshot,
        "tank/data@ono-a82f; rm -rf /",
        RecoveryScope::new("zfs-dataset", "tank/data", "localhost").covering("tank/data"),
        instant(),
    );
    let action = ono_change_core::ProtectionAction::new(
        PROVIDER_ID,
        "a protection action nobody generated",
        candidate,
        hand_built,
    );
    let error = provider(&tools)
        .create(&action)
        .expect_err("§43.6: an unsanitised name is refused before ZFS ever sees it");
    assert_eq!(error.code().name(), "recovery.asset_create_failed");
}

#[test]
fn should_derive_the_same_name_for_every_dataset_of_one_recursive_creation() {
    let tools = runner(vec![(ZPOOL, out("zpool-list"))]);
    let candidate = RecoveryCandidate::new(
        PROVIDER_ID,
        RecoveryScope::new("zfs-dataset", "tank/data", "localhost")
            .covering("tank/data")
            .covering("tank/data/customer"),
        ono_change_core::EffectDomain::FilesystemPersistent,
        RecoveryObjective::PreserveExact,
        "one recursive creation",
    );
    let actions = provider(&tools)
        .plan_protection(&[candidate], ProtectionMode::Prefer)
        .expect("the recorded pool has room");
    let parts: Vec<String> = actions
        .iter()
        .map(|action| {
            action
                .proposed_asset()
                .reference()
                .split_once('@')
                .expect("a snapshot reference")
                .1
                .to_owned()
        })
        .collect();
    assert_eq!(
        parts.len(),
        2,
        "Appendix D.1: one concrete snapshot reference per dataset covered"
    );
    assert_eq!(
        parts[0], parts[1],
        "§13.3: one logical point in time carries one name across every dataset"
    );
}

#[test]
fn should_use_the_plan_short_id_the_asset_is_attributed_to() {
    let other = PlanId::of("session-2", "1788985943", "another intent");
    assert_ne!(
        snapshot_part(Some(plan().short()), instant()),
        snapshot_part(Some(other.short()), instant()),
        "Appendix D.2: the name is collision-resistant across plans"
    );
}
