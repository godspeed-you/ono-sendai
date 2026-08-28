//! The `SpatialMap` projection: ranking, semantic zoom, clustering and expansion
//! (spec v0.4 §22, §8, §23.1, §23.6, §34.2, §43.2, §45.3).
//!
//! §45.3 gives this crate "map graph selection, clustering, semantic zoom". This module is that:
//! it is handed a *horizon* — the places the shell observed around the current place, and the
//! relationship edges the providers asserted about them — and answers with the bounded, ranked,
//! semantically aggregated graph of §22. It asks nothing of a provider, invents no node and no
//! edge (§2.16, §49.5), and carries no screen coordinate, because layout is the renderer's and
//! §22 forbids it here outright.
//!
//! Four rules run through it:
//!
//! - **Bounded, never truncated in silence (§23.6, §53).** What does not fit is clustered, and
//!   what is neither drawn nor clustered is counted in [`HiddenSummary`]. A landmark is never
//!   dropped without the count that says so.
//! - **Ranked in §23.1's own priority order.** The current place first, then canonical exits,
//!   then landmarks, then the strongest relationships, then context.
//! - **Zoom aggregates concepts, it does not scale a drawing (§8).** A node finer than the
//!   requested level is replaced by its canonical ancestor at that level; the current place is
//!   never aggregated away, because it is where the user is standing.
//! - **Every rendered edge reaches a rendered node or the cluster standing for it (§43.2).**

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use jiff::Timestamp;
use ono_spatial_core::{
    Completeness, Confidence, Direction, HierarchyKind, Landmark, LandmarkReason, RelationshipEdge,
    SpaceKind, SpatialId, SpatialType,
};
use ono_spatial_index::{PinRegistry, SpatialIndex};
use ono_value::Provenance;
use sha2::{Digest, Sha256};

/// The default visible-node budget of a text map (§34.2).
pub const TEXT_MAP_BUDGET: usize = 30;

/// The budget an explicit `--all` map is still held to (§34.2, `spatial.map.node_budget`).
pub const MAP_NODE_BUDGET: usize = 100;

/// The canonical zoom levels of §8.1: L0 SYSTEM, L1 DOMAIN, L2 COLLECTION, L3 ENTITY,
/// L4 DETAIL/RELATION.
pub const MAX_ZOOM: u8 = 4;

/// One place the shell observed around the centre, and how it got there.
///
/// The horizon is the shell's to build, because only the shell may ask a provider anything
/// (§2.16, §45.6). What each place *is* — its label, its type, its landmarks — is read from the
/// index here, so a caller cannot describe a node into existence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HorizonPlace {
    /// The place.
    pub id: SpatialId,
    /// How many hierarchy hops from the centre it sits (§6.9's `--depth`).
    pub depth: usize,
    /// The place it is filed under inside this horizon, and why (§3.4).
    pub parent: Option<(SpatialId, HierarchyKind)>,
    /// The provider's own state word for the object, where it answered with one (§22's
    /// `state: StateSummary?`).
    pub state: Option<String>,
}

impl HorizonPlace {
    /// A place at `depth`, filed under `parent`.
    #[must_use]
    pub fn new(id: SpatialId, depth: usize, parent: Option<(SpatialId, HierarchyKind)>) -> Self {
        Self {
            id,
            depth,
            parent,
            state: None,
        }
    }

    /// The same place, carrying the state its provider reported.
    #[must_use]
    pub fn in_state(mut self, state: Option<String>) -> Self {
        self.state = state;
        self
    }
}

/// What the shell observed around the current place (§6.9, §45.6).
#[derive(Debug, Clone, Default)]
pub struct MapHorizon {
    places: Vec<HorizonPlace>,
    edges: Vec<RelationshipEdge>,
}

