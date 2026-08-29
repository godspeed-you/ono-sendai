//! The default textual map (spec v0.4 §23.1, §23.2, §23.5, §23.6, §39, §52.1).
//!
//! §23.2 is a requirement, not a suggestion: "Every terminal MUST have a non-fullscreen textual
//! map representation." This is that map. It works on a pipe, on `TERM=dumb`, at forty columns
//! and with colour switched off, because §39 says all four must work and §52.1 makes "map text
//! rendering works without full-screen TUI" a release criterion.
//!
//! What it draws is exactly what the `SpatialMap` says (§45.4: the renderer "MUST NOT invent
//! semantic nodes/edges"): the hierarchy the map's own edges declare, as a ranked tree — §39.3
//! allows precisely that at narrow widths and the semantics do not change with the drawing — then
//! the relationships that are not hierarchy, then the landmarks, then what the bound left out.
//!
//! Two of §23's rules are visible in every line:
//!
//! - **§23.5: "Inferred edges MUST be visually distinguishable from exact edges."** They are, by
//!   the arrow (`-->` against `~~>`) and by the confidence word beside it — never by colour, which
//!   §39.1 forbids relying on.
//! - **§23.6: a bound is disclosed.** A cluster says how many objects it stands for, and the
//!   closing line says how many the map did not draw at all.
//!
//! The full-screen view of §23.3 is a later phase. It attaches here: [`spatial_map`] is the whole
//! of the text projection, and an interactive view is the same node and edge lists with focus,
//! keys and a viewport around them — not a second reading of the map.

use std::collections::{BTreeMap, BTreeSet};

use ono_value::RecordValue;

use crate::{fit, integer, list, record, text};

/// Which characters the terminal can be promised (§39.2: "ASCII fallback MUST exist").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    /// Box-drawing and arrows, where the terminal and the locale support them.
    Unicode,
    /// Plain ASCII, which every terminal can show.
    Ascii,
}

impl Charset {
    /// The three tree characters: a branch, the last branch, and the trunk below a branch.
    const fn branches(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Charset::Unicode => ("├─ ", "└─ ", "│  "),
            Charset::Ascii => ("+- ", "`- ", "|  "),
        }
    }

    /// The arrow an edge of this confidence is drawn with (§23.5).
    const fn arrow(self, exact: bool) -> &'static str {
        match (self, exact) {
            (Charset::Unicode, true) => "──▸",
            (Charset::Unicode, false) => "╌╌▸",
            (Charset::Ascii, true) => "-->",
            (Charset::Ascii, false) => "~~>",
        }
    }

    /// The mark before a landmark reason.
    const fn landmark(self) -> &'static str {
        match self {
            Charset::Unicode => "◆",
            Charset::Ascii => "*",
        }
    }

    /// The dash that separates a fact from its evidence.
    const fn dash(self) -> &'static str {
        match self {
            Charset::Unicode => "—",
            Charset::Ascii => "-",
        }
    }
}

/// One drawn line, and what of the map it draws.
///
/// The text is what a reader sees; the identity beside it is what a *cursor* needs, so the
/// full-screen view of §23.3 can focus a node without re-deciding which nodes there are (§45.4,
/// §49.5). A line that draws nothing addressable — a heading, a blank, a section label — carries
/// neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLine {
    text: String,
    node: Option<String>,
    relation: Option<String>,
}

impl MapLine {
    /// A line that draws nothing a cursor can land on.
    fn plain(text: String) -> Self {
        Self {
            text,
            node: None,
            relation: None,
        }
    }

    /// A line that draws the node `id`.
    fn at(text: String, id: &str) -> Self {
        Self {
            text,
            node: Some(id.to_owned()),
            relation: None,
        }
    }

    /// A line that draws a relationship edge to `id`.
    fn edge(text: String, id: &str, relation: &str) -> Self {
        Self {
            text,
            node: Some(id.to_owned()),
            relation: Some(relation.to_owned()),
        }
    }

    /// What the line says.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The node or cluster the line draws, where it draws one.
    #[must_use]
    pub fn node(&self) -> Option<&str> {
        self.node.as_deref()
    }

    /// The relation the line draws, where the line is a relationship edge (§23.5).
    #[must_use]
    pub fn relation(&self) -> Option<&str> {
        self.relation.as_deref()
    }
}

/// The lines an `ono.spatial-map/1` reads as, at a terminal `width` columns wide.
///
/// Nothing is laid out to a fixed page: every line is fitted to the width the caller states, so
/// the same map is legible at forty columns and at two hundred (§39.3, §43.5 — the layout is
/// presentation and no snapshot is a contract).
#[must_use]
pub fn spatial_map(map: &RecordValue, width: usize, charset: Charset) -> Vec<String> {
    map_lines(map, width, charset)
        .into_iter()
        .map(|line| line.text)
        .collect()
}

