//! The graph value of spec §22.1: nodes keyed by object identity, edges that carry their own
//! confidence.

use std::collections::HashMap;

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_provider_api::ObjectId;
use ono_render::Confidence;
use ono_value::{
    ErrorValue, MapValue, Provenance, RecordValue, Schema, SchemaId, Value, builtin_schemas,
};

use crate::label::label_of;

/// The provider id a graph assembled by `trace` records in its provenance.
pub const TRACE_PROVIDER: &str = "ono.trace";

/// One object in a relationship graph (spec §22.1's `Node`).
///
/// A node is keyed by [`ObjectId`], so the same process observed down two different paths is one
/// node rather than two — which is the property that makes a graph a graph instead of a tree
/// drawn twice.
#[derive(Debug, Clone)]
pub struct Node {
    id: ObjectId,
    kind: SchemaId,
    identity: MapValue,
    label: String,
    summary: MapValue,
    provenance: Provenance,
    record: Option<std::sync::Arc<RecordValue>>,
}

impl Node {
    /// A node for an object of `kind`, identified by `identity`.
    ///
    /// `label` is what a renderer shows for it. Building a node by hand is for objects a
    /// provider knows without holding a record of them — the endpoint behind a resolved address,
    /// say; a node for a record comes from [`Node::of`] instead.
    #[must_use]
    pub fn new(kind: SchemaId, identity: MapValue, label: impl Into<String>) -> Self {
        let id = ObjectId::new(kind.clone(), identity.values().cloned());
        let provenance = Provenance::local(TRACE_PROVIDER, kind.clone());
        Self {
            id,
            kind,
            identity,
            label: label.into(),
            summary: MapValue::new(),
            provenance,
            record: None,
        }
    }

    /// The node standing for `record`, or `None` when the record's schema declares no identity.
    ///
    /// A schema without identity describes a value rather than an object (spec §27.3), and a
    /// graph of values could not tell two of them apart.
    #[must_use]
    pub fn of(record: &RecordValue) -> Option<Self> {
        let id = ObjectId::of(record)?;
        let schema = record.schema();
        let mut summary = MapValue::new();
        for column in schema.identity().iter().chain(schema.default_view().iter()) {
            if summary.contains_key(column) {
                continue;
            }
            summary.insert(
                column.clone(),
                record.get(column).cloned().unwrap_or(Value::Null),
            );
        }
        Some(Self {
            id,
            kind: record.schema_id().clone(),
            identity: record.identity(),
            label: label_of(record),
            summary,
            provenance: record.provenance().clone(),
            record: Some(std::sync::Arc::new(record.clone())),
        })
    }

    /// Replaces the summary a renderer shows for the object.
    #[must_use]
    pub fn with_summary(mut self, summary: MapValue) -> Self {
        self.summary = summary;
        self
    }

    /// Records where the object was observed (spec §25.2).
    #[must_use]
    pub fn with_provenance(mut self, provenance: Provenance) -> Self {
        self.provenance = provenance;
        self
    }

    /// The object's identity, which is what makes two observations one node.
    #[must_use]
    pub fn id(&self) -> &ObjectId {
        &self.id
    }

    /// The schema of the object the node stands for.
    #[must_use]
    pub fn kind(&self) -> &SchemaId {
        &self.kind
    }

    /// The identity fields and their values.
    #[must_use]
    pub fn identity(&self) -> &MapValue {
        &self.identity
    }

    /// The short text a renderer shows.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Enough of the object's fields to render a row without a second query.
    #[must_use]
    pub fn summary(&self) -> &MapValue {
        &self.summary
    }

    /// Where the object was observed.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The record the node was built from, for a node that stands for one.
    ///
    /// A relationship provider reads the fields it needs from here — a process's parent id, a
    /// unit's main pid — rather than querying the object's own provider a second time.
    #[must_use]
    pub fn record(&self) -> Option<&std::sync::Arc<RecordValue>> {
        self.record.as_ref()
    }

