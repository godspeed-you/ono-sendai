//! The impact graph (spec v0.6 §9), and the boundary where Ono stops knowing.
//!
//! §9.1 is careful about what impact is: *"What known parts of the system could this plan touch
//! directly or indirectly?"* — and explicitly not failure prediction. So an [`ImpactNode`] carries
//! a class from §9.2 and the provenance of the relation that reached it, and nothing here says
//! anything will break.
//!
//! §9.6 is the part that has to be built rather than documented. A graph that simply stops has
//! told the operator nothing about whether it stopped because there was nothing more or because
//! Ono could not see further. [`UnknownBoundary`] is that difference made explicit, and
//! [`ImpactGraph::blast_radius`] counts boundaries beside objects so §9.5's summary cannot hide
//! them.

use std::sync::Arc;

use crate::vocab::vocabulary;

vocabulary! {
    /// How an object came to be in the impact graph (§9.2).
    ImpactClass {
        DirectTarget => "direct-target", "§9.2: the plan names this object.";
        DirectEffect => "direct-effect", "§9.2: an action's declared effect lands on this object.";
        Dependent => "dependent", "§9.2: a v0.4 relation says this object depends on something the plan touches.";
        TransitiveRelated => "transitive-related", "§9.2: reached through more than one relation.";
        ExternalSideEffect => "external-side-effect", "§9.2 and §35.1: an effect that leaves the machine.";
        UnknownBoundary => "unknown-boundary", "§9.2 and §9.6: the graph ends here, and Ono cannot see past it.";
    }
}

impl ImpactClass {
    /// How far from the plan's targets this class sits, for ordering a rendered graph.
    #[must_use]
    pub const fn distance(self) -> u8 {
        match self {
            ImpactClass::DirectTarget => 0,
            ImpactClass::DirectEffect => 1,
            ImpactClass::Dependent => 2,
            ImpactClass::TransitiveRelated => 3,
            ImpactClass::ExternalSideEffect | ImpactClass::UnknownBoundary => 4,
        }
    }
}

/// One object the plan could touch, and how Ono knows about it (§9.2, §3.5).
#[derive(Debug, Clone, PartialEq)]
pub struct ImpactNode {
    id: Arc<str>,
    label: Arc<str>,
    object_type: Arc<str>,
    class: ImpactClass,
    depth: usize,
    reached_by: Option<Arc<str>>,
    evidence: Vec<Arc<str>>,
    confidence: Arc<str>,
    host: Option<Arc<str>>,
}

impl ImpactNode {
    /// Records that `id` is in the graph as `class`, at `depth` hops from a target.
    #[must_use]
    pub fn new(
        id: impl Into<Arc<str>>,
        label: impl Into<Arc<str>>,
        object_type: impl Into<Arc<str>>,
        class: ImpactClass,
        depth: usize,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            object_type: object_type.into(),
            class,
            depth,
            reached_by: None,
            evidence: Vec::new(),
            confidence: Arc::from("unknown"),
            host: None,
        }
    }

    /// Names the v0.4 relation that reached this node (§9.3).
    #[must_use]
    pub fn reached_by(mut self, relation: impl Into<Arc<str>>) -> Self {
        self.reached_by = Some(relation.into());
        self
    }

    /// Records the confidence the v0.4 edge carried, unchanged (§3.5's provenance rule).
    #[must_use]
    pub fn with_confidence(mut self, confidence: impl Into<Arc<str>>) -> Self {
        self.confidence = confidence.into();
        self
    }

    /// Cites the edge or contract that put this node in the graph (§3.5).
    #[must_use]
    pub fn citing(mut self, evidence: impl Into<Arc<str>>) -> Self {
        self.evidence.push(evidence.into());
        self
    }

    /// Records the host the object lives on (§29.1).
    #[must_use]
    pub fn on_host(mut self, host: impl Into<Arc<str>>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// The object's identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The label a person reads.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// What kind of object it is.
    #[must_use]
    pub fn object_type(&self) -> &str {
        &self.object_type
    }

    /// How it came to be in the graph.
    #[must_use]
    pub const fn class(&self) -> ImpactClass {
        self.class
    }

    /// How many relations away from a plan target it sits.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// The relation that reached it.
    #[must_use]
    pub fn relation(&self) -> Option<&str> {
        self.reached_by.as_deref()
    }

    /// The confidence of the edge that reached it, as v0.4 stated it.
    #[must_use]
    pub fn confidence(&self) -> &str {
        &self.confidence
    }

    /// The evidence cited.
    #[must_use]
    pub fn evidence(&self) -> &[Arc<str>] {
        &self.evidence
    }

    /// The host it lives on.
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }
}

/// A place where the graph stops and Ono cannot see further (§9.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownBoundary {
    at: Arc<str>,
    beyond: Arc<str>,
    reason: Arc<str>,
}

