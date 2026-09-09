//! The causal rule registry (spec v0.5 §15.8, §36.4, §41): what the engine runs, what the
//! contract declares, and the guarantee that a causal link cannot exist without its evidence.
//!
//! §15.8: "Every built-in rule that emits `caused_by`, `triggered_by` or `resulted_in` MUST be
//! machine-readable and inspectable." These tests hold the engine against
//! `docs/contracts/temporal/causality.yaml` in both directions, so a rule in the code and not in
//! the registry is a rule nobody can audit, and a rule in the registry and not in the code is a
//! causal claim Ono cannot make.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

mod causal_fixture;

use std::collections::BTreeSet;
use std::path::PathBuf;

use causal_fixture::{World, source};
use ono_temporal_core::{
    CausalRelation, CausalRuleId, EventId, EventKind, EvidenceSource, EvidenceStrength,
};
use ono_temporal_query::causal::{
    BUILTIN_CAUSAL_RULE_IDS, BUILTIN_CORRELATION_RULE_IDS, BUILTIN_RULE_IDS, CausalEngine,
    CausalFinding, LinkRefused,
};

fn registry() -> serde_yaml_ng::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/contracts/temporal/causality.yaml");
    let text = std::fs::read_to_string(&path).expect("the causality registry is readable");
    serde_yaml_ng::from_str(&text).expect("the causality registry parses")
}

fn rows(document: &serde_yaml_ng::Value, key: &str) -> Vec<serde_yaml_ng::Value> {
    document
        .get(key)
        .and_then(serde_yaml_ng::Value::as_sequence)
        .expect("the registry carries the list")
        .clone()
}

fn text_at(row: &serde_yaml_ng::Value, key: &str) -> Option<String> {
    row.get(key)
        .and_then(serde_yaml_ng::Value::as_str)
        .map(str::to_owned)
}