    /// One of the object's fields: from the record it was built from, or from its summary.
    ///
    /// An unknown value and a failed read both answer `None` here, because a caller reading a
    /// pid to follow can do nothing with either; the distinction spec §10.5 draws is kept in the
    /// record itself, which [`Node::record`] hands back untouched.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&Value> {
        let value = self
            .record
            .as_ref()
            .and_then(|record| record.get(name))
            .or_else(|| self.summary.get(name))?;
        match value {
            Value::Null | Value::Error(_) => None,
            known => Some(known),
        }
    }

    /// One of the object's fields as a whole number.
    #[must_use]
    pub fn integer(&self, name: &str) -> Option<i64> {
        match self.field(name)? {
            Value::Int(number) => i64::try_from(*number).ok(),
            Value::Port(port) => Some(i64::from(*port)),
            _ => None,
        }
    }

    /// One of the object's fields as its canonical text.
    #[must_use]
    pub fn text(&self, name: &str) -> Option<String> {
        ono_value::canonical_text(self.field(name)?).ok()
    }

    /// The reference an edge uses to name this node: its schema, its identity and its label.
    fn reference(&self) -> Value {
        let mut reference = MapValue::new();
        reference.insert("schema".into(), Value::String(self.kind.to_string().into()));
        reference.insert("identity".into(), Value::Map(self.identity.clone().into()));
        reference.insert("label".into(), Value::String(self.label.as_str().into()));
        Value::Map(reference.into())
    }

    /// The node as a record of `ono.graph-node/1`.
    fn to_record(&self) -> Result<RecordValue, ErrorValue> {
        Ok(
            RecordValue::builder(schema("ono.graph-node")?, self.provenance.clone())
                .set("id", self.reference())?
                .set("kind", Value::String(self.kind.to_string().into()))?
                .set("value", Value::Map(self.summary.clone().into()))?
                .build(),
        )
    }
}

/// Whether a relationship runs one way or holds symmetrically (spec §22.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    /// The relationship runs from `from` to `to`.
    #[default]
    Directed,
    /// The relationship holds between the two objects, in neither direction in particular.
    Undirected,
}

impl Direction {
    /// The name `docs/spec/schemas/graph-edge.v1.yaml` gives the direction.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Direction::Directed => "directed",
            Direction::Undirected => "undirected",
        }
    }
}

/// One relationship between two objects (spec §22.1's `Edge`).
///
/// An edge is built either as an observation or as an inference and can never become the other:
/// there is no way to raise [`Confidence::Inferred`] to [`Confidence::Exact`] after the fact,
/// because spec §22.2 forbids presenting a derivation as something the provider saw. An
/// inference must name what it was inferred from when it is built, which is spec §31.25's
/// requirement that evidence stay inspectable.
#[derive(Debug, Clone)]
pub struct Edge {
    from: ObjectId,
    to: ObjectId,
    relation: String,
    direction: Direction,
    confidence: Confidence,
    provider: String,
    metadata: MapValue,
}

impl Edge {
    /// A relationship the provider observed.
    #[must_use]
    pub fn exact(
        from: ObjectId,
        to: ObjectId,
        relation: impl Into<String>,
        provider: impl Into<String>,
    ) -> Self {
        Self {
            from,
            to,
            relation: relation.into(),
            direction: Direction::Directed,
            confidence: Confidence::Exact,
            provider: provider.into(),
            metadata: MapValue::new(),
        }
    }

    /// A relationship the provider derived, together with the evidence it derived it from.
    #[must_use]
    pub fn inferred(
        from: ObjectId,
        to: ObjectId,
        relation: impl Into<String>,
        provider: impl Into<String>,
        evidence: impl Into<String>,
    ) -> Self {
        let mut edge = Self::exact(from, to, relation, provider);
        edge.confidence = Confidence::Inferred;
        edge.metadata.insert(
            "inferred_from".into(),
            Value::String(evidence.into().into()),
        );
        edge
    }

    /// Marks the relationship as holding in neither direction in particular.
    #[must_use]
    pub fn undirected(mut self) -> Self {
        self.direction = Direction::Undirected;
        self
    }

