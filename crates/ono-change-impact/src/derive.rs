//! Deriving the impact graph of §9 from the v0.4 topology.
//!
//! §9.3 makes v0.4 relationships "the primary topology source", and the shape of the walk is the
//! one `trace` already uses for the v0.2 graph: breadth-first, depth-bounded, node-bounded, and
//! explicit about having been cut short. What is different here is what the walk carries. A
//! `trace` node is an object; an [`ImpactNode`] is an object *plus the reason Ono believes the
//! plan reaches it*, because §3.5 requires impact to retain provenance and §9.6 requires the
//! places where belief runs out to be visible rather than absent.
//!
//! Four decisions are worth stating before the code:
//!
//! - **Classes describe distance from the plan, not severity.** §9.2's `DEPENDENT` is one
//!   relation hop and `TRANSITIVE_RELATED` is more than one. Neither is a claim that anything
//!   will fail, which §9.1 rules out in as many words.
//! - **An edge's confidence travels unchanged.** [`ono_spatial_core::Confidence::as_str`] is
//!   written onto the node, so `inferred` arrives as `inferred`. §11.5 of v0.4 has no operation
//!   that strengthens one and neither does this module.
//! - **A boundary is recorded where the graph stops.** A declared exit that was withheld, or one
//!   whose relation is acquired at [`AcquisitionCost::External`], ends the walk *and* leaves an
//!   [`UnknownBoundary`] behind. §9.6's whole point is that a graph which simply stops has told
//!   the operator nothing.
//! - **History is evidence, never confidence.** §9.4 permits v0.5 evidence to strengthen
//!   *relevance* and forbids upgrading correlation to causation, so the hook of
//!   [`ImpactRequest::with_history`] contributes a citation and a [`Prominence`], and cannot
//!   reach the confidence field at all.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    EffectConfidence, EffectDomain, FrozenTarget, ImpactClass, ImpactGraph, ImpactNode, PlanAction,
    ProposedEffect, UnknownBoundary,
};
use ono_spatial_core::{
    AcquisitionCost, Confidence, PermissionState, RelationshipEdge, SpatialId, relation,
};
use ono_spatial_index::{IndexEntry, SpatialIndex};
use ono_spatial_query::cost::{self, CostEstimate, INTERACTIVE_BUDGET};
use ono_value::ErrorValue;

/// How many relation hops impact follows when the caller does not say.
///
/// §9.3's own drawing is two hops — a configuration file, the service that reads it, the sockets
/// that service owns — so three answers the question the specification uses to motivate impact
/// and then stops. §52.2 asks impact to begin rendering within 100 ms over an indexed world, and
/// a default that fans out further is a default that cannot.
pub const DEFAULT_DEPTH: usize = 3;

/// How many objects impact collects when the caller does not say.
///
/// §9.5 permits a summary and forbids one that reads as a whole graph. A bounded walk that says
/// it was bounded satisfies both; an unbounded one on a busy host satisfies neither, because
/// nobody reads four thousand transitive relations.
pub const DEFAULT_NODE_BUDGET: usize = 256;

/// What the v0.5 ledger has observed about one object under actions of this kind (§9.4).
///
/// §9.4 is permissive and bounded in the same sentence: evidence "MAY strengthen impact
/// relevance" and "MUST NOT upgrade correlation to causation unless a causal rule supports it".
/// So this type records a count and a sentence, and offers [`HistoricalRelevance::prominence`] —
/// a rendering hint — with nothing that touches a node's confidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalRelevance {
    observations: u32,
    action_kind: Arc<str>,
    note: Arc<str>,
}

