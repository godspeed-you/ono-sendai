#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! §37 cleanup, §38 cost, and §14.7's sentence about what a snapshot is not.

mod support;

use std::sync::Arc;

use ono_change_core::{
    RecoveryAsset, RecoveryAssetType, RecoveryObjective, RecoveryProvider, ToolRunner,
};
use ono_recovery_btrfs::{BtrfsConfig, BtrfsProvider, RootRecovery};
use support::{ROOT_ID, VAR_SNAPSHOT, fixture, mounts, provider, root_asset, runner, var_asset};

fn discovery_script() -> Vec<ono_change_core::ToolOutput> {
    vec![
        fixture("fs-show-mount"),
        fixture("subvol-show-root"),
        fixture("subvol-list-root"),
        fixture("subvol-list-root"),
        fixture("fs-usage"),
    ]
}

#[test]
fn should_delete_exactly_the_snapshot_it_was_given_and_nothing_beneath_it() {
    let runner = runner(vec![fixture("delete-readonly")]);
    let provider =
        BtrfsProvider::new(Arc::clone(&runner) as Arc<dyn ToolRunner>).with_mounts(mounts());
    provider
        .cleanup(&root_asset())
        .expect("§37: removing an asset is a provider operation");
    let calls = runner.calls();
    assert_eq!(
        calls[0].1,
        vec![
            "subvolume".to_owned(),
            "delete".to_owned(),
            support::ROOT_SNAPSHOT.to_owned(),
        ],
        "§37: cleanup deletes the exact snapshot path, with no recursive flag — a recursive \
         removal would take the nested subvolumes §14.3 keeps separate with it"
    );
    assert!(
        !calls[0]
            .1
            .iter()
            .any(|argument| argument == "-R" || argument == "--recursive"),
        "and never recursively"
    );
}

#[test]
fn should_refuse_to_delete_something_outside_its_own_recovery_namespace() {
    let provider = provider(Vec::new());
    let stranger = RecoveryAsset::proposed(
        ono_recovery_btrfs::PROVIDER_ID,
        RecoveryAssetType::BtrfsSnapshot,
        "/mnt/root/var",
        support::scope(support::VAR_ID, "@var", &[]),
        jiff::Timestamp::UNIX_EPOCH,
    );
    let error = provider
        .cleanup(&stranger)
        .expect_err("§37 removes what this provider created, not any subvolume it is pointed at");
    assert_eq!(error.code().name(), "recovery.asset_invalid");
}

#[test]
fn should_report_a_failed_deletion_rather_than_calling_the_asset_removed() {
    let provider = provider(vec![support::failure(
        "ERROR: cannot delete '/mnt/top/@snapshots/ono-a82f-root': Operation not permitted",
    )]);
    let error = provider
        .cleanup(&root_asset())
        .expect_err("§37.4: a retained asset is surfaced rather than forgotten");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("Operation not permitted")),
        "and the refusal carries what the filesystem said"
    );
}

#[test]
fn should_never_report_a_snapshot_as_costing_nothing() {
    let provider = provider(vec![fixture("fs-usage")]);
    let cost = provider
        .estimate_cost(&root_asset())
        .expect("§38: the provider reports what an asset costs now");
    assert!(
        cost.is_estimated(),
        "§37.5 and §38.2: Btrfs shares extents, so what a snapshot occupies is an estimate, and \
         `free` is a word §38.2 forbids for it"
    );
    assert_ne!(
        cost.initial_bytes(),
        Some(ono_value::ByteSize::ZERO),
        "§35.3: unknown is null, never zero — a snapshot that shares every extent still costs \
         something the moment anything is rewritten"
    );
    assert_ne!(cost.retained_bytes(), Some(ono_value::ByteSize::ZERO));
    assert!(cost.initial_bytes().is_none() && cost.retained_bytes().is_none());
}

#[test]
fn should_report_no_zero_cost_on_any_path_that_offers_one() {
    let discovered = provider(discovery_script());
    let domain = discovered
        .resolve_domain(support::NGINX_CONF)
        .expect("resolution runs")
        .expect("on Btrfs");
    let candidates = discovered
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs");
    for candidate in &candidates {
        assert!(candidate.cost().is_estimated());
        assert_ne!(
            candidate.cost().initial_bytes(),
            Some(ono_value::ByteSize::ZERO)
        );
        assert_ne!(
            candidate.cost().retained_bytes(),
            Some(ono_value::ByteSize::ZERO),
            "§38.2: no path through this provider reports a copy-on-write snapshot as free"
        );
    }
}