    /// Adds a detail of the relationship, such as the file descriptor it was read from.
    #[must_use]
    pub fn with_metadata(mut self, key: &str, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// The object the relationship starts at.
    #[must_use]
    pub fn from(&self) -> &ObjectId {
        &self.from
    }

    /// The object the relationship leads to.
    #[must_use]
    pub fn to(&self) -> &ObjectId {
        &self.to
    }

    /// The relationship's name, such as `owns` or `listens`.
    #[must_use]
    pub fn relation(&self) -> &str {
        &self.relation
    }

    /// Whether it runs one way.
    #[must_use]
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// Whether the provider observed the relationship or derived it (spec §22.2).
    #[must_use]
    pub fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// Which provider asserted it.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The relationship's own detail — a file descriptor, a socket inode, the evidence behind an
    /// inference.
    #[must_use]
    pub fn metadata(&self) -> &MapValue {
        &self.metadata
    }

    /// What makes two assertions of a relationship the same assertion. Confidence is part of it,
    /// so an inference is never absorbed into an observation.
    fn key(&self) -> (String, String, String, &'static str, &'static str) {
        (
            self.from.to_string(),
            self.to.to_string(),
            self.relation.clone(),
            confidence_name(self.confidence),
            self.direction.as_str(),
        )
    }

    fn to_record(&self, graph: &Graph) -> Result<RecordValue, ErrorValue> {
        let reference = |id: &ObjectId| {
            graph
                .node(id)
                .map_or_else(|| Value::String(id.to_string().into()), Node::reference)
        };
        Ok(RecordValue::builder(
            schema("ono.graph-edge")?,
            Provenance::local(&self.provider, SchemaId::new("ono.graph-edge", 1))
                .observed_at(Timestamp::now()),
        )
        .set("from", reference(&self.from))?
        .set("to", reference(&self.to))?
        .set("relation", Value::String(self.relation.as_str().into()))?
        .set("direction", Value::String(self.direction.as_str().into()))?
        .set(
            "confidence",
            Value::String(confidence_name(self.confidence).into()),
        )?
        .set("provider", Value::String(self.provider.as_str().into()))?
        .set("metadata", Value::Map(self.metadata.clone().into()))?
        .build())
    }
}

/// The name `docs/spec/schemas/graph-edge.v1.yaml` gives a confidence.
#[must_use]
pub const fn confidence_name(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Exact => "exact",
        Confidence::Inferred => "inferred",
    }
}

/// Something that could not be read while the graph was built, and the object it concerns.
///
/// A relationship this user may not see is not a relationship that does not exist, so it is
/// reported rather than dropped (spec §10.5, §16.5).
#[derive(Debug, Clone)]
pub struct GraphFailure {
    subject: ObjectId,
    error: ErrorValue,
}

impl GraphFailure {
    /// A failure about `subject`.
    #[must_use]
    pub fn new(subject: ObjectId, error: ErrorValue) -> Self {
        Self { subject, error }
    }

    /// The object whose relationships could not be read.
    #[must_use]
    pub fn subject(&self) -> &ObjectId {
        &self.subject
    }

    /// What went wrong.
    #[must_use]
    pub fn error(&self) -> &ErrorValue {
        &self.error
    }
}

/// Why a walk stopped before it ran out of relationships.
///
/// A bounded walk that said nothing would be indistinguishable from a complete one, which is the
/// conflation between absence and ignorance the value model exists to prevent (spec §10.5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Truncation {
    depth_limit: Option<usize>,
    node_limit: Option<usize>,
    unexpanded: usize,
}

impl Truncation {
    /// Whether anything was left out.
    #[must_use]
    pub fn is_truncated(&self) -> bool {
        self.depth_limit.is_some() || self.node_limit.is_some()
    }

    /// The depth limit that stopped the walk, when one did.
    #[must_use]
    pub fn depth_limit(&self) -> Option<usize> {
        self.depth_limit
    }

    /// The node limit that stopped the walk, when one did.
    #[must_use]
    pub fn node_limit(&self) -> Option<usize> {
        self.node_limit
    }

    /// How many objects were reached but not followed any further.
    #[must_use]
    pub fn unexpanded(&self) -> usize {
        self.unexpanded
    }

    /// What a user has to be told, or `None` when the walk was complete.
    #[must_use]
    pub fn message(&self) -> Option<String> {
        if !self.is_truncated() {
            return None;
        }
        let mut reasons = Vec::new();
        if let Some(depth) = self.depth_limit {
            reasons.push(format!("depth limit {depth}"));
        }
        if let Some(nodes) = self.node_limit {
            reasons.push(format!("node limit {nodes}"));
        }
        Some(format!(
            "truncated at {}: {} object(s) not expanded",
            reasons.join(" and "),
            self.unexpanded
        ))
    }

    /// The truncation as a map, for the graph value.
    fn to_map(&self) -> MapValue {
        let mut map = MapValue::new();
        map.insert(
            "depth_limit".into(),
            self.depth_limit
                .map_or(Value::Null, |limit| Value::Int(limit as i128)),
        );
        map.insert(
            "node_limit".into(),
            self.node_limit
                .map_or(Value::Null, |limit| Value::Int(limit as i128)),
        );
        map.insert("unexpanded".into(), Value::Int(self.unexpanded as i128));
        if let Some(message) = self.message() {
            map.insert("message".into(), Value::String(message.into()));
        }
        map
    }

    pub(crate) fn record_depth_limit(&mut self, limit: usize) {
        self.depth_limit = Some(limit);
        self.unexpanded += 1;
    }

    pub(crate) fn record_node_limit(&mut self, limit: usize) {
        self.node_limit = Some(limit);
        self.unexpanded += 1;
    }
}