impl HistoricalRelevance {
    /// Records that the ledger saw this relationship change `observations` times under actions of
    /// `action_kind`, and what a person should read about it.
    #[must_use]
    pub fn new(
        observations: u32,
        action_kind: impl Into<Arc<str>>,
        note: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            observations,
            action_kind: action_kind.into(),
            note: note.into(),
        }
    }

    /// How many times the ledger observed it.
    #[must_use]
    pub const fn observations(&self) -> u32 {
        self.observations
    }

    /// The kind of action the observations were made under.
    #[must_use]
    pub fn action_kind(&self) -> &str {
        &self.action_kind
    }

    /// The sentence a person reads beside the node.
    #[must_use]
    pub fn note(&self) -> &str {
        &self.note
    }

    /// How prominently a renderer should show the node (§9.4).
    ///
    /// One observation is a coincidence and says so: §9.4 speaks of relationships the ledger has
    /// *repeatedly* observed changing, so a single sighting leaves the node where it was.
    #[must_use]
    pub const fn prominence(&self) -> Prominence {
        if self.observations >= 2 {
            Prominence::Raised
        } else {
            Prominence::Ordinary
        }
    }

    /// The citation this relevance contributes to a node's evidence (§3.5).
    #[must_use]
    pub fn citation(&self) -> String {
        format!(
            "ledger:{} observations under `{}`",
            self.observations, self.action_kind
        )
    }
}

/// How prominently a node is shown, which is the only thing v0.5 evidence may change (§9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Prominence {
    /// Shown in the position its §9.2 class earns it.
    #[default]
    Ordinary,
    /// Shown first among its class, because the ledger has repeatedly seen this relationship
    /// change under actions of this kind (§9.4).
    Raised,
}

impl Prominence {
    /// The word a renderer or a record spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Prominence::Ordinary => "ordinary",
            Prominence::Raised => "raised",
        }
    }
}

/// One request to derive impact: a world, a plan, and the bounds the answer must respect (§9).
///
/// The index and the actions are borrowed rather than owned because §2.1 makes planning
/// side-effect free and this crate performs no I/O: everything it reasons over was observed by
/// someone else, and it may not go and look for more.
pub struct ImpactRequest<'a> {
    index: &'a SpatialIndex,
    targets: &'a [FrozenTarget],
    actions: &'a [PlanAction],
    now: Timestamp,
    depth: usize,
    node_budget: usize,
    history: Option<&'a dyn Fn(&SpatialId) -> Option<HistoricalRelevance>>,
    cost_accepted: bool,
}

impl fmt::Debug for ImpactRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImpactRequest")
            .field("targets", &self.targets.len())
            .field("actions", &self.actions.len())
            .field("now", &self.now)
            .field("depth", &self.depth)
            .field("node_budget", &self.node_budget)
            .field("history", &self.history.is_some())
            .field("cost_accepted", &self.cost_accepted)
            .finish()
    }
}

impl<'a> ImpactRequest<'a> {
    /// Asks what `actions` over `targets` could touch in the world `index` holds, as of `now`.
    ///
    /// `now` is a parameter rather than a clock reading because the walk consults it: §3.5 gives
    /// an edge an optional validity window, and a relationship the provider says has already
    /// ended is not a relationship this plan can travel along.
    #[must_use]
    pub fn new(
        index: &'a SpatialIndex,
        targets: &'a [FrozenTarget],
        actions: &'a [PlanAction],
        now: Timestamp,
    ) -> Self {
        Self {
            index,
            targets,
            actions,
            now,
            depth: DEFAULT_DEPTH,
            node_budget: DEFAULT_NODE_BUDGET,
            history: None,
            cost_accepted: false,
        }
    }

    /// Follows at most `depth` relation hops from the plan's targets (§9.5, §52.2).
    #[must_use]
    pub const fn to_depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// Collects at most `nodes` objects before the graph is marked truncated (§9.5).
    #[must_use]
    pub const fn within_nodes(mut self, nodes: usize) -> Self {
        self.node_budget = nodes;
        self
    }

    /// Consults the v0.5 evidence ledger for each object the walk reaches (§9.4).
    ///
    /// The hook answers with what the ledger has repeatedly observed, and the answer becomes a
    /// citation and a [`Prominence`]. It is deliberately unable to raise a node's confidence:
    /// §9.4 forbids upgrading correlation to causation, and a hook that could write the
    /// confidence field would be exactly that upgrade, one call site away.
    #[must_use]
    pub fn with_history(
        mut self,
        history: &'a dyn Fn(&SpatialId) -> Option<HistoricalRelevance>,
    ) -> Self {
        self.history = Some(history);
        self
    }

