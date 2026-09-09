//! `why`, as text and as a graph (spec v0.5 §15.5, §15.6, §16.5, §16.6, §16.7, §45.3).
//!
//! §16.6 carries the sentence that constrains everything here:
//!
//! > The renderer MUST NOT move the config change into the `known cause` section because it
//! > appears plausible.
//!
//! The separation is structural rather than a matter of care: `ono.causal-explanation/1` keeps
//! `cause`, `causal_chain`, `correlations` and `preceding` in four fields because §35.5 requires
//! it, and this renderer draws four sections out of four fields. There is no ranking step in
//! which a correlation could be promoted, because there is no ranking step.
//!
//! §15.6 forbids `because`, `therefore`, `led to` and `caused` for an edge that asserts no
//! causation, and `docs/contracts/temporal/causality.yaml` is where those four words and the two
//! connector styles are declared. A causal edge is drawn as a directed connector labelled with
//! the relation's own inverse label; a non-causal edge is drawn as a non-directional dotted
//! connector labelled `correlated` or `preceded by`. §45.3: "A renderer MUST never use the same
//! edge style/label for both."

use ono_value::{Duration, RecordValue, Value};

use crate::{
    Precision, RenderOptions, clock, field, fit, instant, presentation_instant, record_items,
    record_text, reference, span, text, timeline,
};

/// The indent every section body is written at.
const INDENT: &str = "  ";

/// The column the causal connector hangs from (§16.5, §45.3).
const STEM: &str = "        ";

/// §16.5's and §16.6's rendering of an `ono.causal-explanation/1`.
///
/// A known cause renders the chain that supports it, the evidence behind the chain and the
/// coverage the answer rests on. An unknown cause renders `cause / unknown` with the
/// correlations, the preceding events and the gaps that explain the ignorance — which §15.7
/// makes a complete answer rather than an error.
#[must_use]
pub fn causal_explanation(
    explanation: &RecordValue,
    width: usize,
    options: &RenderOptions,
) -> Vec<String> {
    let width = width.max(20);
    let mut lines = Vec::new();
    for line in heading(explanation, options) {
        lines.push(fit(&line, width));
    }

    let cause = explanation.get("cause").filter(|cause| !cause.is_null());
    match cause {
        Some(cause) => {
            section(
                &mut lines,
                width,
                "known cause",
                cause_lines(cause, options),
            );
            section(
                &mut lines,
                width,
                "chain",
                chain_lines(explanation, options),
            );
            section(&mut lines, width, "evidence", evidence_lines(explanation));
        }
        // §15.7: "cause: unknown" is a valid outcome, and the answer still carries everything
        // that made it unknown.
        None => section(&mut lines, width, "cause", vec!["unknown".to_owned()]),
    }

    section(
        &mut lines,
        width,
        "correlated",
        correlation_lines(explanation, options),
    );
    section(
        &mut lines,
        width,
        "preceded by",
        preceding_lines(explanation, options),
    );
    section(&mut lines, width, "coverage", coverage_lines(explanation));
    section(
        &mut lines,
        width,
        "coverage gap",
        gap_lines(explanation, options),
    );
    lines
}

/// §45.3's causal graph: a directed connector for a causal edge, a dotted one for everything else.
///
/// ```text
/// restart nginx.service
///         |
///         | triggered
///         v
/// systemd job 4821
/// ```
///
/// against
///
/// ```text
/// nginx.conf changed  .... correlated ....  nginx.service failed
/// ```
#[must_use]
pub fn causal_graph(
    explanation: &RecordValue,
    width: usize,
    options: &RenderOptions,
) -> Vec<String> {
    let width = width.max(20);
    let subject = record_text(explanation, "state_or_change").unwrap_or_default();
    let mut lines = Vec::new();

    let chain = record_items(explanation, "causal_chain");
    for step in chain.iter().rev() {
        let node = node(step, options);
        if !node.is_empty() {
            lines.push(fit(&node, width));
        }
        for line in connector(step) {
            lines.push(fit(&line, width));
        }
    }
    if !chain.is_empty() {
        lines.push(fit(&subject, width));
    }

    for association in record_items(explanation, "correlations") {
        // §45.3: a non-directional connector, and the label the class carries — never an arrow
        // and never a causal word (§15.6).
        lines.push(fit(
            &format!(
                "{}  .... {} ....  {subject}",
                node(association, options),
                edge_label(edge(association))
            ),
            width,
        ));
    }
    lines
}

/// `nginx.service failed` and, where the record states it, `at 14:03:17.004`.
fn heading(explanation: &RecordValue, options: &RenderOptions) -> Vec<String> {
    let mut lines = vec![record_text(explanation, "state_or_change").unwrap_or_default()];
    if let Some(at) = anchor(explanation) {
        lines.push(format!("at {}", clock(at, options, Precision::Milli)));
    }
    lines
}