impl MapHorizon {
    /// An empty horizon.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a place, keeping the shallowest depth when it was already reached another way.
    pub fn place(&mut self, place: HorizonPlace) {
        if let Some(existing) = self.places.iter_mut().find(|known| known.id == place.id) {
            if place.depth < existing.depth {
                existing.depth = place.depth;
                existing.parent = place.parent;
            }
            if existing.state.is_none() {
                existing.state = place.state;
            }
            return;
        }
        self.places.push(place);
    }

    /// Adds a relationship edge the providers asserted (§3.5).
    pub fn edge(&mut self, edge: RelationshipEdge) {
        if !self.edges.contains(&edge) {
            self.edges.push(edge);
        }
    }

    /// The places, in the order they were added.
    #[must_use]
    pub fn places(&self) -> &[HorizonPlace] {
        &self.places
    }

    /// The relationship edges.
    #[must_use]
    pub fn edges(&self) -> &[RelationshipEdge] {
        &self.edges
    }
}

/// What a caller asked the map for (§6.9).
#[derive(Debug, Clone, Default)]
pub struct MapRequest {
    zoom: Option<u8>,
    depth: Option<usize>,
    all: bool,
    relations: Vec<String>,
    types: Vec<SpatialType>,
    expand: Vec<String>,
    focus: Option<String>,
}

impl MapRequest {
    /// The default map: the current place, its canonical children and their contents, bounded.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `--zoom <n>` — one of the five canonical levels of §8.1.
    #[must_use]
    pub fn zoom(mut self, level: u8) -> Self {
        self.zoom = Some(level.min(MAX_ZOOM));
        self
    }

    /// `--depth <n>` — how many hierarchy hops the horizon is drawn to (§6.9).
    #[must_use]
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = Some(depth);
        self
    }

    /// `--all` — the explicit larger bound the default is not (§6.9, §53).
    #[must_use]
    pub fn all(mut self, all: bool) -> Self {
        self.all = all;
        self
    }

    /// `--relations <list>` — keep only these relations' edges (§6.9).
    #[must_use]
    pub fn relations(mut self, relations: Vec<String>) -> Self {
        self.relations = relations;
        self
    }

    /// `--type <list>` — keep only nodes of these kinds (§6.9).
    #[must_use]
    pub fn types(mut self, types: Vec<SpatialType>) -> Self {
        self.types = types;
        self
    }

    /// `--expand <cluster>` — draw a cluster's members instead of the cluster (§8.3).
    #[must_use]
    pub fn expand(mut self, clusters: Vec<String>) -> Self {
        self.expand = clusters;
        self
    }

    /// `--focus <node>` — the node the view is centred on, which is never thereby the current
    /// place (§23.4).
    #[must_use]
    pub fn focus(mut self, node: impl Into<String>) -> Self {
        self.focus = Some(node.into());
        self
    }

    /// How many hierarchy hops to draw. Two by default: the current place, its canonical
    /// children and what lies behind them — §6.9's "canonical children and significant direct
    /// relationships within a bounded semantic horizon".
    #[must_use]
    pub fn horizon_depth(&self) -> usize {
        self.depth.unwrap_or(2)
    }

    /// How many nodes may be drawn (§34.2).
    #[must_use]
    pub fn node_budget(&self, configured: usize) -> usize {
        let configured = configured.max(1);
        if self.all {
            configured
        } else {
            TEXT_MAP_BUDGET.min(configured)
        }
    }
}

/// One node of a [`SpatialMap`] (§22's `MapNode`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapNode {
    /// The place's stable identity.
    pub id: SpatialId,
    /// The registry id of the canonical space this node *is*, where it is one (§41.1).
    pub space: Option<&'static str>,
    /// What kind of place it is (§3.3).
    pub object_type: SpatialType,
    /// The name a user reads.
    pub label: String,
    /// The provider's own state word, where it has one.
    pub state: Option<String>,
    /// The place `up` arrives at (§11.3).
    pub canonical_parent: Option<SpatialId>,
    /// Why this node deserves attention (§3.7).
    pub landmark_reasons: Vec<LandmarkReason>,
    /// How many hierarchy hops from the centre it was reached.
    pub depth: usize,
}

