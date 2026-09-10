//! The blast radius of §9.5 and the boundary of §9.6.
//!
//! §9.1 is careful about what impact answers: *what known parts of the system could this plan
//! touch* — never what will break. So every line here is a count or a name the
//! `ono.impact-graph/1` record already holds, and the confidence a v0.4 edge carried travels into
//! the view unchanged (§3.5).
//!
//! §9.5 permits a summary and §9.6 makes one kind of summary a lie: a graph that stopped at a
//! traversal budget looks exactly like a graph that stopped at the edge of the world, and only
//! the record knows which. [`blast_radius`] therefore prints the bound whenever `complete` is
//! false, and says in the same breath that the counts above it are a lower bound.
//!
//! Two records carry the same counts under slightly different names: `ono.impact-graph/1` calls
//! the boundary count `boundary_count`, and `ono.change-plan/1`'s `impact_summary` calls it
//! `boundaries` beside the object counts. Both are read, because §9.6 forbids a summary that hides
//! a boundary and a renderer that only understood one spelling would produce one.

use ono_value::RecordValue;

use crate::symbols::{Charset, Symbol};
use crate::{
    Fields, Item, count, counted, display_width, fit, flag, heading, items, join_fitted, labelled,
    strings, text,
};

/// §9.2's classes, in the order §9.2 lists them, with the noun §9.5 counts them by.
const CLASSES: [(&str, &str, &str, &str); 6] = [
    (
        "direct-target",
        "direct_targets",
        "direct target",
        "direct targets",
    ),
    (
        "direct-effect",
        "direct_effects",
        "direct effect",
        "direct effects",
    ),
    (
        "dependent",
        "dependents",
        "direct dependent",
        "direct dependents",
    ),
    (
        "transitive-related",
        "transitive",
        "known transitive relation",
        "known transitive relations",
    ),
    (
        "external-side-effect",
        "external",
        "external side effect",
        "external side effects",
    ),
    (
        "unknown-boundary",
        "boundary_count",
        "external boundary",
        "external boundaries",
    ),
];

/// §9.5's summary: how far the plan reaches, counted by how it reached (§9.2).
///
/// `graph` is an `ono.impact-graph/1`, or the `impact_summary` of an `ono.change-plan/1` — the
/// counts are the same and the record carries them, so §50.1's rule that a renderer computes
/// nothing holds either way. §9.5 also requires object access to survive the summary, which is
/// what [`impact_block`] is for.
#[must_use]
pub fn blast_radius(graph: &RecordValue, width: usize, charset: Charset) -> Vec<String> {
    radius_lines(graph, width, charset)
}

/// The counts themselves, over anything that answers to §9.5's field names.
///
/// `ono.impact-graph/1` is a record and `ono.change-plan/1`'s `impact_summary` is a map, and the
/// counts in them are the same counts. Reading both through one function is what keeps a plan's
/// summary from disagreeing with the graph it summarises.
fn radius_lines(graph: &dyn Fields, width: usize, charset: Charset) -> Vec<String> {
    let mut lines = vec!["blast radius".to_owned()];
    for (_, field, singular, plural) in CLASSES {
        let counted_here = count(graph, field).or_else(|| {
            // `ono.change-plan/1`'s `impact_summary` spells the boundary count `boundaries`.
            (field == "boundary_count")
                .then(|| count(graph, "boundaries"))
                .flatten()
        });
        if let Some(number) = counted_here.filter(|number| *number > 0) {
            lines.push(fit(
                &format!("  {}", counted(number, singular, plural)),
                width,
            ));
        }
    }
    if let Some(hosts) = count(graph, "hosts").filter(|hosts| *hosts > 1) {
        lines.push(fit(
            &format!("  {}", counted(hosts, "host", "hosts")),
            width,
        ));
    }
    if lines.len() == 1 {
        lines.push(fit("  no object was reached", width));
    }
    // §9.6 and §52.2: a graph cut short by a budget is not a graph that ended. Saying which is
    // the difference between a summary and a claim the evidence does not support.
    if !flag(graph, "complete") {
        let mark = Symbol::Unknown.glyph(charset);
        let reason = text(graph, "truncated_reason")
            .unwrap_or_else(|| "the traversal did not report why".to_owned());
        lines.push(fit(
            &format!("  {mark} traversal stopped early - {reason}"),
            width,
        ));
        lines.push(fit(
            &format!("  {mark} the counts above are a lower bound"),
            width,
        ));
    }
    lines
}

