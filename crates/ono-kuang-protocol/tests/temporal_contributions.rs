//! What a package declares and says on the wire about time (v0.5 §37.2–§37.6, §30.7).
//!
//! §37.1 draws the line this suite holds: "KUANG/11 may extend history and causality, but Ono
//! core retains authority over identity, evidence classes, causal labels, capability policy and
//! rendering truth." So a package declares a temporal source, a causal rule and a timeline view
//! in the same shapes core uses, and everything about who the source *is* stays the host's.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions; a failed precondition should abort loudly"
)]

use ono_kuang_protocol::{
    Answer, CausalRuleContribution, ContributionSet, HOST_API, Hello, Manifest,
    TemporalSourceContribution, VIEW_COMPONENTS, method,
};

const MANIFEST: &str = "\
format: kuang-package/1
package:
  id: dev.example.packet-eye
  name: packet-eye
  version: 0.1.0
  description: Watches packets.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: \">=11.1 <12\"
  ono_language: \">=0.2\"
  platforms: [linux-amd64]
runtime:
  kind: native-process
  entry: runtime/eye
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
capabilities:
  optional:
    - temporal.contribute.events: {kinds: [object.changed]}
    - temporal.contribute.causality: {rules: [dev.example.packet-eye.retransmit-to-drop]}
contributions:
  temporal_sources: [contributions/sources.yaml]
  causal_rules: [contributions/rules.yaml]
network:
  outbound: none
";

#[test]
fn should_read_the_temporal_contribution_paths_a_manifest_declares() {
    let manifest = Manifest::parse(MANIFEST).expect("the manifest reads");
    let contributions = manifest
        .contributions
        .as_ref()
        .expect("the package declares contributions");

    assert_eq!(
        contributions.temporal_sources.as_deref(),
        Some(["contributions/sources.yaml".to_owned()].as_slice()),
        "§37.5: a historical query provider is declared before any package code runs"
    );
    assert_eq!(
        contributions.causal_rules.as_deref(),
        Some(["contributions/rules.yaml".to_owned()].as_slice()),
        "§37.4: a third-party causal rule is declared, namespaced and inspectable"
    );
}

#[test]
fn should_refuse_a_contribution_key_the_manifest_vocabulary_does_not_carry() {
    let text = MANIFEST.replace("temporal_sources:", "temporal_sourcez:");
    let error = Manifest::parse(&text).expect_err("`deny_unknown_fields` refuses a typo");

    assert_eq!(
        error.code(),
        ono_kuang_protocol::KuangErrorCode::PackageInvalid
    );
}

#[test]
fn should_carry_a_temporal_source_and_a_causal_rule_across_the_handshake() {
    let hello = Hello {
        format: "kuang-package/1".to_owned(),
        package: "dev.example.packet-eye".to_owned(),
        version: "0.1.0".to_owned(),
        kuang_api: ">=11.1 <12".to_owned(),
        contributions: ContributionSet {
            temporal_sources: vec![TemporalSourceContribution {
                id: "dev.example.packet-eye.temporal-source.flows".to_owned(),
                summary: "Flow records from the collector's archive.".to_owned(),
                schema: "dev.example.packet-eye.flow/1".to_owned(),
                kinds: vec!["object.changed".to_owned()],
                answer: Answer::Bounded,
                coverage: "Whatever the collector retains, stated per query.".to_owned(),
                retained_history: Some("24h".to_owned()),
            }],
            causal_rules: vec![CausalRuleContribution {
                rule_id: "dev.example.packet-eye.retransmit-to-drop".to_owned(),
                relation: "correlated_with".to_owned(),
                strength: "correlated".to_owned(),
                summary: "Retransmissions cluster around interface drops.".to_owned(),
                inputs: vec!["object.changed".to_owned()],
                identity_constraints: "Same interface identity on both sides.".to_owned(),
            }],
            ..ContributionSet::default()
        },
    };

    let text = serde_json::to_string(&hello).expect("a hello encodes");
    let decoded: Hello = serde_json::from_str(&text).expect("a hello decodes");

    assert_eq!(decoded, hello);
    assert!(
        decoded.contributions.temporal_sources[0]
            .answer
            .is_bounded(),
        "§37.5: a historical query provider is bounded, and the host must know before the first \
         record"
    );
}

#[test]
fn should_leave_a_package_that_contributes_nothing_temporal_unchanged_on_the_wire() {
    let hello = Hello {
        format: "kuang-package/1".to_owned(),
        package: "dev.example.echo".to_owned(),
        version: "0.1.0".to_owned(),
        kuang_api: ">=11.1 <12".to_owned(),
        contributions: ContributionSet::default(),
    };

    let text = serde_json::to_string(&hello).expect("a hello encodes");

    assert!(
        !text.contains("temporal_sources") && !text.contains("causal_rules"),
        "an empty contribution carries nothing, so an 11.1 host reads the frame it always read: \
         {text}"
    );
}

#[test]
fn should_offer_a_timeline_component_a_contributed_view_can_be_built_of() {
    assert!(
        VIEW_COMPONENTS.contains(&"Timeline"),
        "§37.6: a plugin view consumes canonical temporal schemas, so the component tree has to \
         have somewhere to put one"
    );
    assert_eq!(
        VIEW_COMPONENTS.len(),
        14,
        "the length is in the type, so the registry and the tree cannot drift"
    );
}

#[test]
fn should_name_one_host_call_per_temporal_capability() {
    let calls = [
        method::TEMPORAL_CONTEXT,
        method::TEMPORAL_QUERY,
        method::TEMPORAL_EVIDENCE,
        method::TEMPORAL_CONTRIBUTE_EVENTS,
        method::TEMPORAL_CONTRIBUTE_CAUSALITY,
        method::TEMPORAL_RECORDER,
    ];

    assert_eq!(
        calls,
        [
            "temporal.context",
            "temporal.query",
            "temporal.evidence",
            "temporal.contribute.events",
            "temporal.contribute.causality",
            "temporal.recorder",
        ],
        "§30.7's six capabilities each reach the host through a call of their own, so a grant \
         and a call are the same question asked twice"
    );
}

#[test]
fn should_move_the_host_api_minor_forward_without_stranding_an_older_package() {
    assert_eq!(
        HOST_API.to_string(),
        "11.3",
        "the temporal domain is additive host surface"
    );
    let range: ono_kuang_protocol::VersionRange = ">=11.1 <12".parse().expect("range parses");
    assert!(
        range.contains(HOST_API),
        "a package declaring >=11.1 still loads: the minor is additive"
    );
}
