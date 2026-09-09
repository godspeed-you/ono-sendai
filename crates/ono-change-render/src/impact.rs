//! The blast radius of §9.5 and the boundary of §9.6.
//!
//! §9.1 is careful about what impact answers: *what known parts of the system could this plan
//! touch* — never what will break. So every line here is a count or a name the
//! [`ono_change_core::ImpactGraph`] already holds, and the confidence a v0.4 edge carried travels
//! into the view unchanged (§3.5).
//!
//! §9.5 permits a summary and §9.6 makes one kind of summary a lie: a graph that stopped at a
//! traversal budget looks exactly like a graph that stopped at the edge of the world, and only
//! the graph knows which. [`blast_radius`] therefore prints the bound whenever
//! [`ono_change_core::ImpactGraph::is_complete`] answers `false`, and says in the same breath
//! that the counts above it are a lower bound.

use ono_change_core::{ImpactClass, ImpactGraph, ImpactNode, UnknownBoundary};

use crate::symbols::{Charset, Symbol};
use crate::{counted, display_width, fit, heading, join_fitted, safe};

/// §9.5's summary: how far the plan reaches, counted by how it reached (§9.2).
///
/// The counts are [`ono_change_core::ImpactGraph::blast_radius`]'s, read rather than recounted —
/// §50.1 keeps the arithmetic in the value that owns the nodes so a view can never disagree with
/// the object list it summarises. §9.5 also requires object access to survive the summary, which
/// is what [`impact_block`] is for.
#[must_use]
pub fn blast_radius(graph: &ImpactGraph, width: usize, charset: Charset) -> Vec<String> {
    let radius = graph.blast_radius();
    let mut lines = vec!["blast radius".to_owned()];
    for (count, singular, plural) in [
        (radius.direct_targets, "direct target", "direct targets"),
        (radius.direct_effects, "direct effect", "direct effects"),
        (radius.dependents, "direct dependent", "direct dependents"),
        (
            radius.transitive,
            "known transitive relation",
            "known transitive relations",
        ),
        (
            radius.external,
            "external side effect",
            "external side effects",
        ),
        (
            radius.boundaries,
            "external boundary",
            "external boundaries",
        ),
        (radius.hosts, "host", "hosts"),
    ] {
        if count > 0 {
            lines.push(fit(
                &format!("  {}", counted(count, singular, plural)),
                width,
            ));
        }
    }
    if lines.len() == 1 {
        lines.push(fit("  no object was reached", width));
    }
    // §9.6 and §52.2: a graph cut short by a budget is not a graph that ended. Saying which is
    // the difference between a summary and a claim the evidence does not support.
    if let Some(reason) = graph.truncation() {
        let mark = Symbol::Unknown.glyph(charset);
        lines.push(fit(
            &format!("  {mark} traversal stopped early - {}", safe(reason)),
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
/// unknown mark under it carrying the reason. The mark sits under the start of what lies beyond,
/// because that is the half of the line the evidence does not cover.
#[must_use]
pub fn boundaries(graph: &ImpactGraph, width: usize, charset: Charset) -> Vec<String> {
    if graph.boundaries().is_empty() {
        return Vec::new();
    }
    let arrow = arrow(charset);
    let mut lines = Vec::new();
    for boundary in graph.boundaries() {
        lines.extend(boundary_lines(boundary, &arrow, width, charset));
    }
    lines
}

/// §9.5's summary with the objects it summarises still reachable underneath it.
///
/// §9.5 says `impact` MAY summarise counts and MUST preserve object access, so the counts come
/// first and every node is listed under the class that put it in the graph. The graph is read in
/// the order it holds; a caller that wants the closest first sorts the graph, because reordering
/// here would be the view deciding what is nearest.
#[must_use]
pub fn impact_block(graph: &ImpactGraph, width: usize, charset: Charset) -> Vec<String> {
    let mut lines = blast_radius(graph, width, charset);
    for class in [
        ImpactClass::DirectTarget,
        ImpactClass::DirectEffect,
        ImpactClass::Dependent,
        ImpactClass::TransitiveRelated,
        ImpactClass::ExternalSideEffect,
        ImpactClass::UnknownBoundary,
    ] {
        let nodes = graph.of_class(class);
        if nodes.is_empty() {
            continue;
        }
        heading(&mut lines, class.as_str());
        for node in nodes {
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

/// The four labelled rows §20.2 puts under `impact`, grouped by how close the plan comes.
///
/// §20.2's example answers question 4 and question 5 of §20.1 in one block — what else may be
/// affected, and what Ono does not know — so the `unknown` row carries the boundaries and the
/// traversal bound rather than being dropped when the graph happens to hold no unknown node.
pub(crate) fn impact_rows(graph: &ImpactGraph, width: usize, charset: Charset) -> Vec<String> {
    let label = 14;
    let mut lines = Vec::new();
    for (row, classes) in [
        (
            "direct",
            &[ImpactClass::DirectTarget, ImpactClass::DirectEffect][..],
        ),
        ("related", &[ImpactClass::Dependent][..]),
        (
            "possible",
            &[
                ImpactClass::TransitiveRelated,
                ImpactClass::ExternalSideEffect,
            ][..],
        ),
    ] {
        let names: Vec<String> = classes
            .iter()
            .flat_map(|class| graph.of_class(*class))
            .map(|node| safe(node.label()))
            .collect();
        if !names.is_empty() {
            let joined = join_fitted(&names, width.saturating_sub(label + 2));
            lines.push(fit(&crate::labelled(row, &joined, label), width));
        }
    }
    let mut unknown: Vec<String> = graph
        .of_class(ImpactClass::UnknownBoundary)
        .into_iter()
        .map(|node| safe(node.label()))
        .collect();
    unknown.extend(
        graph
            .boundaries()
            .iter()
            .map(|boundary| safe(boundary.beyond())),
    );
    if let Some(reason) = graph.truncation() {
        unknown.push(format!("traversal stopped early - {}", safe(reason)));
    }
    let mark = Symbol::Unknown.glyph(charset);
    let joined = if unknown.is_empty() {
        "nothing was recorded as unknown".to_owned()
    } else {
        join_fitted(&unknown, width.saturating_sub(label + 4))
    };
    lines.push(fit(
        &crate::labelled("unknown", &format!("{mark} {joined}"), label),
        width,
    ));
    lines
}

/// `nginx --> an external API` with the unknown mark under what lies beyond it (§9.6).
fn boundary_lines(
    boundary: &UnknownBoundary,
    arrow: &str,
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let head = format!(
        "  {} {arrow} {}",
        safe(boundary.at()),
        safe(boundary.beyond())
    );
    let indent = display_width(&format!("  {} {arrow} ", safe(boundary.at())));
    let mark = Symbol::Unknown.glyph(charset);
    let mut lines = vec![
        fit(&head, width),
        fit(
            &format!("{}{mark} beyond Ono visibility", " ".repeat(indent)),
            width,
        ),
    ];
    // The reason gets its own line rather than a tail on the previous one. A path and a sentence
    // on one row is the pair that overflows first, and v0.4 §39.3 keeps forty columns usable:
    // truncating the sentence would drop the part the operator has to act on.
    let reason = safe(boundary.reason());
    if !reason.is_empty() {
        lines.push(fit(&format!("{}  {reason}", " ".repeat(indent)), width));
    }
    lines
}

/// `worker/1  service.controls_process  exact` — the node and why Ono believes it is there.
///
/// §3.5 requires impact to retain provenance, so the relation that reached the node and the
/// confidence the v0.4 edge carried travel into the line unchanged.
fn node_line(node: &ImpactNode) -> String {
    let mut line = safe(node.label());
    if let Some(host) = node.host() {
        line.push_str(&format!(" @{}", safe(host)));
    }
    // §3.5: the confidence belongs to the *edge* that reached the node, so it is printed where
    // there is an edge. A direct target the plan names carries no edge and no confidence.
    if let Some(relation) = node.relation() {
        line.push_str(&format!(
            "  {}  {}",
            safe(relation),
            safe(node.confidence())
        ));
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
