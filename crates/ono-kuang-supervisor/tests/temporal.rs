//! What the host refuses to believe a package about (v0.5 §37.3, §37.4; §48.9 scenarios 46, 47).
//!
//! §37.1 keeps identity, evidence classes, causal labels, capability policy and rendering truth
//! with Ono core. The three sentences that make that structural rather than aspirational are
//! held here: a package cannot forge a source, cannot assert an object it could not see, and
//! cannot make a causal claim stronger than `asserted` unless the host contract says so about it
//! by name.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions; a failed precondition should abort loudly"
)]

use ono_kuang_supervisor::{
    AuthoritativeDomains, CONTRIBUTED_STRENGTH_CEILING, Contribution, ContributionLimits,
    ContributionRefusal, VisibleSchemas,
};
use ono_temporal_core::EvidenceStrength;
use serde_json::json;

/// The instant the host clock reads throughout, so nothing here depends on a real one.
fn now() -> jiff::Timestamp {
    "2026-08-31T12:00:00Z".parse().expect("a fixed instant")
}

/// A package that can resolve its own flow objects and nothing else.
fn packet_eye() -> Contribution {
    Contribution::new("dev.example.packet-eye")
        .seeing(VisibleSchemas::of([
            "dev.example.packet-eye.flow/1".to_owned()
        ]))
        .declaring(["dev.example.packet-eye.retransmit-to-drop".to_owned()])
}

// --- §37.3: what the host validates before a claim becomes evidence ---------------------------

#[test]
fn should_reject_a_contributed_event_about_an_object_outside_the_permitted_providers() {
    let mut package = packet_eye();

    let refusal = package
        .check_events(
            &[json!({
                "kind": "object.appeared",
                "observed_at": "2026-08-31T11:59:00Z",
                "subject": {"schema": "ono.process/1", "label": "nginx"},
            })],
            "flows",
            now(),
        )
        .expect_err("§37.3: a plugin cannot assert an object exists outside what it can resolve");

    assert_eq!(
        refusal,
        ContributionRefusal::Invisible {
            schema: "ono.process/1".to_owned()
        }
    );
    assert_eq!(
        refusal.code(),
        ono_core::ErrorCode::KuangCapabilityScopeViolation,
        "the refusal is a scope violation, in the same words a path outside a granted scope is"
    );
}

#[test]
fn should_accept_a_contributed_event_about_the_packages_own_objects() {
    let mut package = packet_eye();

    let claims = package
        .check_events(
            &[json!({
                "kind": "object.changed",
                "source_time": "2026-08-31T11:58:00Z",
                "observed_at": "2026-08-31T11:59:00Z",
                "subject": {"schema": "dev.example.packet-eye.flow/1"},
            })],
            "flows",
            now(),
        )
        .expect("a package may speak about what it can see");

    assert_eq!(claims.len(), 1);
    assert_eq!(
        claims[0].source.as_str(),
        "kuang:dev.example.packet-eye/flows",
        "§37.3: the host owns attribution, so a package cannot forge a source"
    );
    assert_eq!(
        claims[0].source_time,
        Some("2026-08-31T11:58:00Z".parse().expect("a fixed instant")),
        "§3.3: the source's own time survives validation"
    );
}

#[test]
fn should_reject_an_event_naming_a_kind_ono_does_not_have() {
    let mut package = packet_eye();

    let refusal = package
        .check_events(
            &[json!({"kind": "flow.retransmitted", "observed_at": "2026-08-31T11:59:00Z"})],
            "flows",
            now(),
        )
        .expect_err("§37.1: core retains authority over the event vocabulary");

    assert_eq!(
        refusal,
        ContributionRefusal::UnknownKind {
            kind: "flow.retransmitted".to_owned()
        }
    );
    assert!(
        refusal.message().contains("subtype"),
        "and it says where a package's own refinement belongs: {}",
        refusal.message()
    );
}

#[test]
fn should_reject_an_event_with_no_instant_it_could_be_placed_at() {
    let mut package = packet_eye();

    let refusal = package
        .check_events(&[json!({"kind": "object.observed"})], "flows", now())
        .expect_err("§37.3: the host validates timestamps");

    assert!(matches!(refusal, ContributionRefusal::Schema { .. }));
}