/// Whether an edge is hierarchy or graph — the distinction §2.6 forbids blurring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Containment or canonical grouping (§3.4). It asserts no operational dependency.
    Hierarchy,
    /// A real connection a provider asserted (§3.5).
    Relationship,
}

impl EdgeKind {
    /// The name a map legend shows.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Hierarchy => "hierarchy",
            EdgeKind::Relationship => "relationship",
        }
    }
}

/// One edge of a [`SpatialMap`] (§22's `MapEdge`), carrying everything §11.4 makes inspectable.
///
/// The endpoints are ids as text rather than [`SpatialId`]s because §8.2 lets a cluster stand for
/// an object: an edge to something the budget clustered points at the cluster, which is a real
/// thing on the map and not a spatial object (§43.2).
#[derive(Debug, Clone, PartialEq)]
pub struct MapEdge {
    /// The edge's stable identity.
    pub id: String,
    /// Where it starts — a drawn node or the cluster standing for it.
    pub source: String,
    /// What a person calls the source (§11.4).
    pub source_label: String,
    /// Where it leads.
    pub target: String,
    /// What a person calls the target (§11.4).
    pub target_label: String,
    /// The relation asserted, or the §3.4 grouping for a hierarchy edge.
    pub relation: String,
    /// Hierarchy or graph (§2.6).
    pub kind: EdgeKind,
    /// How confident the assertion is (§11.5).
    pub confidence: Confidence,
    /// Which way it runs.
    pub direction: Direction,
    /// Where the assertion came from (§11.4).
    pub provenance: Provenance,
    /// When it was observed (§11.4).
    pub observed_at: Option<Timestamp>,
}

/// A group of objects one node stands for, because the budget could not draw them (§8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapCluster {
    /// The cluster's stable identity, so `map --expand <id>` reaches the same group in a later
    /// run (§8.3).
    pub id: String,
    /// What the cluster is called.
    pub label: String,
    /// The objects it stands for.
    pub member_ids: Vec<SpatialId>,
    /// The dimension it was grouped along (§8.2).
    pub grouping: &'static str,
}

impl MapCluster {
    /// How many objects it stands for (§8.2: "A cluster MUST report the number of hidden
    /// objects").
    #[must_use]
    pub fn members(&self) -> usize {
        self.member_ids.len()
    }

    /// Whether `map --expand` can draw its members (§8.3).
    #[must_use]
    pub fn expandable(&self) -> bool {
        !self.member_ids.is_empty()
    }
}

/// What a bounded map left out (§22's `hidden: HiddenSummary`, §23.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HiddenSummary {
    /// How many known places are not drawn as their own node.
    pub count: usize,
    /// How many of those a cluster stands for.
    pub clustered: usize,
    /// How many were folded into a coarser node by the zoom level (§8.1).
    pub aggregated: usize,
}

/// The renderer-independent map of §22.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialMap {
    /// The current place the map is drawn around. Focus never changes it (§23.4).
    pub center: SpatialId,
    /// The node the view is centred on, where the caller named one (§23.4).
    pub focus: Option<SpatialId>,
    /// Which of §8.1's levels this projection is at.
    pub zoom_level: u8,
    /// The drawn nodes.
    pub nodes: Vec<MapNode>,
    /// The drawn edges.
    pub edges: Vec<MapEdge>,
    /// The groups standing in for what did not fit (§8.2).
    pub clusters: Vec<MapCluster>,
    /// What deserves attention here (§3.7).
    pub landmarks: Vec<Landmark>,
    /// What the bound left out (§23.6).
    pub hidden: HiddenSummary,
    /// When the projection was made.
    pub generated_at: Timestamp,
    /// Whether everything known is drawn (§3.6).
    pub completeness: Completeness,
}

