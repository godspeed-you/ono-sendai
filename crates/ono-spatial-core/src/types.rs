//! The vocabulary of spatial types (spec v0.4 §3.3, §36.1, §41.1).
//!
//! [`SpatialType::ALL`] is the **declared** vocabulary, and it is closed.
//! `docs/contracts/spatial/spatial.yaml` declares the same names under `object_types`, and
//! `cargo run -p xtask -- spec-check` holds the two together: a type the registry knows and this
//! enum does not is a space or relation nothing can serve, and a type this enum knows and the
//! registry does not is undocumented surface. Nothing here weakens that check.
//!
//! Beside it there is a second, open list. §36.1 lets a KUANG/11 package contribute "object
//! schemas that implement `SpatialObject`", and a schema a package declares cannot be in a
//! compile-time enum. Such a type arrives at runtime, through [`contribute`], and is a
//! [`SpatialType::Contributed`]. The two lists never mix: the declared one is what the drift
//! check compares against the contract, and the contributed one is what a session learned from
//! the packages it loaded. [`known`] is their union, which is what a command that has to accept
//! a type the user typed reads.

use std::fmt;

/// A spatial type: one of the seven canonical aggregate places of §3.3, or an object type that a
/// space contains or a relation connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SpatialType {
    /// The root aggregate, the host itself (§7.1).
    System,
    /// The COMPUTE domain (§7.2).
    Compute,
    /// The NETWORK domain (§7.3).
    Network,
    /// The STORAGE domain (§7.4).
    Storage,
    /// The CONTAINERS domain (§7.5).
    Containers,
    /// The IDENTITY domain (§7.6).
    Identity,
    /// The DEVICES domain (§7.7).
    Devices,
    /// A running process (§12).
    Process,
    /// A service-manager unit, stable across the lifetimes of its processes (§13).
    Service,
    /// A job of this shell's own job table (spec v0.2 §18.4).
    Job,
    /// A spatial aggregate over processes, services and containers (§7.2).
    Workload,
    /// A control group (§16.3).
    Cgroup,
    /// A container-like scope (§16.1).
    Container,
    /// A network interface (§14.2).
    Interface,
    /// An address configured on an interface (§14.1).
    Address,
    /// A route (§14.1).
    Route,
    /// A neighbour table entry (§14.1).
    Neighbor,
    /// A socket, as the object a process holds (§12).
    Socket,
    /// A listening socket, as a place (§14.3).
    Listener,
    /// An established connection, as a place (§14.4).
    Connection,
    /// A kernel namespace, which is also a scope boundary (§16.2).
    Namespace,
    /// The far end of a connection, which may be off this host (§11.2, §42.3).
    Endpoint,
    /// A filesystem (§15.2).
    Filesystem,
    /// A mount, which is a boundary of the path tree (§15.3).
    Mount,
    /// A block device behind a filesystem (§15.2).
    BlockDevice,
    /// A directory (§15.4).
    Directory,
    /// A file (§15.5).
    File,
    /// A kernel-visible device (§18).
    Device,
    /// A user (§17).
    User,
    /// A group (§17).
    Group,
    /// An active login session (§7.6).
    Session,
    /// A host, local or remote (§19).
    Host,
    /// A kind of place a KUANG/11 package contributed (§36.1).
    ///
    /// The name is the display name of the schema the package declared for it, interned by
    /// [`contribute`]; nothing outside that function may construct one, because a type nobody
    /// registered has no schema, no target and no identity field, and every part of the spatial
    /// layer that meets a contributed type reads one of those three.
    Contributed(&'static str),
}

