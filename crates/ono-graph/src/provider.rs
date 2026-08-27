//! Contributing relationships (spec §22.2, §31.26).
//!
//! A relationship provider is shaped like [`ono_provider_api::Provider`]: it has an id that
//! appears on everything it asserts, it declares what it answers about and what it needs to be
//! allowed to do, and it says when it cannot answer here. That is deliberate — spec §31.26 lets
//! a KUANG/11 package "add graph edges without owning either endpoint schema", and a plugin that
//! did so through a different interface than the core providers use would be a special case the
//! rest of the shell has to know about.

use ono_provider_api::{Availability, Capability};
use ono_value::{ErrorValue, Value};

use crate::graph::{Edge, Node};

/// One relationship a provider found, together with the object at the far end of it.
///
/// The target travels with the edge because the walk has to be able to show what it reached
/// without asking a second provider what that object is called.
#[derive(Debug, Clone)]
pub struct Relationship {
    edge: Edge,
    target: Node,
}

impl Relationship {
    /// A relationship the provider observed, from `subject` to `target`.
    #[must_use]
    pub fn exact(subject: &Node, target: Node, relation: &str, provider: &str) -> Self {
        let edge = Edge::exact(
            subject.id().clone(),
            target.id().clone(),
            relation,
            provider,
        );
        Self { edge, target }
    }

    /// A relationship the provider derived, naming what it derived it from.
    ///
    /// The evidence is not optional: spec §22.2 requires an inference to identify itself as one,
    /// and spec §31.25 requires the evidence behind it to stay inspectable.
    #[must_use]
    pub fn inferred(
        subject: &Node,
        target: Node,
        relation: &str,
        provider: &str,
        evidence: &str,
    ) -> Self {
        let edge = Edge::inferred(
            subject.id().clone(),
            target.id().clone(),
            relation,
            provider,
            evidence,
        );
        Self { edge, target }
    }

    /// Adds a detail of the relationship, such as the descriptor it was read from.
    #[must_use]
    pub fn with_metadata(mut self, key: &str, value: Value) -> Self {
        self.edge = self.edge.with_metadata(key, value);
        self
    }

    /// Marks the relationship as holding in neither direction in particular.
    #[must_use]
    pub fn undirected(mut self) -> Self {
        self.edge = self.edge.undirected();
        self
    }

    /// The edge itself.
    #[must_use]
    pub fn edge(&self) -> &Edge {
        &self.edge
    }

    /// The object at the far end.
    #[must_use]
    pub fn target(&self) -> &Node {
        &self.target
    }

    pub(crate) fn into_parts(self) -> (Edge, Node) {
        (self.edge, self.target)
    }
}

/// What a provider found about one object, and what it could not read.
///
/// The two are separate because they answer different questions: an empty list of relationships
/// means the object has none, while a failure means nobody knows whether it has any (spec §10.5,
/// §16.5).
#[derive(Debug, Clone, Default)]
pub struct Relationships {
    found: Vec<Relationship>,
    failures: Vec<ErrorValue>,
}

impl Relationships {
    /// Nothing found, nothing failed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Only a failure: the provider could not answer for this object at all.
    #[must_use]
    pub fn failed(error: ErrorValue) -> Self {
        Self {
            found: Vec::new(),
            failures: vec![error],
        }
    }

    /// Adds a relationship.
    pub fn push(&mut self, relationship: Relationship) {
        self.found.push(relationship);
    }

    /// Records something that could not be read.
    pub fn fail(&mut self, error: ErrorValue) {
        self.failures.push(error);
    }

    /// The relationships found.
    #[must_use]
    pub fn found(&self) -> &[Relationship] {
        &self.found
    }

    /// What could not be read.
    #[must_use]
    pub fn failures(&self) -> &[ErrorValue] {
        &self.failures
    }

    pub(crate) fn into_parts(self) -> (Vec<Relationship>, Vec<ErrorValue>) {
        (self.found, self.failures)
    }
}

/// A source of relationships between objects (spec §22.2).
#[async_trait::async_trait]
pub trait RelationshipProvider: Send + Sync + std::fmt::Debug {
    /// The provider's stable id, such as `linux.process-tree`. It appears on every edge it
    /// asserts, so a questionable relationship can be traced back to whoever claimed it.
    fn id(&self) -> &str;

    /// The schema ids of the objects it can expand, such as `ono.process/1`.
    fn subjects(&self) -> &[&str];

    /// The relation names it can contribute, such as `owns` or `listens`.
    ///
    /// A trace restricted to some relations consults only the providers that offer them, so this
    /// is metadata a walk plans with rather than documentation.
    fn relations(&self) -> &[&str];

    /// What it must be allowed to do.
    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }

    /// Whether it can answer on this machine, and why not when it cannot.
    fn availability(&self) -> Availability {
        Availability::Available
    }

    /// The relationships of one object, as they are now.
    ///
    /// A provider that cannot read what it needs returns the failure rather than an empty
    /// result: a relationship this user may not see is not a relationship that does not exist.
    async fn relationships(&self, subject: &Node) -> Relationships;
}
