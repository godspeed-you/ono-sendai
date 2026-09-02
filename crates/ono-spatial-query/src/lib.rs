//! Spatial query planning for Ono-Sendai (spec v0.4 §27, §3.6, §6.8, §9, §34, §45.3).
//!
//! §45.3 gives this crate the planning half of the spatial subsystem: `look` plans, neighborhood
//! ranking, semantic zoom, map graph selection, `find` resolution, cluster construction and
//! cost-aware lazy queries. It reads the index (§45.2) and decides *what to show* and *in which
//! order* — never *what is true*, which is the providers' (§2.16), and never *what it looks
//! like*, which is the renderer's (§45.4).
//!
//! Three rules run through everything here:
//!
//! - **Bounded and ranked, never "all adjacent nodes"** (§3.6). Every projection carries the
//!   `hidden_count` and the `completeness` of what it left out, so a bound is visible rather than
//!   silent (§2.17).
//! - **A refusal outranks a count.** A [`NeighborhoodGroup`] the index marked withheld reaches the
//!   caller with its §35.2 state and no total; nothing here substitutes a number for it (§42.4).
//! - **Deterministic** (§29.3). Every ranking ends in a total order, so the same index answers the
//!   same question the same way in a script.
//!
//! [`NeighborhoodGroup`]: ono_spatial_core::NeighborhoodGroup

pub mod cost;
pub mod discovery;
pub mod find;
pub mod landmark;
pub mod map;
pub mod neighborhood;
pub mod place;
pub mod resolve;

pub use cost::{CostEstimate, INTERACTIVE_BUDGET};
pub use discovery::acquisition_of_target;
pub use discovery::{SpaceSource, TargetPlan, source_of_space, targets_for};
pub use find::{FindRequest, FoundPlace, find_places};
pub use landmark::{LandmarkThresholds, landmarks_of as landmarks_of_object};
pub use map::{
    EdgeKind, HiddenSummary, HorizonPlace, MAP_NODE_BUDGET, MapCluster, MapEdge, MapHorizon,
    MapNode, MapRequest, SpatialMap, TEXT_MAP_BUDGET, project as project_map,
};
pub use neighborhood::{NeighborhoodRequest, neighborhood_of};
pub use place::{Exit, VIEW_BUDGET, declared_children, space_neighborhood};
pub use resolve::{Candidate, Resolution, ResolutionStep, SelectorContext, place_path, resolve};