/// A relationship graph over system objects (spec §22.1).
///
/// It is a value: `trace` produces one, a pipeline carries it, `to json` serializes it and the
/// tree renderer draws it. None of those steps may change what a provider claimed.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    root: Option<ObjectId>,
    nodes: Vec<Node>,
    positions: HashMap<ObjectId, usize>,
    edges: Vec<Edge>,
    edge_keys: Vec<(String, String, String, &'static str, &'static str)>,
    failures: Vec<GraphFailure>,
    truncation: Truncation,
}

impl Graph {
    /// An empty graph with no distinguished origin.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node, unless the graph already holds one for that object.
    ///
    /// Returns whether the node was new, which is what lets a walk enqueue an object exactly
    /// once and therefore terminate on a cycle.
    pub fn insert_node(&mut self, node: Node) -> bool {
        if self.positions.contains_key(node.id()) {
            return false;
        }
        self.positions.insert(node.id().clone(), self.nodes.len());
        if self.root.is_none() {
            self.root = Some(node.id().clone());
        }
        self.nodes.push(node);
        true
    }

    /// Adds an edge, unless the same provider already asserted the same relationship with the
    /// same confidence.
    pub fn insert_edge(&mut self, edge: Edge) {
        let key = edge.key();
        if self.edge_keys.contains(&key) {
            return;
        }
        self.edge_keys.push(key);
        self.edges.push(edge);
    }

    /// Records that something could not be read while `subject` was being expanded.
    pub fn insert_failure(&mut self, failure: GraphFailure) {
        self.failures.push(failure);
    }

    /// The object the trace started from, when it had one.
    #[must_use]
    pub fn root(&self) -> Option<&ObjectId> {
        self.root.as_ref()
    }

    /// Every object in the graph, in the order it was reached.
    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The node for one object.
    #[must_use]
    pub fn node(&self, id: &ObjectId) -> Option<&Node> {
        self.positions
            .get(id)
            .and_then(|position| self.nodes.get(*position))
    }

    /// Every relationship, in the order it was asserted.
    #[must_use]
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// The relationships leading away from one object.
    pub fn edges_from<'a>(&'a self, id: &'a ObjectId) -> impl Iterator<Item = &'a Edge> {
        self.edges.iter().filter(move |edge| edge.from() == id)
    }

    /// What could not be read while the graph was built.
    #[must_use]
    pub fn failures(&self) -> &[GraphFailure] {
        &self.failures
    }

    /// The failures concerning one object.
    pub fn failures_of<'a>(&'a self, id: &'a ObjectId) -> impl Iterator<Item = &'a GraphFailure> {
        self.failures
            .iter()
            .filter(move |failure| failure.subject() == id)
    }

    /// Why the walk stopped, if it stopped early.
    #[must_use]
    pub fn truncation(&self) -> &Truncation {
        &self.truncation
    }

    pub(crate) fn truncation_mut(&mut self) -> &mut Truncation {
        &mut self.truncation
    }

    /// The graph as a record of `ono.graph/1`, so it can travel a pipeline like any other value.
    ///
    /// # Errors
    ///
    /// Returns `provider.schema_violation` if the graph contracts are not loadable, which means
    /// the build embedded a malformed `docs/spec/schemas/` file.
    pub fn to_value(&self) -> Result<Value, ErrorValue> {
        let nodes: Vec<Value> = self
            .nodes
            .iter()
            .map(|node| node.to_record().map(RecordValue::into_value))
            .collect::<Result<_, _>>()?;
        let edges: Vec<Value> = self
            .edges
            .iter()
            .map(|edge| edge.to_record(self).map(RecordValue::into_value))
            .collect::<Result<_, _>>()?;
        let root = self
            .root
            .as_ref()
            .and_then(|id| self.node(id))
            .map_or(Value::Null, Node::reference);

        let mut builder = RecordValue::builder(
            schema("ono.graph")?,
            Provenance::local(TRACE_PROVIDER, SchemaId::new("ono.graph", 1))
                .observed_at(Timestamp::now()),
        )
        .set("root", root)?
        .set("nodes", Value::List(nodes.into()))?
        .set("edges", Value::List(edges.into()))?;

        // The contract of spec §22.1 has three fields and this crate does not get to add a
        // fourth. A walk that stopped early still has to say so, so it says so where spec §10.4
        // puts anything a schema does not declare: a namespaced extension.
        if self.truncation.is_truncated() {
            builder = builder.set_extra(
                "ono.graph.truncation",
                Value::Map(self.truncation.to_map().into()),
            );
        }
        if !self.failures.is_empty() {
            let failures: Vec<Value> = self
                .failures
                .iter()
                .map(|failure| failure.error().clone().into_value())
                .collect();
            builder = builder.set_extra("ono.graph.failures", Value::List(failures.into()));
        }
        Ok(builder.build().into_value())
    }
}