/// §9.6's boundaries: where the graph ends and Ono cannot see past it.
///
/// The shape is §9.6's own — the last object Ono can see, an arrow, what lies beyond, and the
/// unknown mark under it. The reason takes a line of its own: a path and a sentence on one row is
/// the pair that overflows first, and v0.4 §39.3 keeps forty columns usable.
#[must_use]
pub fn boundaries(graph: &RecordValue, width: usize, charset: Charset) -> Vec<String> {
    let arrow = arrow(charset);
    items(graph, "boundaries")
        .iter()
        .flat_map(|boundary| boundary_lines(boundary, &arrow, width, charset))
        .collect()
}

/// §9.5's summary with the objects it summarises still reachable underneath it.
///
/// §9.5 says `impact` MAY summarize counts and MUST preserve object access, so the counts come
/// first and every node is listed under the §9.2 class that put it in the graph. The nodes are
/// read in the order the record holds them; reordering here would be the view deciding which
/// object is nearest.
#[must_use]
pub fn impact_block(graph: &RecordValue, width: usize, charset: Charset) -> Vec<String> {
    let mut lines = blast_radius(graph, width, charset);
    let nodes = items(graph, "nodes");
    for (class, _, _, _) in CLASSES {
        let of_class: Vec<&Item> = nodes
            .iter()
            .filter(|node| text(*node, "class").as_deref() == Some(class))
            .collect();
        if of_class.is_empty() {
            continue;
        }
        heading(&mut lines, class);
        for node in of_class {
            lines.push(fit(&format!("  {}", node_line(node)), width));
        }
    }
    let boundaries = boundaries(graph, width, charset);
    if !boundaries.is_empty() {
        heading(&mut lines, "unknown boundary");
        lines.extend(boundaries);
    }
    lines
}

/// The four labelled rows §20.2 puts under `impact`, from a plan's own summary and targets.
///
/// §20.2's example answers question 4 and question 5 of §20.1 in one block — what else may be
/// affected, and what Ono does not know — so the `unknown` row carries the boundary count and the
/// traversal bound rather than being dropped when the plan happens to reach nothing unknown.
///
/// The `direct` row names objects because `ono.change-plan/1` carries `targets`; the rest are
/// §9.5's counts, because the plan carries the summary and `impact <plan>` carries the graph.
pub(crate) fn impact_rows(plan: &RecordValue, width: usize, charset: Charset) -> Vec<String> {
    let label = 14;
    let mut lines = Vec::new();
    let Some(summary) = crate::nested(plan, "impact_summary") else {
        return vec![fit(
            &labelled(
                "unknown",
                &format!(
                    "{} the impact graph was not analysed",
                    Symbol::Unknown.glyph(charset)
                ),
                label,
            ),
            width,
        )];
    };

    let targets: Vec<String> = items(plan, "targets")
        .iter()
        .filter_map(|target| text(target, "label"))
        .collect();
    if targets.is_empty() {
        if let Some(direct) = count(&summary, "direct_targets").filter(|number| *number > 0) {
            lines.push(fit(
                &labelled(
                    "direct",
                    &counted(direct, "direct target", "direct targets"),
                    label,
                ),
                width,
            ));
        }
    } else {
        let joined = join_fitted(&targets, width.saturating_sub(label + 2));
        lines.push(fit(&labelled("direct", &joined, label), width));
    }

    let related = phrase(
        &summary,
        &[("dependents", "direct dependent", "direct dependents")],
    );
    if !related.is_empty() {
        lines.push(fit(&labelled("related", &related, label), width));
    }
    let possible = phrase(
        &summary,
        &[
            (
                "transitive",
                "known transitive relation",
                "known transitive relations",
            ),
            ("external", "external side effect", "external side effects"),
        ],
    );
    if !possible.is_empty() {
        lines.push(fit(&labelled("possible", &possible, label), width));
    }

    // §9.6: a boundary is named by what lies beyond it. It is not called external, because an
    // opaque command's boundary is an unknown domain on this host (§6.3), and the name comes first
    // so that a cut row still shows it.
    let mut unknown: Vec<String> = Vec::new();
    let boundaries = count(&summary, "boundaries")
        .or_else(|| count(&summary, "boundary_count"))
        .unwrap_or_default();
    let named = strings(&summary, "boundary_labels");
    let unnamed = boundaries.saturating_sub(named.len());
    unknown.extend(named);
    if unnamed > 0 {
        unknown.push(counted(unnamed, "unknown boundary", "unknown boundaries"));
    }
    if !flag(&summary, "complete") {
        unknown.push(text(&summary, "truncated_reason").map_or_else(
            || "the traversal stopped early".to_owned(),
            |reason| format!("traversal stopped early - {reason}"),
        ));
    }
    let mark = Symbol::Unknown.glyph(charset);
    let joined = if unknown.is_empty() {
        "nothing was recorded as unknown".to_owned()
    } else {
        join_fitted(&unknown, width.saturating_sub(label + 4))
    };
    lines.push(fit(
        &labelled("unknown", &format!("{mark} {joined}"), label),
        width,
    ));
    lines
}

