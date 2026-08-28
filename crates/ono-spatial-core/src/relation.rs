//! The relation vocabulary (spec v0.4 §3.5, §11.2, §11.5, §32, §41.2).
//!
//! A relationship edge describes a real connection between two objects, which §2.6 and §11 keep
//! strictly apart from the hierarchical grouping of [`crate::space`]: hierarchy orients, the
//! graph describes what is actually connected. Every relation `follow` accepts is declared here
//! and in `docs/spec/spatial/relations.yaml`, and `spec-check` holds the two together.

use crate::SpatialType;
use std::fmt;

/// How confident the layer is that a relation holds (§11.5).
///
/// The vocabulary is fixed by §11.5 and a value is never raised after the fact: §22.2 forbids
/// presenting a derivation as something a provider observed, which is why this enum has no
/// method that strengthens one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    /// Observed directly.
    Exact,
    /// Not observed, but the evidence leaves no serious alternative.
    Strong,
    /// Derived from evidence that does not prove it. Maps show it differently (§11.5).
    Inferred,
    /// Asserted by the user — a pin, a declared relationship.
    UserDeclared,
    /// The provider could not say. Unknown is visible, never absent (§2.17).
    Unknown,
}

impl Confidence {
    /// Every value, strongest first, as §11.5 lists them.
    pub const ALL: &'static [Confidence] = &[
        Confidence::Exact,
        Confidence::Strong,
        Confidence::Inferred,
        Confidence::UserDeclared,
        Confidence::Unknown,
    ];

    /// The name §11.5 and the registry spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Confidence::Exact => "exact",
            Confidence::Strong => "strong",
            Confidence::Inferred => "inferred",
            Confidence::UserDeclared => "user_declared",
            Confidence::Unknown => "unknown",
        }
    }

    /// The value with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|value| value.as_str() == name)
    }

    /// Whether a map must distinguish this edge from an observed one (§11.5).
    #[must_use]
    pub fn is_certain(self) -> bool {
        matches!(self, Confidence::Exact)
    }

    /// The same claim in the two-valued vocabulary the `trace` graph of spec v0.2 §22 uses.
    ///
    /// Only [`Confidence::Exact`] maps to [`ono_render::Confidence::Exact`]. Everything weaker —
    /// including `strong` and `user_declared` — becomes `Inferred`, because the graph's rule is
    /// that an inference may never be presented as an observation (spec v0.2 §22.2). The bridge
    /// may lose precision; it may not gain certainty.
    #[must_use]
    pub fn to_graph(self) -> ono_render::Confidence {
        match self {
            Confidence::Exact => ono_render::Confidence::Exact,
            _ => ono_render::Confidence::Inferred,
        }
    }

    /// The spatial claim a `trace` edge makes.
    ///
    /// The graph has no value between `exact` and `inferred`, so an inferred graph edge stays
    /// inferred here rather than being promoted to `strong`.
    #[must_use]
    pub fn from_graph(confidence: ono_render::Confidence) -> Self {
        match confidence {
            ono_render::Confidence::Exact => Confidence::Exact,
            ono_render::Confidence::Inferred => Confidence::Inferred,
        }
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a relation's declaration says about the confidence its edges carry (§41.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceClaim {
    /// Every edge of this relation carries exactly this confidence.
    Fixed(Confidence),
    /// §41.2's own `exact_or_provider_declared`: exact where the provider observed the edge, the
    /// provider's own claim otherwise. The provider states it per edge; the registry states only
    /// that it may.
    ProviderDeclared,
}

impl ConfidenceClaim {
    /// The name the registry spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ConfidenceClaim::Fixed(confidence) => confidence.as_str(),
            ConfidenceClaim::ProviderDeclared => "exact_or_provider_declared",
        }
    }

    /// Whether `confidence` is a claim this relation may carry.
    #[must_use]
    pub fn admits(self, confidence: Confidence) -> bool {
        match self {
            ConfidenceClaim::Fixed(declared) => declared == confidence,
            ConfidenceClaim::ProviderDeclared => true,
        }
    }
}

/// Which way an edge runs (§41.2, §22).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    /// From the source to the target.
    Outbound,
    /// From the target to the source — the same edge, read from the other end.
    Inbound,
    /// Between the two, with neither end the origin: a connection has two ends (§14.4).
    Bidirectional,
}