impl SpatialType {
    /// Every **declared** type, in the order `docs/contracts/spatial/spatial.yaml` declares them.
    ///
    /// This is the closed list the contract drift check compares against, and a contributed type
    /// is deliberately not in it: a package cannot add a name to a document the gate holds the
    /// implementation to. [`known`] is the list a session actually answers for.
    pub const ALL: &'static [SpatialType] = &[
        SpatialType::System,
        SpatialType::Compute,
        SpatialType::Network,
        SpatialType::Storage,
        SpatialType::Containers,
        SpatialType::Identity,
        SpatialType::Devices,
        SpatialType::Process,
        SpatialType::Service,
        SpatialType::Job,
        SpatialType::Workload,
        SpatialType::Cgroup,
        SpatialType::Container,
        SpatialType::Interface,
        SpatialType::Address,
        SpatialType::Route,
        SpatialType::Neighbor,
        SpatialType::Socket,
        SpatialType::Listener,
        SpatialType::Connection,
        SpatialType::Namespace,
        SpatialType::Endpoint,
        SpatialType::Filesystem,
        SpatialType::Mount,
        SpatialType::BlockDevice,
        SpatialType::Directory,
        SpatialType::File,
        SpatialType::Device,
        SpatialType::User,
        SpatialType::Group,
        SpatialType::Session,
        SpatialType::Host,
    ];

    /// The seven canonical aggregate places of §3.3.
    pub const AGGREGATES: &'static [SpatialType] = &[
        SpatialType::System,
        SpatialType::Compute,
        SpatialType::Network,
        SpatialType::Storage,
        SpatialType::Containers,
        SpatialType::Identity,
        SpatialType::Devices,
    ];

    /// The name the registry spells, e.g. `Process`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SpatialType::System => "System",
            SpatialType::Compute => "Compute",
            SpatialType::Network => "Network",
            SpatialType::Storage => "Storage",
            SpatialType::Containers => "Containers",
            SpatialType::Identity => "Identity",
            SpatialType::Devices => "Devices",
            SpatialType::Process => "Process",
            SpatialType::Service => "Service",
            SpatialType::Job => "Job",
            SpatialType::Workload => "Workload",
            SpatialType::Cgroup => "Cgroup",
            SpatialType::Container => "Container",
            SpatialType::Interface => "Interface",
            SpatialType::Address => "Address",
            SpatialType::Route => "Route",
            SpatialType::Neighbor => "Neighbor",
            SpatialType::Socket => "Socket",
            SpatialType::Listener => "Listener",
            SpatialType::Connection => "Connection",
            SpatialType::Namespace => "Namespace",
            SpatialType::Endpoint => "Endpoint",
            SpatialType::Filesystem => "Filesystem",
            SpatialType::Mount => "Mount",
            SpatialType::BlockDevice => "BlockDevice",
            SpatialType::Directory => "Directory",
            SpatialType::File => "File",
            SpatialType::Device => "Device",
            SpatialType::User => "User",
            SpatialType::Group => "Group",
            SpatialType::Session => "Session",
            SpatialType::Host => "Host",
            SpatialType::Contributed(name) => name,
        }
    }

    /// The type with this name, declared or contributed, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.as_str() == name)
            .or_else(|| contributed_named(name).map(|entry| entry.object_type))
    }

    /// Whether the type is one of the seven canonical aggregate places (§3.3).
    #[must_use]
    pub fn is_aggregate(self) -> bool {
        Self::AGGREGATES.contains(&self)
    }

    /// The type this one is a special case of, where it is one (§14.3, §15.4).
    ///
    /// The relation table of §41.2 names the general type — `process.owns_socket` runs to a
    /// `Socket` — while §14.3 and §14.4 make the *places* a `Listener` and a `Connection`, and
    /// §15.4 and §15.5 do the same to `ono.file/1`. Rather than declaring one relation per
    /// specialisation, a specialised type answers to the relations of its general type, and
    /// nothing else: a `Directory` is a `File` for `process.opened_file`, a `File` is not a
    /// `Directory` for `mount.backs_directory`.
    #[must_use]
    pub const fn generalises_to(self) -> Option<SpatialType> {
        match self {
            SpatialType::Listener | SpatialType::Connection => Some(SpatialType::Socket),
            SpatialType::Directory => Some(SpatialType::File),
            _ => None,
        }
    }

    /// Whether an object of this type may stand where one of `other` is expected (§41.2).
    #[must_use]
    pub fn is_a(self, other: SpatialType) -> bool {
        let mut here = Some(self);
        while let Some(kind) = here {
            if kind == other {
                return true;
            }
            here = kind.generalises_to();
        }
        false
    }

    /// The identity tier a provider can honestly claim for this type (§10.1).
    ///
    /// This is the *ceiling*, not a promise: a provider may only claim a weaker tier, never a
    /// stronger one, and the renderer must not imply persistence beyond what the tier allows.
    #[must_use]
    pub const fn identity_tier(self) -> crate::IdentityTier {
        use crate::IdentityTier::{Lifetime, Observation, Stable};
        match self {
            // Conceptual identities that outlive any observation: a unit name, a filesystem
            // UUID, an interface's kernel identity, a uid, a host (§10.1 Tier A).
            SpatialType::System
            | SpatialType::Compute
            | SpatialType::Network
            | SpatialType::Storage
            | SpatialType::Containers
            | SpatialType::Identity
            | SpatialType::Devices
            | SpatialType::Service
            | SpatialType::Workload
            | SpatialType::Cgroup
            | SpatialType::Container
            | SpatialType::Interface
            | SpatialType::Filesystem
            | SpatialType::BlockDevice
            | SpatialType::Directory
            | SpatialType::File
            | SpatialType::Device
            | SpatialType::User
            | SpatialType::Group
            | SpatialType::Host => Stable,
            // Identities that are only as long-lived as the thing itself, and whose identifier
            // is reused afterwards: a pid, a socket inode, a connection tuple (§10.1 Tier B).
            SpatialType::Process
            | SpatialType::Job
            | SpatialType::Address
            | SpatialType::Route
            | SpatialType::Neighbor
            | SpatialType::Socket
            | SpatialType::Listener
            | SpatialType::Connection
            | SpatialType::Namespace
            | SpatialType::Mount
            | SpatialType::Session => Lifetime,
            // The far end of a connection is whatever was observed at the time; nothing about
            // it can be trusted to persist (§10.1 Tier C).
            SpatialType::Endpoint => Observation,
            // A package states the fields two observations are compared by; the *tier* is a claim
            // about how long the answer keeps holding, and it is the host's. §10.1 lets a tier
            // only be weakened, never strengthened, and the strongest thing a host can prove
            // about an external resource is that the identity lasts as long as the resource —
            // which is Tier B. A `metadata.uid` outliving its pod is the package's claim to make
            // in its own `identity_doc`, not one the shell may make on its behalf (§2.17).
            SpatialType::Contributed(_) => Lifetime,
        }
    }
}

