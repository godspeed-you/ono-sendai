//! The contract of a causal edge itself (v0.5 §15.2, §15.8, §35): a `CausalLink` names the
//! registered rule that emitted it, the §7.1 source that produced it, the evidence the rule
//! matched on and the strength of the weakest record behind it — and it names all four or it is
//! not a causal edge.
//!
//! §15.8: "Every built-in rule that emits `caused_by`, `triggered_by` or `resulted_in` MUST be
//! machine-readable and inspectable", and "No renderer may create causal language outside this
//! registry." Both sentences depend on one thing being impossible: an edge that arrives without
//! saying where it came from. The four fields are therefore mandatory on the type — none of them
//! is an `Option`, none of them has a default — and `ono.causal-link/1` declares each of them
//! `required`, so an edge that omits one is refused at the boundary a renderer reads it across
//! rather than drawn without its provenance.
//!
//! Which *rules* exist and what each one demands is the engine's contract, and
//! `crates/ono-temporal-query/tests/causality.rs` holds the engine against
//! `docs/contracts/temporal/causality.yaml`. This file is about the edge as a value: what it must
//! carry before any rule, renderer or ledger is involved at all.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::sync::Arc;

use ono_temporal_core::{
    CausalLink, CausalLinkId, CausalRelation, CausalRuleId, EventKind, EvidenceStrength,
    TemporalEvent, value,
};
use ono_value::{Provenance, RecordValue, SchemaId, Value};

use common::{event, evidence, systemd};

/// The rule §17.7's example runs: an Ono action and the transition the authority attributed to
/// the job it created.
const RULE: &str = "ono.action-to-transaction";

/// The four fields §15.8 makes an edge inspectable by. `link_id`, `relation`, `cause` and
/// `effect` say *what* the edge is; these say why anyone should believe it.
const PROVENANCE_FIELDS: [&str; 4] = ["rule", "source", "evidence", "strength"];

/// The two ends of §17.7's chain: the action Ono executed, and the unit transition that followed.
fn ends() -> (TemporalEvent, TemporalEvent) {
    (
        event(
            EventKind::ActionExecuted,
            "2026-08-31T14:03:11Z",
            "ono.session",
        ),
        event(
            EventKind::ObjectChanged,
            "2026-08-31T14:03:13Z",
            "linux.systemd-dbus",
        ),
    )
}

/// An edge of `relation` that `rule` emitted, carrying one real evidence record.
fn edge(rule: &str, relation: CausalRelation) -> CausalLink {
    let (cause, effect) = ends();
    let rule = CausalRuleId::new(rule);
    CausalLink {
        link_id: CausalLinkId::of(&rule, relation, &cause.event_id, &effect.event_id),
        relation,
        cause: cause.event_id,
        effect: effect.event_id,
        rule,
        evidence: vec![evidence().evidence_id],
        strength: EvidenceStrength::Authoritative,
        source: systemd(),
    }
}

fn record_of(link: &CausalLink) -> RecordValue {
    let record = value::causal_link_record(link).expect("a link becomes a record");
    record.validate().unwrap_or_else(|error| {
        panic!(
            "an edge that names all of {PROVENANCE_FIELDS:?} must satisfy its own contract: {}",
            error.message()
        )
    });
    record
}

/// The same record with `omitted` left unset — the edge a caller would have built had the field
/// been optional.
fn without(record: &RecordValue, omitted: &str) -> RecordValue {
    let schema = Arc::clone(record.schema());
    let mut builder = RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("ono.temporal", SchemaId::new("ono.causal-link", 1)),
    );
    for field in schema.fields() {
        if field.name() == omitted {
            continue;
        }
        if let Some(value) = record.get(field.name()) {
            builder = builder
                .set(field.name(), value.clone())
                .expect("a schema declares its own fields");
        }
    }
    builder.build()
}

