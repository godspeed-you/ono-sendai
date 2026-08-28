//! The canonical hierarchy and the canonical parent (spec v0.4 §11.1, §11.3, §6.6).
//!
//! §11.3: "A spatial object MAY have one canonical parent for `up` while participating in many
//! relationships. The canonical parent MUST be deterministic for a given view profile. The
//! canonical parent does not claim that other relationships are less real."
//!
//! Determinism is what this module provides. For each spatial type there is a fixed, ordered
//! list of the relations that may lead to a canonical parent; the first that has a neighbour
//! wins, and if none does, the object falls back to the collection space of the geography that
//! holds its type. `up` therefore never depends on the order edges happened to arrive in, and —
//! the property §43.2 asks for — it never traverses a graph edge that is not on that list.

use crate::{HierarchicalEdge, HierarchyKind, RelationshipEdge, SpatialId, SpatialType, space};

/// The canonical-parent step that is not a relationship: the enclosing directory of the Unix path
/// tree (§15.1, §3.4).
///
/// §3.4 lists "Directory -> child Directory" among the *hierarchical* edges, and §15.1 requires
/// Ono to preserve "canonical Unix filesystem paths and directory semantics". The path tree is
/// therefore hierarchy, not a relationship: no `RelationshipEdge` carries it, and no relation in
/// `relations.yaml` declares it. It appears in a rule chain under this reserved id, and the
/// caller supplies the parent it resolves to, because only the index knows which directories have
/// been observed.
pub const PATH_PARENT: &str = "path.parent";

/// One step a canonical parent may be reached by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentRule {
    /// The relation the step follows.
    pub relation: &'static str,
    /// Why the parent is the parent: containment, or canonical grouping (§3.4).
    pub kind: HierarchyKind,
}

/// The ordered canonical-parent rules of `object_type` (§11.3).
///
/// The order is the answer to "which of an object's many relationships is the one `up` means",
/// and it is deliberately short: a process belongs to its service before its container, because
/// §11.1's own path is SYSTEM → COMPUTE → SERVICES → nginx.service and §13 makes the service the
/// place that survives the process. Everything not on this list is a relationship, not a parent.
#[must_use]
pub fn parent_rules(object_type: SpatialType) -> &'static [ParentRule] {
    use HierarchyKind::{Containment, Grouping};
    const fn rule(relation: &'static str, kind: HierarchyKind) -> ParentRule {
        ParentRule { relation, kind }
    }
    const PROCESS: &[ParentRule] = &[
        rule("service.controls_process", Grouping),
        rule("container.contains_process", Containment),
    ];
    // §6.6 settles the sharp case in its own words: `up` from a socket "returns to the canonical
    // parent of the socket in the currently active map projection, normally `NETWORK/SOCKETS`,
    // not necessarily to the process", and §44.6 makes the distinction from `back` an acceptance
    // scenario. A socket therefore has no operational parent rule at all: it goes up its own
    // network collection, and the process that owns it stays a relationship `follow` traverses.
    const SOCKET: &[ParentRule] = &[];
    const CONNECTION: &[ParentRule] = &[];
    const ADDRESS: &[ParentRule] = &[rule("interface.has_address", Containment)];
    const MOUNT: &[ParentRule] = &[rule("filesystem.mounted_at", Containment)];
    const FILESYSTEM: &[ParentRule] = &[rule("device.backs_filesystem", Containment)];
    // §15.1 is unconditional — "Ono MUST preserve canonical Unix filesystem paths and directory
    // semantics" — so the path tree is walked first: the parent of `/mnt/backup` is `/mnt`, mount
    // point or not. The mount comes next, and is where a directory *root* ends up: `/` has no
    // path parent, so `up` from it reaches the mount that provides it and from there
    // MOUNTS -> FILESYSTEMS -> STORAGE, which is §15.2's hierarchy exactly. Crossing the mount
    // boundary stays discoverable where §3.2 and §15.3 put it — on the place, and on the
    // navigation step that crossed it (ADR-0187).
    const DIRECTORY: &[ParentRule] = &[
        rule(PATH_PARENT, Containment),
        rule("mount.backs_directory", Containment),
    ];
    const FILE: &[ParentRule] = &[rule(PATH_PARENT, Containment)];
    match object_type {
        SpatialType::Process => PROCESS,
        SpatialType::Socket | SpatialType::Listener => SOCKET,
        SpatialType::Connection => CONNECTION,
        SpatialType::Address => ADDRESS,
        SpatialType::Mount => MOUNT,
        SpatialType::Filesystem => FILESYSTEM,
        SpatialType::Directory => DIRECTORY,
        SpatialType::File => FILE,
        _ => &[],
    }
}

