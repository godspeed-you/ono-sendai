//! Tombstones (spec v0.4 §10.3, §20.3, §53).
//!
//! "Recently removed objects MAY remain as short-lived tombstones in navigation history and live
//! maps." §10.3 also fixes the two rules that make them safe: a tombstone "MUST be visually
//! distinct" and "MUST NOT accept actions that require a live object". The second is enforceable
//! here, and is: [`Liveness`] has no variant that lets a tombstone stand in for a live object.
//!
//! §53 states the case they exist for: a restarted service tombstones the old process, the
//! service place remains, and the new process has a new identity.

use std::collections::BTreeMap;

use jiff::{Span, Timestamp};

use crate::{RelationType, SpatialId, SpatialType};

/// The record of an object that was there and is not (§10.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tombstone {
    spatial_id: SpatialId,
    object_type: SpatialType,
    display_name: String,
    removed_at: Timestamp,
    replacement: Option<SpatialId>,
    replacement_via: Option<RelationType>,
}

impl Tombstone {
    /// The tombstone of an object that ended at `removed_at`.
    #[must_use]
    pub fn new(
        spatial_id: SpatialId,
        object_type: SpatialType,
        display_name: impl Into<String>,
        removed_at: Timestamp,
    ) -> Self {
        Self {
            spatial_id,
            object_type,
            display_name: display_name.into(),
            removed_at,
            replacement: None,
            replacement_via: None,
        }
    }

    /// Records the object that took the old one's place, and the relation that identifies it.
    ///
    /// §10.2's continuity relation: `nginx.service` still stands, `process/1842` exited and
    /// `process/2198` is running. The replacement is a *candidate*, never a claim that the two
    /// are the same object — §53: "the new process has new identity".
    #[must_use]
    pub fn replaced_by(mut self, replacement: SpatialId, via: RelationType) -> Self {
        self.replacement = Some(replacement);
        self.replacement_via = Some(via);
        self
    }

    /// The identity of the object that is gone.
    #[must_use]
    pub fn spatial_id(&self) -> &SpatialId {
        &self.spatial_id
    }

    /// What kind of object it was.
    #[must_use]
    pub fn object_type(&self) -> SpatialType {
        self.object_type
    }

    /// What it was called.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// When it went away.
    #[must_use]
    pub fn removed_at(&self) -> Timestamp {
        self.removed_at
    }

    /// The replacement candidate, where one can be identified.
    #[must_use]
    pub fn replacement(&self) -> Option<&SpatialId> {
        self.replacement.as_ref()
    }

    /// The relation through which the replacement was identified (§10.2).
    #[must_use]
    pub fn replacement_via(&self) -> Option<&RelationType> {
        self.replacement_via.as_ref()
    }

    /// How long ago the object went away, as of `now`.
    #[must_use]
    pub fn age(&self, now: Timestamp) -> Span {
        now - self.removed_at
    }
}

/// Whether a place is still there (§20.3, §33.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Liveness {
    /// The object is there.
    Live,
    /// The object is gone and its tombstone is still held. Actions that need a live object refuse
    /// (§10.3); navigation may still arrive, and says what happened.
    Tombstoned(Tombstone),
    /// The object is gone and no tombstone remains: `spatial.destination_gone` with nothing more
    /// to say than that (§40).
    Gone,
}

impl Liveness {
    /// Whether an action that requires a live object may proceed (§10.3).
    #[must_use]
    pub fn accepts_actions(&self) -> bool {
        matches!(self, Liveness::Live)
    }

    /// Whether navigation may arrive here at all.
    ///
    /// A tombstone is a destination — that is the point of §20.3's "resolve a tombstone if
    /// available" — and a place with nothing left is not.
    #[must_use]
    pub fn is_reachable(&self) -> bool {
        !matches!(self, Liveness::Gone)
    }

