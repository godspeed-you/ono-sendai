//! The canonical geography: the root, the six domains and their collections (spec v0.4 §4, §7).
//!
//! §4 requires every local host to expose a canonical root space "even if some providers are
//! unavailable", and §4.1 says why: the system graph is not hierarchical, so the domains exist
//! as orientation anchors rather than as a claim about the ontology underneath. A user learns
//! six stable directions and then meets the graph.
//!
//! This table is the same geography `docs/contracts/spatial/spaces.yaml` declares, and
//! `cargo run -p xtask -- spec-check` fails when the two disagree in either direction.

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

use crate::{SpatialScope, SpatialType};

/// The zoom level of §8.1 a space sits at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpaceKind {
    /// L0 — the root (§8.1).
    Root,
    /// L1 — one of the six canonical domains (§8.1, §53).
    Domain,
    /// L2 — a collection of objects inside a domain (§8.1).
    Collection,
}

impl SpaceKind {
    /// The name the registry spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SpaceKind::Root => "root",
            SpaceKind::Domain => "domain",
            SpaceKind::Collection => "collection",
        }
    }

    /// The zoom level of §8.1.
    #[must_use]
    pub const fn zoom_level(self) -> &'static str {
        match self {
            SpaceKind::Root => "L0",
            SpaceKind::Domain => "L1",
            SpaceKind::Collection => "L2",
        }
    }
}

/// Whether a space is part of the geography the shell serves today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpaceStatus {
    /// Served: the shell shows it, with a state where no provider can fill it (§4, §35.2).
    Stable,
    /// Declared for completeness, not served. §7.2's `workloads` is the only one: it is a `MAY`
    /// that Ono forms "when reliable evidence connects processes/services/containers", and a
    /// space nothing serves would otherwise be a promise nobody keeps (ADR-0128).
    Planned,
}

impl SpaceStatus {
    /// The name the registry spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SpaceStatus::Stable => "stable",
            SpaceStatus::Planned => "planned",
        }
    }
}

/// One canonical place: the root, a domain, or a collection inside a domain (§41.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalSpace {
    /// The dotted id, e.g. `compute.services`.
    pub id: &'static str,
    /// The label a place view shows, e.g. `services`.
    pub label: &'static str,
    /// The canonical parent's id, or `None` for the root (§11.1).
    pub parent: Option<&'static str>,
    /// Which zoom level the space sits at.
    pub kind: SpaceKind,
    /// What a user standing here is standing in (§41.1).
    pub object_type: SpatialType,
    /// What a user finds inside, or `None` for a place holding only other places.
    pub member_type: Option<SpatialType>,
    /// The schema of the records the place is built from, or `None` where no provider answers
    /// for them yet — null is honest, an invented schema is not (§2.17).
    pub schema: Option<&'static str>,
    /// Whether `enter` accepts it as a destination.
    pub enterable: bool,
    /// The spatial commands that mean something here, spelled as the user types them.
    pub commands: &'static [&'static str],
    /// What a `look` summary of the place reports.
    pub summary_fields: &'static [&'static str],
    /// Whether the shell serves it today.
    pub status: SpaceStatus,
}

impl CanonicalSpace {
    /// Whether the space is one of the six canonical domains (§53).
    #[must_use]
    pub fn is_domain(&self) -> bool {
        self.kind == SpaceKind::Domain
    }

    /// Whether the shell serves the space today.
    #[must_use]
    pub fn is_served(&self) -> bool {
        self.status == SpaceStatus::Stable
    }