/// The canonical parent of an object, from the edges known about it (§11.3).
///
/// `edges` is every relationship edge that touches `subject`; the order it is given in does not
/// matter, which is what makes the result deterministic. Returns `None` only for an object that
/// is nowhere in the geography — the root, and the off-map endpoints of §42.3 — and that `None`
/// is what `up` reports as `spatial.no_parent` (§40).
#[must_use]
pub fn canonical_parent(
    subject: &SpatialId,
    object_type: SpatialType,
    edges: &[RelationshipEdge],
) -> Option<HierarchicalEdge> {
    canonical_parent_with(subject, object_type, edges, None)
}

/// The canonical parent, with the enclosing directory the caller knows about (§15.1).
///
/// `path_parent` is the place the Unix path tree leads to — the directory `/etc/nginx` for
/// `/etc/nginx/nginx.conf` — where the caller has observed it. It is consulted at exactly the
/// position [`PATH_PARENT`] holds in the type's rule chain, so a mount point still goes up to its
/// mount (§15.3) while an ordinary file goes up its own path.
///
/// Returns `None` only for an object that is nowhere in the geography — the root, and the off-map
/// endpoints of §42.3 — and that `None` is what `up` reports as `spatial.no_parent` (§40).
#[must_use]
pub fn canonical_parent_with(
    subject: &SpatialId,
    object_type: SpatialType,
    edges: &[RelationshipEdge],
    path_parent: Option<&SpatialId>,
) -> Option<HierarchicalEdge> {
    for rule in parent_rules(object_type) {
        let found = if rule.relation == PATH_PARENT {
            path_parent
        } else {
            edges
                .iter()
                .filter(|edge| edge.relation().as_str() == rule.relation)
                .find_map(|edge| edge.other_end(subject))
        };
        if let Some(parent) = found {
            return Some(HierarchicalEdge::new(
                parent.clone(),
                subject.clone(),
                rule.kind,
            ));
        }
    }
    // No operational parent: the object is filed under the collection of the geography that
    // holds its type, so `up` from a process with no service still arrives somewhere (§11.3).
    let collection = space::collection_for(object_type)?;
    Some(HierarchicalEdge::new(
        collection.spatial_id(),
        subject.clone(),
        HierarchyKind::Grouping,
    ))
}

/// The canonical parent of a canonical space (§11.1).
///
/// The root has none, which is what makes `up` from `home` a `spatial.no_parent` refusal rather
/// than a silent no-op (§40).
#[must_use]
pub fn parent_of_space(space_id: &str) -> Option<HierarchicalEdge> {
    let here = space::space(space_id)?;
    let parent = here.parent?;
    Some(HierarchicalEdge::new(
        SpatialId::of_space(parent),
        here.spatial_id(),
        HierarchyKind::Grouping,
    ))
}

/// The canonical path from the root down to `space_id`, outermost first (§11.1, §20.2).
///
/// This is the breadcrumb `web01 > compute > services > nginx.service` is built from, and it is
/// hierarchy only: no relationship edge can put a step in it.
#[must_use]
pub fn path_to_space(space_id: &str) -> Vec<&'static space::CanonicalSpace> {
    let mut path = Vec::new();
    let mut current = space::space(space_id);
    while let Some(here) = current {
        path.push(here);
        current = here.parent.and_then(space::space);
    }
    path.reverse();
    path
}
