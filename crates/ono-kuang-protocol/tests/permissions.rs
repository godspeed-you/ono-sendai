//! The permission layer at the manifest boundary (K11P §6–§9, §21.3, §34.1, §34.5, §34.6;
//! ADR-0600): what a package declares is checked against what it asks the broker for, a package
//! that declares nothing gets a derived layer, and an upgrade's delta is what widened.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_kuang_protocol::{
    Capability, ConsentClass, DeltaKind, KuangErrorCode, Manifest, PermissionKind, PermissionPhase,
    PermissionRisk, PermissionSet, ScopeTemplate, delta,
};

/// A `kuang-package/2` manifest in the shape of the Kubernetes reference provider (K11P §8.2).
fn reference_manifest(permissions: &str) -> String {
    format!(
        r#"
format: kuang-package/2
package:
  id: dev.example.cluster
  name: cluster
  version: 0.2.0
  description: A reference provider.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.2 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: native-process
  entry: runtime/cluster
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
capabilities:
  optional:
    - network.connect
    - provider.mutate
    - filesystem.read: {{paths: ["~/.kube/config", "~/.kube/*.yaml"]}}
    - secret.use
    - clock.read
    - relation.write
    - process.exec
network:
  outbound: brokered
contributions:
  relations:
    - "dev.example.cluster.pod/1->dev.example.cluster.node/1"
{permissions}
"#
    )
}

const REFERENCE_PERMISSIONS: &str = r#"
permissions:
  profiles:
    minimal:
      title: Minimal
      permissions: [spatial-relations]
    recommended:
      title: Recommended
      permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations]
    operate:
      title: Observe and change
      permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations, cluster-mutation]
  requests:
    - id: cluster-access
      kind: external-observe
      title: Connect to clusters
      phase: install
      recommended: true
      grants:
        - capability: network.connect
          scope: runtime-derived
    - id: kubeconfig-read
      kind: filesystem-read
      title: Read cluster configuration
      phase: install
      recommended: true
      grants:
        - capability: filesystem.read
          scope: {paths: ["~/.kube/config", "~/.kube/*.yaml"]}
    - id: credential-use
      kind: secret-use
      title: Use credentials without exposing their values
      phase: install
      recommended: true
      grants:
        - capability: secret.use
    - id: spatial-relations
      kind: local-contribution
      title: Add relationships to Ono
      phase: automatic
      recommended: true
      grants:
        - capability: relation.write
          scope: package-contributions
    - id: credential-helper
      kind: execute-helper
      title: Run an external login helper when a context requires it
      phase: jit
      recommended: false
      grants:
        - capability: process.exec
          scope: runtime-derived
    - id: cluster-mutation
      kind: external-change
      title: Change cluster resources
      phase: explicit
      recommended: false
      grants:
        - capability: provider.mutate
          scope: provider-instance
"#;

#[test]
fn should_parse_the_reference_permission_layer_when_every_rule_holds() {
    let manifest = Manifest::parse(&reference_manifest(REFERENCE_PERMISSIONS)).expect("valid");
    let set = manifest.permissions.as_ref().expect("declared");
    assert!(set.declared);
    assert_eq!(set.descriptors.len(), 6);
    let helper = set.descriptor("credential-helper").expect("declared");
    assert_eq!(helper.phase, PermissionPhase::Jit);
    assert_eq!(helper.risk, PermissionRisk::Execute);
    assert_eq!(helper.consent_class(), ConsentClass::Conditional);
    let mutation = set.descriptor("cluster-mutation").expect("declared");
    assert_eq!(mutation.risk, PermissionRisk::Mutate);
    assert_eq!(mutation.phase, PermissionPhase::Explicit);
    let relations = set.descriptor("spatial-relations").expect("declared");
    assert_eq!(relations.consent_class(), ConsentClass::ExtensionLocal);
    assert!(relations.is_automatic());
    assert_eq!(
        set.profile("operate")
            .map(|profile| profile.permissions.len()),
        Some(5)
    );
    assert_eq!(
        set.jit_permission(Capability::ProcessExec)
            .map(|d| d.id.as_str()),
        Some("credential-helper")
    );
}

