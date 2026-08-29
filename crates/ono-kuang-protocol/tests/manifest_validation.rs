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

#[test]
fn should_carry_adapter_packs_and_the_executables_scope_of_an_adapter_package() {
    // Spec v0.3 §1.22, §1.44: an adapter package declares its packs under contributions and a
    // process.exec scope of executables with the declared-invocations-only policy (ADR-0065).
    let manifest = r#"
format: kuang-package/1
package:
  id: dev.example.users
  name: users
  version: 0.1.0
  description: Accounts as User records.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64]
roles: [adapter]
capabilities:
  optional:
    - process.exec:
        executables: [getent]
        argv_policy: declared-invocations-only
network:
  outbound: none
contributions:
  adapters: [adapters.yaml]
"#;
    let parsed = Manifest::parse(manifest).expect("an adapter package without a runtime is valid");
    assert_eq!(
        parsed
            .contributions
            .as_ref()
            .and_then(|c| c.adapters.clone()),
        Some(vec!["adapters.yaml".to_owned()])
    );
    let exec = parsed
        .optional_capabilities
        .iter()
        .find(|request| request.capability == Capability::ProcessExec)
        .expect("process.exec is requested");
    let scope = exec.scope.as_ref().expect("scoped");
    assert!(scope.contains_key("executables") && scope.contains_key("argv_policy"));
}

#[test]
fn should_refuse_an_annotation_key_outside_the_packages_namespace() {
    // §31.23: annotation keys are namespaced, and declaring them "is what keeps an annotation
    // from being an undeclared schema fork". A key outside the package's own namespace is a
    // claim on someone else's records; `contributions.v1.yaml`'s `id-in-namespace` check makes
    // it `package.invalid` at install time, which is the whole point of manifest-before-code.
    let manifest = valid_manifest().replace(
        "roles: [provider]",
        "contributions:\n  annotations: [ono.risk.score]\nroles: [provider]",
    );
    let error = Manifest::parse(&manifest).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
    assert!(
        error.message().contains("ono.risk.score"),
        "the refusal names the key, got {}",
        error.message()
    );
}

#[test]
fn should_accept_an_annotation_key_inside_the_packages_namespace() {
    let manifest = valid_manifest().replace(
        "roles: [provider]",
        "contributions:\n  annotations: [dev.example.echo.risk.score]\nroles: [provider]",
    );
    let manifest = Manifest::parse(&manifest).expect("the key is the package's own");
    assert_eq!(
        manifest
            .contributions
            .and_then(|paths| paths.annotations)
            .unwrap_or_default(),
        vec!["dev.example.echo.risk.score".to_owned()]
    );
}

#[test]
fn should_refuse_a_view_contribution_this_host_cannot_register() {
    // §31.27 views need `views.open`/`views.submit`/`views.close`, and this host implements no
    // view protocol at all. Accepting the declaration, listing it in `inspect plugin` and then
    // registering nothing tells the operator a view exists when none does — §2.17's rule, and
    // the reason `package.incompatible` names a host feature the system does not provide.
    let manifest = valid_manifest().replace(
        "roles: [provider]",
        "contributions:\n  views: [views/flow.yaml]\nroles: [provider]",
    );
    let parsed = Manifest::parse(&manifest).expect("the shape is valid");
    let error = parsed
        .check_host(HOST_API, "linux-amd64")
        .expect_err("a view contribution has nowhere to be registered");
    assert_eq!(error.code(), KuangErrorCode::PackageIncompatible);
    assert!(
        error.message().contains("view"),
        "the refusal names what is missing, got {}",
        error.message()
    );
}

#[test]
fn should_refuse_a_nested_document_before_it_costs_anything_to_refuse_it() {
    // Found by the §35.6 plugin-protocol fuzz target (ADR-0313). A manifest arrives from a
    // package, so it is somebody else's document; 50 kB of it took thirteen seconds to be turned
    // down, because the YAML parser's cost of refusing deep nesting grows with the square of the
    // depth. The nesting is counted first now, and the count ignores quoting on purpose: the
    // input that got through the first version was one whose unbalanced quote made a
    // quote-tracking scan read the whole bomb as a string.
    for bomb in [
        "{".repeat(50_000),
        format!("{{\"a\":{{\"b\":q\":1,\"c\":{}", "{".repeat(50_000)),
        format!("{}1{}", "{e: ".repeat(50_000), "}".repeat(50_000)),
    ] {
        let started = std::time::Instant::now();
        let error = Manifest::parse(&bomb).expect_err("a bomb is not a manifest");
        let elapsed = started.elapsed();
        assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
        assert!(
            elapsed.as_millis() < 250,
            "refusing a {} byte document took {elapsed:?}; the cost of saying no must not grow \
             with the bomb (spec §49 T7)",
            bomb.len()
        );
    }
}