impl Direction {
    /// Every direction, as §41.2 lists them.
    pub const ALL: &'static [Direction] = &[
        Direction::Outbound,
        Direction::Inbound,
        Direction::Bidirectional,
    ];

    /// The name the registry spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Direction::Outbound => "outbound",
            Direction::Inbound => "inbound",
            Direction::Bidirectional => "bidirectional",
        }
    }

    /// The direction with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|value| value.as_str() == name)
    }

    /// The same edge read from the other end.
    #[must_use]
    pub const fn inverted(self) -> Self {
        match self {
            Direction::Outbound => Direction::Inbound,
            Direction::Inbound => Direction::Outbound,
            Direction::Bidirectional => Direction::Bidirectional,
        }
    }

    /// The same direction in the vocabulary of the `trace` graph of spec v0.2 §22.
    #[must_use]
    pub const fn to_graph(self) -> ono_graph::Direction {
        match self {
            Direction::Outbound | Direction::Inbound => ono_graph::Direction::Directed,
            Direction::Bidirectional => ono_graph::Direction::Undirected,
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a relation costs to answer (§32.1).
///
/// Default `look` and `map` avoid [`CostClass::Expensive`] relations unless they are already
/// cached; §32.2 makes those appear as discoverable but unloaded exits instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CostClass {
    /// Already available, or an O(1)/small local lookup.
    Cheap,
    /// A bounded system query.
    Normal,
    /// A broad scan or a cross-provider correlation.
    Expensive,
    /// Requires elevated permission, which navigation itself never requests (§35.3).
    Privileged,
    /// Requires a remote operation across a link (§19, §35.4).
    Remote,
}

impl CostClass {
    /// Every class, as §32.1 lists them.
    pub const ALL: &'static [CostClass] = &[
        CostClass::Cheap,
        CostClass::Normal,
        CostClass::Expensive,
        CostClass::Privileged,
        CostClass::Remote,
    ];

    /// The name the registry spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CostClass::Cheap => "cheap",
            CostClass::Normal => "normal",
            CostClass::Expensive => "expensive",
            CostClass::Privileged => "privileged",
            CostClass::Remote => "remote",
        }
    }

    /// The class with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|value| value.as_str() == name)
    }

    /// Whether a default `look` or `map` may follow this relation eagerly (§32.1, §34.2).
    #[must_use]
    pub fn is_eager(self) -> bool {
        matches!(self, CostClass::Cheap | CostClass::Normal)
    }
}

/// The identity of a relation, as the registry declares it (`process.owns_socket`).
///
/// It holds the declaration rather than a name, so a `RelationType` that exists is a relation
/// that is declared: an edge can never carry a relation nobody wrote down, which is what §2.5's
/// "every edge is explainable" starts from.
#[derive(Debug, Clone, Copy)]
pub struct RelationType(&'static RelationSpec);

impl PartialEq for RelationType {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl Eq for RelationType {}

impl PartialOrd for RelationType {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RelationType {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.id.cmp(other.0.id)
    }
}

impl std::hash::Hash for RelationType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}

impl RelationType {
    /// The relation with this id, or `None` when the registry declares none.
    #[must_use]
    pub fn new(id: &str) -> Option<Self> {
        spec(id).map(Self)
    }

    /// The relation's id.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        self.0.id
    }

    /// The declaration behind the relation.
    #[must_use]
    pub fn spec(&self) -> &'static RelationSpec {
        self.0
    }
}

impl fmt::Display for RelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.id)
    }
}

/// One declared relation (§41.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationSpec {
    /// The relation's id, e.g. `process.owns_socket`.
    pub id: &'static str,
    /// The type the edge starts at.
    pub source: SpatialType,
    /// The type the edge leads to.
    pub target: SpatialType,
    /// Which way the edge runs.
    pub direction: Direction,
    /// The word `follow` takes to traverse from source to target.
    pub canonical_label: &'static str,
    /// The word `follow` takes to traverse from target back to source.
    pub inverse_label: &'static str,
    /// What confidence the relation's edges may carry.
    pub confidence: ConfidenceClaim,
    /// What answering the relation costs.
    pub cost_class: CostClass,
}

