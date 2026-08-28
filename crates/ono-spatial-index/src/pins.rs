//! Pins (spec v0.4 §20.4, §26.4, §46).
//!
//! "`pin` marks a place as a persistent user landmark. Pins MUST store a resilient selector and
//! identity metadata rather than only a rendered path. If the target cannot be resolved later,
//! the pin remains but reports unresolved state."
//!
//! Both halves matter. Storing only the [`SpatialId`] would break every pin the moment an object's
//! identity legitimately changed — a service moved into a container, a process restarted — and
//! storing only a rendered path would resolve to whatever happens to be at that path now, which
//! is worse. A pin therefore carries the id *and* the selector that found it, and reports
//! honestly when neither answers.

use std::collections::BTreeMap;

use jiff::Timestamp;
use ono_spatial_core::{SpatialId, SpatialType};

/// A place the user marked (§20.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    name: String,
    spatial_id: SpatialId,
    selector: String,
    object_type: SpatialType,
    scope: String,
    pinned_at: Timestamp,
}

impl Pin {
    /// A pin on `spatial_id`, found by `selector`.
    ///
    /// `selector` is the resilient half: the spelling that reached the place — `nginx.service`,
    /// `:443`, `/data` — so a pin can still be resolved after an identity change that a
    /// re-observation would produce.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        spatial_id: SpatialId,
        selector: impl Into<String>,
        object_type: SpatialType,
        scope: impl Into<String>,
        pinned_at: Timestamp,
    ) -> Self {
        Self {
            name: name.into(),
            spatial_id,
            selector: selector.into(),
            object_type,
            scope: scope.into(),
            pinned_at,
        }
    }

    /// The name `jump @name` takes (§20.4's `pin --name edge-proxy`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The identity the place had when it was pinned.
    #[must_use]
    pub fn spatial_id(&self) -> &SpatialId {
        &self.spatial_id
    }

    /// The spelling that found the place.
    #[must_use]
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// What kind of place it was — the identity metadata §20.4 asks for beside the selector.
    #[must_use]
    pub fn object_type(&self) -> SpatialType {
        self.object_type
    }

    /// The scope the place belonged to, rendered (§3.2).
    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// When it was pinned.
    #[must_use]
    pub fn pinned_at(&self) -> Timestamp {
        self.pinned_at
    }
}

/// What a pin resolved to (§20.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinResolution {
    /// The place the pin named is still there, under the identity it was pinned with.
    Resolved(SpatialId),
    /// The identity is gone but the pin's selector found the place again — a service that moved,
    /// a process that restarted. The pin is rewritten to the new identity by
    /// [`PinRegistry::rebind`], never silently.
    Rebound(SpatialId),
    /// Neither the identity nor the selector answers. "The pin remains but reports unresolved
    /// state" (§20.4) — it is not deleted, because a place that is unreachable today may be a
    /// host that is merely offline (§40's `spatial.remote_unavailable`).
    Unresolved,
}

/// The pins of one user (§46's `pins: PinRegistry`).
///
/// §46.1 allows pins to persist across sessions where the trail may not; this type holds them and
/// says nothing about where they are stored.
#[derive(Debug, Clone, Default)]
pub struct PinRegistry {
    pins: BTreeMap<String, Pin>,
}

impl PinRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a pin, returning the one it replaced.
    pub fn insert(&mut self, pin: Pin) -> Option<Pin> {
        self.pins.insert(pin.name.clone(), pin)
    }

    /// Removes the pin called `name` (`unpin`).
    pub fn remove(&mut self, name: &str) -> Option<Pin> {
        self.pins.remove(name)
    }

    /// The pin called `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Pin> {
        self.pins.get(name)
    }

    /// Every pin, by name.
    pub fn pins(&self) -> impl Iterator<Item = &Pin> {
        self.pins.values()
    }

    /// How many pins there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pins.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
    }

    /// Resolves a pin against what is known now (§20.4).
    ///
    /// `alive` says whether an id is still a place; `by_selector` re-runs the pin's own selector.
    /// The identity is tried first, because a pin that still points at the same object should not
    /// be re-resolved by name — §49.4 warns against name-first navigation for exactly this reason.
    #[must_use]
    pub fn resolve(
        &self,
        name: &str,
        alive: impl Fn(&SpatialId) -> bool,
        by_selector: impl Fn(&str, SpatialType) -> Option<SpatialId>,
    ) -> Option<PinResolution> {
        let pin = self.pins.get(name)?;
        if alive(&pin.spatial_id) {
            return Some(PinResolution::Resolved(pin.spatial_id.clone()));
        }
        Some(
            by_selector(&pin.selector, pin.object_type)
                .map_or(PinResolution::Unresolved, PinResolution::Rebound),
        )
    }

    /// Points the pin called `name` at `spatial_id`, after a [`PinResolution::Rebound`].
    ///
    /// Rebinding is explicit so that a pin's identity never changes as a side effect of being
    /// read: §20.4's contract is that the pin keeps reporting its state, not that it quietly
    /// follows whatever now answers to its selector.
    pub fn rebind(&mut self, name: &str, spatial_id: SpatialId) -> bool {
        match self.pins.get_mut(name) {
            Some(pin) => {
                pin.spatial_id = spatial_id;
                true
            }
            None => false,
        }
    }
}