#[test]
fn should_complete_a_declared_layer_with_derived_descriptors_for_unmapped_capabilities() {
    // `clock.read` is declared and mapped by nothing: it is not hidden, it is derived.
    let manifest = Manifest::parse(&reference_manifest(REFERENCE_PERMISSIONS)).expect("valid");
    let set = manifest.permission_set();
    let clock = set
        .descriptor("clock-read")
        .expect("derived for the unmapped capability");
    assert!(clock.derived);
    assert_eq!(clock.phase, PermissionPhase::Automatic);
    assert_eq!(clock.kind, PermissionKind::LocalContribution);
    // A derived descriptor over a capability the package left out is in no declared profile.
    assert!(
        !set.profile("recommended")
            .expect("declared")
            .permissions
            .contains(&"clock-read".to_owned())
    );
}

#[test]
fn should_refuse_a_permissions_section_under_the_first_package_format() {
    let text =
        reference_manifest(REFERENCE_PERMISSIONS).replace("kuang-package/2", "kuang-package/1");
    let error = Manifest::parse(&text).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
    assert!(
        error.message().contains("kuang-package/2"),
        "the refusal names the format that carries the section: {}",
        error.message()
    );
}

#[test]
fn should_read_a_first_format_manifest_without_permissions_unchanged() {
    let text = reference_manifest("").replace("kuang-package/2", "kuang-package/1");
    let manifest = Manifest::parse(&text).expect("a /1 manifest still reads");
    assert!(manifest.permissions.is_none());
    assert_eq!(manifest.format, "kuang-package/1");
}

#[test]
fn should_derive_a_whole_layer_for_a_package_that_declares_none() {
    let manifest = Manifest::parse(&reference_manifest("")).expect("valid");
    let set = manifest.permission_set();
    assert!(!set.declared);
    assert_eq!(
        set.descriptors.len(),
        7,
        "one derived permission per declared capability"
    );
    let recommended = set.profile("recommended").expect("derived");
    let minimal = set.profile("minimal").expect("derived");
    // Observation and extension-local families are recommended; mutation and helper execution
    // are not (K11P §7, §9.1).
    assert!(
        recommended
            .permissions
            .contains(&"network-connect".to_owned())
    );
    assert!(
        recommended
            .permissions
            .contains(&"filesystem-read".to_owned())
    );
    assert!(
        recommended
            .permissions
            .contains(&"relation-write".to_owned())
    );
    assert!(
        !recommended
            .permissions
            .contains(&"provider-mutate".to_owned())
    );
    assert!(!recommended.permissions.contains(&"process-exec".to_owned()));
    assert_eq!(minimal.permissions, vec!["clock-read", "relation-write"]);
    let exec = set.descriptor("process-exec").expect("derived");
    assert_eq!(exec.phase, PermissionPhase::Jit);
    assert_eq!(exec.title, "Run external programs");
    let relations = set.descriptor("relation-write").expect("derived");
    assert_eq!(
        relations.grants[0].scope,
        Some(ScopeTemplate::PackageContributions),
        "a bare relation.write is bounded to the package's contributions when derived"
    );
    let read = set.descriptor("filesystem-read").expect("derived");
    assert!(
        matches!(&read.grants[0].scope, Some(ScopeTemplate::Concrete(scope)) if scope.contains_key("paths")),
        "the manifest's own scope is kept"
    );
}

#[test]
fn should_refuse_a_derived_jit_phase_for_a_required_capability() {
    // A prompt can never precede a load: a required helper execution is decided explicitly.
    let text = reference_manifest("").replace(
        "capabilities:\n  optional:\n    - network.connect",
        "capabilities:\n  required:\n    - process.exec\n  optional:\n    - network.connect",
    );
    let text = text.replace("    - process.exec\nnetwork:", "network:");
    let manifest = Manifest::parse(&text).expect("valid");
    let set = manifest.permission_set();
    assert_eq!(
        set.descriptor("process-exec").map(|d| d.phase),
        Some(PermissionPhase::Explicit)
    );
}

fn invalid(permissions: &str) -> ono_kuang_protocol::KuangError {
    let error = Manifest::parse(&reference_manifest(permissions)).unwrap_err();
    assert_eq!(
        error.code(),
        KuangErrorCode::PermissionInvalidMapping,
        "{}",
        error.message()
    );
    error
}