impl RelationSpec {
    /// The relation as a [`RelationType`].
    #[must_use]
    pub fn relation_type(&'static self) -> RelationType {
        RelationType(self)
    }

    /// The labels a user standing on an object of `from` may type to traverse this relation.
    ///
    /// This is what makes the graph readable from both ends: `follow socket` from a process and
    /// `follow owner` from a socket are the two ends of one edge (§6.4). A relation between two
    /// objects of the same type yields *both* labels, because both ends are here: §12 lists
    /// `parent` and `children` as separate exits of one process place, and they are the two ends
    /// of `process.parent_of`.
    pub fn labels_from(&self, from: SpatialType) -> impl Iterator<Item = &'static str> {
        let canonical = (from == self.source).then_some(self.canonical_label);
        let inverse = (from == self.target).then_some(self.inverse_label);
        canonical.into_iter().chain(inverse)
    }

    /// The type a user reaches by following this relation from `from`, or `None` when the
    /// relation does not touch that type.
    #[must_use]
    pub fn target_from(&self, from: SpatialType) -> Option<SpatialType> {
        if from == self.source {
            Some(self.target)
        } else if from == self.target {
            Some(self.source)
        } else {
            None
        }
    }
}

/// Every declared relation, in the order `docs/spec/spatial/relations.yaml` declares them.
pub const RELATIONS: &[RelationSpec] = &[
    RelationSpec {
        id: "process.parent_of",
        source: SpatialType::Process,
        target: SpatialType::Process,
        direction: Direction::Outbound,
        canonical_label: "child",
        inverse_label: "parent",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Cheap,
    },
    RelationSpec {
        id: "process.owns_socket",
        source: SpatialType::Process,
        target: SpatialType::Socket,
        direction: Direction::Outbound,
        canonical_label: "socket",
        inverse_label: "owner",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Normal,
    },
    RelationSpec {
        id: "process.opened_file",
        source: SpatialType::Process,
        target: SpatialType::File,
        direction: Direction::Outbound,
        canonical_label: "file",
        inverse_label: "opener",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Expensive,
    },
    RelationSpec {
        id: "process.member_of_cgroup",
        source: SpatialType::Process,
        target: SpatialType::Cgroup,
        direction: Direction::Outbound,
        canonical_label: "cgroup",
        inverse_label: "process",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Cheap,
    },
    RelationSpec {
        id: "process.in_namespace",
        source: SpatialType::Process,
        target: SpatialType::Namespace,
        direction: Direction::Outbound,
        canonical_label: "namespace",
        inverse_label: "member",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Cheap,
    },
    RelationSpec {
        id: "process.connects_to",
        source: SpatialType::Process,
        target: SpatialType::Endpoint,
        direction: Direction::Outbound,
        canonical_label: "endpoint",
        inverse_label: "client",
        confidence: ConfidenceClaim::ProviderDeclared,
        cost_class: CostClass::Normal,
    },
    RelationSpec {
        id: "service.controls_process",
        source: SpatialType::Service,
        target: SpatialType::Process,
        direction: Direction::Outbound,
        canonical_label: "process",
        inverse_label: "service",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Cheap,
    },
    RelationSpec {
        id: "service.depends_on",
        source: SpatialType::Service,
        target: SpatialType::Service,
        direction: Direction::Outbound,
        canonical_label: "dependency",
        inverse_label: "dependent",
        confidence: ConfidenceClaim::ProviderDeclared,
        cost_class: CostClass::Normal,
    },
    RelationSpec {
        id: "container.contains_process",
        source: SpatialType::Container,
        target: SpatialType::Process,
        direction: Direction::Outbound,
        canonical_label: "process",
        inverse_label: "container",
        // The kernel does not report container membership. A runtime that lists its own
        // processes observes it; the container id in `/proc/<pid>/cgroup` is strong evidence
        // and not an observation, so the claim is the provider's per edge (§11.5, ADR-0135).
        confidence: ConfidenceClaim::ProviderDeclared,
        cost_class: CostClass::Normal,
    },
    RelationSpec {
        id: "socket.connected_to",
        source: SpatialType::Socket,
        target: SpatialType::Endpoint,
        direction: Direction::Bidirectional,
        canonical_label: "peer",
        inverse_label: "peer",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Normal,
    },
    RelationSpec {
        id: "socket.accepts_connection",
        source: SpatialType::Socket,
        target: SpatialType::Connection,
        direction: Direction::Outbound,
        canonical_label: "connection",
        inverse_label: "listener",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Normal,
    },
    RelationSpec {
        id: "interface.has_address",
        source: SpatialType::Interface,
        target: SpatialType::Address,
        direction: Direction::Outbound,
        canonical_label: "address",
        inverse_label: "interface",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Cheap,
    },
    RelationSpec {
        id: "route.via_interface",
        source: SpatialType::Route,
        target: SpatialType::Interface,
        direction: Direction::Outbound,
        canonical_label: "interface",
        inverse_label: "route",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Cheap,
    },
    RelationSpec {
        id: "filesystem.mounted_at",
        source: SpatialType::Filesystem,
        target: SpatialType::Mount,
        direction: Direction::Outbound,
        canonical_label: "mount",
        inverse_label: "filesystem",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Cheap,
    },
    RelationSpec {
        id: "mount.backs_directory",
        source: SpatialType::Mount,
        target: SpatialType::Directory,
        direction: Direction::Outbound,
        canonical_label: "directory",
        inverse_label: "mount",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Cheap,
    },
    RelationSpec {
        id: "device.backs_filesystem",
        source: SpatialType::BlockDevice,
        target: SpatialType::Filesystem,
        direction: Direction::Outbound,
        canonical_label: "filesystem",
        inverse_label: "device",
        confidence: ConfidenceClaim::ProviderDeclared,
        cost_class: CostClass::Normal,
    },
    RelationSpec {
        id: "user.owns_process",
        source: SpatialType::User,
        target: SpatialType::Process,
        direction: Direction::Outbound,
        canonical_label: "process",
        inverse_label: "user",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Normal,
    },
    RelationSpec {
        id: "user.owns_file",
        source: SpatialType::User,
        target: SpatialType::File,
        direction: Direction::Outbound,
        canonical_label: "file",
        inverse_label: "owner",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Expensive,
    },
    RelationSpec {
        id: "user.member_of_group",
        source: SpatialType::User,
        target: SpatialType::Group,
        direction: Direction::Outbound,
        canonical_label: "group",
        inverse_label: "member",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Cheap,
    },
    RelationSpec {
        id: "host.linked_to",
        source: SpatialType::Host,
        target: SpatialType::Host,
        direction: Direction::Bidirectional,
        canonical_label: "host",
        inverse_label: "host",
        confidence: ConfidenceClaim::Fixed(Confidence::Exact),
        cost_class: CostClass::Remote,
    },
];

