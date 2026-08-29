//! Scopes and their boundaries (spec v0.4 §3.2, §2.18, §16.2, §35.4).
//!
//! "A scope defines the execution and discovery boundary to which an object belongs", scopes
//! nest, and "crossing a scope boundary MUST be observable in the navigation trail and
//! prompt/HUD". Detecting the crossing is this module's job; showing it is the renderer's.

use std::fmt;
use std::sync::Arc;

use crate::BootIdentity;

/// The kinds of boundary an object can belong to (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeKind {
    /// The local host.
    Host,
    /// Another host, reached through a link (§19).
    RemoteHost,
    /// A container-like scope (§16.1).
    Container,
    /// A kernel namespace (§16.2).
    Namespace,
    /// A filesystem, whose boundary the path tree crosses at a mount (§15.3).
    Filesystem,
    /// A user's own view of the system (§17).
    User,
    /// A space a KUANG/11 package contributes (§36.4).
    Plugin,
}

impl ScopeKind {
    /// Every kind, in the order §3.2 lists them.
    pub const ALL: &'static [ScopeKind] = &[
        ScopeKind::Host,
        ScopeKind::RemoteHost,
        ScopeKind::Container,
        ScopeKind::Namespace,
        ScopeKind::Filesystem,
        ScopeKind::User,
        ScopeKind::Plugin,
    ];

    /// The name `docs/spec/spatial/spatial.yaml` spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ScopeKind::Host => "host",
            ScopeKind::RemoteHost => "remote_host",
            ScopeKind::Container => "container",
            ScopeKind::Namespace => "namespace",
            ScopeKind::Filesystem => "filesystem",
            ScopeKind::User => "user",
            ScopeKind::Plugin => "plugin",
        }
    }

    /// The kind with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_str() == name)
    }

    /// Whether crossing into this kind of scope leaves the local host (§19, §35.4).
    #[must_use]
    pub fn is_remote(self) -> bool {
        matches!(self, ScopeKind::RemoteHost)
    }
}

impl fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The execution and discovery boundary an object belongs to (§3.2).
///
/// Scopes nest, outermost first, the way §3.2's own example does:
///
/// ```text
/// host:web01
///   -> container:payments-api
///       -> namespace:net:[4026533331]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpatialScope {
    kind: ScopeKind,
    id: Arc<str>,
    boot: Option<BootIdentity>,
    parent: Option<Arc<SpatialScope>>,
}

impl SpatialScope {
    /// The scope of the local host, with the boot identity every lifetime identity under it
    /// depends on (§10.2).
    #[must_use]
    pub fn host(hostname: &str, boot: BootIdentity) -> Self {
        Self {
            kind: ScopeKind::Host,
            id: hostname.into(),
            boot: Some(boot),
            parent: None,
        }
    }

    /// The scope of another host, reached through a link (§19.1).
    #[must_use]
    pub fn remote_host(hostname: &str, boot: BootIdentity) -> Self {
        Self {
            kind: ScopeKind::RemoteHost,
            id: hostname.into(),
            boot: Some(boot),
            parent: None,
        }
    }

    /// A scope of `kind` named `id`, nested inside this one.
    #[must_use]
    pub fn nest(&self, kind: ScopeKind, id: &str) -> Self {
        Self {
            kind,
            id: id.into(),
            boot: self.boot.clone(),
            parent: Some(Arc::new(self.clone())),
        }
    }

    /// The kind of boundary.
    #[must_use]
    pub fn kind(&self) -> ScopeKind {
        self.kind
    }

    /// The scope's identifier within its parent — a hostname, a container id, a namespace inode.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The scope this one is nested in, if any.
    #[must_use]
    pub fn parent(&self) -> Option<&SpatialScope> {
        self.parent.as_deref()
    }

    /// The boot identity of the host this scope ultimately belongs to (§10.2).
    #[must_use]
    pub fn boot(&self) -> Option<&BootIdentity> {
        self.boot.as_ref()
    }

    /// The host scope at the root of this chain.
    #[must_use]
    pub fn host_scope(&self) -> &SpatialScope {
        let mut scope = self;
        while let Some(parent) = scope.parent() {
            scope = parent;
        }
        scope
    }

    /// Whether the objects in this scope live on another host (§2.18, §35.4).
    #[must_use]
    pub fn is_remote(&self) -> bool {
        self.host_scope().kind.is_remote()
    }

    /// This scope and its ancestors, outermost first — §3.2's own rendering order.
    #[must_use]
    pub fn chain(&self) -> Vec<&SpatialScope> {
        let mut chain = Vec::new();
        let mut scope = Some(self);
        while let Some(current) = scope {
            chain.push(current);
            scope = current.parent();
        }
        chain.reverse();
        chain
    }

    /// Whether `other` is this scope or nested inside it.
    #[must_use]
    pub fn contains(&self, other: &SpatialScope) -> bool {
        other.chain().contains(&self)
    }

    /// The boundary a movement from this scope to `other` crosses, or `None` when the two are
    /// the same scope.
    ///
    /// The boundary reported is the *outermost* one that differs, because that is the one a user
    /// needs to know about: moving from a container into another container on another host has
    /// crossed a host boundary, and saying "container" would understate it (§2.18).
    #[must_use]
    pub fn boundary_to(&self, other: &SpatialScope) -> Option<ScopeBoundary> {
        if self == other {
            return None;
        }
        let here = self.chain();
        let there = other.chain();
        let shared = here
            .iter()
            .zip(there.iter())
            .take_while(|(a, b)| a == b)
            .count();
        // The first scope that differs, on whichever side still has one. Leaving a nested scope
        // for its own ancestor is a crossing too, and the kind that matters is the one being
        // left or entered at the outermost differing level.
        let entered = there.get(shared).copied();
        let left = here.get(shared).copied();
        let kind = entered.or(left).map(|scope| scope.kind)?;
        Some(ScopeBoundary {
            from: self.clone(),
            to: other.clone(),
            kind,
            entering: entered.is_some(),
        })
    }
}

impl fmt::Display for SpatialScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.id)
    }
}

/// A crossing between two scopes, recorded on the navigation step that crossed it (§20.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeBoundary {
    from: SpatialScope,
    to: SpatialScope,
    kind: ScopeKind,
    entering: bool,
}

impl ScopeBoundary {
    /// The scope left behind.
    #[must_use]
    pub fn from(&self) -> &SpatialScope {
        &self.from
    }

    /// The scope arrived in.
    #[must_use]
    pub fn to(&self) -> &SpatialScope {
        &self.to
    }

    /// The kind of boundary crossed — the outermost one that differs.
    #[must_use]
    pub fn kind(&self) -> ScopeKind {
        self.kind
    }

    /// Whether the movement entered a narrower scope rather than leaving one.
    #[must_use]
    pub fn is_entering(&self) -> bool {
        self.entering
    }

    /// Whether this crossing leaves the host the session started on (§2.18, §35.4).
    #[must_use]
    pub fn is_remote(&self) -> bool {
        self.from.is_remote() != self.to.is_remote()
            || self.from.host_scope() != self.to.host_scope()
    }
}

impl fmt::Display for ScopeBoundary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.from, self.to)
    }
}