    /// The v0.2 schema of the object a user standing here is standing in (§3.1's `object_type`).
    ///
    /// §7.1 gives the root a contract of its own — `SystemPlace`, declared as `ono.system/1` —
    /// because the object behind the root place is the system. No other space has one: a domain
    /// is geography rather than an observed object, and a collection is not one of its members,
    /// so both report the place contract itself (ADR-0140, ADR-0142). [`Self::schema`] answers a
    /// different question: which records the place is *built from*.
    #[must_use]
    pub fn place_schema(&self) -> &'static str {
        match self.kind {
            SpaceKind::Root => self.schema.unwrap_or(PLACE_SCHEMA),
            SpaceKind::Domain | SpaceKind::Collection => PLACE_SCHEMA,
        }
    }

    /// The space's opaque identity, which is the same in every session (§42.1).
    ///
    /// The geography is per host (§19.2: a `jump` across a link "MUST produce a new
    /// `SystemPlace`"), so the id is the one for the host this process is currently standing in
    /// — the local one until [`stand_in`] says otherwise. Two hosts' `COMPUTE` are two places,
    /// and §43.7 forbids merging them.
    #[must_use]
    pub fn spatial_id(&self) -> crate::SpatialId {
        self.spatial_id_in(standing_in().as_ref())
    }

    /// The space's identity in `scope`'s host geography.
    ///
    /// `None`, and any scope on the local host, give the local geography — so every id this
    /// crate produced before hosts existed is unchanged (§42.1: the same object resolves to the
    /// same id).
    #[must_use]
    pub fn spatial_id_in(&self, scope: Option<&SpatialScope>) -> crate::SpatialId {
        crate::SpatialIdentity::space_in(self.id, scope).spatial_id()
    }

    /// The label a place view shows for the space, in `scope`'s host geography.
    ///
    /// §7.1 makes the root place of a host the host itself, and §19.3 draws a federated map as a
    /// list of hosts — `prod/web01`, `home/nas01` — so a remote root is called by the name of
    /// the host it is the root of. Everything below it keeps its own label: the host is already
    /// the first segment of the place path (§27.2's `prod/web01/compute/processes`), and
    /// repeating it on every domain would say the same thing twice.
    #[must_use]
    pub fn label_in(&self, scope: Option<&SpatialScope>) -> String {
        match scope.filter(|scope| scope.is_remote() && self.kind == SpaceKind::Root) {
            Some(scope) => scope.host_scope().id().to_owned(),
            _ => self.label.to_owned(),
        }
    }
}

/// The host geographies this process can stand in (§19.2, §29.2, §46).
///
/// A session is a process (§29.2: "the current place is script-local"; the spatial state of
/// `ono-cli` is per process for exactly that reason), so the set of host geographies a session
/// has entered is process state and lives here, beside the geography it indexes. Nothing in this
/// module observes anything: `stand_in` is told which host the shell moved to, and everything
/// else is arithmetic over the declared table above.
#[derive(Debug)]
struct Geographies {
    /// The host the process is standing in, or `None` for the local one.
    standing: Option<SpatialScope>,
    /// Every canonical space id this process has produced, and the host geography it belongs to.
    known: BTreeMap<crate::SpatialId, (&'static CanonicalSpace, Option<SpatialScope>)>,
    /// The remote hosts whose geography is in `known`, so learning one twice is cheap.
    hosts: Vec<SpatialScope>,
}

impl Geographies {
    fn local() -> Self {
        let mut known = BTreeMap::new();
        for space in SPACES {
            known.insert(space.spatial_id_in(None), (space, None));
        }
        Self {
            standing: None,
            known,
            hosts: Vec::new(),
        }
    }

    fn learn(&mut self, scope: &SpatialScope) {
        if self.hosts.contains(scope) {
            return;
        }
        for space in SPACES {
            self.known.insert(
                space.spatial_id_in(Some(scope)),
                (space, Some(scope.clone())),
            );
        }
        self.hosts.push(scope.clone());
    }
}

fn geographies() -> &'static RwLock<Geographies> {
    static GEOGRAPHIES: OnceLock<RwLock<Geographies>> = OnceLock::new();
    GEOGRAPHIES.get_or_init(|| RwLock::new(Geographies::local()))
}

/// Moves the process into `scope`'s host geography, or back to the local one with `None`.
///
/// Every canonical space id produced afterwards is that host's, which is what makes `home`
/// return to "the root `SYSTEM` place for the current host" (§6.6) and what keeps a remote
/// `COMPUTE` from being the local one (§43.7). The host is remembered, so a place left behind on
/// the trail is still recognisable after the session has moved on (§20.3).
pub fn stand_in(scope: Option<SpatialScope>) {
    let Ok(mut geographies) = geographies().write() else {
        return;
    };
    if let Some(scope) = scope.as_ref().filter(|scope| scope.is_remote()) {
        geographies.learn(scope);
        geographies.standing = Some(scope.clone());
    } else {
        geographies.standing = None;
    }
}

/// Registers `scope`'s geography without moving into it (§19.1, §19.3).
///
/// A host this session holds a link to has a root place whether or not anyone has jumped to it:
/// §4 gives every host the canonical geography, and §19.1 already lists the link as somewhere to
/// go. Learning it is what lets the federated map name the far root without pretending to stand
/// on it (§19.3, §35.4).
pub fn learn(scope: &SpatialScope) {
    if !scope.is_remote() {
        return;
    }
    if let Ok(mut geographies) = geographies().write() {
        geographies.learn(scope);
    }
}

