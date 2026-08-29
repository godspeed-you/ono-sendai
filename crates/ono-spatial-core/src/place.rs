//! Places (spec v0.4 §3.3).
//!
//! "A `Place` is the current spatial interpretation of a `SpatialObject` or a canonical aggregate
//! space." Both are places and both answer the same questions — what is this called, may I stand
//! in it, what is its identity — which is what lets `enter`, `look` and the trail treat the root,
//! a collection and a single process uniformly (§2.2, §2.3).

use crate::{CanonicalSpace, SpatialId, SpatialObject, SpatialScope, SpatialType, space};

/// Somewhere a user can stand (§3.3).
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    /// One of the canonical aggregate spaces: the root, a domain, a collection (§3.3, §4).
    Space(&'static CanonicalSpace),
    /// A specific object — a service, a process, a socket, a directory (§3.3).
    Object(Box<SpatialObject>),
}

impl Place {
    /// The root space every session starts at (§46.1).
    #[must_use]
    pub fn root() -> Self {
        Place::Space(space::root())
    }

    /// The canonical space with this id, or `None`.
    #[must_use]
    pub fn space(id: &str) -> Option<Self> {
        space::space(id).map(Place::Space)
    }

    /// A place that is one object.
    #[must_use]
    pub fn object(object: SpatialObject) -> Self {
        Place::Object(Box::new(object))
    }

    /// The place's identity (§3.1).
    #[must_use]
    pub fn spatial_id(&self) -> SpatialId {
        match self {
            Place::Space(space) => space.spatial_id(),
            Place::Object(object) => object.spatial_id().clone(),
        }
    }

    /// What kind of place it is.
    #[must_use]
    pub fn object_type(&self) -> SpatialType {
        match self {
            Place::Space(space) => space.object_type,
            Place::Object(object) => object.object_type(),
        }
    }

    /// What a person calls it. Not identity (§3.1).
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Place::Space(space) => space.label,
            Place::Object(object) => object.display_name(),
        }
    }

    /// Whether `enter` accepts it (§40's `spatial.not_enterable`).
    #[must_use]
    pub fn is_enterable(&self) -> bool {
        match self {
            Place::Space(space) => space.enterable,
            Place::Object(object) => object.is_enterable(),
        }
    }

    /// The boundary the place belongs to, for an object; a canonical space belongs to whichever
    /// host scope the session is in, which the session knows and the place does not (§46).
    #[must_use]
    pub fn scope(&self) -> Option<&SpatialScope> {
        match self {
            Place::Space(_) => None,
            Place::Object(object) => Some(object.scope()),
        }
    }

    /// The canonical space, when the place is one.
    #[must_use]
    pub fn as_space(&self) -> Option<&'static CanonicalSpace> {
        match self {
            Place::Space(space) => Some(space),
            Place::Object(_) => None,
        }
    }

    /// The object, when the place is one.
    #[must_use]
    pub fn as_object(&self) -> Option<&SpatialObject> {
        match self {
            Place::Space(_) => None,
            Place::Object(object) => Some(object),
        }
    }
}
