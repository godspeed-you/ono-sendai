//! What `why` reads as, and the one sentence the renderer may never break (spec v0.5 §15.5,
//! §15.6, §16.5, §16.6, §45.2, §45.3).
//!
//! §16.6: "The renderer MUST NOT move the config change into the `known cause` section because
//! it appears plausible." §15.6 forbids four words on an edge that asserts no causation, and
//! `docs/contracts/temporal/causality.yaml` is where those four words are written down. The test
//! below reads them from that registry rather than restating them, so the prohibition and the
//! renderer cannot drift apart.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_temporal_render::{RenderOptions, causal_explanation, causal_graph};
use ono_value::{Value, builtin_schemas, from_yaml};

mod support;
use support::{event, explanation, gap, link, subject};

/// A field of a parsed YAML mapping.
fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::Map(map) => map.get(name),
        Value::Record(record) => record.get(name),
        _ => None,
    }
}

/// The four words §15.6 forbids and the two connector styles §45.3 requires, read from the
/// registry that declares them.
fn wording() -> (Vec<String>, String, String) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/contracts/temporal/causality.yaml"
    );
    let text = std::fs::read_to_string(path).expect("the causal registry is in the tree");
    let document = from_yaml(&text, builtin_schemas()).expect("the causal registry parses");
    let wording = field(&document, "renderer_wording").expect("§15.6's wording block");
    let forbidden = match field(wording, "forbidden_for_non_causal") {
        Some(Value::List(words)) => words
            .iter()
            .map(|word| word.to_string().to_ascii_lowercase())
            .collect::<Vec<_>>(),
        other => panic!("the registry lists the forbidden words, got {other:?}"),
    };
    let styles = field(wording, "connector_style").expect("§45.3's connector styles");
    let style = |class: &str| {
        field(styles, class)
            .and_then(|entry| field(entry, "style"))
            .expect("a declared connector style")
            .to_string()
    };
    assert!(
        !forbidden.is_empty(),
        "the registry names at least one forbidden word"
    );
    (forbidden, style("causal"), style("non_causal"))
}

/// The nginx failure of §16.6: no cause, one correlation, one preceding event, one gap.
fn unknown_cause() -> ono_value::RecordValue {
    explanation(&[
        ("at", support::at("14:03:17.004")),
        (
            "correlations",
            Value::list(vec![link(
                "correlated_with",
                "ono.config-change-to-service-failure",
                false,
                event(
                    "e14030600000000000000001",
                    "object.changed",
                    "14:03:06.000",
                    "/etc/nginx/nginx.conf",
                    &[("subject", subject("/etc/nginx/nginx.conf", "file"))],
                ),
            )]),
        ),
        (
            "preceding",
            Value::list(vec![event(
                "e14031600000000000000002",
                "object.disappeared",
                "14:03:16.000",
                "process/1827",
                &[("subject", subject("process/1827", "process"))],
            )]),
        ),
        (
            "gaps",
            Value::list(vec![gap(
                "14:03:10",
                "14:03:17",
                "provider_unavailable",
                Some("process exit status unavailable"),
            )]),
        ),
    ])
}

/// The nginx failure of §16.5: a registered rule found the cause.
fn known_cause() -> ono_value::RecordValue {
    let exited = event(
        "e14031681200000000000000",
        "object.disappeared",
        "14:03:16.812",
        "process/1827",
        &[("subject", subject("process/1827", "process"))],
    );
    explanation(&[
        ("at", support::at("14:03:17.004")),
        (
            "cause",
            support::map(&[
                ("relation", Value::string("caused_by")),
                ("rule", Value::string("ono.systemd-job-result")),
                ("event", exited.clone()),
            ]),
        ),
        (
            "causal_chain",
            Value::list(vec![link(
                "caused_by",
                "ono.systemd-job-result",
                true,
                exited,
            )]),
        ),
    ])
}

#[test]
fn should_use_no_causal_word_when_no_edge_asserts_causation() {
    // §15.6, read from `causality.yaml` so the list and the renderer cannot drift.
    let (forbidden, _, _) = wording();
    let rendered = causal_explanation(&unknown_cause(), 100, &RenderOptions::default()).join("\n");
    let graph = causal_graph(&unknown_cause(), 100, &RenderOptions::default()).join("\n");
    for text in [&rendered, &graph] {
        let lower = text.to_ascii_lowercase();
        for word in &forbidden {
            let present = if word.contains(' ') {
                lower.contains(word.as_str())
            } else {
                lower
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .any(|found| found == word)
            };
            assert!(
                !present,
                "§15.6: `{word}` is causal language, and no edge here asserts causation. Got:\n{text}"
            );
        }
    }
}