/// The remote host geography the process is standing in, or `None` for the local one (§19.2).
#[must_use]
pub fn standing_in() -> Option<SpatialScope> {
    geographies()
        .read()
        .ok()
        .and_then(|geographies| geographies.standing.clone())
}

/// The canonical space an id names and the host geography it belongs to, where it names one.
///
/// `None` in the second position is the local host. An id of a host this process never entered
/// is not a canonical space here — which is honest: nothing in this session can say what it is.
#[must_use]
pub fn space_of_id(
    id: &crate::SpatialId,
) -> Option<(&'static CanonicalSpace, Option<SpatialScope>)> {
    geographies()
        .read()
        .ok()
        .and_then(|geographies| geographies.known.get(id).cloned())
}

/// The id of the root space every session starts at (§46.1).
pub const ROOT: &str = "system";

/// The schema every place the spatial commands emit satisfies (ADR-0140).
const PLACE_SCHEMA: &str = "ono.spatial-place/1";

/// The canonical geography, in the order `docs/contracts/spatial/spaces.yaml` declares it.
/// The root space every session starts at (§7.1, §46.1).
pub const SYSTEM: CanonicalSpace = CanonicalSpace {
    id: "system",
    label: "SYSTEM",
    parent: None,
    kind: SpaceKind::Root,
    object_type: SpatialType::System,
    member_type: None,
    schema: Some("ono.system/1"),
    enterable: true,
    commands: &["look", "map", "near", "enter", "find place", "pin"],
    summary_fields: &["hostname", "os", "kernel", "uptime", "domains"],
    status: SpaceStatus::Stable,
};