    /// Records that the operator asked for this traversal knowing what it costs (v0.4.1 §34.3).
    ///
    /// The cost refusal exists to stop work nobody meant. A caller who typed the request has
    /// meant it, so [`derive_within_budget`] answers rather than refusing.
    #[must_use]
    pub const fn accepting_cost(mut self) -> Self {
        self.cost_accepted = true;
        self
    }

    /// The world the walk reads.
    #[must_use]
    pub const fn index(&self) -> &SpatialIndex {
        self.index
    }

    /// The plan's frozen targets (§7.1).
    #[must_use]
    pub const fn targets(&self) -> &[FrozenTarget] {
        self.targets
    }

    /// The plan's actions.
    #[must_use]
    pub const fn actions(&self) -> &[PlanAction] {
        self.actions
    }

    /// The instant the walk is made at.
    #[must_use]
    pub const fn now(&self) -> Timestamp {
        self.now
    }

    /// How many hops it follows.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// How many objects it collects.
    #[must_use]
    pub const fn node_budget(&self) -> usize {
        self.node_budget
    }

    /// Every effect the plan's actions declare.
    fn effects(&self) -> impl Iterator<Item = (&PlanAction, &ProposedEffect)> {
        self.actions
            .iter()
            .flat_map(|action| action.effects().iter().map(move |effect| (action, effect)))
    }

    /// The identities of the plan's targets that the index actually holds.
    fn seeds(&self) -> Vec<SpatialId> {
        let mut seeds = Vec::new();
        for target in self.targets {
            if let Some(id) = target.spatial_id().and_then(SpatialId::parse)
                && self.index.get(&id).is_some()
                && !seeds.contains(&id)
            {
                seeds.push(id);
            }
        }
        for (_, effect) in self.effects() {
            if let Some(id) = effect.object().and_then(SpatialId::parse)
                && self.index.get(&id).is_some()
                && !seeds.contains(&id)
            {
                seeds.push(id);
            }
        }
        seeds
    }
}

/// The graph, and the historical relevance §9.4 permits beside it.
///
/// The prominence lives here rather than on [`ImpactNode`] for the reason §9.4 gives: it is a
/// display property derived from the ledger, and an impact node is a statement about topology.
/// Keeping them apart is what makes "history may raise prominence and may not raise confidence"
/// a property of the types rather than a rule a renderer has to remember.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImpactDerivation {
    graph: ImpactGraph,
    history: BTreeMap<Arc<str>, HistoricalRelevance>,
}

impl ImpactDerivation {
    /// The impact graph (§9.2).
    #[must_use]
    pub const fn graph(&self) -> &ImpactGraph {
        &self.graph
    }

    /// The graph, taken out of the derivation.
    #[must_use]
    pub fn into_graph(self) -> ImpactGraph {
        self.graph
    }

    /// What the ledger said about one node, where it said anything (§9.4).
    #[must_use]
    pub fn relevance(&self, node: &str) -> Option<&HistoricalRelevance> {
        self.history.get(node)
    }

    /// How prominently a renderer should show one node (§9.4).
    #[must_use]
    pub fn prominence(&self, node: &str) -> Prominence {
        self.history
            .get(node)
            .map_or(Prominence::Ordinary, HistoricalRelevance::prominence)
    }

    /// Every node the ledger raised, in identity order (§9.4).
    #[must_use]
    pub fn raised(&self) -> Vec<&str> {
        self.history
            .iter()
            .filter(|(_, relevance)| relevance.prominence() == Prominence::Raised)
            .map(|(id, _)| id.as_ref())
            .collect()
    }
}