/// The instant the explained state or change happened, where the record carries one.
///
/// §16.5 renders `failed at 14:03:17.004` and §16.6 renders an offset against that instant, and
/// neither is derivable from an event id alone. A renderer may not look one up (§39.3), so
/// `ono.causal-explanation/1` declares a nullable `at` and the producer fills it; the heading and
/// the offsets are left out where it is null, which is what an unrecorded answer looks like.
fn anchor(explanation: &RecordValue) -> Option<i128> {
    match explanation.get("at") {
        Some(Value::Timestamp(instant)) => Some(instant.as_nanosecond()),
        _ => None,
    }
}

/// Writes a section, unless it has nothing in it.
fn section(lines: &mut Vec<String>, width: usize, title: &str, body: Vec<String>) {
    if body.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(fit(title, width));
    for line in body {
        lines.push(fit(&format!("{INDENT}{line}"), width));
    }
}

/// The immediate cause: the event a registered rule found, and the rule that found it (§15.8).
fn cause_lines(cause: &Value, options: &RenderOptions) -> Vec<String> {
    let mut lines = Vec::new();
    let node = node(cause, options);
    if !node.is_empty() {
        lines.push(node);
    }
    if let Some(rule) = text(edge(cause), "rule") {
        lines.push(format!("rule {rule}"));
    }
    lines
}

/// §16.5's chain, drawn from the origin down to what is being explained (§16.7).
fn chain_lines(explanation: &RecordValue, options: &RenderOptions) -> Vec<String> {
    let chain = record_items(explanation, "causal_chain");
    if chain.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for step in chain.iter().rev() {
        let node = node(step, options);
        if !node.is_empty() {
            lines.push(node);
        }
        lines.extend(connector(step));
    }
    lines.push(record_text(explanation, "state_or_change").unwrap_or_default());
    lines
}

/// The evidence behind the chain: the source of each link and the strength it carries (§7.2).
fn evidence_lines(explanation: &RecordValue) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for step in record_items(explanation, "causal_chain") {
        let link = edge(step);
        let source = text(link, "source").unwrap_or_else(|| "unknown".to_owned());
        let strength = text(link, "strength").unwrap_or_else(|| "unknown".to_owned());
        let line = format!("{source}   {strength}");
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines
}

/// §16.6's correlations, kept apart from the chain and rendered without causal language (§15.5).
fn correlation_lines(explanation: &RecordValue, options: &RenderOptions) -> Vec<String> {
    let anchor = anchor(explanation);
    let mut lines = Vec::new();
    for association in record_items(explanation, "correlations") {
        let node = node(association, options);
        if node.is_empty() {
            continue;
        }
        lines.push(node);
        if let Some(offset) = offset_line(association, anchor) {
            lines.push(format!("{INDENT}{offset}"));
        }
        // §15.5: the association is stated as an association. The rule that found it is named so
        // it can be inspected (§15.8), and the class says how little it claims (§7.2).
        let mut note = format!("{INDENT}correlation only");
        if let Some(rule) = text(edge(association), "rule") {
            note.push_str(&format!("  {rule}"));
        }
        lines.push(note);
    }
    lines
}

/// How far an association is from what is being explained (§16.6's `11s before failure`).
///
/// The producer measures the offset where it has both instants, and a renderer may not look one
/// up (§39.3), so the offset is read where it travels and derived from the explained instant only
/// where the far end's own event record came along.
fn offset_line(association: &Value, anchor: Option<i128>) -> Option<String> {
    if let Some(Value::Duration(offset)) = field(association, "offset") {
        let word = if offset.nanoseconds() > 0 {
            "after"
        } else {
            "before"
        };
        let magnitude = Duration::from_nanoseconds(offset.nanoseconds().abs());
        return Some(format!("{magnitude} {word}"));
    }
    let at = endpoint(association).and_then(presentation_instant)?;
    let anchor = anchor?;
    let (from, until, word) = if at <= anchor {
        (at, anchor, "before")
    } else {
        (anchor, at, "after")
    };
    Some(format!("{} {word}", span(from, until)))
}

/// Events the ordering model supports as earlier, with no claim beyond order (§15.6, §26.3).
fn preceding_lines(explanation: &RecordValue, options: &RenderOptions) -> Vec<String> {
    record_items(explanation, "preceding")
        .iter()
        .map(|entry| node(entry, options))
        .filter(|line| !line.is_empty())
        .collect()
}

/// The composed coverage the answer rests on (§8.5).
fn coverage_lines(explanation: &RecordValue) -> Vec<String> {
    let Some(coverage) = explanation.get("coverage").filter(|value| !value.is_null()) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if let Some(headline) = text(coverage, "headline") {
        lines.push(headline);
    }
    if let Some(Value::Map(capabilities)) = field(coverage, "capabilities") {
        for (capability, state) in capabilities.iter() {
            lines.push(format!("{capability}   {state}"));
        }
    }
    lines
}