/// The bounded, ranked, semantically aggregated map of `horizon` (§22).
#[must_use]
pub fn project(
    index: &SpatialIndex,
    center: &SpatialId,
    horizon: &MapHorizon,
    request: &MapRequest,
    pins: &PinRegistry,
    budget: usize,
    now: Timestamp,
) -> SpatialMap {
    let pinned: Vec<&SpatialId> = pins
        .pins()
        .map(ono_spatial_index::Pin::spatial_id)
        .collect();

    // 1. The horizon, cut to the requested depth and to the requested kinds of place.
    let depth = request.horizon_depth();
    let mut candidates: Vec<HorizonPlace> = horizon
        .places()
        .iter()
        .filter(|place| place.depth <= depth)
        .filter(|place| keeps_type(index, &place.id, request))
        .cloned()
        .collect();
    let known = candidates.len();

    // 2. Semantic zoom (§8.1). A place finer than the requested level is replaced by its
    //    canonical ancestor at that level; the centre is never folded away, because it is where
    //    the user is standing and §23.1 draws it first.
    let level = request
        .zoom
        .unwrap_or_else(|| finest_tier(index, &candidates));
    let mut folded: BTreeMap<SpatialId, SpatialId> = BTreeMap::new();
    if request.zoom.is_some() {
        candidates = fold(index, candidates, center, level, &mut folded);
    }
    let mut seen: BTreeSet<SpatialId> = BTreeSet::new();
    candidates.retain(|place| seen.insert(place.id.clone()));
    let aggregated = known.saturating_sub(candidates.len());

    // 3. Rank in §23.1's priority order, then bound (§34.2).
    let focus = request
        .focus
        .as_deref()
        .and_then(|text| candidates.iter().find(|place| place.id.as_str() == text))
        .map(|place| place.id.clone());
    candidates.sort_by_cached_key(|place| rank(index, place, center, focus.as_ref(), &pinned));
    // The geography keeps its declared order; the objects behind it take turns (§23.6).
    let objects = candidates
        .iter()
        .position(|place| tier_of(index, &place.id) > 2)
        .unwrap_or(candidates.len());
    let tail = interleave(candidates.split_off(objects));
    candidates.extend(tail);
    let (mut drawn, rest) = split(candidates, request.node_budget(budget));

    // 4. What did not fit is clustered rather than truncated (§8.2), and a cluster the caller
    //    named is opened instead — a view action that leaves the place alone (§8.3).
    let (clusters, opened) = expand(cluster(index, &rest), &request.expand);
    for id in opened {
        if let Some(place) = rest.iter().find(|place| place.id == id) {
            drawn.push(place.clone());
        }
    }
    let clustered: usize = clusters.iter().map(MapCluster::members).sum();
    let hidden = HiddenSummary {
        count: known.saturating_sub(drawn.len()),
        clustered,
        aggregated,
    };

    // 5. The nodes, then the edges between whatever is now drawn (§43.2).
    let nodes: Vec<MapNode> = drawn.iter().map(|place| node_of(index, place)).collect();
    let representative = representatives(&nodes, &clusters, &folded);
    let edges = edges_of(index, horizon, &representative, &clusters, request);
    let landmarks = landmarks_of(index, &nodes, &pinned);
    let completeness = if hidden.count == 0 {
        Completeness::Complete
    } else {
        Completeness::Bounded
    };

    SpatialMap {
        center: center.clone(),
        focus,
        zoom_level: level,
        nodes,
        edges,
        clusters,
        landmarks,
        hidden,
        generated_at: now,
        completeness,
    }
}

/// Whether a place survives `--type` (§6.9).
fn keeps_type(index: &SpatialIndex, id: &SpatialId, request: &MapRequest) -> bool {
    if request.types.is_empty() {
        return true;
    }
    let object_type = type_of(index, id);
    request.types.iter().any(|wanted| object_type.is_a(*wanted))
}

/// What kind of place this is: the geography's word for a canonical space, the index's for an
/// observed object.
fn type_of(index: &SpatialIndex, id: &SpatialId) -> SpatialType {
    crate::resolve::space_of(id).map_or_else(
        || {
            index
                .get(id)
                .map_or(SpatialType::System, |entry| entry.object().object_type())
        },
        |space| space.object_type,
    )
}

