//! `trace`: the bounded walk of spec §22.3.

use std::collections::VecDeque;
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_provider_api::{ProviderRegistry, Query};
use ono_value::{ErrorValue, Value};

use crate::graph::{Graph, GraphFailure, Node};
use crate::provider::RelationshipProvider;

/// How many hops a trace follows when the caller does not say.
///
/// Three is one more than the deepest drawing in spec §22.4 — a unit, its process, that
/// process's sockets and files — so the default answers the question the specification uses to
/// motivate the feature, and stops.
pub const DEFAULT_DEPTH: usize = 3;

/// How many objects a trace collects when the caller does not say.
///
/// A process tree with a few hundred nodes is still something a person can read; the point of
/// the limit is that a trace of `pid 1` on a busy machine ends, and says that it was cut short.
pub const DEFAULT_MAX_NODES: usize = 256;

/// The bounds of one trace.
#[derive(Debug, Clone)]
pub struct TraceOptions {
    depth: usize,
    max_nodes: usize,
    relations: Option<Vec<String>>,
}

impl Default for TraceOptions {
    fn default() -> Self {
        Self {
            depth: DEFAULT_DEPTH,
            max_nodes: DEFAULT_MAX_NODES,
            relations: None,
        }
    }
}

impl TraceOptions {
    /// The defaults: [`DEFAULT_DEPTH`] hops, [`DEFAULT_MAX_NODES`] objects, every relation.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Follows at most `depth` hops from the roots.
    #[must_use]
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// Collects at most `max_nodes` objects. A limit of zero would produce a graph with no root,
    /// so it is raised to one.
    #[must_use]
    pub fn max_nodes(mut self, max_nodes: usize) -> Self {
        self.max_nodes = max_nodes.max(1);
        self
    }

    /// Follows only these relations, as `trace process --relations` asks.
    #[must_use]
    pub fn only_relations<I, S>(mut self, relations: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.relations = Some(relations.into_iter().map(Into::into).collect());
        self
    }

    /// How many hops the walk follows.
    #[must_use]
    pub fn max_depth(&self) -> usize {
        self.depth
    }

    /// How many objects the walk collects.
    #[must_use]
    pub fn node_limit(&self) -> usize {
        self.max_nodes
    }

    /// The relations the walk follows, when it is restricted to some.
    #[must_use]
    pub fn relations(&self) -> Option<&[String]> {
        self.relations.as_deref()
    }

    /// The bounds a `trace` command line asks for: the `depth` and `relations` options of
    /// `docs/spec/commands/*.yaml`. An option that is absent or of the wrong shape leaves the
    /// default in place rather than failing, because the command layer has already validated it.
    #[must_use]
    pub fn from_query(query: &Query) -> Self {
        let mut options = Self::new();
        if let Some(Value::Int(depth)) = query.option_value("depth")
            && let Ok(depth) = usize::try_from(*depth)
        {
            options = options.depth(depth);
        }
        if let Some(Value::List(relations)) = query.option_value("relations") {
            let names: Vec<String> = relations
                .iter()
                .filter_map(|relation| relation.as_str().ok().map(str::to_owned))
                .collect();
            if !names.is_empty() {
                options = options.only_relations(names);
            }
        }
        options
    }

    fn wants(&self, relation: &str) -> bool {
        self.relations
            .as_ref()
            .is_none_or(|wanted| wanted.iter().any(|name| name == relation))
    }
}

/// Walks relationship providers from one or more objects and returns what it found.
///
/// ```
/// use ono_graph::{TraceOptions, Tracer};
/// let tracer = Tracer::new().with_options(TraceOptions::new().depth(2));
/// assert_eq!(tracer.options().max_depth(), 2);
/// ```
#[derive(Debug, Default, Clone)]
pub struct Tracer {
    providers: Vec<Arc<dyn RelationshipProvider>>,
    options: TraceOptions,
}

impl Tracer {
    /// A tracer with no providers and the default bounds.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a relationship provider. Providers are consulted in the order they were added, which
    /// is what makes the shape of a trace reproducible.
    #[must_use]
    pub fn with(mut self, provider: Arc<dyn RelationshipProvider>) -> Self {
        self.providers.push(provider);
        self
    }

    /// Adds several providers.
    #[must_use]
    pub fn with_all(
        mut self,
        providers: impl IntoIterator<Item = Arc<dyn RelationshipProvider>>,
    ) -> Self {
        self.providers.extend(providers);
        self
    }