#[test]
fn should_refuse_a_recommended_profile_that_carries_mutation_however_it_is_labelled() {
    // K11P §34.6: hidden mutation. The profile says `recommended`; the capability says no.
    let text = REFERENCE_PERMISSIONS.replace(
        "permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations]\n    operate:",
        "permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations, cluster-mutation]\n    operate:",
    );
    let error = invalid(&text);
    assert!(
        error.message().contains("cluster-mutation"),
        "{}",
        error.message()
    );
}

#[test]
fn should_refuse_a_friendly_title_that_lowers_the_risk_of_what_it_grants() {
    // K11P §34.1: "Harmless theme access" over filesystem.write is still destructive.
    let text = reference_manifest(
        r#"
permissions:
  profiles:
    minimal: {permissions: []}
    recommended: {permissions: []}
  requests:
    - id: theme-access
      kind: local-contribution
      title: Harmless theme access
      phase: explicit
      risk: local
      grants:
        - capability: filesystem.write
          scope: {paths: ["~/.config/themes/**"]}
"#,
    )
    .replace(
        "    - process.exec\n",
        "    - process.exec\n    - filesystem.write\n",
    );
    let error = Manifest::parse(&text).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PermissionInvalidMapping);
    assert!(
        error.message().contains("destructive") && error.message().contains("never lower"),
        "{}",
        error.message()
    );
}

#[test]
fn should_refuse_a_destructive_capability_below_the_explicit_phase() {
    let text = reference_manifest(
        r#"
permissions:
  profiles:
    minimal: {permissions: []}
    recommended: {permissions: []}
  requests:
    - id: theme-access
      kind: filesystem-write
      title: Theme access
      phase: install
      grants:
        - capability: filesystem.write
"#,
    )
    .replace(
        "    - process.exec\n",
        "    - process.exec\n    - filesystem.write\n",
    );
    let error = Manifest::parse(&text).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PermissionInvalidMapping);
    assert!(error.message().contains("class E"), "{}", error.message());
}

#[test]
fn should_refuse_a_grant_of_a_capability_the_manifest_does_not_declare() {
    let text =
        REFERENCE_PERMISSIONS.replace("capability: secret.use", "capability: service.mutate");
    let error = invalid(&text);
    assert!(
        error.message().contains("does not declare"),
        "{}",
        error.message()
    );
}

#[test]
fn should_refuse_a_missing_recommended_profile_when_requests_exist() {
    let text = REFERENCE_PERMISSIONS.replace(
        "    recommended:\n      title: Recommended\n      permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations]\n",
        "",
    );
    let error = invalid(&text);
    assert!(
        error.message().contains("`recommended` profile"),
        "{}",
        error.message()
    );
}

#[test]
fn should_refuse_a_profile_naming_an_unknown_permission() {
    let text =
        REFERENCE_PERMISSIONS.replace("[spatial-relations]\n", "[spatial-relations, nonesuch]\n");
    let error = invalid(&text);
    assert!(error.message().contains("nonesuch"), "{}", error.message());
}

#[test]
fn should_refuse_a_title_with_a_line_break_that_could_spoof_the_text_beside_it() {
    let text = REFERENCE_PERMISSIONS.replace(
        "title: Connect to clusters",
        "title: \"Connect to clusters\\nSignature: valid\"",
    );
    let error = invalid(&text);
    assert!(
        error.message().contains("line break"),
        "{}",
        error.message()
    );
}

#[test]
fn should_refuse_a_scope_word_the_family_does_not_accept() {
    let text = REFERENCE_PERMISSIONS.replace(
        "capability: secret.use\n",
        "capability: secret.use\n          scope: package-contributions\n",
    );
    let error = invalid(&text);
    assert!(
        error.message().contains("package-contributions"),
        "{}",
        error.message()
    );
}

#[test]
fn should_refuse_a_scope_key_the_family_does_not_declare() {
    let text = REFERENCE_PERMISSIONS.replace(
        r#"scope: {paths: ["~/.kube/config", "~/.kube/*.yaml"]}"#,
        r#"scope: {units: ["kubelet.service"]}"#,
    );
    let error = invalid(&text);
    assert!(error.message().contains("units"), "{}", error.message());
}

