//! Manifest validation conformance (spec §31.74 "manifest validation", spec §31.5, §31.7).
//!
//! Every case asserts the outcome the contract names: a manifest that breaks a rule is
//! `package.invalid` — fail closed, never a warning — and a valid one parses into the shape the
//! supervisor negotiates from.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_kuang_protocol::{
    ApiVersion, Capability, HOST_API, KuangErrorCode, Manifest, Outbound, Persistence, Role,
    RuntimeKind,
};

fn valid_manifest() -> String {
    r#"
format: kuang-package/1
package:
  id: dev.example.echo
  name: echo
  version: 0.1.0
  description: Emits what it is asked to emit.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: native-process
  entry: runtime/echo
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
capabilities:
  required:
    - clock.read
  optional:
    - filesystem.read: {paths: ["/tmp/**"]}
network:
  outbound: none
"#
    .to_owned()
}

#[test]
fn should_parse_a_valid_manifest_when_every_rule_holds() {
    let manifest = Manifest::parse(&valid_manifest()).expect("the manifest is valid");
    assert_eq!(manifest.package.id, "dev.example.echo");
    assert_eq!(manifest.roles, vec![Role::Provider]);
    assert_eq!(manifest.network.outbound, Outbound::None);
    assert_eq!(
        manifest.runtime.as_ref().map(|runtime| runtime.kind),
        Some(RuntimeKind::NativeProcess)
    );
    assert_eq!(
        manifest.runtime.map(|runtime| runtime.memory_max),
        Some(64 * 1024 * 1024)
    );
    let required: Vec<Capability> = manifest
        .required_capabilities
        .iter()
        .map(|request| request.capability)
        .collect();
    assert_eq!(required, vec![Capability::ClockRead]);
    let optional = &manifest.optional_capabilities[0];
    assert_eq!(optional.capability, Capability::FilesystemRead);
    assert!(
        optional
            .scope
            .as_ref()
            .is_some_and(|scope| scope.contains_key("paths"))
    );
}

#[test]
fn should_refuse_a_third_party_claim_on_the_ono_namespace() {
    // Spec §31.5: no package MAY claim `ono.*` unless shipped by the Ono project.
    let manifest = valid_manifest()
        .replace("id: dev.example.echo", "id: ono.echo")
        .replace("publisher: dev.example", "publisher: ono");
    let error = Manifest::parse(&manifest).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
    assert!(error.message().contains("ono.*"), "{}", error.message());
}

#[test]
fn should_refuse_a_package_id_outside_its_publisher_namespace() {
    let manifest = valid_manifest().replace("publisher: dev.example", "publisher: dev.other");
    let error = Manifest::parse(&manifest).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
}

#[test]
fn should_fail_closed_on_an_unknown_key_in_a_closed_section() {
    // Spec §31.7: unknown mandatory fields MUST fail closed (ADR-0022 §10).
    let manifest = valid_manifest().replace("  license: MIT", "  license: MIT\n  telemetry: on");
    let error = Manifest::parse(&manifest).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
}

#[test]
fn should_refuse_a_manifest_without_a_network_section() {
    // `network` is required even when the answer is `none` (spec §31.21, ADR-0022 §10).
    let manifest = valid_manifest().replace("network:\n  outbound: none\n", "");
    let error = Manifest::parse(&manifest).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
}

#[test]
fn should_refuse_an_unknown_capability_id_rather_than_ignore_it() {
    let manifest = valid_manifest().replace("- clock.read", "- clock.bend");
    let error = Manifest::parse(&manifest).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
}

#[test]
fn should_refuse_a_scope_key_the_capability_does_not_declare() {
    let manifest = valid_manifest().replace(
        "filesystem.read: {paths: [\"/tmp/**\"]}",
        "filesystem.read: {hosts: [\"a\"]}",
    );
    let error = Manifest::parse(&manifest).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
}

#[test]
fn should_report_package_incompatible_when_the_host_api_is_outside_the_range() {
    let manifest = valid_manifest().replace(">=11.1 <12", ">=12");
    let manifest = Manifest::parse(&manifest).expect("shape is valid");
    let error = manifest.check_host(HOST_API, "linux-amd64").unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageIncompatible);
}

#[test]
fn should_report_package_incompatible_when_the_platform_is_missing() {
    let manifest = Manifest::parse(&valid_manifest()).expect("valid");
    let error = manifest.check_host(HOST_API, "darwin-arm64").unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageIncompatible);
}

#[test]
fn should_accept_a_future_minor_of_the_same_major_when_the_range_allows_it() {
    // Spec §31.63: the minor is what negotiation resolves; the 11 does not move.
    let manifest = Manifest::parse(&valid_manifest()).expect("valid");
    manifest
        .check_host(
            ApiVersion {
                major: 11,
                minor: 3,
            },
            "linux-amd64",
        )
        .expect("11.3 satisfies >=11.1 <12");
}

#[test]
fn should_require_quota_and_version_for_persistent_state() {
    let manifest =
        valid_manifest().replace("network:", "state:\n  persistence: persistent\nnetwork:");
    let error = Manifest::parse(&manifest).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
}

#[test]
fn should_require_the_state_persist_capability_for_persistent_state() {
    // Spec §31.31: `persistent` additionally requires the `state.persist` capability.
    let manifest = valid_manifest().replace(
        "network:",
        "state:\n  persistence: persistent\n  quota: 1MiB\n  version: 1\nnetwork:",
    );
    let error = Manifest::parse(&manifest).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);

    let manifest = valid_manifest()
        .replace(
            "network:",
            "state:\n  persistence: persistent\n  quota: 1MiB\n  version: 1\nnetwork:",
        )
        .replace("- clock.read", "- clock.read\n    - state.persist");
    let manifest = Manifest::parse(&manifest).expect("state.persist satisfies the rule");
    assert_eq!(
        manifest.state.map(|state| state.persistence),
        Some(Persistence::Persistent)
    );
}

#[test]
fn should_refuse_destinations_that_contradict_outbound_none() {
    let manifest = valid_manifest().replace(
        "  outbound: none",
        "  outbound: none\n  destinations:\n    - {host: example.org}",
    );
    let error = Manifest::parse(&manifest).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
}
