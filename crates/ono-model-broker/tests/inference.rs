//! What a model may say about causality, and the wall between saying it and it becoming true
//! (v0.5 §38.1, §38.2).
//!
//! §38.2 fixes the representation — an `Inference` with its kind, model, inputs and confidence —
//! and then the sentence that matters: it "MUST NOT become a canonical `caused_by` edge without
//! independent registered evidence". Two things have to hold for that to be more than a promise,
//! and this file asserts both from outside the crate:
//!
//! - **A hypothesis is not shaped like an edge.** The value a model's claim becomes carries no
//!   relation class, no rule and no evidence ids, so nothing downstream can render it as a causal
//!   link by mistake. §15.8: "No renderer may create causal language outside this registry."
//! - **No registered rule accepts it.** `docs/contracts/temporal/causality.yaml` is that registry,
//!   and every rule in it names the sources it will read from. None of them names a model, and
//!   §7.1's source vocabulary has no member a model could occupy — so a hypothesis has no door
//!   into the causal graph, rather than a door that is politely left shut.
//!
//! §38's own intent paragraph is the reason: "Structured temporal context is valuable precisely
//! because it gives AI better evidence. AI must not be allowed to contaminate the evidence model
//! in return."

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::PathBuf;

use ono_model_broker::{Inference, InferenceKind};

/// The evidence ids the model was shown, in the order it was shown them.
const SHOWN: [&str; 2] = ["v000000000000000000000aa", "v000000000000000000000bb"];

/// §38.2's own example sentence.
const STATEMENT: &str = "The config change may have contributed to the failure.";

/// The five relationship classes of §15.1. A hypothesis may carry none of them.
const RELATION_CLASSES: [&str; 5] = [
    "caused_by",
    "triggered_by",
    "resulted_in",
    "correlated_with",
    "preceded_by",
];

/// The fields an `ono.causal-link/1` is inspectable by. A hypothesis carries none of them either:
/// it has no rule to name and no evidence of its own.
const EDGE_FIELDS: [&str; 6] = [
    "relation", "rule", "evidence", "link_id", "strength", "source",
];

fn hypothesis() -> Inference {
    Inference::hypothesis(
        "local.llama",
        SHOWN.iter().map(|id| (*id).to_owned()).collect(),
        0.72,
        STATEMENT,
    )
}

/// The causal rule registry of §15.8, as the gate reads it.
fn registry() -> serde_yaml_ng::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/contracts/temporal/causality.yaml");
    let text = std::fs::read_to_string(&path).expect("the causality registry is readable");
    serde_yaml_ng::from_str(&text).expect("the causality registry parses")
}

/// Every rule row the registry declares, causal and correlational alike.
fn rules(document: &serde_yaml_ng::Value) -> Vec<serde_yaml_ng::Value> {
    ["rules", "correlation_rules"]
        .iter()
        .filter_map(|key| document.get(*key))
        .filter_map(serde_yaml_ng::Value::as_sequence)
        .flat_map(|rows| rows.iter().cloned())
        .collect()
}

fn strings(row: &serde_yaml_ng::Value, key: &str) -> Vec<String> {
    row.get(key)
        .and_then(serde_yaml_ng::Value::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn should_carry_its_model_its_inputs_and_its_confidence_when_a_model_states_a_hypothesis() {
    let inference = hypothesis();
    let value = inference.to_json();

    assert_eq!(
        inference.kind,
        InferenceKind::Hypothesis,
        "§38.2 gives a model's causal claim exactly one representation, and it is a hypothesis"
    );
    assert_eq!(value["kind"], "hypothesis");
    assert_eq!(
        value["model"], "local.llama",
        "§38.2: the claim names the model that made it, so a reader knows whose guess this is"
    );
    assert_eq!(
        value["inputs"],
        serde_json::json!(SHOWN),
        "§38.2: the inputs are exactly what the model was shown, in order — the set §38.2's \
         independence test is taken against"
    );
    assert_eq!(
        value["confidence"], 0.72,
        "§38.2: how sure it said it was, recorded rather than acted on"
    );
    assert_eq!(
        value["statement"], STATEMENT,
        "a hypothesis nobody can read is not one"
    );
}

#[test]
fn should_carry_no_causal_relation_when_a_hypothesis_becomes_a_value() {
    let value = hypothesis().to_json();
    let text = serde_json::to_string(&value).expect("a hypothesis serialises");

    for field in EDGE_FIELDS {
        assert!(
            value.get(field).is_none(),
            "§15.8: `{field}` belongs to `ono.causal-link/1`, and a renderer handed a hypothesis \
             that carried it would have a causal edge with no registered rule behind it — got \
             {text}"
        );
    }
    for class in RELATION_CLASSES {
        assert!(
            !text.contains(class),
            "§15.1's classes are the ledger's vocabulary and a hypothesis is not one of them; \
             `{class}` appears in {text}"
        );
    }
}

#[test]
fn should_admit_no_model_as_a_source_when_every_registered_causal_rule_is_read() {
    let document = registry();
    let rows = rules(&document);
    assert!(
        rows.len() >= 10,
        "the registry declares the built-in rules; a run that read none of them would pass this \
         test for the wrong reason, got {} rows",
        rows.len()
    );

    let inference = hypothesis();
    for row in &rows {
        let id = row
            .get("rule_id")
            .and_then(serde_yaml_ng::Value::as_str)
            .expect("§15.8: every registered rule has an id");
        let sources = strings(row, "provider_source_constraints");
        assert!(
            !sources.is_empty(),
            "§15.8: `{id}` must say which sources may supply its inputs"
        );
        assert!(
            !sources.contains(&inference.model),
            "§38.2: `{id}` would read from `{}`, so a model's claim could enter the causal graph \
             as evidence",
            inference.model
        );
        for source in &sources {
            let lowered = source.to_ascii_lowercase();
            assert!(
                ["model", "inference", "hypothesis", "assistant", "llm"]
                    .iter()
                    .all(|word| !lowered.contains(word)),
                "§7.1's source vocabulary is closed and holds no model; `{id}` names `{source}`"
            );
        }
        for kind in strings(row, "input_event_kinds") {
            let lowered = kind.to_ascii_lowercase();
            assert!(
                ["model", "inference", "hypothesis", "assistant"]
                    .iter()
                    .all(|word| !lowered.contains(word)),
                "§38.1: a model is a consumer of temporal events, never a producer of one; \
                 `{id}` reads `{kind}`"
            );
        }
    }
}

#[test]
fn should_leave_the_edge_to_the_registered_rule_when_independent_evidence_exists() {
    let inference = hypothesis();
    let already_seen: Vec<String> = SHOWN.iter().map(|id| (*id).to_owned()).collect();

    assert!(
        !inference.may_support_causal_edge(&already_seen),
        "§38.2: pointing again at the three records a model was shown establishes nothing it did \
         not already contain, so there is no independent registered evidence and no edge"
    );

    let mut with_more = already_seen.clone();
    with_more.push("v000000000000000000000cc".to_owned());
    assert_eq!(
        inference.independent_evidence(&with_more),
        ["v000000000000000000000cc"],
        "§38.2: independent means evidence the model did not have"
    );
    assert!(inference.may_support_causal_edge(&with_more));
    assert!(
        inference.to_json().get("relation").is_none(),
        "§38.2, §15.8: even where an edge may be built beside it, the hypothesis is still not \
         that edge — the edge comes from the registered rule the independent evidence satisfies, \
         and the model contributed the question rather than the answer"
    );
}