/// The same drawing, with the identity of whatever each line draws (§23.3, §23.4).
#[must_use]
pub fn map_lines(map: &RecordValue, width: usize, charset: Charset) -> Vec<MapLine> {
    let width = width.max(20);
    let nodes = list(map, "nodes");
    let clusters = list(map, "clusters");
    let edges = list(map, "edges");

    let mut lines = vec![MapLine::plain(fit(&heading(map, nodes.len()), width))];

    // The hierarchy, as the tree §39.3 allows a map to collapse into. It is drawn from the map's
    // own `hierarchy` edges: the renderer decides nothing about who contains whom (§45.4).
    let labels = labels_of(&nodes, &clusters);
    let children = hierarchy(&edges);
    let center = text(map, "center").unwrap_or_default();
    let ambiguous = ambiguous(&labels);
    let mut drawn: BTreeSet<String> = BTreeSet::new();
    lines.push(MapLine::plain(String::new()));
    lines.push(MapLine::at(
        fit(
            &format!("  {}", node_line(&labels, &ambiguous, &center, charset)),
            width,
        ),
        &center,
    ));
    lines.extend(branch(
        &labels, &ambiguous, &children, &center, "  ", charset, width, &mut drawn,
    ));

    // Anything the hierarchy did not reach is still on the map, and dropping it here would be the
    // renderer deciding what is true (§45.4, §2.17).
    let loose: Vec<&String> = labels
        .keys()
        .filter(|id| **id != center && !drawn.contains(*id))
        .collect();
    if !loose.is_empty() {
        // §23.5: "Edges MUST show relation labels", and §11.4 makes a relationship explainable.
        // A neighbour outside the hierarchy is there *because of* an edge, and a row that named
        // only its display name could not be chosen from — which is what the view is for.
        let reached = reached_by(&edges, map, &nodes);
        lines.push(MapLine::plain(String::new()));
        lines.push(MapLine::plain("  also here".to_owned()));
        for id in loose {
            let drawn_line = node_line(&labels, &ambiguous, id, charset);
            let line = match reached.get(id) {
                Some(relation) => format!("    {drawn_line}  {} {relation}", charset.dash()),
                None => format!("    {drawn_line}"),
            };
            match reached.get(id) {
                Some(relation) => {
                    lines.push(MapLine::edge(fit(&line, width), id, relation));
                }
                None => lines.push(MapLine::at(fit(&line, width), id)),
            }
        }
    }

    let relations = relation_lines(&edges, charset);
    if !relations.is_empty() {
        lines.push(MapLine::plain(String::new()));
        lines.push(MapLine::plain("  relations".to_owned()));
        for drawn in relations {
            lines.push(MapLine::edge(
                fit(&format!("    {}", drawn.text), width),
                &drawn.target,
                &drawn.relation,
            ));
        }
    }

    let landmarks = list(map, "landmarks");
    if !landmarks.is_empty() {
        lines.push(MapLine::plain(String::new()));
        lines.push(MapLine::plain("  landmarks".to_owned()));
        for landmark in &landmarks {
            let name = text(landmark, "name").unwrap_or_default();
            let reason = text(landmark, "reason")
                .unwrap_or_default()
                .replace('_', " ");
            let evidence = text(landmark, "evidence").unwrap_or_default();
            lines.push(MapLine::plain(fit(
                &format!(
                    "    {} {name}  {reason} {} {evidence}",
                    charset.landmark(),
                    charset.dash()
                ),
                width,
            )));
        }
    }

    if let Some(closing) = hidden_line(map) {
        lines.push(MapLine::plain(String::new()));
        lines.push(MapLine::plain(fit(&format!("  {closing}"), width)));
    }
    lines
}

/// `map  COMPUTE  ·  L2  ·  9 nodes` — where the map is, how far it is zoomed out, how much of
/// the system it drew.
fn heading(map: &RecordValue, nodes: usize) -> String {
    let center = text(map, "center").unwrap_or_default();
    let label = list(map, "nodes")
        .iter()
        .find(|node| text(node, "id").as_deref() == Some(center.as_str()))
        .and_then(|node| text(node, "label"))
        .unwrap_or(center);
    let zoom = integer(map, "zoom_level").unwrap_or_default();
    let completeness = text(map, "completeness").unwrap_or_default();
    format!("map {label}  L{zoom}  {nodes} nodes  {completeness}")
}