/// Which of §8.1's levels a place sits at.
///
/// L0–L2 are the canonical geography's own three kinds — root, domain, collection. An observed
/// object is L3, except for the objects §8.1 itself calls detail: a socket, a connection, an open
/// file, a namespace, a cgroup.
fn tier_of(index: &SpatialIndex, id: &SpatialId) -> u8 {
    if let Some(space) = crate::resolve::space_of(id) {
        return match space.kind {
            SpaceKind::Root => 0,
            SpaceKind::Domain => 1,
            SpaceKind::Collection => 2,
        };
    }
    match type_of(index, id) {
        SpatialType::Socket
        | SpatialType::Listener
        | SpatialType::Connection
        | SpatialType::Endpoint
        | SpatialType::File
        | SpatialType::Namespace
        | SpatialType::Cgroup
        | SpatialType::Address => 4,
        _ => 3,
    }
}

/// The finest level anything in the horizon sits at — the level a map without `--zoom` is drawn
/// at, because it folds nothing.
fn finest_tier(index: &SpatialIndex, places: &[HorizonPlace]) -> u8 {
    places
        .iter()
        .map(|place| tier_of(index, &place.id))
        .max()
        .unwrap_or(0)
        .min(MAX_ZOOM)
}

/// Replaces every place finer than `level` by its canonical ancestor at that level (§8.1).
fn fold(
    index: &SpatialIndex,
    places: Vec<HorizonPlace>,
    center: &SpatialId,
    level: u8,
    folded: &mut BTreeMap<SpatialId, SpatialId>,
) -> Vec<HorizonPlace> {
    let mut projected = Vec::with_capacity(places.len());
    for place in places {
        if &place.id == center || tier_of(index, &place.id) <= level {
            projected.push(place);
            continue;
        }
        match ancestor_at(index, &place.id, level) {
            Some(ancestor) => {
                folded.insert(place.id.clone(), ancestor.clone());
                projected.push(HorizonPlace::new(
                    ancestor,
                    place.depth,
                    place.parent.clone(),
                ));
            }
            // A place with no ancestor at that level cannot be folded honestly, so it is drawn
            // as itself rather than dropped (§2.17).
            None => projected.push(place),
        }
    }
    projected
}

/// The canonical ancestor of `id` at level `level` or coarser (§8.1, §11.3).
fn ancestor_at(index: &SpatialIndex, id: &SpatialId, level: u8) -> Option<SpatialId> {
    let mut current = crate::resolve::parent_of(index, id)?;
    for _ in 0..16 {
        if tier_of(index, &current) <= level {
            return Some(current);
        }
        current = crate::resolve::parent_of(index, &current)?;
    }
    None
}

/// §23.1's priority order, as a sort key: the current place, then the focused node, then the
/// canonical exits nearest the centre, then landmarks, then everything else — and always
/// deterministically (§29.3).
fn rank(
    index: &SpatialIndex,
    place: &HorizonPlace,
    center: &SpatialId,
    focus: Option<&SpatialId>,
    pinned: &[&SpatialId],
) -> (u8, u8, u8, usize, u8, String, String) {
    let entry = index.get(&place.id);
    // §23.1 ranks landmarks third, and §26.3 forbids treating every heuristic as an incident: a
    // landmark that merely *informs* — a recent start, a privilege — is shown on the node it
    // belongs to but does not push another object off the map. Only the reasons that ask for
    // attention reorder it, and those are properties of the object rather than of the clock, so
    // two maps of an unchanged system name the same nodes (§29.3, §43.2).
    let landmark = if pinned.contains(&&place.id) {
        0
    } else if entry.is_some_and(|entry| entry.landmarks().iter().any(attention)) {
        1
    } else {
        2
    };
    let name = entry.map_or_else(
        || {
            crate::resolve::space_of(&place.id)
                .map_or_else(String::new, |space| space.label.to_ascii_lowercase())
        },
        |entry| entry.object().display_name().to_ascii_lowercase(),
    );
    (
        u8::from(&place.id != center),
        u8::from(focus != Some(&place.id)),
        // §23.1 ranks canonical exits second, above every individual object: a map of a host with
        // two hundred devices that drew twenty of the devices and none of the collections would
        // be a list, not a map. The geography is coarse (L0–L2), the objects are fine (L3–L4).
        tier_of(index, &place.id),
        place.depth,
        landmark,
        name,
        place.id.to_string(),
    )
}