#[test]
fn should_reject_an_event_stamped_after_the_host_clock() {
    let mut package = packet_eye();

    let refusal = package
        .check_events(
            &[json!({"kind": "object.observed", "observed_at": "2026-09-01T00:00:00Z"})],
            "flows",
            now(),
        )
        .expect_err("an event stamped in the future reorders a timeline it was never part of");

    assert!(matches!(refusal, ContributionRefusal::Timestamp { .. }));
}

#[test]
fn should_reject_a_call_carrying_more_events_than_one_call_may() {
    let mut package = packet_eye().with_limits(ContributionLimits {
        max_events_per_call: 2,
        ..ContributionLimits::default()
    });
    let event = json!({"kind": "object.observed", "observed_at": "2026-08-31T11:59:00Z"});

    let refusal = package
        .check_events(&[event.clone(), event.clone(), event], "flows", now())
        .expect_err("§37.3: the host enforces event size limits");

    assert_eq!(refusal, ContributionRefusal::TooMany { count: 3, limit: 2 });
    assert_eq!(refusal.code(), ono_core::ErrorCode::ResourceItemLimit);
}

#[test]
fn should_reject_an_event_larger_than_one_event_may_be() {
    let mut package = packet_eye().with_limits(ContributionLimits {
        max_event_bytes: 128,
        ..ContributionLimits::default()
    });

    let refusal = package
        .check_events(
            &[json!({
                "kind": "object.observed",
                "observed_at": "2026-08-31T11:59:00Z",
                "payload": {"body": "x".repeat(512)},
            })],
            "flows",
            now(),
        )
        .expect_err("§37.3: the host enforces event size limits");

    assert!(matches!(refusal, ContributionRefusal::TooLarge { .. }));
    assert_eq!(refusal.code(), ono_core::ErrorCode::ResourceByteLimit);
}

#[test]
fn should_reject_a_contribution_beyond_the_rate_the_host_allows() {
    let mut package = packet_eye().with_limits(ContributionLimits {
        max_events_per_window: 2,
        rate_window_seconds: 60,
        ..ContributionLimits::default()
    });
    let event = json!({"kind": "object.observed", "observed_at": "2026-08-31T11:59:00Z"});

    package
        .check_events(&[event.clone(), event.clone()], "flows", now())
        .expect("the first two fit");
    let refusal = package
        .check_events(std::slice::from_ref(&event), "flows", now())
        .expect_err("§37.3: the host enforces a rate limit");

    assert!(matches!(refusal, ContributionRefusal::TooFast { .. }));

    let later: jiff::Timestamp = "2026-08-31T12:02:00Z".parse().expect("a fixed instant");
    package
        .check_events(std::slice::from_ref(&event), "flows", later)
        .expect("the window moves on, and the package is not punished forever");
}

// --- §37.4: what a package may claim about why -------------------------------------------------

#[test]
fn should_hold_a_plugin_causal_claim_to_asserted_when_the_host_trusts_nobody() {
    let package = packet_eye();

    let (rule, strength) = package
        .check_link(
            &json!({
                "rule": "dev.example.packet-eye.retransmit-to-drop",
                "relation": "caused_by",
                "strength": "authoritative",
            }),
            "network",
        )
        .expect("the link names a declared rule and a real class");

    assert_eq!(rule, "dev.example.packet-eye.retransmit-to-drop");
    assert_eq!(
        strength,
        EvidenceStrength::Asserted,
        "§37.4: plugin causal strength MUST NOT exceed `asserted`"
    );
}

#[test]
fn should_keep_a_plugin_correlation_a_correlation() {
    let package = packet_eye();

    let (_, strength) = package
        .check_link(
            &json!({
                "rule": "dev.example.packet-eye.retransmit-to-drop",
                "relation": "correlated_with",
                "strength": "correlated",
            }),
            "network",
        )
        .expect("a correlation rule is a rule");

    assert_eq!(
        strength,
        EvidenceStrength::Correlated,
        "§15.5: correlation stays structurally distinct from causation, whoever contributed it"
    );
    assert!(
        !ono_temporal_core::CausalRelation::from_name("correlated_with")
            .expect("the class exists")
            .is_causal(),
        "§15.6: a renderer keys its wording on the class, and the class did not change"
    );
}