/// The gaps that materially affect the answer (§7.5, §16.6).
fn gap_lines(explanation: &RecordValue, options: &RenderOptions) -> Vec<String> {
    record_items(explanation, "gaps")
        .iter()
        .map(|gap| {
            let reason = text(gap, "detail").unwrap_or_else(|| {
                text(gap, "reason")
                    .unwrap_or_else(|| "not recorded".to_owned())
                    .replace('_', " ")
            });
            match (instant(gap, "from"), instant(gap, "until")) {
                (Some(from), Some(until)) => format!(
                    "{reason}   {} - {}",
                    clock(from, options, Precision::Second),
                    clock(until, options, Precision::Second)
                ),
                _ => reason,
            }
        })
        .collect()
}

/// One entry as a node: when it happened, what it happened to, and what happened.
///
/// An entry that carries the far end's own event record is drawn from it. An entry that carries
/// the producer's own summary and an id — which is the shape `CausalNode`, `CausalStep` and
/// `TemporalAssociation` take (§16.4) — is drawn from those, because a renderer may not resolve
/// an id to an event (§39.3).
fn node(entry: &Value, options: &RenderOptions) -> String {
    if let Some(event) = endpoint(entry) {
        let drawn = event_node(event, options);
        if !drawn.is_empty() {
            return drawn;
        }
    }
    let mut parts = Vec::new();
    // §16.5 draws a clock beside every node. The producer states it (§39.3 forbids resolving an
    // id to find one), and a node the producer could not date stays undated rather than guessed.
    if let Some(at) = instant(entry, "at") {
        parts.push(clock(at, options, Precision::Milli));
    }
    if let Some(summary) = text(entry, "summary") {
        parts.push(summary);
    }
    for name in ["event", "cause"] {
        if let Some(Value::String(id)) = field(entry, name) {
            parts.push(reference(id));
            break;
        }
    }
    parts.join("  ")
}

/// One event record as a node.
fn event_node(event: &Value, options: &RenderOptions) -> String {
    let mut parts = Vec::new();
    if let Some(at) = presentation_instant(event) {
        parts.push(clock(at, options, Precision::Milli));
    }
    let label = timeline::subject_label(event);
    if !label.is_empty() {
        parts.push(label);
    }
    let what = timeline::describe(event);
    if !what.is_empty() {
        parts.push(what);
    }
    parts.join("  ")
}

/// The endpoint event an entry is about, where the entry carries the record itself.
///
/// A `preceding` entry may be the event; another entry may embed it under `event`. An `event`
/// that is an id rather than a record is not an endpoint: the renderer has nothing to resolve it
/// with (§39.3), and the producer's summary is what stands in its place.
fn endpoint(entry: &Value) -> Option<&Value> {
    match field(entry, "event") {
        Some(event @ (Value::Record(_) | Value::Map(_))) => Some(event),
        _ => field(entry, "event_id").map(|_| entry),
    }
}

/// The `ono.causal-link/1` fields of an entry.
///
/// A chain step is a link with a depth and a summary around it (§16.4), so the link's own fields
/// are either nested under `link` or spelled beside them; either way this is where `relation`,
/// `is_causal`, `rule`, `source` and `strength` are read from.
fn edge(entry: &Value) -> &Value {
    match field(entry, "link") {
        Some(link @ (Value::Record(_) | Value::Map(_))) => link,
        _ => entry,
    }
}

/// The label an edge is drawn with: the relation's own inverse label where it asserts causation,
/// and the class's own word where it does not (§15.1, §15.6).
fn edge_label(link: &Value) -> String {
    if is_causal(link) {
        return text(link, "inverse").unwrap_or_else(|| "caused".to_owned());
    }
    match text(link, "relation").as_deref() {
        Some("preceded_by") => "preceded by".to_owned(),
        _ => "correlated".to_owned(),
    }
}

/// Whether the link asserts causation, read from the field the schema declares for it.
///
/// §15.5 and §15.6 make the distinction structural, so it is read rather than derived from the
/// class name — a renderer that re-derived it would be one rename away from calling a
/// correlation a cause.
fn is_causal(link: &Value) -> bool {
    matches!(field(link, "is_causal"), Some(Value::Bool(true)))
}

/// The connector between two nodes (§45.3).
///
/// A causal edge is directed and labelled with the relation and the rule that emitted it. A
/// non-causal edge is a dotted, non-directional connector carrying the class's own word, so the
/// two never look alike in a monochrome terminal, in a pipe or to a screen reader (§45.2).
fn connector(entry: &Value) -> Vec<String> {
    let link = edge(entry);
    let mut label = edge_label(link);
    if let Some(rule) = text(link, "rule") {
        label.push_str(&format!("  {rule}"));
    }
    if is_causal(link) {
        return vec![
            format!("{STEM}|"),
            format!("{STEM}| {label}"),
            format!("{STEM}v"),
        ];
    }
    vec![format!("{STEM}.... {label} ....")]
}