/// Deals the objects out over the collections they belong to, one from each in turn.
///
/// §23.6 requires a map that cannot fit its set to "cluster; rank; paginate". Ranking alone lets
/// the largest collection take every remaining slot — two hundred devices and no processes — and
/// a map that shows one corner of the system is not orientation. So the objects keep their rank
/// *within* each collection and take turns *between* them, which is the shape §8.2's own example
/// draws: every collection visible, each with a few members and a count for the rest.
fn interleave(places: Vec<HorizonPlace>) -> Vec<HorizonPlace> {
    let mut groups: Vec<(Option<SpatialId>, Vec<HorizonPlace>)> = Vec::new();
    for place in places {
        let key = place.parent.as_ref().map(|(parent, _)| parent.clone());
        match groups.iter_mut().find(|(known, _)| *known == key) {
            Some((_, members)) => members.push(place),
            None => groups.push((key, vec![place])),
        }
    }
    let mut dealt = Vec::new();
    let mut round = 0;
    loop {
        let mut any = false;
        for (_, members) in &groups {
            if let Some(place) = members.get(round) {
                dealt.push(place.clone());
                any = true;
            }
        }
        if !any {
            return dealt;
        }
        round += 1;
    }
}

/// Whether a landmark asks for attention rather than merely offering orientation (§24.1).
fn attention(landmark: &Landmark) -> bool {
    matches!(
        landmark.reason(),
        LandmarkReason::Failed
            | LandmarkReason::Restarting
            | LandmarkReason::PublicListener
            | LandmarkReason::StoragePressure
            | LandmarkReason::UserPinned
            | LandmarkReason::HighCpu
            | LandmarkReason::HighMemory
    )
}

/// The first `budget` places, and the rest.
fn split(places: Vec<HorizonPlace>, budget: usize) -> (Vec<HorizonPlace>, Vec<HorizonPlace>) {
    let mut drawn = places;
    let rest = if drawn.len() > budget {
        drawn.split_off(budget)
    } else {
        Vec::new()
    };
    (drawn, rest)
}

/// The clusters that stand for what the budget could not draw (§8.2).
///
/// The dimension is §8.2's first: the canonical collection each object is filed under. It is the
/// one every place has, it is the one `enter` reaches, and it makes the cluster's id the same in
/// two separate runs, which is what `map --expand <id>` needs (§8.3).
fn cluster(index: &SpatialIndex, rest: &[HorizonPlace]) -> Vec<MapCluster> {
    let mut groups: BTreeMap<String, (String, Vec<SpatialId>)> = BTreeMap::new();
    for place in rest {
        let parent = place
            .parent
            .as_ref()
            .map(|(parent, _)| parent.clone())
            .or_else(|| crate::resolve::parent_of(index, &place.id));
        let (key, label) = match parent {
            Some(parent) => (parent.to_string(), label_of(index, &parent)),
            None => (
                type_of(index, &place.id).as_str().to_owned(),
                format!("{} places", type_of(index, &place.id).as_str()),
            ),
        };
        groups
            .entry(key)
            .or_insert_with(|| (label, Vec::new()))
            .1
            .push(place.id.clone());
    }
    groups
        .into_iter()
        .map(|(key, (label, mut members))| {
            members.sort();
            MapCluster {
                id: token("cluster", &["canonical_collection", &key]),
                label,
                member_ids: members,
                grouping: "canonical_collection",
            }
        })
        .collect()
}