impl fmt::Display for SpatialType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The spatial types a v0.2 provider target names (§42).
///
/// The provider registry of `docs/contracts/providers/` speaks the target vocabulary of spec v0.2
/// §8.1 — `process`, `socket`, `dir` — and the spatial layer speaks [`SpatialType`]. This table
/// is the join between them, and it is deliberately the *only* one: `docs/contracts/providers/*.yaml`
/// declares its §42 claims per spatial type, `spec-check` reads the claims through this
/// function, and the provider bridge decides an observed record's type through it too, so a
/// provider cannot claim for one type and serve another.
///
/// A target that names no spatial type — `env`, `package`, `dns`, `log` — yields the empty
/// slice: those objects are values in the typed shell, and §7 gives them no place.
///
/// One target may name more than one type. A `socket` is a [`SpatialType::Listener`] or a
/// [`SpatialType::Connection`] depending on its state (§14.3, §14.4), a `device` is a
/// [`SpatialType::BlockDevice`] or a [`SpatialType::Device`] depending on its kind (§7.4, §7.7),
/// and which one a given record is stays the bridge's decision from the record itself.
#[must_use]
pub fn types_of_target(target: &str) -> &'static [SpatialType] {
    use SpatialType as T;
    match target {
        "process" => &[T::Process],
        "service" => &[T::Service],
        "job" => &[T::Job],
        "container" => &[T::Container],
        "socket" => &[T::Listener, T::Connection],
        "connection" => &[T::Connection],
        "interface" => &[T::Interface],
        "route" => &[T::Route],
        "neighbor" => &[T::Neighbor],
        "filesystem" => &[T::Filesystem],
        "mount" => &[T::Mount],
        "device" => &[T::BlockDevice, T::Device],
        "dir" => &[T::Directory],
        "file" => &[T::File],
        "user" => &[T::User],
        "group" => &[T::Group],
        "session" => &[T::Session],
        "host" => &[T::Host],
        // A target a package contributed answers with one kind of place: the schema it declared
        // for it. The declared join above is consulted first, so a package naming a core target
        // extends what that target serves rather than redefining it (§31.23).
        other => contributed_target(other).map_or(&[][..], |entry| entry.types),
    }
}