fn strings_at(row: &serde_yaml_ng::Value, key: &str) -> Vec<String> {
    row.get(key)
        .and_then(serde_yaml_ng::Value::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_yaml_ng::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn should_run_exactly_the_rules_the_registry_declares_when_the_engine_is_built() {
    let document = registry();
    let declared: BTreeSet<String> = rows(&document, "rules")
        .iter()
        .chain(rows(&document, "correlation_rules").iter())
        .filter_map(|row| text_at(row, "rule_id"))
        .collect();
    let engine = CausalEngine::builtin();
    let running: BTreeSet<String> = engine
        .rule_ids()
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();

    assert_eq!(
        running, declared,
        "v0.5 §15.8: the rules the engine runs and the rules the registry declares are one set"
    );
    let exported: BTreeSet<String> = BUILTIN_RULE_IDS.iter().map(|id| (*id).to_owned()).collect();
    assert_eq!(
        exported, declared,
        "the exported id list is what `xtask spec-check` compares the registry against"
    );
    assert_eq!(
        BUILTIN_RULE_IDS.len(),
        BUILTIN_CAUSAL_RULE_IDS.len() + BUILTIN_CORRELATION_RULE_IDS.len(),
        "the exported list is the two lists and nothing else"
    );
}

#[test]
fn should_match_the_registry_row_when_a_rule_is_inspected() {
    let document = registry();
    let engine = CausalEngine::builtin();
    for (key, expected_causal) in [("rules", true), ("correlation_rules", false)] {
        for row in rows(&document, key) {
            let id = text_at(&row, "rule_id").expect("a row names a rule");
            let rule = engine
                .rule(&id)
                .unwrap_or_else(|| panic!("the engine runs `{id}`"));
            let described = rule.describe();

            assert_eq!(
                described.output_relation.as_str(),
                text_at(&row, "output_relation").unwrap_or_default(),
                "`{id}` emits the relation the registry declares"
            );
            assert_eq!(
                described.output_relation.is_causal(),
                expected_causal,
                "`{id}` is in the list its class belongs to (§15.5)"
            );

            let declared_kinds: BTreeSet<String> =
                strings_at(&row, "input_event_kinds").into_iter().collect();
            let implemented_kinds: BTreeSet<String> = described
                .input_event_kinds
                .iter()
                .map(|kind| kind.as_str().to_owned())
                .collect();
            assert_eq!(
                implemented_kinds, declared_kinds,
                "`{id}` reads the event kinds the registry declares"
            );

            let strengths = row
                .get("required_evidence_strengths")
                .and_then(serde_yaml_ng::Value::as_mapping)
                .expect("a row states its required strengths");
            for (kind, strength) in strengths {
                let kind = kind.as_str().unwrap_or_default();
                let strength = strength.as_str().unwrap_or_default();
                let kind = EventKind::from_name(kind).expect("a declared kind");
                let strength = EvidenceStrength::from_name(strength).expect("a declared strength");
                assert_eq!(
                    described.required_evidence.minimum_for(kind),
                    Some(strength),
                    "`{id}` requires the strength the registry declares for {kind}"
                );
            }

            let declared_sources: BTreeSet<String> =
                strings_at(&row, "provider_source_constraints")
                    .into_iter()
                    .collect();
            let implemented_sources: BTreeSet<String> = described
                .source_constraints
                .iter()
                .map(|constraint| constraint.as_str().to_owned())
                .collect();
            assert_eq!(
                implemented_sources, declared_sources,
                "`{id}` accepts the sources the registry declares"
            );

            assert!(
                !described.identity_constraints.is_empty(),
                "`{id}` states what must be equal for it to fire (§15.8)"
            );
            if expected_causal {
                assert!(
                    described.window.is_none(),
                    "`{id}` is causal, so it has no correlation window (§15.5)"
                );
            } else {
                assert!(
                    described.window.is_some(),
                    "`{id}` is a correlation rule and declares its window (§15.5)"
                );
            }
        }
    }
}

#[test]
fn should_refuse_a_causal_link_when_no_evidence_supports_it() {
    let rule = CausalRuleId::new("ono.process-parent");
    let cause = EventId::parse("@e000000000000000000000a1").expect("a well-formed reference");
    let effect = EventId::parse("@e000000000000000000000a2").expect("a well-formed reference");

    let refused = CausalFinding::emit(
        &rule,
        CausalRelation::CausedBy,
        &cause,
        &effect,
        Vec::new(),
        EvidenceStrength::Authoritative,
        EvidenceSource::recorder(),
    );
    assert_eq!(
        refused,
        Err(LinkRefused::NoEvidence),
        "v0.5 §15.2: a causal link with no evidence is temporal proximity wearing a rule id"
    );
}

#[test]
fn should_refuse_a_link_when_both_ends_are_the_same_event() {
    let mut world = World::new();
    let evidence = world.transaction(
        "linux.systemd-dbus",
        "2026-08-31T14:03:11Z",
        None,
        "/org/freedesktop/systemd1/job/4821",
        EvidenceStrength::Authoritative,
    );
    let rule = CausalRuleId::new("ono.systemd-job-result");
    let event = EventId::parse("@e000000000000000000000a1").expect("a well-formed reference");

    let refused = CausalFinding::emit(
        &rule,
        CausalRelation::CausedBy,
        &event,
        &event,
        vec![evidence],
        EvidenceStrength::Authoritative,
        source("linux.systemd-dbus"),
    );
    assert_eq!(refused, Err(LinkRefused::SelfLink));
}

#[test]
fn should_name_its_rule_source_and_evidence_when_a_link_is_emitted() {
    let mut world = World::new();
    let evidence = world.transaction(
        "linux.systemd-dbus",
        "2026-08-31T14:03:11Z",
        None,
        "/org/freedesktop/systemd1/job/4821",
        EvidenceStrength::Authoritative,
    );
    let rule = CausalRuleId::new("ono.systemd-job-result");
    let cause = EventId::parse("@e000000000000000000000a1").expect("a well-formed reference");
    let effect = EventId::parse("@e000000000000000000000a2").expect("a well-formed reference");

    let finding = CausalFinding::emit(
        &rule,
        CausalRelation::CausedBy,
        &cause,
        &effect,
        vec![evidence.clone()],
        EvidenceStrength::Authoritative,
        source("linux.systemd-dbus"),
    )
    .expect("the link carries a rule, a source and evidence");
    let link = finding.link();

    assert_eq!(link.rule, rule);
    assert_eq!(link.source, source("linux.systemd-dbus"));
    assert_eq!(link.evidence, vec![evidence]);
    assert!(link.is_causal(), "§48.5 scenario 27");
}

#[test]
fn should_refuse_a_model_hypothesis_as_evidence_when_a_source_is_named() {
    // §38.2: an AI hypothesis is an `Inference`, and an `Inference` has no way into this engine.
    // The nine evidence source classes of §7.1 are closed, so a model cannot name itself as one,
    // and every link the engine emits carries evidence ids that resolve to `Evidence` records.
    assert!(EvidenceSource::parse("model:gpt").is_none());
    assert!(EvidenceSource::parse("ai.hypothesis").is_none());
    assert!(EvidenceSource::parse("inference").is_none());
}

/// The §17.7 restart, as a handful of events one source stamped with one job path.
fn job_world() -> (
    Vec<ono_temporal_core::TemporalEvent>,
    ono_temporal_query::causal::CausalContext,
) {
    use causal_fixture::{EventBuilder, service};
    use ono_spatial_core::SpatialType;
    use ono_temporal_query::causal::SYSTEMD_JOB_REMOVED;
    use ono_value::Value;

    let mut world = World::new();
    let unit = service("nginx.service");
    let token = "/org/freedesktop/systemd1/job/4821";
    let job = world.transaction(
        "linux.systemd-dbus",
        "2026-08-31T14:03:11Z",
        Some(&unit),
        token,
        EvidenceStrength::Authoritative,
    );
    let carried = world.transaction(
        "linux.systemd-dbus",
        "2026-08-31T14:03:13Z",
        Some(&unit),
        token,
        EvidenceStrength::Authoritative,
    );
    let observed = world.transition(
        "linux.systemd-dbus",
        "2026-08-31T14:03:13Z",
        &unit,
        "active_state",
        Value::string("activating"),
        Value::string("active"),
        EvidenceStrength::Authoritative,
    );

    let result = EventBuilder::new(EventKind::ProviderEvent, "2026-08-31T14:03:11Z")
        .subtype(SYSTEMD_JOB_REMOVED)
        .provider("systemd")
        .related(&unit, SpatialType::Service, "nginx.service")
        .evidence(&job)
        .seal();
    let became = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:13Z")
        .provider("systemd")
        .subject(&unit, SpatialType::Service, "nginx.service")
        .changed(
            "active_state",
            Value::string("activating"),
            Value::string("active"),
        )
        .evidence(&carried)
        .evidence(&observed)
        .seal();

    let context = world.context();
    (vec![result, became], context)
}

#[test]
fn should_produce_the_same_links_when_the_same_events_arrive_in_another_order() {
    // §41: "Rules MUST be deterministic for the same canonical input", and §47.1 lists causal
    // rule determinism as required unit coverage. The engine canonicalises the input, so the
    // order events happen to arrive in is not part of it.
    let (events, context) = job_world();
    let engine = CausalEngine::builtin();

    let forwards = engine.links(&events, &context);
    let mut backwards_input = events.clone();
    backwards_input.reverse();
    let backwards = engine.links(&backwards_input, &context);

    assert_eq!(forwards, backwards);
    assert!(!forwards.is_empty());
    assert_eq!(
        forwards,
        engine.links(&events, &context),
        "the same call twice is the same answer"
    );
}

#[test]
fn should_bound_the_candidate_set_when_the_ceiling_is_reached() {
    use causal_fixture::{EventBuilder, instant, service};
    use ono_spatial_core::SpatialType;
    use ono_temporal_query::causal::{WhyOptions, WhyRequest};
    use ono_value::Value;

    let mut world = World::new();
    let unit = service("nginx.service");
    let mut events = Vec::new();
    for minute in 0..30_u32 {
        let at = format!("2026-08-31T14:{minute:02}:00Z");
        let seen = world.field(
            "linux.systemd-dbus",
            &at,
            &unit,
            "active_state",
            Value::string("active"),
            EvidenceStrength::Authoritative,
        );
        events.push(
            EventBuilder::new(EventKind::ObjectChanged, &at)
                .provider("systemd")
                .subject(&unit, SpatialType::Service, "nginx.service")
                .changed(
                    "active_state",
                    Value::string("activating"),
                    Value::string("active"),
                )
                .evidence(&seen)
                .seal(),
        );
    }
    let context = world.context();
    let engine = CausalEngine::builtin();

    // `temporal.why.max_candidates` keeps the newest events, so the answer is still about the
    // most recent transition (§16.3) and the work stays bounded (§32.3).
    let explanation = engine
        .explain(
            &WhyRequest::target(&unit, "nginx.service"),
            &events,
            &context,
            &WhyOptions::at(instant("2026-08-31T15:00:00Z")).with_max_candidates(5),
        )
        .expect("an explanation");

    assert_eq!(
        explanation.explained_event,
        Some(events[29].event_id.clone())
    );
    assert!(explanation.preceding.len() < 5);
}
