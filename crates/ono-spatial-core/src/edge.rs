//! Edges: the hierarchy of §3.4 and the relationship graph of §3.5.
//!
//! §2.6 is the invariant these two types exist to keep: "Hierarchy and graph are separate
//! concepts. Parent/child spatial grouping MUST NOT be confused with arbitrary relationships."
//! They are therefore different types, with no conversion between them — a hierarchical edge
//! cannot become an operational dependency by being re-read, and `up` cannot arrive somewhere
//! only a relationship edge leads.

use std::fmt;

use jiff::Timestamp;
use ono_value::{MapValue, Provenance, Value};
use sha2::{Digest, Sha256};

use crate::{Confidence, Direction, RelationType, SpatialId};

/// Why a hierarchical edge exists (§3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HierarchyKind {
    /// The child is physically inside the parent: a directory in a filesystem, a process in a
    /// container.
    Containment,
    /// The parent is a canonical grouping the child is filed under for orientation: a service in
    /// `compute.services`, a domain under the root (§4.1).
    Grouping,
}

impl HierarchyKind {
    /// The name a place view or a map legend shows.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            HierarchyKind::Containment => "containment",
            HierarchyKind::Grouping => "grouping",
        }
    }
}

/// Containment or canonical spatial grouping (§3.4).
///
/// A hierarchical edge exists "primarily to support orientation and zoom" and "MUST NOT assert
/// operational dependency unless such a dependency is separately represented as a relationship
/// edge" (§3.4). There is deliberately no way to read a dependency out of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchicalEdge {
    parent: SpatialId,
    child: SpatialId,
    kind: HierarchyKind,
}

impl HierarchicalEdge {
    /// The edge that files `child` under `parent`.
    #[must_use]
    pub fn new(parent: SpatialId, child: SpatialId, kind: HierarchyKind) -> Self {
        Self {
            parent,
            child,
            kind,
        }
    }

    /// The place `up` arrives at from the child (§6.6, §11.1).
    #[must_use]
    pub fn parent(&self) -> &SpatialId {
        &self.parent
    }

    /// The place filed under the parent.
    #[must_use]
    pub fn child(&self) -> &SpatialId {
        &self.child
    }

    /// Whether the grouping is containment or orientation.
    #[must_use]
    pub fn kind(&self) -> HierarchyKind {
        self.kind
    }
}

impl fmt::Display for HierarchicalEdge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.parent, self.child)
    }
}

/// When a relationship was true, where the provider can say (§3.5's `validity: ValidityWindow?`).
///
/// Both ends are optional and an absent end is not "now": a connection observed at 10:00 with no
/// end recorded is a connection whose end nobody knows (§2.17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidityWindow {
    from: Option<Timestamp>,
    to: Option<Timestamp>,
}

impl ValidityWindow {
    /// A window with either end optional.
    #[must_use]
    pub fn new(from: Option<Timestamp>, to: Option<Timestamp>) -> Self {
        Self { from, to }
    }

    /// A window that opened at `from` and has not been seen to close.
    #[must_use]
    pub fn since(from: Timestamp) -> Self {
        Self {
            from: Some(from),
            to: None,
        }
    }

    /// When the relationship began, where known.
    #[must_use]
    pub fn from(&self) -> Option<Timestamp> {
        self.from
    }

    /// When it ended, where known.
    #[must_use]
    pub fn to(&self) -> Option<Timestamp> {
        self.to
    }

    /// Whether the window is known to have closed before `at`.
    ///
    /// An unknown end never answers `true`: not knowing when something ended is not knowing that
    /// it did.
    #[must_use]
    pub fn has_closed_by(&self, at: Timestamp) -> bool {
        self.to.is_some_and(|end| end <= at)
    }
}

/// The stable identity of one asserted relationship (§11.4's `inspect relation @edge-17`).
///
/// Two assertions of the same relationship between the same two objects in the same direction
/// are the same edge and share an id; an inference is not the same assertion as an observation,
/// so the confidence is part of the identity, exactly as it is in the `trace` graph of spec v0.2
/// §22.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeId(String);

impl EdgeId {
    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A real connection between two objects (§3.5).
///
/// Every field §3.5 lists is here, and every one of them is what makes §2.5 true: "`inspect
/// relation` or equivalent MUST expose why two objects are considered related."
#[derive(Debug, Clone, PartialEq)]
pub struct RelationshipEdge {
    source: SpatialId,
    target: SpatialId,
    relation: RelationType,
    direction: Direction,
    confidence: Confidence,
    provenance: Provenance,
    observed_at: Timestamp,
    validity: Option<ValidityWindow>,
    attributes: MapValue,
}

impl RelationshipEdge {
    /// An edge asserting `relation` between `source` and `target`.
    ///
    /// The direction defaults to the one the relation declares; [`RelationshipEdge::inverted`]
    /// is how the other end is read. The confidence must be one the relation's declaration
    /// admits — a relation declared `exact` cannot carry an inferred edge, because that would
    /// make the registry's promise untrue — and is otherwise the provider's to state.
    #[must_use]
    pub fn new(
        source: SpatialId,
        target: SpatialId,
        relation: RelationType,
        confidence: Confidence,
        provenance: Provenance,
        observed_at: Timestamp,
    ) -> Self {
        let direction = relation.spec().direction;
        Self {
            source,
            target,
            relation,
            direction,
            confidence,
            provenance,
            observed_at,
            validity: None,
            attributes: MapValue::new(),
        }
    }

