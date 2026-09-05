//! The spatial data model of Ono-Sendai (spec v0.4 §3, §10, §11, §20, §45.1).
//!
//! v0.4 adds a second projection of the same system the typed shell already exposes: not a table
//! of rows but a place with exits. This crate is the model underneath it — identity, scopes,
//! places, the canonical geography, hierarchical and relationship edges, the navigation trail and
//! tombstones. §45.1 fixes its responsibilities and one prohibition: **it must not depend on
//! terminal rendering.** It does not; nothing here decides how anything looks.
//!
//! Three invariants of §2 shape almost every type in it:
//!
//! - **§2.6, hierarchy and graph are separate concepts.** [`HierarchicalEdge`] and
//!   [`RelationshipEdge`] are different types with no conversion between them, and
//!   [`hierarchy::canonical_parent`] reaches a parent only through the fixed rules of §11.3 —
//!   never through an arbitrary edge.
//! - **§2.8, stable identity beats transient identifiers.** A [`SpatialId`] is built from a
//!   [`SpatialIdentity`] whose components are the facts that make the object that object; a pid
//!   alone is never one of them ([`ProcessIdentity`], §10.2).
//! - **§2.17, unknown is visible.** [`neighborhood::PermissionState`] keeps "denied" apart from
//!   "empty", [`neighborhood::Freshness`] keeps "never observed" apart from "fresh", and a
//!   [`BootIdentity`] says when it does not know which boot it names.
//!
//! The canonical geography and the relation vocabulary are the same ones
//! `docs/contracts/spatial/spaces.yaml` and `relations.yaml` declare; `cargo run -p xtask --
//! spec-check` fails when the registry and this crate disagree in either direction.

pub mod edge;
pub mod hierarchy;
pub mod id;
pub mod landmark;
pub mod neighborhood;
pub mod object;
pub mod place;
pub mod relation;
pub mod scope;
pub mod space;
pub mod tombstone;
pub mod trail;
pub mod types;

pub use edge::{EdgeId, HierarchicalEdge, HierarchyKind, RelationshipEdge, ValidityWindow};
pub use hierarchy::{
    PATH_PARENT, ParentRule, canonical_parent, canonical_parent_with, parent_of_space,
    parent_rules, path_to_space,
};
pub use id::{BootIdentity, IdentityTier, ProcessIdentity, SpatialId, SpatialIdentity};
pub use landmark::{Landmark, LandmarkReason, LandmarkSource};
pub use neighborhood::{Completeness, Freshness, Neighborhood, NeighborhoodGroup, PermissionState};
pub use object::{
    LifetimeDescriptor, Projection, SpatialCapability, SpatialObject, aliases_of, spatial_types_of,
};
pub use place::Place;
pub use relation::AcquisitionCost;
pub use relation::{
    Confidence, ConfidenceClaim, CostClass, Direction, RelationSpec, RelationType, relations,
};
pub use scope::{ScopeBoundary, ScopeKind, SpatialScope};
pub use space::{CanonicalSpace, ROOT, SpaceKind, SpaceStatus, spaces};
pub use tombstone::{Liveness, Tombstone, TombstoneRegistry};
pub use trail::{BackOutcome, Movement, NavigationStep, NavigationTrail};
pub use types::{SpatialType, types_of_target};