#[test]
fn should_name_its_rule_its_source_and_its_evidence_whatever_class_an_edge_carries() {
    for relation in CausalRelation::ALL.iter().copied() {
        let link = edge(RULE, relation);
        let record = record_of(&link);

        assert_eq!(
            record.get("rule"),
            Some(&Value::string(RULE)),
            "§15.8: an edge names the registered rule that emitted it, so a reader can go and \
             read what that rule requires — `{relation}` did not"
        );
        assert_eq!(
            record.get("source"),
            Some(&Value::string(systemd().as_str())),
            "§7.1, §15.8: an edge names the source that produced it — `{relation}` did not"
        );
        assert_eq!(
            record.get("evidence"),
            Some(&Value::list([Value::string(&link.evidence[0].to_string())])),
            "§15.2: an edge names the evidence the rule matched on, because temporal proximity \
             is insufficient — `{relation}` did not"
        );
        assert_eq!(
            record.get("strength"),
            Some(&Value::string("authoritative")),
            "§7.2: an edge carries the weakest strength in its chain — `{relation}` did not"
        );
    }
}

#[test]
fn should_refuse_an_edge_that_names_no_rule_no_source_no_evidence_or_no_strength() {
    let complete = record_of(&edge(RULE, CausalRelation::CausedBy));

    for omitted in PROVENANCE_FIELDS {
        let error = without(&complete, omitted)
            .validate()
            .err()
            .unwrap_or_else(|| {
                panic!(
                    "§15.8: `ono.causal-link/1` declares `{omitted}` required, so an edge that \
                     omits it is not an edge — it validated instead"
                )
            });
        assert!(
            error.message().contains(omitted),
            "the refusal names the field that is missing, so the caller knows what an edge owes: \
             got {:?}",
            error.message()
        );
    }
}

#[test]
fn should_give_two_rules_two_edges_when_they_join_the_same_pair_of_events() {
    let built_in = edge(RULE, CausalRelation::CausedBy);
    let plugin = edge("dev.example.packet-eye.retry", CausalRelation::CausedBy);

    assert_eq!(
        (&built_in.cause, &built_in.effect),
        (&plugin.cause, &plugin.effect),
        "the fixture joins one pair of events, so the rule is the only thing that differs"
    );
    assert_ne!(
        built_in.link_id, plugin.link_id,
        "§15.8: the rule is part of what an edge *is*, so two rules that reach the same \
         conclusion are two inspectable claims rather than one anonymous edge"
    );
    assert!(
        built_in.rule.is_builtin() && !plugin.rule.is_builtin(),
        "v0.2 §31.5: `ono.*` is the project's namespace, and a package rule is visibly not it"
    );
}

#[test]
fn should_give_one_rule_one_edge_however_often_it_derives_the_same_pair() {
    let first = edge(RULE, CausalRelation::CausedBy);
    let again = edge(RULE, CausalRelation::CausedBy);
    let other_class = edge(RULE, CausalRelation::TriggeredBy);

    assert_eq!(
        first.link_id, again.link_id,
        "§15.8: one rule emitting one relation between one pair of events is one edge, so \
         re-deriving causality over a window does not multiply the graph"
    );
    assert_ne!(
        first.link_id, other_class.link_id,
        "§15.3: the class a rule emits is part of its claim, so `caused_by` and `triggered_by` \
         between the same pair are two different statements"
    );
}

#[test]
fn should_mark_only_a_causal_class_as_causal_when_an_edge_becomes_a_record() {
    for relation in CausalRelation::ALL.iter().copied() {
        let link = edge(RULE, relation);
        let record = record_of(&link);
        let causal = matches!(
            relation,
            CausalRelation::CausedBy | CausalRelation::TriggeredBy | CausalRelation::ResultedIn
        );

        assert_eq!(
            record.get("is_causal"),
            Some(&Value::Bool(causal)),
            "§15.5 and §15.6: whether an edge asserts causation is a field a renderer keys its \
             wording on, never prose it re-derives from the class name — `{relation}`"
        );
        assert_eq!(
            record.get("inverse"),
            Some(&Value::string(relation.inverse_label())),
            "§15.1 fixes the label the same edge carries read from the cause's side — \
             `{relation}`"
        );
        assert_eq!(
            record.get("rule"),
            Some(&Value::string(RULE)),
            "§15.5: a correlation is a registered rule's finding too, so it names its rule as \
             a cause does — `{relation}`"
        );
    }
}