/// v0.4.1 §34.1's coarse estimate for the traversal `request` asks for.
///
/// The four inputs §34.1 names are all here: how many candidates the walk may consider, the
/// fan-out it may meet, the acquisition class of the relations it would follow, and the depth.
/// It is conservative in the direction §34.1 points — the widest fan-out in the index rather than
/// the average, the most expensive class the seeds' types declare rather than the typical one —
/// because the estimate exists to catch obviously explosive work, not to be accurate.
#[must_use]
pub fn estimate(request: &ImpactRequest<'_>) -> CostEstimate {
    let seeds = request.seeds();
    let candidates = request
        .node_budget
        .min(request.index.len())
        .max(seeds.len())
        .max(1);
    let fan_out = request
        .index
        .entries()
        .map(|entry| entry.edges().len())
        .max()
        .unwrap_or(1)
        .max(1);
    let class = seeds
        .iter()
        .filter_map(|id| request.index.get(id))
        .flat_map(|entry| relation::exits_from(entry.object().object_type()))
        .map(|(_, spec)| spec.cost_class.acquisition())
        .max()
        .unwrap_or(AcquisitionCost::Moderate);
    let depth = u32::try_from(request.depth).unwrap_or(u32::MAX);
    let estimate = CostEstimate::new(candidates, fan_out, class, depth);
    if request.cost_accepted {
        estimate.requested()
    } else {
        estimate
    }
}

/// The impact graph for `request` (§9).
///
/// Always answers. A walk that runs out of budget produces a graph that says it was truncated
/// (§9.5); a walk that meets a boundary produces one that says where Ono stopped seeing (§9.6).
#[must_use]
pub fn derive(request: &ImpactRequest<'_>) -> ImpactGraph {
    derive_in_detail(request).into_graph()
}

/// The impact graph and the §9.4 relevance beside it.
#[must_use]
pub fn derive_in_detail(request: &ImpactRequest<'_>) -> ImpactDerivation {
    Walk::new(request).run()
}

/// The impact graph, or the refusal v0.4.1 §33.3 requires for a traversal nobody could have meant.
///
/// # Errors
///
/// `spatial.cost_refused` when [`estimate`] is beyond [`INTERACTIVE_BUDGET`] and the caller did
/// not say the cost is acceptable. §33.3 is explicit that the choice is to refuse or to bound
/// rather than "silently appear hung", and [`ImpactRequest::accepting_cost`] is §34.3's request
/// path for the caller who wants it anyway.
pub fn derive_within_budget(request: &ImpactRequest<'_>) -> Result<ImpactDerivation, ErrorValue> {
    let estimate = estimate(request);
    if estimate.exceeds(INTERACTIVE_BUDGET) {
        return Err(cost::refusal(&estimate, "impact"));
    }
    Ok(derive_in_detail(request))
}

/// The breadth-first closure of §9.3, and the bookkeeping §9.5 and §9.6 require of it.
struct Walk<'a> {
    request: &'a ImpactRequest<'a>,
    graph: ImpactGraph,
    history: BTreeMap<Arc<str>, HistoricalRelevance>,
    visited: BTreeSet<SpatialId>,
    truncation: Option<String>,
}

impl<'a> Walk<'a> {
    fn new(request: &'a ImpactRequest<'a>) -> Self {
        Self {
            request,
            graph: ImpactGraph::empty(),
            history: BTreeMap::new(),
            visited: BTreeSet::new(),
            truncation: None,
        }
    }

    fn run(mut self) -> ImpactDerivation {
        let frontier = self.seed_targets();
        let frontier = self.seed_effects(frontier);
        self.expand(frontier);
        self.external_boundaries();
        self.unknown_boundaries();
        self.graph.sort();
        let graph = match self.truncation {
            Some(reason) => self.graph.truncated(reason),
            None => self.graph,
        };
        ImpactDerivation {
            graph,
            history: self.history,
        }
    }

