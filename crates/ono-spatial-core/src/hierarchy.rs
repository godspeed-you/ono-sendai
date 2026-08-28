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
    const SOCKET: &[ParentRule] = &[rule("process.owns_socket", Containment)];
    const CONNECTION: &[ParentRule] = &[rule("socket.accepts_connection", Containment)];
    const ADDRESS: &[ParentRule] = &[rule("interface.has_address", Containment)];
    const MOUNT: &[ParentRule] = &[rule("filesystem.mounted_at", Containment)];
    const FILESYSTEM: &[ParentRule] = &[rule("device.backs_filesystem", Containment)];
    const PATH: &[ParentRule] = &[rule("mount.backs_directory", Containment)];
    match object_type {
        SpatialType::Process => PROCESS,
        SpatialType::Socket | SpatialType::Listener => SOCKET,
        SpatialType::Connection => CONNECTION,
        SpatialType::Address => ADDRESS,
        SpatialType::Mount => MOUNT,
        SpatialType::Filesystem => FILESYSTEM,
        SpatialType::Directory | SpatialType::File => PATH,
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
    for rule in parent_rules(object_type) {
        let found = edges
            .iter()
            .filter(|edge| edge.relation().as_str() == rule.relation)
            .find_map(|edge| edge.other_end(subject));
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