    /// The tombstone, where there is one.
    #[must_use]
    pub fn tombstone(&self) -> Option<&Tombstone> {
        match self {
            Liveness::Tombstoned(tombstone) => Some(tombstone),
            _ => None,
        }
    }
}

/// The short-lived tombstones of one session (§10.3).
///
/// "Short-lived" is the contract: a tombstone that never expired would let a place come back from
/// the dead an hour after it went, which is the disorientation §10.3's Intent warns about. The
/// registry is therefore built with a lifetime and prunes against a clock the caller supplies —
/// no wall-clock reads here, so the behaviour is testable (AGENTS.md §11).
#[derive(Debug, Clone)]
pub struct TombstoneRegistry {
    lifetime: Span,
    entries: BTreeMap<SpatialId, Tombstone>,
}

impl TombstoneRegistry {
    /// A registry that keeps a tombstone for `lifetime` after the object went away.
    #[must_use]
    pub fn new(lifetime: Span) -> Self {
        Self {
            lifetime,
            entries: BTreeMap::new(),
        }
    }

    /// How long a tombstone is kept.
    #[must_use]
    pub fn lifetime(&self) -> Span {
        self.lifetime
    }

    /// Records that an object went away.
    pub fn record(&mut self, tombstone: Tombstone) {
        self.entries.insert(tombstone.spatial_id.clone(), tombstone);
    }

    /// The liveness of `id` as of `now`, given whether the providers still see it.
    ///
    /// `live` is the providers' answer, because §33.2 makes them the truth and this registry a
    /// cache of what is no longer there.
    #[must_use]
    pub fn resolve(&self, id: &SpatialId, live: bool, now: Timestamp) -> Liveness {
        if live {
            return Liveness::Live;
        }
        match self.entries.get(id) {
            Some(tombstone) if !self.has_expired(tombstone, now) => {
                Liveness::Tombstoned(tombstone.clone())
            }
            _ => Liveness::Gone,
        }
    }

    /// Whether this registry ever recorded that `id` went away — expired or not.
    ///
    /// [`TombstoneRegistry::resolve`] takes the providers' answer as its input, because §33.2
    /// makes them authoritative. A caller that has no fresh answer needs this instead: without
    /// it, an expired tombstone would read as a place nobody ever saw go, which is the opposite
    /// of what happened.
    #[must_use]
    pub fn recorded(&self, id: &SpatialId) -> bool {
        self.entries.contains_key(id)
    }

    /// Drops the tombstone of `id`, because a provider answered for it again.
    ///
    /// §33.2: the index is a cache and the providers are authoritative. An object that is there
    /// is there, whatever this registry remembers about an id that had gone quiet.
    pub fn forget(&mut self, id: &SpatialId) -> bool {
        self.entries.remove(id).is_some()
    }

    /// The tombstone of `id`, if one is still held as of `now`.
    #[must_use]
    pub fn get(&self, id: &SpatialId, now: Timestamp) -> Option<&Tombstone> {
        self.entries
            .get(id)
            .filter(|tombstone| !self.has_expired(tombstone, now))
    }

    /// Drops every tombstone older than the registry's lifetime.
    pub fn prune(&mut self, now: Timestamp) {
        let lifetime = self.lifetime;
        self.entries
            .retain(|_, tombstone| !expired(tombstone, lifetime, now));
    }

    /// The tombstones still held as of `now`, oldest first.
    pub fn entries(&self, now: Timestamp) -> impl Iterator<Item = &Tombstone> {
        self.entries
            .values()
            .filter(move |tombstone| !self.has_expired(tombstone, now))
    }

    fn has_expired(&self, tombstone: &Tombstone, now: Timestamp) -> bool {
        expired(tombstone, self.lifetime, now)
    }
}

fn expired(tombstone: &Tombstone, lifetime: Span, now: Timestamp) -> bool {
    tombstone
        .removed_at
        .checked_add(lifetime)
        .is_ok_and(|expiry| expiry <= now)
}
