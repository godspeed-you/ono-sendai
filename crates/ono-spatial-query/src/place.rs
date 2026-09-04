//! The neighborhood of a canonical place (spec v0.4 §4, §7, §24.2, §34.2, §45.3).
//!
//! [`neighborhood_of`] projects what surrounds an *observed object*: the index knows its edges,
//! and the ranking decides which of them to show. A canonical space has no edges — it is declared
//! geography rather than an observed object (§4.1) — so its neighborhood is built here, from the
//! same two ingredients every place view has: the exits, and what lies behind each of them.
//!
//! One rule fixes the whole shape, and it is §24.2's:
//!
//! > When `look` displays `children 14`, those group labels MUST be valid navigation or query
//! > targets where practical: `enter children`.
//!
//! So an exit is named after the place it leads into, and its members are the places *inside*
//! that place. At the root the exits are the six domains and their members are the collections
//! each domain holds; at COMPUTE the exits are `processes`, `services`, `jobs` and `cgroups` and
//! their members are the processes, the services, the jobs; at `compute.processes` the one exit
//! is the collection's own contents. A count is therefore always the number of members, and the
//! `hidden_count` of §3.6 is always the number the budget left out — there is no second meaning
//! for either (ADR-0143).
//!
//! [`neighborhood_of`]: crate::neighborhood::neighborhood_of

use jiff::Timestamp;
use ono_spatial_core::{
    Freshness, Neighborhood, NeighborhoodGroup, PermissionState, SpatialId, space,
};
use ono_spatial_index::{PinRegistry, SpatialIndex};

use crate::neighborhood::{NeighborhoodRequest, keeps_member, rank_of};

/// How many places one view of a canonical space shows before it starts counting instead (§34.2).
///
/// §34.2 budgets the *view*, not the group: "interactive map 100 nodes before mandatory
/// clustering", which §47 spells as `spatial.map.node_budget = 100`. A place with one exit spends
/// the budget on that exit — standing in `identity/users` and being shown eight of forty accounts
/// would be a list that hides its subject — and a place with six exits divides it, which is what
/// keeps the root horizon bounded however many devices a host has (§7.1, §2.9).
pub const VIEW_BUDGET: usize = 100;

/// One exit of a canonical place: where it goes, and what the shell could learn about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Exit {
    label: String,
    members: Vec<SpatialId>,
    state: PermissionState,
    detail: Option<String>,
    /// How many places lie behind the exit, when that is more than the members it carries.
    ///
    /// `None` — the ordinary case — means the members *are* the population, and the count is
    /// theirs. `Some(n)` is the figure a provider stated beside a bounded answer: the shell read
    /// fewer objects than exist and was told how many exist, so the count it shows stays true
    /// while the work it did stays bounded (v0.4.1 §34.4, §2.17; ADR-0576).
    population: Option<usize>,
    /// Whether the members are all of them, or as many as the orientation stopped at.
    ///
    /// A bounded read whose provider stated no population has no count at all: the members are a
    /// sample, and reporting how big the sample is as though it were the collection is the
    /// fabricated figure §2.17 forbids. `count` is then null and the group says why (§42.4).
    bounded: bool,
}

impl Exit {
    /// An exit whose members were read.
    #[must_use]
    pub fn open(label: impl Into<String>, members: Vec<SpatialId>) -> Self {
        let state = if members.is_empty() {
            PermissionState::Empty
        } else {
            PermissionState::Available
        };
        Self {
            label: label.into(),
            members,
            state,
            detail: None,
            population: None,
            bounded: false,
        }
    }

    /// An exit whose members are a bounded read of a collection of `population` places.
    ///
    /// The members are what the shell holds and can rank; the population is what the count says.
    /// §34.4 permits the bounded read — "a local neighborhood query SHOULD NOT require
    /// construction of the complete system graph when provider APIs can answer the neighborhood
    /// incrementally" — and §2.17 requires the difference to be visible rather than absorbed.
    #[must_use]
    pub fn sampled(
        label: impl Into<String>,
        members: Vec<SpatialId>,
        population: Option<usize>,
    ) -> Self {
        let mut exit = Self::open(label, members);
        exit.bounded = true;
        exit.population = population.filter(|population| *population > exit.members.len());
        exit
    }

    /// An exit that is there, and whose contents could not be read (§4, §35.2).
    ///
    /// The place stays enterable — an unavailable domain "remains visible" rather than
    /// disappearing — and it carries the reason instead of a count, because `0` would be the
    /// claim that there is nothing there (§2.17, §42.4).
    #[must_use]
    pub fn withheld(
        label: impl Into<String>,
        state: PermissionState,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            members: Vec::new(),
            state,
            detail: Some(detail.into()),
            population: None,
            bounded: false,
        }
    }

    /// The word a user types to take the exit (§24.2).
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// What lies behind it.
    #[must_use]
    pub fn members(&self) -> &[SpatialId] {
        &self.members
    }

    /// What the user was told about what lies behind it (§35.2).
    #[must_use]
    pub fn state(&self) -> PermissionState {
        self.state
    }

    /// How many places lie behind it, where that is more than it carries (§34.4).
    #[must_use]
    pub const fn population(&self) -> Option<usize> {
        self.population
    }
}

