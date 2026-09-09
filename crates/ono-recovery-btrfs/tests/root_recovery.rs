#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! §14.6 and Appendix D.9: the three root workflows, told apart.

mod support;

use std::sync::Arc;

use ono_change_core::{RecoveryGoal, RecoveryProvider, RestoreMethod};
use ono_recovery_btrfs::{BtrfsConfig, BtrfsProvider, RootRecovery, SafetyFact};
use support::{
    mounts, root_asset, root_recovery_script, var_asset, var_recovery_script,
};

fn provider_recovering_root_by(policy: RootRecovery) -> BtrfsProvider {
    BtrfsProvider::new(support::runner(root_recovery_script()))
        .with_mounts(mounts())
        .with_files(Arc::new(support::recorded_files()))
        .with_config(BtrfsConfig::default().recovering_root_by(policy))
}

#[test]
fn should_recover_the_root_online_when_named_objects_are_what_must_come_back() {
    let provider = provider_recovering_root_by(RootRecovery::NextBoot);
    let (fragment, checklist) = provider
        .plan_recovery_with_checklist(&root_asset(), None, RecoveryGoal::RestoreChangedObjects)
        .expect("the recovery plans");
    assert_eq!(
        fragment.method(),
        RestoreMethod::SelectiveFileRestore,
        "§14.6's first workflow: an online selective restore. Appendix C.1 prefers it even under \
         a next-boot root policy, because putting one file back needs no reboot at all"
    );
    assert!(!fragment.requires_reboot());
    assert!(
        checklist
            .evidence(SafetyFact::MountAndRebootRequirement)
            .is_some_and(|evidence| evidence.contains("neither an unmount nor a reboot")),
        "§14.6: the chosen method MUST be shown before execution, and this is what it says"
    );
}

#[test]
fn should_recover_the_root_offline_when_the_policy_replaces_the_subvolume() {
    let provider = provider_recovering_root_by(RootRecovery::OfflineSubvolumeReplacement);
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recovery plans");
    assert_eq!(
        fragment.method(),
        RestoreMethod::SubvolumeReplacement,
        "§14.6's second workflow: an offline subvolume replacement"
    );
    assert!(
        fragment.requires_offline(),
        "and it needs the filesystem offline, which is the whole difference from the first"
    );
    assert!(!fragment.requires_reboot());
}