impl UnknownBoundary {
    /// Records that beyond `at` lies `beyond`, and why Ono cannot follow.
    #[must_use]
    pub fn new(
        at: impl Into<Arc<str>>,
        beyond: impl Into<Arc<str>>,
        reason: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            at: at.into(),
            beyond: beyond.into(),
            reason: reason.into(),
        }
    }

    /// The last object Ono can see.
    #[must_use]
    pub fn at(&self) -> &str {
        &self.at
    }

    /// What lies past it, as far as anything can say.
    #[must_use]
    pub fn beyond(&self) -> &str {
        &self.beyond
    }

    /// Why the graph stops here.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// The blast radius summary of §9.5.
///
/// §9.5 permits counts and requires that object access be preserved, so this is derived from the
/// graph rather than stored beside it: a count that can disagree with its nodes is a count that
/// eventually will.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlastRadius {
    /// Objects the plan names (§9.2).
    pub direct_targets: usize,
    /// Objects an action's declared effects land on.
    pub direct_effects: usize,
    /// Objects one relation away.
    pub dependents: usize,
    /// Objects more than one relation away.
    pub transitive: usize,
    /// Effects that leave the machine (§35.1).
    pub external: usize,
    /// Places the graph stops (§9.6).
    pub boundaries: usize,
    /// Hosts the graph spans (§29.1).
    pub hosts: usize,
}

/// The objects and boundaries one plan could touch (§3.5, §9).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImpactGraph {
    nodes: Vec<ImpactNode>,
    boundaries: Vec<UnknownBoundary>,
    truncated: Option<Arc<str>>,
}