/// `4 direct dependents` for each count that is greater than zero, joined.
fn phrase(summary: &Item, fields: &[(&str, &str, &str)]) -> String {
    fields
        .iter()
        .filter_map(|(field, singular, plural)| {
            count(summary, field)
                .filter(|number| *number > 0)
                .map(|number| counted(number, singular, plural))
        })
        .collect::<Vec<String>>()
        .join(", ")
}

/// `nginx --> an external API` with the unknown mark under what lies beyond it (§9.6).
fn boundary_lines(boundary: &Item, arrow: &str, width: usize, charset: Charset) -> Vec<String> {
    let at = text(boundary, "at").unwrap_or_else(|| "unknown".to_owned());
    let beyond = text(boundary, "beyond").unwrap_or_else(|| "unknown".to_owned());
    let indent = display_width(&format!("  {at} {arrow} "));
    let mark = Symbol::Unknown.glyph(charset);
    let mut lines = vec![
        fit(&format!("  {at} {arrow} {beyond}"), width),
        fit(
            &format!("{}{mark} beyond Ono visibility", " ".repeat(indent)),
            width,
        ),
    ];
    if let Some(reason) = text(boundary, "reason") {
        lines.push(fit(&format!("{}  {reason}", " ".repeat(indent)), width));
    }
    lines
}

/// `worker/1  service.controls_process  exact` — the node and why Ono believes it is there.
///
/// §3.5 requires impact to retain provenance, so the relation that reached the node and the
/// confidence the v0.4 edge carried travel into the line unchanged. A node the plan names has no
/// edge and therefore no confidence to print.
fn node_line(node: &Item) -> String {
    let mut line = text(node, "label")
        .or_else(|| text(node, "id"))
        .unwrap_or_else(|| "unnamed".to_owned());
    if let Some(host) = text(node, "host") {
        line.push_str(&format!(" @{host}"));
    }
    if let Some(relation) = text(node, "relation") {
        let confidence = text(node, "confidence").unwrap_or_else(|| "unknown".to_owned());
        line.push_str(&format!("  {relation}  {confidence}"));
    }
    line
}

/// The arrow a boundary is drawn with, in the alphabet the session chose (§20.3).
fn arrow(charset: Charset) -> String {
    match charset {
        Charset::Unicode => "\u{2500}\u{2500}\u{25b8}".to_owned(),
        Charset::Ascii => "->".to_owned(),
    }
}