impl Graph {
    /// Parses a graph back out of an `ono.graph/1` record.
    ///
    /// This is [`Graph::to_value`]'s inverse, and it exists so a graph that travelled a pipeline
    /// — serialised, stored, piped — renders exactly as the live one does. What the record does
    /// not carry (a node's full source record, an edge's original metadata types after a lossy
    /// codec) stays absent; the trees only need what the record keeps.
    ///
    /// # Errors
    ///
    /// Returns `type.mismatch` when the record is not an `ono.graph/1` or a node or edge in it
    /// cannot be read.
    pub fn from_record(record: &RecordValue) -> Result<Self, ErrorValue> {
        if record.schema_id() != &SchemaId::new("ono.graph", 1) {
            return Err(ErrorValue::new(
                ono_core::ErrorCode::TypeMismatch,
                format!("{} is not an ono.graph/1 record", record.schema_id()),
            ));
        }

        let reference = |value: &Value| -> Result<(SchemaId, MapValue, String), ErrorValue> {
            let map = value.as_map().map_err(|_| bad_graph("a node reference"))?;
            let schema: SchemaId = map
                .get("schema")
                .and_then(|schema| schema.as_str().ok())
                .ok_or_else(|| bad_graph("a reference schema"))?
                .parse()?;
            let identity = map
                .get("identity")
                .and_then(|identity| identity.as_map().ok())
                .ok_or_else(|| bad_graph("a reference identity"))?
                .clone();
            let label = map
                .get("label")
                .and_then(|label| label.as_str().ok())
                .unwrap_or_default()
                .to_owned();
            Ok((schema, identity, label))
        };

        let mut graph = Graph::new();
        let entries = |name: &str| -> Vec<Value> {
            record
                .get(name)
                .and_then(|value| value.as_list().ok())
                .map(|list| list.to_vec())
                .unwrap_or_default()
        };

        for value in entries("nodes") {
            let node = value.as_record().map_err(|_| bad_graph("a node"))?;
            let (kind, identity, label) = node
                .get("id")
                .map(reference)
                .transpose()?
                .ok_or_else(|| bad_graph("a node id"))?;
            let summary = node
                .get("value")
                .and_then(|summary| summary.as_map().ok())
                .cloned()
                .unwrap_or_default();
            graph.insert_node(
                Node::new(kind, identity, label)
                    .with_summary(summary)
                    .with_provenance(node.provenance().clone()),
            );
        }

        for value in entries("edges") {
            let edge = value.as_record().map_err(|_| bad_graph("an edge"))?;
            let end = |name: &str| -> Result<ObjectId, ErrorValue> {
                edge.get(name)
                    .map(reference)
                    .transpose()?
                    .map(|(kind, identity, _)| Node::new(kind, identity, "").id().clone())
                    .ok_or_else(|| bad_graph("an edge end"))
            };
            let text = |name: &str| {
                edge.get(name)
                    .and_then(|value| value.as_str().ok())
                    .unwrap_or_default()
                    .to_owned()
            };
            let mut revived = match text("confidence").as_str() {
                "exact" => {
                    Edge::exact(end("from")?, end("to")?, text("relation"), text("provider"))
                }
                _ => Edge::inferred(
                    end("from")?,
                    end("to")?,
                    text("relation"),
                    text("provider"),
                    "",
                ),
            };
            if text("direction") == "undirected" {
                revived = revived.undirected();
            }
            if let Some(metadata) = edge.get("metadata").and_then(|value| value.as_map().ok()) {
                for (key, value) in metadata {
                    revived = revived.with_metadata(key, value.clone());
                }
            }
            graph.insert_edge(revived);
        }

        // The root travels as a reference; parsing keeps it even when insertion order differed.
        if let Some((kind, identity, _)) = record
            .get("root")
            .filter(|root| !matches!(root, Value::Null))
            .map(reference)
            .transpose()?
        {
            graph.root = Some(Node::new(kind, identity, "").id().clone());
        }
        Ok(graph)
    }
}

fn bad_graph(what: &str) -> ErrorValue {
    ErrorValue::new(
        ono_core::ErrorCode::TypeMismatch,
        format!("this ono.graph/1 record cannot be read: {what} is missing or malformed"),
    )
}

/// One of this crate's contracts, by name.
fn schema(name: &str) -> Result<std::sync::Arc<Schema>, ErrorValue> {
    builtin_schemas()
        .get(&SchemaId::new(name, 1))
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("the contract for {name}/1 is not loaded"),
            )
        })
}