#[test]
fn should_keep_a_correlation_out_of_the_cause_section_when_the_cause_is_unknown() {
    // §16.6, the sentence this package exists to honour: the config change stays under
    // `correlated`, however plausible it looks, and the cause stays `unknown`.
    let lines = causal_explanation(&unknown_cause(), 100, &RenderOptions::default());
    let rendered = lines.join("\n");

    let cause = lines
        .iter()
        .position(|line| line.trim() == "cause")
        .unwrap_or_else(|| {
            panic!("§15.7: an unknown cause is still a `cause` section, got {rendered}")
        });
    assert_eq!(
        lines.get(cause + 1).map(|line| line.trim()),
        Some("unknown"),
        "§15.7: `cause: unknown` is a valid answer, got {rendered}"
    );
    assert!(
        !rendered.contains("known cause"),
        "§16.6: there is no known cause here, so there is no `known cause` section, got {rendered}"
    );

    let correlated = lines
        .iter()
        .position(|line| line.trim() == "correlated")
        .unwrap_or_else(|| panic!("§15.5: the correlation is listed apart, got {rendered}"));
    let config = lines
        .iter()
        .position(|line| line.contains("nginx.conf"))
        .unwrap_or_else(|| panic!("the correlated event is drawn, got {rendered}"));
    assert!(
        config > correlated,
        "§16.6: the config change belongs under `correlated`, got {rendered}"
    );
    assert!(
        rendered.contains("correlation only"),
        "§15.5: the correlation says it is only a correlation, got {rendered}"
    );
    assert!(
        rendered.contains("preceded by") && rendered.contains("process/1827"),
        "§16.6: the preceding event is listed with no claim beyond order, got {rendered}"
    );
    assert!(
        rendered.contains("coverage gap") && rendered.contains("process exit status unavailable"),
        "§16.6: the gap that explains the ignorance is named, got {rendered}"
    );
}

#[test]
fn should_draw_a_different_connector_for_a_correlated_edge_than_for_a_causal_one() {
    // §45.3: "A renderer MUST never use the same edge style/label for both." The registry calls
    // the two styles `arrow` and `dotted`; here that is a directed connector against a
    // non-directional one, and two different labels.
    let (_, causal_style, non_causal_style) = wording();
    assert_ne!(
        causal_style, non_causal_style,
        "the registry itself declares two different styles"
    );

    let causal = causal_graph(&known_cause(), 100, &RenderOptions::default()).join("\n");
    let correlated = causal_graph(&unknown_cause(), 100, &RenderOptions::default()).join("\n");

    assert!(
        causal.contains("v") && causal.contains("|") && causal.contains("caused"),
        "§45.3: a causal edge is a directed connector labelled with the relation, got:\n{causal}"
    );
    assert!(
        correlated.contains("....") && correlated.contains("correlated"),
        "§45.3: a non-causal edge uses a distinct non-directional connector, got:\n{correlated}"
    );
    assert!(
        !correlated.contains("\n        v\n") && !correlated.contains(" v "),
        "§45.3: the correlated edge carries no arrow head, got:\n{correlated}"
    );
    assert!(
        !causal.contains("...."),
        "§45.3: the causal edge does not borrow the correlation connector, got:\n{causal}"
    );
}

#[test]
fn should_name_the_chain_the_evidence_and_the_coverage_when_a_rule_found_the_cause() {
    // §16.5's information architecture: the cause, the chain that supports it, the evidence
    // behind the chain and the coverage the answer rests on.
    let lines = causal_explanation(&known_cause(), 100, &RenderOptions::default());
    let rendered = lines.join("\n");
    for section in ["known cause", "chain", "evidence", "coverage"] {
        assert!(
            lines.iter().any(|line| line.trim() == section),
            "§16.5: `{section}` is one of the sections, got {rendered}"
        );
    }
    assert!(
        rendered.contains("ono.systemd-job-result"),
        "§15.8: the rule that made the claim is inspectable, got {rendered}"
    );
    assert!(
        rendered.contains("authoritative"),
        "§7.2: the evidence carries its strength, got {rendered}"
    );
}

#[test]
fn should_carry_every_distinction_in_words_when_the_terminal_is_monochrome() {
    // §45.2: "Color MUST not be the sole carrier of meaning." Nothing here emits an escape
    // sequence at all, so the words are all there is.
    for width in [40usize, 80, 120] {
        for explanation in [known_cause(), unknown_cause()] {
            for line in causal_explanation(&explanation, width, &RenderOptions::default()) {
                assert!(
                    !line.contains('\u{1b}'),
                    "§45.2: the renderer writes words rather than colour, got {line:?}"
                );
                assert!(
                    line.chars().count() <= width,
                    "§39.3: nothing is drawn past column {width}, got {line:?}"
                );
            }
        }
    }
}