    /// Records when the relationship was true (§3.5).
    #[must_use]
    pub fn valid(mut self, window: ValidityWindow) -> Self {
        self.validity = Some(window);
        self
    }

    /// Adds a detail of the relationship — a file descriptor, a socket inode, a port.
    #[must_use]
    pub fn with_attribute(mut self, key: &str, value: Value) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }

    /// Whether `other` is the same edge as this one — the same question
    /// [`RelationshipEdge::edge_id`] answers, asked without computing either id.
    ///
    /// An edge is identified by the five fields the id hashes, so two edges share an id exactly
    /// when those five agree. Attributes, validity and observation time are not part of the
    /// identity: a re-observation of the same relationship is the same edge carrying newer
    /// detail (§33.2).
    ///
    /// This exists because the index answers it once per edge already held by a place, and
    /// hashing there made recording an edge cost a SHA-256 per neighbour.
    #[must_use]
    pub fn same_edge_as(&self, other: &Self) -> bool {
        self.source == other.source
            && self.target == other.target
            && self.relation == other.relation
            && self.direction == other.direction
            && self.confidence == other.confidence
    }

    /// The edge's stable identity.
    #[must_use]
    pub fn edge_id(&self) -> EdgeId {
        let mut hasher = Sha256::new();
        for part in [
            self.source.as_str(),
            self.target.as_str(),
            self.relation.as_str(),
            self.direction.as_str(),
            self.confidence.as_str(),
        ] {
            hasher.update(part.as_bytes());
            hasher.update([0x1f]);
        }
        let digest = hasher.finalize();
        let mut token = String::with_capacity(24);
        for byte in digest.iter().take(12) {
            use fmt::Write as _;
            let _ = write!(token, "{byte:02x}");
        }
        EdgeId(format!("edge:{token}"))
    }

    /// The object the edge starts at.
    #[must_use]
    pub fn source(&self) -> &SpatialId {
        &self.source
    }

    /// The object the edge leads to.
    #[must_use]
    pub fn target(&self) -> &SpatialId {
        &self.target
    }

    /// Which relation is asserted.
    #[must_use]
    pub fn relation(&self) -> &RelationType {
        &self.relation
    }

    /// Which way it runs.
    #[must_use]
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// How confident the assertion is (§11.5).
    #[must_use]
    pub fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// Where the assertion came from (§11.4).
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// When it was observed (§3.5).
    #[must_use]
    pub fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    /// When the relationship was true, where the provider could say.
    #[must_use]
    pub fn validity(&self) -> Option<ValidityWindow> {
        self.validity
    }

    /// The relationship's own detail.
    #[must_use]
    pub fn attributes(&self) -> &MapValue {
        &self.attributes
    }

    /// The same assertion, read from the other end.
    ///
    /// This is one edge, not two: `follow socket` from a process and `follow owner` from that
    /// socket traverse the same relationship, so the confidence, the provenance and the
    /// observation time are unchanged and only the ends and the direction swap. A bidirectional
    /// relation inverts to itself in direction, because neither end is the origin (§14.4).
    #[must_use]
    pub fn inverted(&self) -> Self {
        Self {
            source: self.target.clone(),
            target: self.source.clone(),
            relation: self.relation,
            direction: self.direction.inverted(),
            confidence: self.confidence,
            provenance: self.provenance.clone(),
            observed_at: self.observed_at,
            validity: self.validity,
            attributes: self.attributes.clone(),
        }
    }

    /// The label a user standing on `at` types to traverse this edge, or `None` when the edge
    /// does not touch that end.
    #[must_use]
    pub fn label_from(&self, at: &SpatialId) -> Option<&'static str> {
        let spec = self.relation.spec();
        if at == &self.source {
            Some(spec.canonical_label)
        } else if at == &self.target {
            Some(spec.inverse_label)
        } else {
            None
        }
    }

    /// The word `look` prints for this edge's exit, seen from `at` (§12, §24.2).
    #[must_use]
    pub fn group_from(&self, at: &SpatialId) -> Option<&'static str> {
        let spec = self.relation.spec();
        if at == &self.source {
            Some(spec.canonical_group)
        } else if at == &self.target {
            Some(spec.inverse_group)
        } else {
            None
        }
    }

    /// The other end of the edge, seen from `at`.
    #[must_use]
    pub fn other_end(&self, at: &SpatialId) -> Option<&SpatialId> {
        if at == &self.source {
            Some(&self.target)
        } else if at == &self.target {
            Some(&self.source)
        } else {
            None
        }
    }

    /// Whether the relation's declaration admits the confidence this edge carries.
    ///
    /// A `false` here is `spatial.identity_conflict`-adjacent drift: the registry promised a
    /// certainty the provider did not deliver, and §41.3 makes that registry the source of the
    /// map legend a user reads.
    #[must_use]
    pub fn honours_declaration(&self) -> bool {
        self.relation.spec().confidence.admits(self.confidence)
    }
}