/// What each drawn node and each cluster is called, and what it carries.
fn labels_of(nodes: &[RecordValue], clusters: &[RecordValue]) -> BTreeMap<String, Drawn> {
    let mut labels = BTreeMap::new();
    for node in nodes {
        let Some(id) = text(node, "id") else { continue };
        labels.insert(
            id,
            Drawn {
                label: text(node, "label").unwrap_or_default(),
                identity: identity_of(node),
                state: text(node, "state"),
                reasons: node
                    .get("landmark_reasons")
                    .and_then(|value| match value {
                        ono_value::Value::List(items) => Some(
                            items
                                .iter()
                                .filter_map(|item| item.as_str().ok().map(str::to_owned))
                                .collect::<Vec<String>>(),
                        ),
                        _ => None,
                    })
                    .unwrap_or_default(),
                members: None,
            },
        );
    }
    for cluster in clusters {
        let Some(id) = text(cluster, "id") else {
            continue;
        };
        labels.insert(
            id,
            Drawn {
                label: text(cluster, "label").unwrap_or_default(),
                identity: None,
                state: None,
                reasons: Vec::new(),
                members: integer(cluster, "members"),
            },
        );
    }
    labels
}

/// One thing the map draws: a node, or the cluster standing for many.
struct Drawn {
    label: String,
    /// What tells this node from another with the same display name, from the node's own
    /// `object_ref` (§11.4). `pid 4711` for a process, `inode 88213` for a socket.
    identity: Option<String>,
    state: Option<String>,
    reasons: Vec<String>,
    members: Option<i128>,
}

/// The first identity field of a node's `object_ref`, as `pid 4711` (`map-node.v1.yaml`).
///
/// `object_ref` carries "the schema it is served under and the values of that schema's identity
/// fields", so the first field after the schema is what the provider identifies the object by —
/// and therefore what tells two `containerd-shim` rows apart (§11.4). A canonical place is
/// identified by its `space`, which is already its label, so it has nothing to add.
fn identity_of(node: &RecordValue) -> Option<String> {
    let ono_value::Value::Map(reference) = node.get("object_ref")? else {
        return None;
    };
    reference
        .iter()
        .find(|(key, _)| &**key != "schema" && &**key != "space")
        .map(|(key, value)| format!("{key} {value}"))
}

/// The containment the map's own hierarchy edges declare, child lists in drawing order.
fn hierarchy(edges: &[RecordValue]) -> BTreeMap<String, Vec<String>> {
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for edge in edges {
        if text(edge, "kind").as_deref() != Some("hierarchy") {
            continue;
        }
        let (Some(source), Some(target)) = (text(edge, "source"), text(edge, "target")) else {
            continue;
        };
        children.entry(source).or_default().push(target);
    }
    children
}

/// The subtree below `parent`, indented under `prefix`.
#[allow(
    clippy::too_many_arguments,
    reason = "one drawing step, and every argument is part of the line it draws"
)]
fn branch(
    labels: &BTreeMap<String, Drawn>,
    ambiguous: &BTreeSet<String>,
    children: &BTreeMap<String, Vec<String>>,
    parent: &str,
    prefix: &str,
    charset: Charset,
    width: usize,
    drawn: &mut BTreeSet<String>,
) -> Vec<MapLine> {
    let (branch_mark, last_mark, trunk) = charset.branches();
    let Some(here) = children.get(parent) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let last = here.len().saturating_sub(1);
    for (position, child) in here.iter().enumerate() {
        if !drawn.insert(child.clone()) {
            // A place reached twice is drawn once: the second line would say the same thing in
            // a different spot, and a map that draws one object twice is not a map of anything.
            continue;
        }
        let mark = if position == last {
            last_mark
        } else {
            branch_mark
        };
        lines.push(MapLine::at(
            fit(
                &format!(
                    "{prefix}{mark}{}",
                    node_line(labels, ambiguous, child, charset)
                ),
                width,
            ),
            child,
        ));
        let deeper = if position == last {
            format!("{prefix}   ")
        } else {
            format!("{prefix}{trunk}")
        };
        lines.extend(branch(
            labels, ambiguous, children, child, &deeper, charset, width, drawn,
        ));
    }
    lines
}