/// The shape `ono-temporal-query`'s own `CausalExplanation` takes (§16.4): the far end is an event
/// id and a summary the producer wrote, the offset is measured there, and the chain step wraps
/// the `ono.causal-link/1` fields.
fn summarised() -> ono_value::RecordValue {
    explanation(&[
        (
            "cause",
            support::map(&[
                ("event", Value::string("e14031681200000000000000")),
                ("relation", Value::string("caused_by")),
                ("rule", Value::string("ono.systemd-job-result")),
                ("summary", Value::string("process/1827 exited code=1")),
            ]),
        ),
        (
            "causal_chain",
            Value::list(vec![support::map(&[
                (
                    "link",
                    link("caused_by", "ono.systemd-job-result", true, Value::Null),
                ),
                ("depth", Value::Int(1)),
                ("summary", Value::string("process/1827 exited code=1")),
            ])]),
        ),
        (
            "correlations",
            Value::list(vec![support::map(&[
                ("event", Value::string("e14030600000000000000001")),
                ("relation", Value::string("correlated_with")),
                (
                    "rule",
                    Value::string("ono.config-change-to-service-failure"),
                ),
                ("summary", Value::string("/etc/nginx/nginx.conf changed")),
                (
                    "offset",
                    Value::Duration(ono_value::Duration::parse("-11s").expect("a duration")),
                ),
            ])]),
        ),
    ])
}

#[test]
fn should_read_the_producers_own_summary_when_the_entry_carries_no_event_record() {
    // §39.3: a renderer may not resolve an event id, so where the answer summarised the far end
    // in words the row is drawn from those words and the reference stays typeable (§11.6).
    let lines = causal_explanation(&summarised(), 100, &RenderOptions::default());
    let rendered = lines.join("\n");
    assert!(
        rendered.contains("process/1827 exited code=1"),
        "§16.4: the producer's summary is what the node says, got {rendered}"
    );
    assert!(
        rendered.contains("@e14031681"),
        "§11.6: the far end stays referenceable, got {rendered}"
    );
    assert!(
        rendered.contains("11.00s before"),
        "§16.6: the measured offset is what the correlation says, got {rendered}"
    );
    assert!(
        lines.iter().any(|line| line.trim() == "known cause"),
        "§16.5: a rule found the cause, got {rendered}"
    );
    assert!(
        rendered.contains("authoritative") && rendered.contains("linux.systemd-dbus"),
        "§7.2: the nested link's evidence is still read, got {rendered}"
    );
}

#[test]
fn should_keep_the_two_connectors_apart_when_the_link_is_nested() {
    // §45.3 again, over the nested shape: the causal step draws an arrow and the correlation
    // draws the dotted connector, whichever way the link's fields travel.
    let (forbidden, _, _) = wording();
    let graph = causal_graph(&summarised(), 100, &RenderOptions::default()).join("\n");
    assert!(
        graph.contains("| caused") && graph.contains(".... correlated ...."),
        "§45.3: one arrow and one dotted connector, got:\n{graph}"
    );
    let correlated = graph
        .lines()
        .find(|line| line.contains("...."))
        .unwrap_or_else(|| panic!("the correlated edge is drawn, got:\n{graph}"));
    let lower = correlated.to_ascii_lowercase();
    for word in &forbidden {
        let present = if word.contains(' ') {
            lower.contains(word.as_str())
        } else {
            lower
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .any(|found| found == word)
        };
        assert!(
            !present,
            "§15.6: `{word}` on a correlated edge, got {correlated:?}"
        );
    }
}

#[test]
fn should_draw_a_clock_beside_a_node_when_the_producer_dated_it() {
    // §16.5's chain reads `14:03:16.812  process/1827 exited code=1`. §39.3 forbids the renderer
    // resolving the id to find that instant, so the producer states it on the step.
    let explanation = explanation(&[
        ("at", support::at("14:03:17.004")),
        (
            "cause",
            support::map(&[
                ("event", Value::string("e14031681200000000000000")),
                ("relation", Value::string("caused_by")),
                ("inverse", Value::string("caused")),
                ("is_causal", Value::Bool(true)),
                ("rule", Value::string("ono.systemd-job-result")),
                ("summary", Value::string("process/1827 exited code=1")),
                ("at", support::at("14:03:16.812")),
            ]),
        ),
        (
            "causal_chain",
            Value::list(vec![support::map(&[
                (
                    "link",
                    link("caused_by", "ono.systemd-job-result", true, Value::Null),
                ),
                ("depth", Value::Int(1)),
                ("event", Value::string("e14031681200000000000000")),
                ("summary", Value::string("process/1827 exited code=1")),
                ("at", support::at("14:03:16.812")),
            ])]),
        ),
    ]);
    let rendered = causal_explanation(&explanation, 100, &RenderOptions::default()).join("\n");
    assert!(
        rendered.contains("at 14:03:17.004"),
        "§16.5: the explained instant heads the answer, got {rendered}"
    );
    assert!(
        rendered.contains("14:03:16.812  process/1827 exited code=1"),
        "§16.5: the chain node carries the clock the producer stated, got {rendered}"
    );
}