// --- contributed types (spec v0.4 §36.1) -------------------------------------------------------

/// One kind of place a KUANG/11 package contributed, and everything the spatial layer needs to
/// treat it like a declared one (§36.1, §31.23, §31.64).
///
/// The shape mirrors [`crate::relation::Contributed`], for the same reason: a contribution is a
/// declaration the host records with its origin, never a replacement for the declared table
/// beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContributedType {
    /// The type itself — always a [`SpatialType::Contributed`].
    pub object_type: SpatialType,
    /// The id of the schema the package's records carry, e.g. `dev.example.echo.place/1`.
    pub schema: &'static str,
    /// The target the package contributes objects of this kind under.
    pub target: &'static str,
    /// The fields the schema calls its `identity` — what makes two observations one object
    /// (§3.1, §31.23's `identity_doc` beside the schema's `identity` list).
    pub identity: &'static [&'static str],
    /// The package that contributed it — §31.64: every registry entry records its origin.
    pub origin: &'static str,
    /// The semantic roles objects of this kind carry — `workload`, `storage` — as the package
    /// declared them (external-system-provider §25; ADR-0596). Additional semantics beside the
    /// type, never a replacement for it.
    pub roles: &'static [&'static str],
    /// The schema id of the kind of place that is this kind's canonical spatial parent, where
    /// the package declared one (spec v0.4 §11.3, §36.4; ADR-0597).
    pub parent: Option<&'static str>,
    /// A one-element slice holding [`Self::object_type`], so [`types_of_target`] can lend it out.
    types: &'static [SpatialType],
}

impl ContributedType {
    /// The field another record names one of these objects by — the first identity field.
    ///
    /// A reference is one value, and a schema whose identity is composite is named by the first
    /// of its parts wherever a single key is what a caller has (§42.3's reference table).
    #[must_use]
    pub fn reference_field(&self) -> &'static str {
        self.identity.first().copied().unwrap_or_default()
    }
}

/// The contributions packages have made this session.
fn contributions() -> &'static std::sync::RwLock<Vec<ContributedType>> {
    static CONTRIBUTED: std::sync::OnceLock<std::sync::RwLock<Vec<ContributedType>>> =
        std::sync::OnceLock::new();
    CONTRIBUTED.get_or_init(|| std::sync::RwLock::new(Vec::new()))
}

/// Records the kind of place a package's target answers with, and returns the type it is
/// (§36.1, §31.23).
///
/// The *kind* is the schema, not the target: a package that answers one schema under several
/// targets is offering several ways of asking about one kind of thing, and a record that arrives
/// through a pipe carries the schema and never the target it was asked for. So the type is
/// registered once per schema, and a second target naming that schema joins it.
///
/// The name is the schema's display name — `EchoPlace`, `Pod` — which is what a user types after
/// `--type` and what a place view prints. Where that name is already taken, by a declared type or
/// by another package's schema, the schema id is the name instead: two kinds of place answering
/// to one word would make `--type` a question with two answers (§27.2).
///
/// Registering the same schema twice is idempotent, so a reload costs nothing further. The names
/// are leaked once each, exactly as [`crate::relation::contribute`] leaks a relation's, and the
/// total is bounded by how many distinct schemas the installed packages declare.
pub fn contribute(
    schema: &str,
    schema_name: &str,
    target: &str,
    identity: &[&str],
    origin: &str,
    roles: &[&str],
    parent: Option<&str>,
) -> SpatialType {
    let leak = |text: &str| -> &'static str { Box::leak(text.to_owned().into_boxed_str()) };
    let Ok(mut registry) = contributions().write() else {
        // A poisoned registry is not a reason to invent a type: the caller sees the schema's own
        // name and the session simply does not place objects of it.
        return SpatialType::Contributed(leak(schema_name));
    };
    if let Some(known) = registry
        .iter()
        .find(|entry| entry.schema == schema && entry.target == target)
    {
        return known.object_type;
    }
    let object_type = match registry.iter().find(|entry| entry.schema == schema) {
        Some(known) => known.object_type,
        None => {
            let taken = SpatialType::ALL
                .iter()
                .any(|kind| kind.as_str().eq_ignore_ascii_case(schema_name))
                || registry
                    .iter()
                    .any(|entry| entry.object_type.as_str().eq_ignore_ascii_case(schema_name));
            SpatialType::Contributed(leak(if taken { schema } else { schema_name }))
        }
    };
    registry.push(ContributedType {
        object_type,
        schema: leak(schema),
        target: leak(target),
        identity: Box::leak(
            identity
                .iter()
                .map(|field| leak(field))
                .collect::<Vec<&'static str>>()
                .into_boxed_slice(),
        ),
        origin: leak(origin),
        roles: Box::leak(
            roles
                .iter()
                .map(|role| leak(role))
                .collect::<Vec<&'static str>>()
                .into_boxed_slice(),
        ),
        parent: parent.map(leak),
        types: Box::leak(Box::new([object_type])),
    });
    object_type
}