/// The relation each node outside the hierarchy is reached by, from the map's own edges (§23.5).
///
/// Where more than one relation reaches the same node the labels are joined, in the map's own
/// edge order: "why is this here" has two answers and giving one would be choosing.
fn reached_by(
    edges: &[RecordValue],
    map: &RecordValue,
    nodes: &[RecordValue],
) -> BTreeMap<String, String> {
    let mut reached: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut note = |id: Option<String>, relation: &str| {
        let Some(id) = id else { return };
        let labels: &mut Vec<String> = reached.entry(id).or_default();
        if !labels.iter().any(|held| held == relation) {
            labels.push(relation.to_owned());
        }
    };
    for edge in edges {
        let Some(relation) = text(edge, "relation") else {
            continue;
        };
        // Both ends: an edge explains why either of the objects it joins is on this map, and the
        // parent of a process is reached by the same `process.parent_of` its children are.
        note(text(edge, "source"), &relation);
        note(text(edge, "target"), &relation);
    }
    // The canonical parent is on the map because it is the parent (§11.3) — `up` reaches it, and
    // no relationship edge does.
    if let Some(center) = text(map, "center")
        && let Some(parent) = nodes
            .iter()
            .find(|node| text(node, "id").as_deref() == Some(center.as_str()))
            .and_then(|node| text(node, "canonical_parent"))
    {
        note(Some(parent), "parent");
    }
    reached
        .into_iter()
        .map(|(id, labels)| (id, labels.join(", ")))
        .collect()
}

/// Which display names more than one drawn thing answers to (§11.4).
fn ambiguous(labels: &BTreeMap<String, Drawn>) -> BTreeSet<String> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for drawn in labels.values() {
        *seen.entry(drawn.label.as_str()).or_default() += 1;
    }
    seen.into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(label, _)| label.to_owned())
        .collect()
}

/// One drawn thing, on one line: what it is called, what state it is in, and why it was promoted.
///
/// A name two drawn things share carries the identity that tells them apart (§11.4): four
/// `containerd-shim` rows with nothing between them cannot be chosen from, which is what the
/// view is for.
fn node_line(
    labels: &BTreeMap<String, Drawn>,
    ambiguous: &BTreeSet<String>,
    id: &str,
    charset: Charset,
) -> String {
    let Some(drawn) = labels.get(id) else {
        return id.to_owned();
    };
    if let Some(members) = drawn.members {
        return format!("+ {members} more in {}", drawn.label);
    }
    let mut line = drawn.label.clone();
    if ambiguous.contains(&drawn.label)
        && let Some(identity) = &drawn.identity
    {
        line.push_str(&format!(" ({identity})"));
    }
    if let Some(state) = &drawn.state
        && !state.is_empty()
    {
        line.push_str(&format!("  {state}"));
    }
    if !drawn.reasons.is_empty() {
        line.push_str(&format!(
            "  {} {}",
            charset.landmark(),
            drawn.reasons.join(" ").replace('_', " ")
        ));
    }
    line
}

/// One relationship line: what it says, and which edge it says it about.
struct DrawnEdge {
    text: String,
    target: String,
    relation: String,
}

/// The relationships that are not containment, each showing its direction, its relation and — as
/// §23.5 requires — whether it was observed or inferred.
fn relation_lines(edges: &[RecordValue], charset: Charset) -> Vec<DrawnEdge> {
    edges
        .iter()
        .filter(|edge| text(edge, "kind").as_deref() == Some("relationship"))
        .map(|edge| {
            let source = text(edge, "source_label")
                .or_else(|| text(edge, "source"))
                .unwrap_or_default();
            let target_label = text(edge, "target_label")
                .or_else(|| text(edge, "target"))
                .unwrap_or_default();
            let relation = text(edge, "relation").unwrap_or_default();
            let confidence = text(edge, "confidence").unwrap_or_default();
            let arrow = charset.arrow(confidence == "exact");
            DrawnEdge {
                text: format!("{source} {arrow} {target_label}  {relation} ({confidence})"),
                target: text(edge, "target").unwrap_or_default(),
                relation,
            }
        })
        .collect()
}

/// What the bound left out, in one sentence — §23.6 forbids leaving it unsaid.
fn hidden_line(map: &RecordValue) -> Option<String> {
    let hidden = record(map, "hidden")?;
    let count = integer(&hidden, "count").unwrap_or_default();
    if count <= 0 {
        return None;
    }
    let clustered = integer(&hidden, "clustered").unwrap_or_default();
    let folded = integer(&hidden, "aggregated").unwrap_or_default();
    let mut how = Vec::new();
    if clustered > 0 {
        how.push(format!("{clustered} clustered"));
    }
    if folded > 0 {
        how.push(format!("{folded} aggregated"));
    }
    if how.is_empty() {
        Some(format!("{count} more not drawn"))
    } else {
        Some(format!("{count} more not drawn ({})", how.join(", ")))
    }
}