#[test]
fn should_refuse_a_required_capability_decided_just_in_time() {
    let text = reference_manifest(REFERENCE_PERMISSIONS).replace(
        "capabilities:\n  optional:\n    - network.connect",
        "capabilities:\n  required:\n    - process.exec\n  optional:\n    - network.connect",
    );
    let text = text.replace("    - process.exec\nnetwork:", "network:");
    let error = Manifest::parse(&text).unwrap_err();
    assert_eq!(error.code(), KuangErrorCode::PermissionInvalidMapping);
    assert!(
        error.message().contains("precede a load"),
        "{}",
        error.message()
    );
}

#[test]
fn should_answer_an_empty_delta_when_an_upgrade_asks_for_nothing_new() {
    let before = Manifest::parse(&reference_manifest(REFERENCE_PERMISSIONS)).expect("valid");
    let after = Manifest::parse(
        &reference_manifest(REFERENCE_PERMISSIONS).replace("version: 0.2.0", "version: 0.2.1"),
    )
    .expect("valid");
    assert!(
        delta(
            &before.permission_set(),
            &after.permission_set(),
            "recommended"
        )
        .is_empty()
    );
}

#[test]
fn should_detect_a_widened_scope_under_a_reused_permission_id() {
    // K11P §34.5: `kubeconfig-read` moves from ~/.kube/** to ~/** and that is new consent.
    let before = Manifest::parse(&reference_manifest(REFERENCE_PERMISSIONS)).expect("valid");
    let widened = REFERENCE_PERMISSIONS.replace(
        r#"scope: {paths: ["~/.kube/config", "~/.kube/*.yaml"]}"#,
        r#"scope: {paths: ["~/**"]}"#,
    );
    let after = Manifest::parse(&reference_manifest(&widened).replace(
        r#"filesystem.read: {paths: ["~/.kube/config", "~/.kube/*.yaml"]}"#,
        r#"filesystem.read: {paths: ["~/**"]}"#,
    ))
    .expect("valid");
    let entries = delta(
        &before.permission_set(),
        &after.permission_set(),
        "recommended",
    );
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].permission, "kubeconfig-read");
    assert_eq!(entries[0].kind, DeltaKind::ScopeWidened);
    assert!(entries[0].detail.contains("~/**"), "{}", entries[0].detail);
}

#[test]
fn should_detect_a_permission_newly_in_the_recommended_profile() {
    let before = Manifest::parse(&reference_manifest(REFERENCE_PERMISSIONS)).expect("valid");
    let added = REFERENCE_PERMISSIONS
        .replace(
            "permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations]\n    operate:",
            "permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations, cloud-config]\n    operate:",
        )
        .replace(
            "  requests:\n",
            "  requests:\n    - id: cloud-config\n      kind: filesystem-read\n      title: Read cloud configuration\n      phase: install\n      recommended: true\n      grants:\n        - capability: filesystem.read\n          scope: {paths: [\"~/.config/cloud/**\"]}\n",
        );
    let after = Manifest::parse(&reference_manifest(&added)).expect("valid");
    let entries = delta(
        &before.permission_set(),
        &after.permission_set(),
        "recommended",
    );
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].kind, DeltaKind::Added);
    assert_eq!(entries[0].detail, "+ Read cloud configuration");
}

#[test]
fn should_treat_a_repurposed_id_as_new_consent() {
    let before = Manifest::parse(&reference_manifest(REFERENCE_PERMISSIONS)).expect("valid");
    let repurposed = REFERENCE_PERMISSIONS.replace(
        "        - capability: secret.use\n",
        "        - capability: network.connect\n",
    );
    let after = Manifest::parse(&reference_manifest(&repurposed)).expect("valid");
    let entries = delta(
        &before.permission_set(),
        &after.permission_set(),
        "recommended",
    );
    assert!(
        entries.iter().any(
            |entry| entry.permission == "credential-use" && entry.kind == DeltaKind::Repurposed
        ),
        "{entries:?}"
    );
}

#[test]
fn should_keep_the_permission_set_of_a_declared_package_ordered_declared_first() {
    let manifest = Manifest::parse(&reference_manifest(REFERENCE_PERMISSIONS)).expect("valid");
    let set: PermissionSet = manifest.permission_set();
    assert_eq!(set.descriptors[0].id, "cluster-access");
    assert!(set.descriptors.last().expect("one").derived);
}