    /// Sets the bounds of the walk.
    #[must_use]
    pub fn with_options(mut self, options: TraceOptions) -> Self {
        self.options = options;
        self
    }

    /// The bounds this tracer walks with.
    #[must_use]
    pub fn options(&self) -> &TraceOptions {
        &self.options
    }

    /// The providers this tracer consults.
    #[must_use]
    pub fn providers(&self) -> &[Arc<dyn RelationshipProvider>] {
        &self.providers
    }

    /// Whether this provider is asked about this object at all.
    fn consults(&self, provider: &Arc<dyn RelationshipProvider>, subject: &Node) -> bool {
        answers_about(provider.as_ref(), subject) && offers_wanted(provider, self)
    }

    /// Whether any provider would have been asked about this object.
    fn can_expand(&self, subject: &Node) -> bool {
        self.providers
            .iter()
            .any(|provider| self.consults(provider, subject))
    }

    /// Walks from `roots` and returns the graph.
    ///
    /// The walk is breadth-first, adds every object at most once and never expands an object
    /// twice, so it terminates on a cycle. It stops at the depth and node limits of
    /// [`TraceOptions`] and records in [`Graph::truncation`] that it did.
    pub async fn trace(&self, roots: impl IntoIterator<Item = Node>) -> Graph {
        let mut graph = Graph::new();
        let mut queue: VecDeque<(Node, usize)> = VecDeque::new();
        for root in roots {
            if graph.insert_node(root.clone()) {
                queue.push_back((root, 0));
            }
        }

        while let Some((subject, depth)) = queue.pop_front() {
            if depth >= self.options.depth {
                // Only a subject somebody could have answered about was truncated. An object no
                // registered provider expands has no relationships left to find, and saying a
                // complete answer was cut short would make the warning worthless.
                if self.can_expand(&subject) {
                    graph
                        .truncation_mut()
                        .record_depth_limit(self.options.depth);
                }
                continue;
            }
            for provider in &self.providers {
                if !self.consults(provider, &subject) {
                    continue;
                }
                let (found, failures) = provider.relationships(&subject).await.into_parts();
                for failure in failures {
                    graph.insert_failure(GraphFailure::new(subject.id().clone(), failure));
                }
                for relationship in found {
                    let (edge, target) = relationship.into_parts();
                    if !self.options.wants(edge.relation()) {
                        continue;
                    }
                    if graph.node(target.id()).is_none() {
                        if graph.nodes().len() >= self.options.max_nodes {
                            graph
                                .truncation_mut()
                                .record_node_limit(self.options.max_nodes);
                            continue;
                        }
                        graph.insert_node(target.clone());
                        queue.push_back((target, depth + 1));
                    }
                    graph.insert_edge(edge);
                }
            }
        }
        graph
    }
}

/// Whether the provider answers about this kind of object.
fn answers_about(provider: &dyn RelationshipProvider, subject: &Node) -> bool {
    let kind = subject.kind().to_string();
    provider.subjects().contains(&kind.as_str())
}

/// Whether the provider offers any of the relations the caller asked for.
fn offers_wanted(provider: &Arc<dyn RelationshipProvider>, tracer: &Tracer) -> bool {
    let Some(wanted) = tracer.options.relations() else {
        return true;
    };
    // A provider that declares no relations is not saying it has none — a plugin may contribute
    // whatever it likes — so it is still consulted and its edges are filtered afterwards.
    provider.relations().is_empty()
        || provider
            .relations()
            .iter()
            .any(|relation| wanted.iter().any(|name| name == relation))
}

/// The objects a query resolves to, as the roots of a trace.
///
/// # Errors
///
/// Returns whatever the provider reports, or `resolve.target_not_found` when nothing matched —
/// which is not the same answer as a graph with no relationships, and must not look like one.
pub async fn roots(registry: &ProviderRegistry, query: &Query) -> Result<Vec<Node>, ErrorValue> {
    let collected = registry.snapshot(query)?.collect().await;
    let nodes: Vec<Node> = collected
        .values()
        .iter()
        .filter_map(|value| value.as_record().ok())
        .filter(|record| query.matches(record))
        .filter_map(Node::of)
        .collect();
    if nodes.is_empty() {
        if let Some(error) = collected.errors().first() {
            return Err(error.clone());
        }
        return Err(ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            format!("nothing to trace: no `{}` matched", query.target_name()),
        )
        .with_help("`trace` starts from an object that exists; `get` shows which ones do"));
    }
    Ok(nodes)
}