/// Removes the clusters the caller expanded, and answers with the members they stood for (§8.3).
fn expand(clusters: Vec<MapCluster>, wanted: &[String]) -> (Vec<MapCluster>, Vec<SpatialId>) {
    if wanted.is_empty() {
        return (clusters, Vec::new());
    }
    let mut kept = Vec::new();
    let mut opened = Vec::new();
    for cluster in clusters {
        if wanted.contains(&cluster.id) {
            opened.extend(cluster.member_ids.iter().cloned());
        } else {
            kept.push(cluster);
        }
    }
    (kept, opened)
}

/// The label a place is drawn under.
fn label_of(index: &SpatialIndex, id: &SpatialId) -> String {
    crate::resolve::space_of(id).map_or_else(
        || {
            index.get(id).map_or_else(
                || id.to_string(),
                |entry| entry.object().display_name().to_owned(),
            )
        },
        |space| space.label.to_owned(),
    )
}

/// One drawn node (§22's `MapNode`).
fn node_of(index: &SpatialIndex, place: &HorizonPlace) -> MapNode {
    let entry = index.get(&place.id);
    MapNode {
        id: place.id.clone(),
        space: crate::resolve::space_of(&place.id).map(|space| space.id),
        object_type: type_of(index, &place.id),
        label: label_of(index, &place.id),
        state: place.state.clone(),
        canonical_parent: crate::resolve::parent_of(index, &place.id),
        landmark_reasons: entry.map_or_else(Vec::new, |entry| {
            let mut reasons: Vec<LandmarkReason> =
                entry.landmarks().iter().map(Landmark::reason).collect();
            reasons.sort_unstable();
            reasons.dedup();
            reasons
        }),
        depth: place.depth,
    }
}

/// Which drawn node or cluster stands for each place of the horizon (§43.2).
fn representatives(
    nodes: &[MapNode],
    clusters: &[MapCluster],
    folded: &BTreeMap<SpatialId, SpatialId>,
) -> BTreeMap<SpatialId, String> {
    let mut map: BTreeMap<SpatialId, String> = BTreeMap::new();
    for node in nodes {
        map.insert(node.id.clone(), node.id.to_string());
    }
    for cluster in clusters {
        for member in &cluster.member_ids {
            map.insert(member.clone(), cluster.id.clone());
        }
    }
    for (place, target) in folded {
        if let Some(representative) = map.get(target).cloned() {
            map.insert(place.clone(), representative);
        }
    }
    map
}