#[test]
fn should_recover_the_root_at_the_next_boot_under_the_default_policy() {
    let provider = provider_recovering_root_by(RootRecovery::NextBoot);
    let (fragment, checklist) = provider
        .plan_recovery_with_checklist(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recovery plans");
    assert_eq!(
        fragment.method(),
        RestoreMethod::OfflineRootRecovery,
        "§14.6's third workflow and §53's default `root_recovery = \"next-boot\"`"
    );
    assert!(
        fragment.requires_reboot(),
        "§55.4 case 22: root recovery requiring a reboot is explicit"
    );
    assert!(
        checklist
            .evidence(SafetyFact::MountAndRebootRequirement)
            .is_some_and(|evidence| evidence.contains("next reboot")),
        "and it is stated before execution rather than discovered during it"
    );
    assert!(
        checklist
            .evidence(SafetyFact::DefaultSubvolumeAndBootImpact)
            .is_some_and(|evidence| evidence.contains("changes what boots")),
        "§56.2: the default-subvolume and boot impact is one of the ten facts, and this recovery \
         changes it"
    );
}

#[test]
fn should_materialise_the_recovery_point_beside_the_root_when_the_policy_stays_online() {
    let provider = provider_recovering_root_by(RootRecovery::OnlineSelectiveRestore);
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recovery plans");
    assert_eq!(
        fragment.method(),
        RestoreMethod::CloneAndCopy,
        "Appendix C.1: an online whole-domain recovery materialises the recovery point as a \
         separate writable subvolume and copies out of it — Btrfs has no in-place primitive to \
         offer instead (§14.4)"
    );
    assert!(!fragment.requires_reboot());
    assert!(!fragment.requires_offline());
}

#[test]
fn should_declare_the_derived_writable_subvolume_before_the_boot_is_pointed_at_it() {
    let provider = provider_recovering_root_by(RootRecovery::NextBoot);
    let fragment = provider
        .plan_recovery(&root_asset(), None, RecoveryGoal::RestoreDomain)
        .expect("the recovery plans");
    let summaries: Vec<&str> = fragment
        .actions()
        .iter()
        .map(ono_change_core::PlanAction::summary)
        .collect();
    assert!(
        summaries[0].contains("writable subvolume"),
        "Appendix D.9: the reference root policy creates a writable recovery subvolume derived \
         from the snapshot before it sets up the next-boot target"
    );
    assert!(
        summaries[1].contains("next boot"),
        "and then points the next boot at it"
    );
    assert!(
        fragment
            .actions()
            .iter()
            .all(ono_change_core::PlanAction::needs_privilege),
        "§43.4: every step of it needs privilege, and the plan says so before it runs"
    );
}

#[test]
fn should_not_treat_a_separately_mounted_subvolume_as_the_root() {
    let provider = BtrfsProvider::new(support::runner(var_recovery_script()))
        .with_mounts(mounts())
        .with_files(Arc::new(support::recorded_files()))
        .with_config(BtrfsConfig::default().recovering_root_by(RootRecovery::NextBoot));
    let fragment = provider
        .plan_recovery(
            &var_asset(&["/mnt/root/var/log/syslog"]),
            None,
            RecoveryGoal::RestoreDomain,
        )
        .expect("the recovery plans");
    assert_eq!(
        fragment.method(),
        RestoreMethod::SubvolumeReplacement,
        "§14.6 is about the root filesystem. `@var` is not the root, so the next-boot policy does \
         not apply to it and no reboot is claimed"
    );
    assert!(!fragment.requires_reboot());
}

#[test]
fn should_recognise_the_root_subvolume_from_the_mount_data() {
    let mounts = mounts();
    let root = mounts
        .for_subvolume_id(support::ROOT_ID)
        .expect("the root subvolume is mounted");
    assert!(
        mounts.is_root_subvolume(root),
        "§14.6: `@` is the subvolume the rest of the tree hangs beneath, which is what makes its \
         recovery a boot question"
    );
    for other in [support::HOME_ID, support::VAR_ID] {
        let mount = mounts.for_subvolume_id(other).expect("also mounted");
        assert!(
            !mounts.is_root_subvolume(mount),
            "and a subvolume mounted inside it is not the root"
        );
    }
    let top = mounts
        .filesystem_tree()
        .expect("the filesystem's top level is mounted too");
    assert!(
        !mounts.is_root_subvolume(top),
        "nor is the filesystem's own top level, which is the container the named subvolumes live \
         in rather than the thing that boots"
    );
}

#[test]
fn should_map_each_method_back_to_the_workflow_section_fourteen_point_six_names() {
    assert_eq!(
        BtrfsProvider::root_workflow(RestoreMethod::SelectiveFileRestore),
        Some(RootRecovery::OnlineSelectiveRestore)
    );
    assert_eq!(
        BtrfsProvider::root_workflow(RestoreMethod::SubvolumeReplacement),
        Some(RootRecovery::OfflineSubvolumeReplacement)
    );
    assert_eq!(
        BtrfsProvider::root_workflow(RestoreMethod::OfflineRootRecovery),
        Some(RootRecovery::NextBoot),
        "§14.6: online selective restore, offline subvolume replacement and next-boot are three \
         distinct answers, and the method a plan declares says which one it is"
    );
    assert_eq!(
        BtrfsProvider::root_workflow(RestoreMethod::DatasetRollback),
        None,
        "§14.4: a dataset rollback is not one of them, and Btrfs is never presented as having one"
    );
}

#[test]
fn should_read_the_root_policy_from_the_token_configuration_uses() {
    assert_eq!(
        RootRecovery::from_token("next-boot"),
        Some(RootRecovery::NextBoot),
        "§53: `recovery.btrfs.root_recovery = \"next-boot\"` is the default"
    );
    assert_eq!(RootRecovery::default(), RootRecovery::NextBoot);
    assert_eq!(
        RootRecovery::from_token("in-place"),
        None,
        "§53: configuration MUST NOT silently weaken an explicit plan requirement, and reading an \
         unknown policy as the default is exactly that"
    );
}