#[test]
fn should_let_a_package_the_host_trusts_be_authoritative_for_that_domain_only() {
    let package = packet_eye()
        .trusted_for(AuthoritativeDomains::none().trusting("dev.example.packet-eye", "network"));
    let link = json!({
        "rule": "dev.example.packet-eye.retransmit-to-drop",
        "relation": "caused_by",
        "strength": "authoritative",
    });

    let (_, trusted) = package
        .check_link(&link, "network")
        .expect("the link is well formed");
    let (_, untrusted) = package
        .check_link(&link, "storage")
        .expect("the link is well formed");

    assert_eq!(
        trusted,
        EvidenceStrength::Authoritative,
        "§37.4's exception is exactly a package and a domain the host contract names"
    );
    assert_eq!(
        untrusted,
        EvidenceStrength::Asserted,
        "and it is not a licence for every other domain"
    );
}

#[test]
fn should_refuse_a_contributed_rule_that_claims_the_projects_namespace() {
    let package = packet_eye().declaring(["ono.action-to-transaction".to_owned()]);

    let refusal = package
        .check_link(
            &json!({
                "rule": "ono.action-to-transaction",
                "relation": "caused_by",
                "strength": "asserted",
            }),
            "network",
        )
        .expect_err("§31.5: `ono.*` belongs to the project");

    assert_eq!(
        refusal,
        ContributionRefusal::ReservedNamespace {
            rule_id: "ono.action-to-transaction".to_owned()
        }
    );
}

#[test]
fn should_refuse_a_link_from_a_rule_the_package_never_declared() {
    let package = packet_eye();

    let refusal = package
        .check_link(
            &json!({
                "rule": "dev.example.packet-eye.invented-on-the-spot",
                "relation": "caused_by",
                "strength": "asserted",
            }),
            "network",
        )
        .expect_err("§15.8: a link names a registered rule a reader can go and read");

    assert!(matches!(
        refusal,
        ContributionRefusal::UndeclaredRule { .. }
    ));
}

#[test]
fn should_refuse_a_link_naming_a_relation_class_ono_does_not_have() {
    let package = packet_eye();

    let refusal = package
        .check_link(
            &json!({
                "rule": "dev.example.packet-eye.retransmit-to-drop",
                "relation": "probably_caused",
                "strength": "asserted",
            }),
            "network",
        )
        .expect_err("§37.1: core retains authority over causal labels");

    assert_eq!(
        refusal,
        ContributionRefusal::UnknownRelation {
            relation: "probably_caused".to_owned()
        }
    );
}

// --- the ceiling is the registry's, not this crate's -------------------------------------------

#[test]
fn should_enforce_the_strength_ceiling_the_causal_registry_declares() {
    // The suite reads the contract rather than a second copy of the specification, exactly as
    // the remote suites read `limits.yaml` (§52.2). A registry that lowered the ceiling and an
    // implementation that did not would be drift the gate cannot see any other way.
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/contracts/temporal/causality.yaml"),
    )
    .expect("the causal registry is part of the repository");
    let document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&text).expect("the causal registry is YAML");

    let declared = document
        .get("contributed_rules")
        .and_then(|section| section.get("strength_ceiling"))
        .and_then(serde_yaml_ng::Value::as_str)
        .expect("`contributed_rules.strength_ceiling` is declared");

    assert_eq!(
        declared,
        CONTRIBUTED_STRENGTH_CEILING.as_str(),
        "the ceiling the host enforces is the one the registry declares"
    );

    let capability = document
        .get("contributed_rules")
        .and_then(|section| section.get("capability"))
        .and_then(serde_yaml_ng::Value::as_str)
        .expect("`contributed_rules.capability` is declared");
    assert_eq!(
        capability,
        ono_kuang_protocol::Capability::TemporalContributeCausality.id(),
        "and the capability the registry names is the one the broker checks"
    );
}