/// The edges between what is drawn: the hierarchy of §3.4 and the relationships of §3.5, each
/// re-pointed at whatever now stands for its endpoints, and never invented (§43.2).
fn edges_of(
    index: &SpatialIndex,
    horizon: &MapHorizon,
    representative: &BTreeMap<SpatialId, String>,
    clusters: &[MapCluster],
    request: &MapRequest,
) -> Vec<MapEdge> {
    let mut edges: Vec<MapEdge> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let names: BTreeMap<&str, &str> = clusters
        .iter()
        .map(|cluster| (cluster.id.as_str(), cluster.label.as_str()))
        .collect();

    // The hierarchy first: §23.1 ranks canonical exits above every relationship.
    for place in horizon.places() {
        let Some((parent, kind)) = &place.parent else {
            continue;
        };
        let (Some(source), Some(target)) = (
            representative.get(parent).cloned(),
            representative.get(&place.id).cloned(),
        ) else {
            continue;
        };
        let relation = kind.as_str();
        if source == target || !keeps_relation(relation, request) {
            continue;
        }
        let id = token("edge", &[&source, &target, relation, "outbound", "exact"]);
        if !seen.insert(id.clone()) {
            continue;
        }
        edges.push(MapEdge {
            source_label: naming(index, &source, &names),
            target_label: naming(index, &target, &names),
            id,
            source,
            target,
            relation: relation.to_owned(),
            kind: EdgeKind::Hierarchy,
            confidence: Confidence::Exact,
            direction: Direction::Outbound,
            // §4.1: the canonical geography is declared by the spatial layer itself, so it is the
            // thing that says so — not a provider that never asserted it.
            provenance: Provenance::local(
                "ono.spatial",
                ono_value::SchemaId::new("ono.spatial-map", 1),
            ),
            observed_at: index
                .get(&place.id)
                .map(ono_spatial_index::IndexEntry::observed_at),
        });
    }

    for edge in horizon.edges() {
        let (Some(source), Some(target)) = (
            representative.get(edge.source()).cloned(),
            representative.get(edge.target()).cloned(),
        ) else {
            continue;
        };
        let relation = edge.relation().as_str();
        if source == target || !keeps_relation(relation, request) {
            continue;
        }
        let id = token(
            "edge",
            &[
                &source,
                &target,
                relation,
                edge.direction().as_str(),
                edge.confidence().as_str(),
            ],
        );
        if !seen.insert(id.clone()) {
            continue;
        }
        edges.push(MapEdge {
            source_label: naming(index, &source, &names),
            target_label: naming(index, &target, &names),
            id,
            source,
            target,
            relation: relation.to_owned(),
            kind: EdgeKind::Relationship,
            confidence: edge.confidence(),
            direction: edge.direction(),
            provenance: edge.provenance().clone(),
            observed_at: Some(edge.observed_at()),
        });
    }
    edges
}

/// What a person calls whatever an edge endpoint is — a place, or the cluster standing for one.
fn naming(index: &SpatialIndex, endpoint: &str, clusters: &BTreeMap<&str, &str>) -> String {
    if let Some(label) = clusters.get(endpoint) {
        return (*label).to_owned();
    }
    SpatialId::parse(endpoint).map_or_else(|| endpoint.to_owned(), |id| label_of(index, &id))
}

/// Whether an edge survives `--relations` (§6.9, §43.2: filtering removes, it never invents).
fn keeps_relation(relation: &str, request: &MapRequest) -> bool {
    request.relations.is_empty() || request.relations.iter().any(|wanted| wanted == relation)
}

/// A stable identity for something the map draws that is not a spatial object: an edge between
/// two endpoints, or a cluster over a grouping key.
///
/// It is a hash rather than a rendering so that two runs of the same map name the same edges and
/// the same clusters — which is what `--relations` filtering and `--expand` are checked against
/// (§8.3, §43.2) — without the id becoming a second, parseable spelling of the graph.
fn token(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    let digest = hasher.finalize();
    let mut id = String::with_capacity(prefix.len() + 25);
    id.push_str(prefix);
    id.push(':');
    for byte in digest.iter().take(12) {
        let _ = write!(id, "{byte:02x}");
    }
    id
}

/// The landmarks of the drawn nodes (§22, §26.4).
fn landmarks_of(index: &SpatialIndex, nodes: &[MapNode], pinned: &[&SpatialId]) -> Vec<Landmark> {
    let mut landmarks: Vec<Landmark> = Vec::new();
    for node in nodes {
        if let Some(entry) = index.get(&node.id) {
            landmarks.extend(entry.landmarks().iter().cloned());
        }
        if pinned.contains(&&node.id)
            && !landmarks.iter().any(|landmark| {
                landmark.subject() == &node.id && landmark.reason() == LandmarkReason::UserPinned
            })
        {
            landmarks.push(Landmark::built_in(
                node.id.clone(),
                LandmarkReason::UserPinned,
                format!("pinned as `{}`", node.label),
            ));
        }
    }
    landmarks.sort_by_key(|landmark| {
        (
            landmark.reason(),
            landmark.subject().to_string(),
            landmark.evidence().to_owned(),
        )
    });
    landmarks.dedup_by(|a, b| {
        a.reason() == b.reason() && a.subject() == b.subject() && a.evidence() == b.evidence()
    });
    landmarks
}