#[test]
fn should_count_a_reboot_as_part_of_what_recovering_the_root_costs() {
    let provider = BtrfsProvider::new(runner(vec![fixture("fs-usage")]))
        .with_mounts(mounts())
        .with_config(BtrfsConfig::default().recovering_root_by(RootRecovery::NextBoot));
    let cost = provider
        .estimate_cost(&root_asset())
        .expect("the cost is reported");
    assert!(
        cost.requires_reboot(),
        "§38.1 lists the reboot requirement among the dimensions a cost has, and §55.4 case 22 \
         wants it visible before execution rather than at it"
    );
    assert!(!cost.requires_offline());
}

#[test]
fn should_not_claim_a_reboot_for_a_subvolume_that_is_not_the_root() {
    let provider = BtrfsProvider::new(runner(vec![fixture("fs-usage")])).with_mounts(mounts());
    let cost = provider
        .estimate_cost(&var_asset(&[]))
        .expect("the cost is reported");
    assert!(
        !cost.requires_reboot(),
        "recovering `@var` does not reboot anything, and a cost that says otherwise is one an \
         operator plans a maintenance window around for no reason"
    );
}

#[test]
fn should_refuse_to_estimate_when_the_filesystem_no_longer_answers() {
    let provider = provider(vec![support::failure("ERROR: not a btrfs filesystem")]);
    assert!(
        provider.estimate_cost(&root_asset()).is_err(),
        "§37.5: a filesystem that no longer answers is a cost question whose answer has changed, \
         and yesterday's number is not it"
    );
}

#[test]
fn should_state_that_a_snapshot_shares_the_storage_failure_domain_of_what_it_protects() {
    let provider = provider(discovery_script());
    let domain = provider
        .resolve_domain(support::NGINX_CONF)
        .expect("resolution runs")
        .expect("on Btrfs");
    let candidate = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs")
        .into_iter()
        .next()
        .expect("one candidate");
    assert!(
        candidate.detail().contains("not a backup"),
        "§14.7: the Btrfs provider MUST explicitly state that snapshots share the same \
         filesystem/storage failure domain unless a separate backup provider exists"
    );
    assert!(
        candidate.exclusions().iter().any(|exclusion| exclusion
            .reason()
            .contains("storage failure domain")
            && exclusion.reason().contains("backup provider")),
        "and it is an exclusion as well as a sentence, so a renderer that shows only what an \
         asset excludes still shows it"
    );
    assert!(
        RecoveryAsset::proposed(
            ono_recovery_btrfs::PROVIDER_ID,
            RecoveryAssetType::BtrfsSnapshot,
            "/mnt/top/@snapshots/x",
            support::scope(ROOT_ID, "@", &[]),
            jiff::Timestamp::UNIX_EPOCH,
        )
        .is_local_recovery_point(),
        "§11.5: and the asset type itself answers the question, so nothing has to remember to ask"
    );
}

#[test]
fn should_say_what_creating_and_restoring_a_snapshot_needs() {
    let provider = provider(discovery_script());
    let domain = provider
        .resolve_domain(support::NGINX_CONF)
        .expect("resolution runs")
        .expect("on Btrfs");
    let candidate = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs")
        .into_iter()
        .next()
        .expect("one candidate");
    assert!(
        candidate
            .creation_requirements()
            .iter()
            .any(|requirement| requirement.contains("Operation not permitted")),
        "§43.4: creating a snapshot needs privilege, and the recorded unprivileged refusal is \
         what happens without it"
    );
    assert!(
        candidate
            .creation_requirements()
            .iter()
            .any(|requirement| requirement.contains("free")),
        "§38.1 and Appendix D.3: room on the filesystem is a precondition of creating the asset"
    );
    assert!(
        candidate
            .restore_requirements()
            .iter()
            .any(|requirement| requirement.contains("next boot")),
        "§14.6: restoring the root subvolume as a whole takes effect on the next boot under the \
         default policy, and that is said at discovery rather than at recovery"
    );
}

#[test]
fn should_keep_the_default_retention_the_specification_names() {
    let asset = root_asset();
    assert_eq!(
        asset.retention().window(),
        ono_change_core::DEFAULT_RETENTION,
        "§37.1: twenty-four hours after successful verification"
    );
    assert!(!asset.retention().is_held());
}

#[test]
fn should_name_the_snapshot_a_cleanup_would_remove() {
    let runner = runner(vec![fixture("delete-readonly")]);
    let provider =
        BtrfsProvider::new(Arc::clone(&runner) as Arc<dyn ToolRunner>).with_mounts(mounts());
    let asset = var_asset(&[]);
    provider.cleanup(&asset).expect("the deletion runs");
    assert_eq!(
        runner.calls()[0].1[2],
        VAR_SNAPSHOT.to_owned(),
        "§37.5: cleanup acts on the asset's own reference, so what is removed is what was listed"
    );
}
