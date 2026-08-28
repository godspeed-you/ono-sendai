//! The spatial discovery index of Ono-Sendai (spec v0.4 §33, §45.2).
//!
//! §45.2 gives this crate six responsibilities — registration and reconciliation, the alias and
//! search index, freshness state, canonical parent lookup, bounded relation summaries and pin
//! resolution — and one rule that governs all of them: **it MUST treat providers as truth and
//! revalidate mutation targets.**
//!
//! That rule is why nothing here observes anything. The index is fed [`SpatialObject`]s that a
//! provider produced and [`RelationshipEdge`]s that a provider asserted; it holds them, indexes
//! them, and says how old they are. When a mutation asks for a target,
//! [`SpatialIndex::resolve_for_action`] refuses a stale entry with `spatial.stale` rather than
//! answering from cache — because §33.2 says the providers are authoritative, and an index that
//! answered anyway would have made itself "an undocumented source of system truth" (§2.16).
//!
//! [`SpatialObject`]: ono_spatial_core::SpatialObject
//! [`RelationshipEdge`]: ono_spatial_core::RelationshipEdge

pub mod bridge;
pub mod freshness;
pub mod index;
pub mod pins;

pub use bridge::{Absorbed, ProviderBridge, carries_full_identity, spatial_type_of};
pub use freshness::FreshnessPolicy;
pub use index::{IndexEntry, Registration, SpatialIndex};
pub use pins::{Pin, PinRegistry, PinResolution};