    /// §9.2: "the plan names this object" — every frozen target is a `DIRECT_TARGET`, whether or
    /// not the index holds it. A target the index has never seen is still a target, and dropping
    /// it would make the blast radius disagree with the plan (§9.5).
    fn seed_targets(&mut self) -> Vec<SpatialId> {
        let index = self.request.index;
        let mut frontier = Vec::new();
        for target in self.request.targets {
            let spatial = target.spatial_id().and_then(SpatialId::parse);
            match spatial.as_ref().and_then(|id| index.get(id)) {
                Some(entry) => {
                    let Some(id) = spatial else { continue };
                    let node = self
                        .node_of(entry, ImpactClass::DirectTarget, 0)
                        // §7.1 froze this object against a resolved observation, so the plan's
                        // own claim on it is exact. The relation fields stay empty: no edge
                        // reached it, and inventing one would be provenance nobody asserted.
                        .with_confidence(Confidence::Exact.as_str())
                        .citing(format!("target:{}", target.identity()));
                    self.graph.add(node);
                    if !frontier.contains(&id) {
                        frontier.push(id.clone());
                    }
                    self.record_boundaries(&id);
                    self.visited.insert(id);
                }
                None => {
                    let id = target.spatial_id().unwrap_or_else(|| target.identity());
                    let mut node = ImpactNode::new(
                        id,
                        target.label(),
                        target.schema(),
                        ImpactClass::DirectTarget,
                        0,
                    )
                    .with_confidence(Confidence::Exact.as_str())
                    .citing(format!("target:{}", target.identity()));
                    if let Some(host) = target.host() {
                        node = node.on_host(host);
                    }
                    self.graph.add(node);
                }
            }
        }
        frontier
    }

    /// §9.2: an object an action's declared effect lands on is a `DIRECT_EFFECT`, and one in an
    /// external or remote domain is an `EXTERNAL_SIDE_EFFECT` (§35.1).
    fn seed_effects(&mut self, mut frontier: Vec<SpatialId>) -> Vec<SpatialId> {
        let index = self.request.index;
        for (_, effect) in self.request.effects() {
            let Some(object) = effect.object() else {
                continue;
            };
            let class = if effect.domain().is_external() {
                ImpactClass::ExternalSideEffect
            } else {
                ImpactClass::DirectEffect
            };
            let spatial = SpatialId::parse(object);
            match spatial.as_ref().and_then(|id| index.get(id)) {
                Some(entry) => {
                    let Some(id) = spatial else { continue };
                    let node = self
                        .node_of(entry, class, 0)
                        // §8.1's confidence is what Ono can say about this effect, and it is the
                        // honest provenance of a node no edge reached.
                        .with_confidence(effect.confidence().as_str())
                        .citing(effect.id().to_string());
                    self.graph.add(node);
                    if class != ImpactClass::ExternalSideEffect && !self.visited.contains(&id) {
                        frontier.push(id.clone());
                        self.record_boundaries(&id);
                        self.visited.insert(id);
                    }
                }
                None => {
                    // An effect may name something the spatial layer has no place for — a cloud
                    // resource, a webhook endpoint, a package. It is still an object the plan
                    // touches, and the domain is the most Ono can say about what kind it is.
                    self.graph.add(
                        ImpactNode::new(object, object, effect.domain().as_str(), class, 0)
                            .with_confidence(effect.confidence().as_str())
                            .citing(effect.id().to_string()),
                    );
                }
            }
        }
        frontier
    }