/// The semantic roles a kind of place carries, from every contribution registered for it
/// (ADR-0596). Empty for a declared type, which carries none yet.
#[must_use]
pub fn roles_of(object_type: SpatialType) -> Vec<&'static str> {
    let mut roles: Vec<&'static str> = contributed_types()
        .iter()
        .filter(|entry| entry.object_type == object_type)
        .flat_map(|entry| entry.roles.iter().copied())
        .collect();
    roles.sort_unstable();
    roles.dedup();
    roles
}

/// Every role some loaded package declared, so a `--role` nobody answers for can be refused with
/// the words that would have answered (ADR-0596).
#[must_use]
pub fn known_roles() -> Vec<&'static str> {
    let mut roles: Vec<&'static str> = contributed_types()
        .iter()
        .flat_map(|entry| entry.roles.iter().copied())
        .collect();
    roles.sort_unstable();
    roles.dedup();
    roles
}

/// The contribution behind a contributed kind of place, where there is one.
#[must_use]
pub fn contributed_for_type(object_type: SpatialType) -> Option<ContributedType> {
    contributions()
        .read()
        .ok()?
        .iter()
        .find(|entry| entry.object_type == object_type)
        .copied()
}

/// Every contributed kind of place, in the order the packages were mounted.
#[must_use]
pub fn contributed_types() -> Vec<ContributedType> {
    contributions()
        .read()
        .map(|registry| registry.clone())
        .unwrap_or_default()
}

/// The contribution a target belongs to, or `None` for a target no package contributed.
#[must_use]
pub fn contributed_target(target: &str) -> Option<ContributedType> {
    contributions()
        .read()
        .ok()?
        .iter()
        .find(|entry| entry.target == target)
        .copied()
}

/// The kind of place a schema's records are, where a package contributed one.
#[must_use]
pub fn contributed_for_schema(schema: &str) -> Option<ContributedType> {
    contributions()
        .read()
        .ok()?
        .iter()
        .find(|entry| entry.schema == schema)
        .copied()
}

/// The contribution answering to a type name, however it was spelled.
fn contributed_named(name: &str) -> Option<ContributedType> {
    contributions()
        .read()
        .ok()?
        .iter()
        .find(|entry| entry.object_type.as_str().eq_ignore_ascii_case(name.trim()))
        .copied()
}

/// The target a place of `object_type` is read from again, and the field another record names one
/// by — the contributed half of the shell's `target_of` (§33.2).
///
/// `None` where more than one target answers with the schema: they are several ways of asking one
/// question, and the host has nothing to choose between them by. Re-reading a live place through
/// the wrong one would report it gone (§2.17, §10.3).
#[must_use]
pub fn canonical_target_of(object_type: SpatialType) -> Option<(&'static str, &'static str)> {
    let registry = contributions().read().ok()?;
    let mut answering = registry
        .iter()
        .filter(|entry| entry.object_type == object_type);
    let only = answering.next()?;
    if answering.next().is_some() {
        return None;
    }
    Some((only.target, only.reference_field()))
}

/// Every type this session answers for: the declared vocabulary, then what packages contributed.
///
/// This is what a command that has to accept a type *the user typed* reads — `find place --type`,
/// `near --type`, the `<type>/<key>` selector of §11.2. [`SpatialType::ALL`] stays the closed
/// declared list the contract is checked against.
#[must_use]
pub fn known() -> Vec<SpatialType> {
    let mut types = SpatialType::ALL.to_vec();
    for entry in contributed_types() {
        if !types.contains(&entry.object_type) {
            types.push(entry.object_type);
        }
    }
    types
}