/// The canonical geography, in the order `docs/contracts/spatial/spaces.yaml` declares it.
pub const SPACES: &[CanonicalSpace] = &[
    SYSTEM,
    CanonicalSpace {
        id: "compute",
        label: "COMPUTE",
        parent: Some("system"),
        kind: SpaceKind::Domain,
        object_type: SpatialType::Compute,
        member_type: None,
        schema: None,
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &[
            "process_count",
            "service_states",
            "job_count",
            "container_count",
        ],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "network",
        label: "NETWORK",
        parent: Some("system"),
        kind: SpaceKind::Domain,
        object_type: SpatialType::Network,
        member_type: None,
        schema: None,
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["interface_count", "listener_count", "connection_count"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "storage",
        label: "STORAGE",
        parent: Some("system"),
        kind: SpaceKind::Domain,
        object_type: SpatialType::Storage,
        member_type: None,
        schema: None,
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["filesystem_count", "mount_count", "pressure"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "containers",
        label: "CONTAINERS",
        parent: Some("system"),
        kind: SpaceKind::Domain,
        object_type: SpatialType::Containers,
        member_type: Some(SpatialType::Container),
        schema: Some("ono.container/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["container_count", "running", "runtimes"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "identity",
        label: "IDENTITY",
        parent: Some("system"),
        kind: SpaceKind::Domain,
        object_type: SpatialType::Identity,
        member_type: None,
        schema: None,
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["user_count", "group_count", "session_count"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "devices",
        label: "DEVICES",
        parent: Some("system"),
        kind: SpaceKind::Domain,
        object_type: SpatialType::Devices,
        member_type: Some(SpatialType::Device),
        schema: Some("ono.device/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["device_count", "categories"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "compute.processes",
        label: "processes",
        parent: Some("compute"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Process,
        member_type: Some(SpatialType::Process),
        schema: Some("ono.process/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "running", "sleeping", "stopped"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "compute.services",
        label: "services",
        parent: Some("compute"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Service,
        member_type: Some(SpatialType::Service),
        schema: Some("ono.service/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "running", "failed", "inactive"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "compute.jobs",
        label: "jobs",
        parent: Some("compute"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Job,
        member_type: Some(SpatialType::Job),
        schema: Some("ono.job/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "running", "stopped"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "compute.cgroups",
        label: "cgroups",
        parent: Some("compute"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Cgroup,
        member_type: Some(SpatialType::Cgroup),
        schema: Some("ono.cgroup/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "compute.workloads",
        label: "workloads",
        parent: Some("compute"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Workload,
        member_type: Some(SpatialType::Workload),
        schema: None,
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count"],
        status: SpaceStatus::Planned,
    },
    CanonicalSpace {
        id: "network.interfaces",
        label: "interfaces",
        parent: Some("network"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Interface,
        member_type: Some(SpatialType::Interface),
        schema: Some("ono.interface/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "up", "down"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "network.addresses",
        label: "addresses",
        parent: Some("network"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Address,
        member_type: Some(SpatialType::Address),
        schema: Some("ono.interface-address/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "ipv4", "ipv6"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "network.routes",
        label: "routes",
        parent: Some("network"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Route,
        member_type: Some(SpatialType::Route),
        schema: Some("ono.route/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "default"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "network.neighbors",
        label: "neighbors",
        parent: Some("network"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Neighbor,
        member_type: Some(SpatialType::Neighbor),
        schema: Some("ono.neighbor/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "reachable"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "network.listeners",
        label: "listeners",
        parent: Some("network"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Listener,
        member_type: Some(SpatialType::Listener),
        schema: Some("ono.socket/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "public"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "network.connections",
        label: "connections",
        parent: Some("network"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Connection,
        member_type: Some(SpatialType::Connection),
        schema: Some("ono.socket/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "established"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "network.namespaces",
        label: "namespaces",
        parent: Some("network"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Namespace,
        member_type: Some(SpatialType::Namespace),
        schema: Some("ono.namespace/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "storage.filesystems",
        label: "filesystems",
        parent: Some("storage"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Filesystem,
        member_type: Some(SpatialType::Filesystem),
        schema: Some("ono.filesystem/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "used_share", "read_only"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "storage.mounts",
        label: "mounts",
        parent: Some("storage"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Mount,
        member_type: Some(SpatialType::Mount),
        schema: Some("ono.mount/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "read_only"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "storage.devices",
        label: "devices",
        parent: Some("storage"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::BlockDevice,
        member_type: Some(SpatialType::BlockDevice),
        schema: Some("ono.block-device/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "size"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "storage.directories",
        label: "directories",
        parent: Some("storage"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Directory,
        member_type: Some(SpatialType::Directory),
        schema: Some("ono.file/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["cwd", "roots"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "identity.users",
        label: "users",
        parent: Some("identity"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::User,
        member_type: Some(SpatialType::User),
        schema: Some("ono.user/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "human", "system"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "identity.groups",
        label: "groups",
        parent: Some("identity"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Group,
        member_type: Some(SpatialType::Group),
        schema: Some("ono.group/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count"],
        status: SpaceStatus::Stable,
    },
    CanonicalSpace {
        id: "identity.sessions",
        label: "sessions",
        parent: Some("identity"),
        kind: SpaceKind::Collection,
        object_type: SpatialType::Session,
        member_type: Some(SpatialType::Session),
        schema: Some("ono.session/1"),
        enterable: true,
        commands: &["look", "map", "near", "enter", "find place", "pin"],
        summary_fields: &["count", "active"],
        status: SpaceStatus::Stable,
    },
];

/// Every canonical space.
#[must_use]
pub fn spaces() -> &'static [CanonicalSpace] {
    SPACES
}

/// The space with this id, or `None`.
#[must_use]
pub fn space(id: &str) -> Option<&'static CanonicalSpace> {
    SPACES.iter().find(|space| space.id == id)
}

/// The root space (§7.1).
#[must_use]
pub fn root() -> &'static CanonicalSpace {
    &SYSTEM
}

/// The six canonical domains, in the order §4 draws them (§53).
pub fn domains() -> impl Iterator<Item = &'static CanonicalSpace> {
    SPACES.iter().filter(|space| space.is_domain())
}

/// The spaces whose canonical parent is `id`, in declaration order.
pub fn children(id: &str) -> impl Iterator<Item = &'static CanonicalSpace> {
    SPACES.iter().filter(move |space| space.parent == Some(id))
}

/// The collection space that holds objects of `object_type`, where one exists.
///
/// This is the fallback of the canonical-parent rule: an object with no operational parent still
/// belongs somewhere in the geography, so `up` from a process with no service arrives at
/// `compute.processes` rather than nowhere (§11.3).
#[must_use]
pub fn collection_for(object_type: SpatialType) -> Option<&'static CanonicalSpace> {
    SPACES
        .iter()
        .filter(|space| space.is_served())
        .find(|space| space.member_type == Some(object_type))
}
