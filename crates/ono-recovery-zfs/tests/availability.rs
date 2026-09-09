//! Version variance and the capability declaration (Appendix G.4, G.5, §12.2).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::sync::Arc;

use ono_change_core::{
    ProviderAvailability, RECOVERY_PROVIDER_CONFORMANCE, RecoveryCapability, RecoveryProvider,
    ScriptedRunner, ToolOutput, ToolRunner,
};
use ono_recovery_zfs::{VALIDATED_VERSIONS, ZFS, ZfsProvider};

use support::{out, provider, runner};

#[test]
fn should_report_available_when_zfs_reports_a_validated_version() {
    let tools = runner(vec![(ZFS, out("version"))]);
    let availability = provider(&tools).availability();
    assert_eq!(
        availability,
        ProviderAvailability::Available {
            version: Arc::from("2.4.1-1ubuntu5.1")
        },
        "Appendix G.4: the recorded pool runs OpenZFS 2.4.1, which this provider validated against"
    );
}

#[test]
fn should_report_unsupported_when_zfs_reports_a_version_this_provider_has_not_validated() {
    let tools = runner(vec![(
        ZFS,
        ToolOutput::ok("zfs-0.8.3-1ubuntu12\nzfs-kmod-0.8.3\n"),
    )]);
    let availability = provider(&tools).availability();
    assert!(
        matches!(availability, ProviderAvailability::Unsupported { .. }),
        "Appendix G.4: a provider degrades rather than executing semantics it has not validated"
    );
    assert!(
        availability
            .reason()
            .is_some_and(|reason| reason.contains("2.4.1")),
        "the refusal names the versions that were validated"
    );
}

#[test]
fn should_report_unavailable_when_the_zfs_binary_is_not_on_this_host() {
    let absent: Arc<dyn ToolRunner> = Arc::new(ScriptedRunner::new(Vec::new()));
    let availability = ZfsProvider::new(absent).availability();
    assert!(
        matches!(availability, ProviderAvailability::Unavailable { .. }),
        "§54.4: a provider whose tool is absent degrades rather than answering with nothing"
    );
}

#[test]
fn should_report_unavailable_when_zfs_version_exits_non_zero() {
    let tools = runner(vec![(ZFS, ToolOutput::failed(1, "no such module"))]);
    assert!(
        matches!(
            provider(&tools).availability(),
            ProviderAvailability::Unavailable { .. }
        ),
        "Appendix G.4: a tool that will not report its version is a tool this provider will not use"
    );
}

#[test]
fn should_report_unavailable_when_zfs_version_prints_something_unrecognised() {
    let tools = runner(vec![(ZFS, ToolOutput::ok("something else entirely\n"))]);
    assert!(
        matches!(
            provider(&tools).availability(),
            ProviderAvailability::Unavailable { .. }
        ),
        "§56.3: an unparseable version is not a version this provider claims to support"
    );
}

#[test]
fn should_declare_every_capability_a_recovery_provider_is_required_to_have() {
    let tools = runner(Vec::new());
    let capabilities = provider(&tools).capabilities();
    assert!(
        capabilities.missing_required().is_empty(),
        "§12.2: discover, prepare, restore, cleanup and estimate-cost are all required"
    );
    assert!(!capabilities.has_recovery(RecoveryCapability::Transaction));
}

#[test]
fn should_advertise_the_conformance_version_and_the_tool_it_was_tested_against() {
    let tools = runner(Vec::new());
    let capabilities = provider(&tools).capabilities();
    assert_eq!(
        capabilities.conformance(),
        RECOVERY_PROVIDER_CONFORMANCE,
        "Appendix G.5: a provider advertises a conformance version"
    );
    assert!(
        capabilities.tool_versions().iter().any(
            |(tool, version)| tool.as_ref() == "zfs" && version.contains(VALIDATED_VERSIONS[0])
        ),
        "Appendix G.4: the versions the provider was tested against are inspectable"
    );
}

#[test]
fn should_name_itself_with_the_provider_id_section_twelve_gives_it() {
    let tools = runner(Vec::new());
    assert_eq!(provider(&tools).id(), "ono.recovery.zfs");
}
