//! How current an answer is, and how long each object class stays current (spec v0.4 §33.3, §33.4).
//!
//! §33.2 fixes the relationship this module exists to serve: "The index is a cache. Providers
//! remain authoritative." A TTL is therefore not a claim that an object has changed — it is the
//! point past which the index stops pretending to know, which is what §2.17 asks for and what
//! §33.2's "Actions MUST resolve/revalidate live objects before mutation" is enforced against.

use std::collections::BTreeMap;

use jiff::{Span, Timestamp};
use ono_spatial_core::{Freshness, SpatialType};

/// How many lifetimes an object nobody answers for again is kept: one to be stale in and one to
/// be found in by a `back` or a `trail` that arrives late (§20.1, §33.3).
const DEFAULT_RETENTION: i64 = 2;

/// How long an observation of each object class stays fresh (§33.3).
///
/// §33.3 calls its numbers "implementation defaults ... MAY be tuned without changing
/// semantics", so they are data rather than constants in a match: a caller with a live
/// subscription for a class shortens nothing and lengthens nothing, it simply reports
/// [`Freshness::Live`].
#[derive(Debug, Clone)]
pub struct FreshnessPolicy {
    ttls: BTreeMap<SpatialType, Span>,
    default: Span,
    retention: i64,
}

impl FreshnessPolicy {
    /// The starting points §33.3 recommends, for a passive view.
    ///
    /// Processes and connections are the volatile ones; interfaces, mounts and services change
    /// rarely and are event-driven where a provider offers events, with these as the fallback.
    #[must_use]
    pub fn recommended() -> Self {
        let seconds = |count: i64| Span::new().seconds(count);
        let ttls = [
            (SpatialType::Process, seconds(5)),
            (SpatialType::Connection, seconds(5)),
            (SpatialType::Socket, seconds(5)),
            (SpatialType::Listener, seconds(5)),
            (SpatialType::Service, seconds(5)),
            (SpatialType::Job, seconds(5)),
            (SpatialType::Interface, seconds(10)),
            (SpatialType::Address, seconds(10)),
            (SpatialType::Route, seconds(10)),
            (SpatialType::Neighbor, seconds(10)),
            (SpatialType::Mount, seconds(10)),
            (SpatialType::Filesystem, seconds(10)),
            (SpatialType::Container, seconds(10)),
            (SpatialType::User, seconds(30)),
            (SpatialType::Group, seconds(30)),
            (SpatialType::Session, seconds(30)),
        ]
        .into_iter()
        .collect();
        Self {
            ttls,
            default: seconds(30),
            retention: DEFAULT_RETENTION,
        }
    }

    /// A policy with one lifetime for every class — the shape a test or a script wants.
    #[must_use]
    pub fn uniform(ttl: Span) -> Self {
        Self {
            ttls: BTreeMap::new(),
            default: ttl,
            retention: DEFAULT_RETENTION,
        }
    }

    /// Overrides the lifetime of one class.
    #[must_use]
    pub fn with_ttl(mut self, object_type: SpatialType, ttl: Span) -> Self {
        self.ttls.insert(object_type, ttl);
        self
    }

    /// Sets how many lifetimes an unanswered observation is kept for before it is forgotten.
    ///
    /// A multiple below one would forget an object the index still calls fresh, so one is the
    /// floor.
    #[must_use]
    pub fn with_retention(mut self, lifetimes: i64) -> Self {
        self.retention = lifetimes.max(1);
        self
    }

    /// How long an observation of `object_type` stays fresh.
    #[must_use]
    pub fn ttl(&self, object_type: SpatialType) -> Span {
        self.ttls.get(&object_type).copied().unwrap_or(self.default)
    }

    /// How long an observation of `object_type` is kept before the index forgets it (§33.2).
    ///
    /// A TTL says when the index stops *trusting* what it holds; a retention says when it stops
    /// *holding* it. They are different questions: a place the session walked away from a moment
    /// ago is stale and still worth keeping, and the same place an hour later is a claim about a
    /// moment that has passed. Keeping every such claim is what turns a cache into an
    /// accumulator, and §34.2 forbids the unbounded growth that follows.
    #[must_use]
    pub fn retention(&self, object_type: SpatialType) -> Span {
        let ttl = self.ttl(object_type);
        ttl.checked_mul(self.retention).unwrap_or(ttl)
    }

    /// The freshness of an observation of `object_type` made at `observed_at`, as of `now`.
    ///
    /// `subscribed` says whether a provider subscription is delivering changes for the object; if
    /// it is, the value is current by construction and no TTL applies (§33.3's "event-driven").
    #[must_use]
    pub fn freshness(
        &self,
        object_type: SpatialType,
        observed_at: Option<Timestamp>,
        subscribed: bool,
        now: Timestamp,
    ) -> Freshness {
        if subscribed {
            return Freshness::Live;
        }
        // §2.17: never observed is not fresh. Guessing would make `spatial.stale` unreachable
        // for exactly the objects that most need it.
        let Some(observed_at) = observed_at else {
            return Freshness::Unknown;
        };
        match observed_at.checked_add(self.ttl(object_type)) {
            Ok(expiry) if expiry > now => Freshness::Fresh,
            Ok(_) => Freshness::Stale,
            Err(_) => Freshness::Unknown,
        }
    }
}

impl Default for FreshnessPolicy {
    fn default() -> Self {
        Self::recommended()
    }
}
