#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! Appendix G.4 and G.5: version variance, conformance, and §11.4's validation.

mod support;

use std::sync::Arc;

use ono_change_core::{
    AssetState, ProviderAvailability, RECOVERY_PROVIDER_CONFORMANCE, RecoveryCapability,
    RecoveryProvider, ScriptedRunner, ToolOutput, ToolRunner,
};
use ono_recovery_btrfs::{BtrfsProvider, PROVIDER_ID, VALIDATED_VERSIONS};
use support::{BTRFS, fixture, mounts, provider, root_asset};

fn probing(version: ToolOutput, installed: bool) -> BtrfsProvider {
    let runner = ScriptedRunner::new(vec![(BTRFS, version)]);
    let runner = if installed {
        runner.with_available(&[BTRFS])
    } else {
        runner
    };
    BtrfsProvider::new(Arc::new(runner) as Arc<dyn ToolRunner>).with_mounts(mounts())
}

#[test]
fn should_report_available_at_a_version_it_has_been_validated_against() {
    match probing(fixture("version"), true).availability() {
        ProviderAvailability::Available { version } => assert_eq!(
            version.as_ref(),
            "v6.17.1",
            "Appendix G.4: the version is inspectable, so a report says which semantics ran"
        ),
        other => panic!("btrfs-progs v6.17.1 is a validated series, and this said {other:?}"),
    }
}

#[test]
fn should_degrade_to_unsupported_at_a_version_it_has_not_validated() {
    match probing(ToolOutput::ok("btrfs-progs v4.4\n"), true).availability() {
        ProviderAvailability::Unsupported { version, reason } => {
            assert_eq!(version.as_ref(), "v4.4");
            assert!(
                reason.contains("validated against"),
                "Appendix G.4: providers degrade to unsupported rather than executing semantics \
                 they have not validated"
            );
        }
        other => panic!("btrfs-progs v4.4 is not a validated series, and this said {other:?}"),
    }
}

#[test]
fn should_report_unavailable_when_the_tool_is_not_installed() {
    match probing(fixture("version"), false).availability() {
        ProviderAvailability::Unavailable { reason } => assert!(
            reason.contains("not installed"),
            "§54.4: a provider whose tool is absent says so rather than answering discovery with \
             nothing"
        ),
        other => panic!("with no `btrfs` on the host the provider is unavailable, not {other:?}"),
    }
}

#[test]
fn should_report_unavailable_when_the_version_cannot_be_read() {
    match probing(ToolOutput::ok("a stray line\n"), true).availability() {
        ProviderAvailability::Unavailable { reason } => assert!(
            reason.contains("could not be established"),
            "Appendix G.4: a provider that cannot tell which version it is talking to does not \
             guess that it is a validated one"
        ),
        other => panic!("an unreadable version leaves the provider unavailable, not {other:?}"),
    }
}

#[test]
fn should_declare_every_capability_a_recovery_provider_owes() {
    let capabilities = provider(Vec::new()).capabilities();
    assert_eq!(capabilities.provider(), PROVIDER_ID);
    assert!(
        capabilities.missing_required().is_empty(),
        "§12.2: discover, prepare, restore, cleanup and estimate-cost are required of every \
         recovery provider"
    );
    assert_eq!(
        capabilities.conformance(),
        RECOVERY_PROVIDER_CONFORMANCE,
        "Appendix G.5: a provider advertises the conformance version it was built against"
    );
    assert!(
        !capabilities.has_recovery(RecoveryCapability::Quiesce),
        "§39.3: nothing here quiesces an application, and a capability not declared is a promise \
         not made"
    );
}

#[test]
fn should_record_the_tool_versions_the_provider_was_tested_against() {
    let capabilities = provider(Vec::new()).capabilities();
    let recorded: Vec<String> = capabilities
        .tool_versions()
        .iter()
        .map(|(_, version)| version.to_string())
        .collect();
    assert_eq!(
        recorded,
        VALIDATED_VERSIONS
            .iter()
            .map(|version| (*version).to_owned())
            .collect::<Vec<_>>(),
        "Appendix G.4 and G.5: the versions a provider was validated against travel with its \
         declaration rather than living in a comment"
    );
    assert!(!VALIDATED_VERSIONS.is_empty());
}

#[test]
fn should_call_the_provider_by_the_id_the_specification_registers() {
    assert_eq!(
        provider(Vec::new()).id(),
        "ono.recovery.btrfs",
        "§12.1: the provider id is the name a plan binds to"
    );
}

#[test]
fn should_validate_a_snapshot_that_exists_and_still_matches_its_source() {
    let provider = provider(vec![
        fixture("subvol-show-snapshot"),
        fixture("snapshot-ro-flag"),
        fixture("subvol-show-root"),
    ]);
    let validation = provider
        .validate(&root_asset())
        .expect("the validation runs");
    assert!(
        validation.is_complete(),
        "§11.4: existence, identity, scope, restore availability and permissions were all checked \
         and all held: {:?}",
        validation.failures()
    );
    assert_eq!(
        root_asset().validated(validation).state(),
        AssetState::Ready,
        "§11.4: only a validation that passed makes an asset usable"
    );
}

#[test]
fn should_refuse_to_call_a_vanished_snapshot_valid() {
    let provider = provider(vec![fixture("subvol-show-missing")]);
    let validation = provider
        .validate(&root_asset())
        .expect("the validation runs");
    assert!(
        !validation.is_complete(),
        "Appendix G.2's vanished snapshot: an asset whose object is gone is not protection"
    );
    assert_eq!(
        root_asset().validated(validation).state(),
        AssetState::Invalid
    );
}

#[test]
fn should_refuse_to_call_a_writable_recovery_point_identical_to_what_was_captured() {
    let provider = provider(vec![
        fixture("subvol-show-snapshot"),
        ToolOutput::ok("ro=false\n"),
        fixture("subvol-show-root"),
    ]);
    let validation = provider
        .validate(&root_asset())
        .expect("the validation runs");
    assert!(
        validation
            .failures()
            .contains(&"the asset's identity does not match the planned source"),
        "§14.5: a retained recovery snapshot is read-only, and a writable one no longer provably \
         holds the state it captured"
    );
}

#[test]
fn should_refuse_to_validate_when_the_metadata_cannot_be_read_at_all() {
    let provider = provider(vec![fixture("unprivileged-list")]);
    let error = provider
        .validate(&root_asset())
        .expect_err("§56.3: a check that could not be made is not a check that failed");
    assert_eq!(error.code().name(), "recovery.provider_unavailable");
}