impl ImpactGraph {
    /// An empty graph.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            boundaries: Vec::new(),
            truncated: None,
        }
    }

    /// Adds a node, keeping the shallowest reading of an object seen twice.
    ///
    /// A service reachable both as a direct target and as a transitive relation is a direct
    /// target: §9.2's classes are about how close the plan comes to an object, and the closest
    /// reading is the true one.
    pub fn add(&mut self, node: ImpactNode) {
        if let Some(existing) = self
            .nodes
            .iter_mut()
            .find(|candidate| candidate.id() == node.id())
        {
            if node.class().distance() < existing.class().distance() {
                *existing = node;
            }
            return;
        }
        self.nodes.push(node);
    }

    /// Adds a boundary (§9.6).
    pub fn add_boundary(&mut self, boundary: UnknownBoundary) {
        if !self.boundaries.contains(&boundary) {
            self.boundaries.push(boundary);
        }
    }

    /// Records why the graph is not the whole answer: traversal stopped at a budget, or an effect
    /// reaches something Ono has no model of (§6.3, §8.1).
    ///
    /// §9.5 lets `impact` summarise, and §52.2 bounds how long it may take. A graph cut short by
    /// a budget, or ended by an effect nobody can follow, is not a graph that ended at the edge of
    /// the world, and saying which is the difference between a summary and a lie.
    #[must_use]
    pub fn truncated(mut self, reason: impl Into<Arc<str>>) -> Self {
        self.truncated = Some(reason.into());
        self
    }

    /// Every node, closest first.
    #[must_use]
    pub fn nodes(&self) -> &[ImpactNode] {
        &self.nodes
    }

    /// The nodes of one class.
    #[must_use]
    pub fn of_class(&self, class: ImpactClass) -> Vec<&ImpactNode> {
        self.nodes
            .iter()
            .filter(|node| node.class() == class)
            .collect()
    }

    /// Every boundary (§9.6).
    #[must_use]
    pub fn boundaries(&self) -> &[UnknownBoundary] {
        &self.boundaries
    }

    /// Why the graph is incomplete, where it is.
    #[must_use]
    pub fn truncation(&self) -> Option<&str> {
        self.truncated.as_deref()
    }

    /// Whether the graph is complete as far as Ono's evidence reaches.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.truncated.is_none()
    }

    /// The §9.5 summary, derived from the nodes.
    #[must_use]
    pub fn blast_radius(&self) -> BlastRadius {
        let mut hosts: Vec<&str> = self.nodes.iter().filter_map(ImpactNode::host).collect();
        hosts.sort_unstable();
        hosts.dedup();
        BlastRadius {
            direct_targets: self.of_class(ImpactClass::DirectTarget).len(),
            direct_effects: self.of_class(ImpactClass::DirectEffect).len(),
            dependents: self.of_class(ImpactClass::Dependent).len(),
            transitive: self.of_class(ImpactClass::TransitiveRelated).len(),
            external: self.of_class(ImpactClass::ExternalSideEffect).len(),
            boundaries: self.boundaries.len() + self.of_class(ImpactClass::UnknownBoundary).len(),
            hosts: hosts.len(),
        }
    }

    /// Sorts the nodes closest first, then by label, for a stable rendering.
    pub fn sort(&mut self) {
        self.nodes.sort_by(|left, right| {
            left.class()
                .distance()
                .cmp(&right.class().distance())
                .then_with(|| left.depth().cmp(&right.depth()))
                .then_with(|| left.label().cmp(right.label()))
                .then_with(|| left.id().cmp(right.id()))
        });
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

    fn node(id: &str, class: ImpactClass, depth: usize) -> ImpactNode {
        ImpactNode::new(id, id, "ono.process/1", class, depth)
    }

    #[test]
    fn should_count_the_blast_radius_out_of_the_nodes_it_holds() {
        let mut graph = ImpactGraph::empty();
        graph.add(node("nginx.conf", ImpactClass::DirectTarget, 0));
        graph.add(node("nginx.service", ImpactClass::DirectTarget, 0));
        graph.add(node("worker/1", ImpactClass::Dependent, 1));
        graph.add(node("worker/2", ImpactClass::Dependent, 1));
        graph.add(node(":443", ImpactClass::TransitiveRelated, 2));
        graph.add_boundary(UnknownBoundary::new(
            "nginx",
            "an external API over outbound HTTPS",
            "the call leaves this machine",
        ));
        let radius = graph.blast_radius();
        assert_eq!(radius.direct_targets, 2, "§9.5 counts what §9.2 classified");
        assert_eq!(radius.dependents, 2);
        assert_eq!(radius.transitive, 1);
        assert_eq!(
            radius.boundaries, 1,
            "§9.6: a boundary is counted, not swallowed by the summary"
        );
    }

    #[test]
    fn should_keep_the_closest_reading_of_an_object_seen_twice() {
        let mut graph = ImpactGraph::empty();
        graph.add(node("nginx.service", ImpactClass::TransitiveRelated, 3));
        graph.add(node("nginx.service", ImpactClass::DirectTarget, 0));
        assert_eq!(graph.nodes().len(), 1, "one object is one node");
        assert_eq!(
            graph.nodes()[0].class(),
            ImpactClass::DirectTarget,
            "§9.2's classes describe how close the plan comes, so the closest reading wins"
        );
    }

    #[test]
    fn should_not_downgrade_a_direct_target_to_a_transitive_relation() {
        let mut graph = ImpactGraph::empty();
        graph.add(node("nginx.service", ImpactClass::DirectTarget, 0));
        graph.add(node("nginx.service", ImpactClass::TransitiveRelated, 3));
        assert_eq!(graph.nodes()[0].class(), ImpactClass::DirectTarget);
    }

    #[test]
    fn should_say_when_the_graph_stopped_at_a_budget_rather_than_at_the_world() {
        let complete = ImpactGraph::empty();
        assert!(complete.is_complete());
        let bounded = ImpactGraph::empty().truncated("the interactive budget was reached");
        assert!(
            !bounded.is_complete(),
            "§9.5's summary must not read as a whole graph when it is a bounded one"
        );
        assert_eq!(
            bounded.truncation(),
            Some("the interactive budget was reached")
        );
    }

    #[test]
    fn should_keep_the_evidence_that_put_a_node_in_the_graph() {
        let node = node("worker/1", ImpactClass::Dependent, 1)
            .reached_by("service.controls_process")
            .with_confidence("exact")
            .citing("edge:1234");
        assert_eq!(node.relation(), Some("service.controls_process"));
        assert_eq!(
            node.confidence(),
            "exact",
            "§3.5: impact MUST retain provenance, and v0.4's confidence travels unchanged"
        );
        assert_eq!(node.evidence(), &[Arc::from("edge:1234")]);
    }

    #[test]
    fn should_count_the_hosts_a_remote_plan_spans() {
        let mut graph = ImpactGraph::empty();
        graph.add(node("a", ImpactClass::DirectTarget, 0).on_host("api-01"));
        graph.add(node("b", ImpactClass::DirectTarget, 0).on_host("api-02"));
        graph.add(node("c", ImpactClass::DirectTarget, 0).on_host("api-01"));
        assert_eq!(
            graph.blast_radius().hosts,
            2,
            "§29.1: a remote plan's per-host truth begins with knowing how many there are"
        );
    }

    #[test]
    fn should_order_the_graph_closest_first() {
        let mut graph = ImpactGraph::empty();
        graph.add(node("far", ImpactClass::TransitiveRelated, 3));
        graph.add(node("near", ImpactClass::DirectTarget, 0));
        graph.add(node("mid", ImpactClass::Dependent, 1));
        graph.sort();
        let order: Vec<&str> = graph.nodes().iter().map(ImpactNode::id).collect();
        assert_eq!(order, vec!["near", "mid", "far"]);
    }
}