    /// The depth-bounded breadth-first closure itself (§9.3).
    fn expand(&mut self, mut frontier: Vec<SpatialId>) {
        let index = self.request.index;
        for hop in 1..=self.request.depth {
            if frontier.is_empty() || self.truncation.is_some() {
                break;
            }
            let class = if hop == 1 {
                ImpactClass::Dependent
            } else {
                ImpactClass::TransitiveRelated
            };
            let mut next = Vec::new();
            for from in &frontier {
                let Some(entry) = index.get(from) else {
                    continue;
                };
                for edge in entry.edges() {
                    if self.has_ended(edge) {
                        continue;
                    }
                    let Some(other) = edge.other_end(from).cloned() else {
                        continue;
                    };
                    if self.visited.contains(&other) {
                        continue;
                    }
                    let Some(other_entry) = index.get(&other) else {
                        continue;
                    };
                    if self.graph.nodes().len() >= self.request.node_budget {
                        self.truncation = Some(format!(
                            "the node budget of {} was reached at {hop} relation hops; the graph \
                             below this point was not walked",
                            self.request.node_budget
                        ));
                        break;
                    }
                    let label = edge
                        .label_from(from)
                        .unwrap_or_else(|| edge.relation().as_str());
                    let node = self
                        .node_of(other_entry, class, hop)
                        .reached_by(label)
                        // §3.5 and v0.4 §11.5: the edge's own confidence, spelled the way v0.4
                        // spells it. There is no operation here that raises one.
                        .with_confidence(edge.confidence().as_str())
                        .citing(edge.edge_id().to_string())
                        .citing(format!("provider:{}", edge.provenance().provider()));
                    self.graph.add(node);
                    self.record_boundaries(&other);
                    self.visited.insert(other.clone());
                    next.push(other);
                }
                if self.truncation.is_some() {
                    break;
                }
            }
            frontier = next;
        }
        if !frontier.is_empty() && self.truncation.is_none() {
            self.truncation = Some(format!(
                "the walk stopped at the depth budget of {} relation hops with {} objects still \
                 to follow",
                self.request.depth,
                frontier.len()
            ));
        }
    }

    /// §3.5's validity window, read the way v0.4 reads it: a relationship the provider says
    /// closed before `now` is not one this plan travels along. An absent end is not "now", so an
    /// edge that has never been seen to close stays.
    fn has_ended(&self, edge: &RelationshipEdge) -> bool {
        edge.validity()
            .is_some_and(|window| window.has_closed_by(self.request.now))
    }

    /// The node an index entry becomes, with the ledger's citation where §9.4 offers one.
    fn node_of(&mut self, entry: &IndexEntry, class: ImpactClass, depth: usize) -> ImpactNode {
        let id = entry.object().spatial_id().clone();
        let mut node = ImpactNode::new(
            id.as_str(),
            entry.object().display_name(),
            entry.object().object_type().as_str(),
            class,
            depth,
        )
        // §46.7: the canonical object ref of v0.4 travels with the node, so a reader can get
        // from an impact row back to the object the provider owns.
        .citing(format!("ref:{}", entry.canonical_ref().id()))
        .on_host(entry.object().scope().host_scope().id());
        if let Some(relevance) = self.request.history.and_then(|history| history(&id)) {
            node = node.citing(relevance.citation());
            self.history.insert(Arc::from(id.as_str()), relevance);
        }
        node
    }

    /// §9.6: every declared exit of an object that Ono could not read is a boundary.
    ///
    /// Two shapes qualify, and §9.6 treats them alike because the operator's question is the
    /// same: an exit the provider withheld ("permission denied for 14 process FDs" of v0.4
    /// §35.2), and one whose relation is acquired at [`AcquisitionCost::External`] — another
    /// host, or a privilege navigation never requests (v0.4 §34.2, §35.3).
    fn record_boundaries(&mut self, id: &SpatialId) {
        let index = self.request.index;
        let Some(entry) = index.get(id) else {
            return;
        };
        let label = entry.object().display_name().to_owned();
        let object_type = entry.object().object_type();
        let withheld: BTreeMap<&str, (PermissionState, &str)> = index
            .withheld(id)
            .into_iter()
            .map(|(exit, state, detail)| (exit, (state, detail)))
            .collect();
        for (exit, spec) in relation::exits_from(object_type) {
            if let Some((state, detail)) = withheld.get(exit) {
                if matches!(state, PermissionState::Available | PermissionState::Empty) {
                    continue;
                }
                let detail = if detail.is_empty() {
                    String::new()
                } else {
                    format!(" — {detail}")
                };
                self.graph.add_boundary(UnknownBoundary::new(
                    label.clone(),
                    format!("its `{exit}`"),
                    format!(
                        "the `{exit}` exit answered `{}`{detail}, so what lies beyond it is \
                         unknown rather than empty (v0.4 §35.2)",
                        state.as_str()
                    ),
                ));
                continue;
            }
            if spec.cost_class.acquisition() == AcquisitionCost::External {
                self.graph.add_boundary(UnknownBoundary::new(
                    label.clone(),
                    format!("its `{exit}`"),
                    format!(
                        "`{}` is acquired at `external` cost — another host, or a privilege this \
                         shell does not request — so impact stops here (v0.4 §34.2)",
                        spec.id
                    ),
                ));
            }
        }
    }