/// Every declared relation.
#[must_use]
pub fn relations() -> &'static [RelationSpec] {
    RELATIONS
}

/// The relation with this id, or `None`.
#[must_use]
pub fn spec(id: &str) -> Option<&'static RelationSpec> {
    RELATIONS.iter().find(|relation| relation.id == id)
}

/// Every exit a user standing on an object of `from` has, as the label they type and the
/// relation behind it.
pub fn exits_from(
    from: SpatialType,
) -> impl Iterator<Item = (&'static str, &'static RelationSpec)> {
    RELATIONS.iter().flat_map(move |relation| {
        relation
            .labels_from(from)
            .map(move |label| (label, relation))
    })
}

/// The relation `label` names for a user standing on an object of `from`.
///
/// Resolution is by source type, not globally: `follow process` means the obvious thing from a
/// service, a user and a container alike, and each is a different relation (ADR-0128). A label
/// that names nothing here is `spatial.no_relation` (§40); a label that names one relation with
/// no neighbour is `spatial.not_found`, because the name *was* understood.
#[must_use]
pub fn resolve_label(from: SpatialType, label: &str) -> Vec<&'static RelationSpec> {
    RELATIONS
        .iter()
        .filter(|relation| relation.labels_from(from).any(|declared| declared == label))
        .collect()
}

/// Every label any relation accepts, for completion and for `help spatial` (§41.3).
#[must_use]
pub fn labels() -> Vec<&'static str> {
    let mut labels: Vec<&'static str> = RELATIONS
        .iter()
        .flat_map(|relation| [relation.canonical_label, relation.inverse_label])
        .collect();
    labels.sort_unstable();
    labels.dedup();
    labels
}