/// The bounded, ranked projection around a canonical place (§3.6 for a space of §7).
///
/// The exits are the caller's, because only the shell can ask a provider anything (§2.16, §45.6);
/// the filtering, the ranking, the bound and the honesty about what was left out are here.
#[must_use]
pub fn space_neighborhood(
    index: &SpatialIndex,
    center: &SpatialId,
    exits: Vec<Exit>,
    request: &NeighborhoodRequest,
    pins: &PinRegistry,
    now: Timestamp,
) -> Neighborhood {
    let kept: Vec<Exit> = exits
        .into_iter()
        .filter(|exit| request.keeps_relation(&exit.label))
        .collect();
    let budget = budget_for(request, kept.len());
    let pinned: Vec<&SpatialId> = pins
        .pins()
        .map(ono_spatial_index::Pin::spatial_id)
        .collect();

    let groups: Vec<NeighborhoodGroup> = kept
        .into_iter()
        .map(|exit| bound(index, exit, request, &pinned, budget, now))
        .collect();
    let landmarks = crate::neighborhood::landmarks_of(index, center, &groups, &pinned);
    Neighborhood::new(center.clone(), groups, now).with_landmarks(landmarks)
}

/// The places one canonical space holds without asking a provider anything (§7).
///
/// A domain that holds only other places — COMPUTE, NETWORK, STORAGE, IDENTITY — has its served
/// collections behind it, and they are declared rather than observed, so listing them costs
/// nothing and can never fail.
#[must_use]
pub fn declared_children(space_id: &str) -> Vec<SpatialId> {
    space::children(space_id)
        .filter(|child| child.is_served())
        .map(|child| child.spatial_id())
        .collect()
}

/// How many members one exit may list.
///
/// `--all` lifts the bound, `--limit` is the user's own number, and otherwise the view budget of
/// §34.2 is divided among the exits — never below [`DEFAULT_GROUP_BUDGET`], so a place with many
/// exits still shows something behind each of them.
///
/// [`DEFAULT_GROUP_BUDGET`]: crate::neighborhood::DEFAULT_GROUP_BUDGET
fn budget_for(request: &NeighborhoodRequest, exits: usize) -> usize {
    if request.is_complete() {
        return usize::MAX;
    }
    if let Some(limit) = request.stated_limit() {
        return limit;
    }
    (VIEW_BUDGET / exits.max(1)).max(crate::neighborhood::DEFAULT_GROUP_BUDGET)
}

/// Whether a member survives the request's filters.
///
/// A canonical space is declared rather than observed: it answers a `--type` filter from the
/// geography, and it is never a *recent change*, because nothing observed it changing (§4.1).
fn keeps(
    index: &SpatialIndex,
    id: &SpatialId,
    request: &NeighborhoodRequest,
    now: Timestamp,
) -> bool {
    match crate::resolve::space_of(id) {
        Some(space) => request.accepts_type(space.object_type) && !request.wants_changed(),
        None => keeps_member(index, id, request, now),
    }
}

/// Ranks an exit's members, keeps the first `budget` of them and counts the rest.
fn bound(
    index: &SpatialIndex,
    exit: Exit,
    request: &NeighborhoodRequest,
    pinned: &[&SpatialId],
    budget: usize,
    now: Timestamp,
) -> NeighborhoodGroup {
    let Exit {
        label,
        members,
        state,
        detail,
        population,
        bounded,
    } = exit;
    if !state.is_complete() {
        // Nothing was read, so there is nothing to rank and no total a budget could bound. The
        // state travels to the caller, and `None` is not zero (§2.17, §42.4).
        let mut group = NeighborhoodGroup::reported(label, state, Vec::new(), None);
        if let Some(detail) = detail {
            group = group.explained(detail);
        }
        return group.observed(Freshness::Unknown);
    }

    let held = members.len();
    let mut members: Vec<SpatialId> = members
        .into_iter()
        .filter(|id| keeps(index, id, request, now))
        .collect();
    // The stated population is a figure about the whole collection, so it survives a bound and
    // not a filter: once `--type`, `--changed` or a predicate has refused one of the members the
    // shell holds, nobody knows how many of the ones it never read would have been refused too.
    // Then the count is what is really in hand, which is the honest smaller claim (§2.17).
    let total = match population {
        Some(population) if members.len() == held => Some(population),
        // A bounded read nobody counted: the members are a sample, and their number is a fact
        // about the reading rather than about the place (§2.17, §42.4).
        None if bounded && members.len() == held => None,
        _ => Some(members.len()),
    };
    // Declared geography keeps the order §4 and §41.1 declare it in — the six domains are drawn
    // in one order everywhere, and `processes, services, jobs, cgroups` is COMPUTE's own list.
    // Observed objects have no such order, so they are ranked (§3.6).
    if !members
        .iter()
        .all(|id| crate::resolve::space_of(id).is_some())
    {
        members.sort_by_cached_key(|id| rank_of(index, id, pinned));
    }
    members.truncate(budget);
    let group = NeighborhoodGroup::reported(label, PermissionState::Available, members, total);
    let group = if total.is_none() {
        group.explained("read as far as the orientation bound; the whole count is not known")
    } else {
        group
    };
    group.observed(Freshness::Fresh)
}