    /// §9.6's own example: an effect that leaves the machine ends the graph at a boundary.
    ///
    /// ```text
    /// nginx -> outbound HTTPS request -> external API
    ///                                   ? beyond Ono visibility
    /// ```
    fn external_boundaries(&mut self) {
        for (action, effect) in self.request.effects() {
            if !effect.domain().is_external() {
                continue;
            }
            let at = action
                .target()
                .or_else(|| effect.object())
                .unwrap_or_else(|| action.summary());
            let beyond = effect.object().unwrap_or_else(|| effect.explanation());
            self.graph.add_boundary(UnknownBoundary::new(
                at,
                beyond,
                format!(
                    "{} — beyond Ono visibility (§9.6), and §35.2 keeps it separately visible \
                     because no local snapshot reaches it",
                    effect.explanation()
                ),
            ));
        }
    }

    /// §6.3 and §8.1: an effect Ono cannot classify is where the graph stops being an answer.
    ///
    /// An effect in the `unknown` domain, or one whose confidence is `unknown`, says that the
    /// action reaches *something* Ono has no model of. §2.4 forbids promoting that to "nothing
    /// else", so the effect leaves a boundary behind and the graph stops claiming to be
    /// complete. No node is added for what lies beyond: the effect does not name it, and
    /// inventing one is the future §1.3 forbids. An external effect already ended the graph in
    /// [`Walk::external_boundaries`], and a second boundary for it would be one place counted
    /// twice.
    fn unknown_boundaries(&mut self) {
        for (action, effect) in self.request.effects() {
            if effect.domain() != EffectDomain::Unknown
                && effect.confidence() != EffectConfidence::Unknown
            {
                continue;
            }
            if !effect.domain().is_external() {
                let at = action
                    .target()
                    .or_else(|| effect.object())
                    .unwrap_or_else(|| action.summary());
                let beyond = effect.object().unwrap_or_else(|| action.summary());
                self.graph.add_boundary(UnknownBoundary::new(
                    at,
                    beyond,
                    format!(
                        "{} — Ono has no model of what this reaches, so its impact is unknown \
                         rather than empty (§6.3, §8.1, §9.6)",
                        effect.explanation()
                    ),
                ));
            }
            if self.truncation.is_none() {
                self.truncation = Some(format!(
                    "{} has effects Ono cannot classify, so what it touches is not in this graph \
                     (§6.3, §8.1)",
                    action.summary()
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_treat_a_single_ledger_sighting_as_ordinary() {
        let once = HistoricalRelevance::new(1, "restart service", "seen once");
        assert_eq!(
            once.prominence(),
            Prominence::Ordinary,
            "§9.4 speaks of relationships the ledger has repeatedly observed, and once is not \
             repeatedly"
        );
    }

    #[test]
    fn should_raise_a_relationship_the_ledger_has_seen_repeatedly() {
        let often = HistoricalRelevance::new(7, "restart service", "seen seven times");
        assert_eq!(
            often.prominence(),
            Prominence::Raised,
            "§9.4 permits evidence to strengthen impact relevance"
        );
        assert!(
            often.citation().contains('7'),
            "the citation carries the count, because §9.4 shows the history as evidence"
        );
    }

    #[test]
    fn should_default_to_ordinary_prominence_without_a_ledger() {
        let derivation = ImpactDerivation::default();
        assert_eq!(
            derivation.prominence("anything"),
            Prominence::Ordinary,
            "a node no ledger spoke about is shown where its §9.2 class puts it"
        );
        assert!(derivation.raised().is_empty());
    }

    #[test]
    fn should_spell_prominence_for_a_record() {
        assert_eq!(Prominence::Ordinary.as_str(), "ordinary");
        assert_eq!(Prominence::Raised.as_str(), "raised");
    }
}
