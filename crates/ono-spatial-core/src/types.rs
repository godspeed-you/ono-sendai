//! The closed vocabulary of spatial types (spec v0.4 §3.3, §41.1).
//!
//! `docs/spec/spatial/spatial.yaml` declares the same names under `object_types`, and
//! `cargo run -p xtask -- spec-check` holds the two together: a type the registry knows and this
//! enum does not is a space or relation nothing can serve, and a type this enum knows and the
//! registry does not is undocumented surface.

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
}

impl SpatialType {
    /// Every type, in the order `docs/spec/spatial/spatial.yaml` declares them.
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
        }
    }

    /// The type with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_str() == name)
    }

    /// Whether the type is one of the seven canonical aggregate places (§3.3).
    #[must_use]
    pub fn is_aggregate(self) -> bool {
        Self::AGGREGATES.contains(&self)
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
        }
    }
}

impl fmt::Display for SpatialType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
