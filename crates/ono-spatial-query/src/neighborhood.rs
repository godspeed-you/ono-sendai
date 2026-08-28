//! Neighborhood ranking (spec v0.4 §3.6, §6.2, §8.2, §32.2, §34.2, §35.2).
//!
//! §3.6: "A neighborhood is a bounded, ranked projection of objects and relationships around the
//! current place. It is not simply 'all adjacent nodes'." The index answers what is adjacent
//! (§45.2); this module decides what of it is *shown*, in which order, and says what it left out.
//!
//! §3.6 lists what generation MUST consider, and each one has a place here:
//!
//! | §3.6 input | How it is considered |
//! |---|---|
//! | relationship relevance | groups keep the declared order of `relations.yaml`, and a cheap relation outranks an expensive one (§32.1) |
//! | object importance | a landmark the index holds ranks its member first, and a pin outranks every heuristic (§26.4) |
//! | recent change | a more recently observed member ranks above an older one |
//! | current view purpose | [`NeighborhoodRequest`] carries the purpose: one relation, one type, the changed window |
//! | user filters | `--type`, `--changed`, `<relation>` and `--all` are the filters, and each narrows before the bound applies |
//! | terminal size | the member budget follows the terminal's height where the caller states one (§34.2) |
//! | security and permission boundaries | a withheld group keeps its §35.2 state and is never counted, filtered or bounded away (§42.4) |
//!
//! The last row is the one that constrains everything else: a refused group has no total (§2.17),
//! so it can neither be ranked by size nor hidden by a budget, and it survives every filter that
//! is not about it.

use jiff::{Span, Timestamp};
use ono_spatial_core::{
    CostClass, Landmark, LandmarkReason, Neighborhood, NeighborhoodGroup, SpatialId, SpatialType,
    relation,
};
use ono_spatial_index::{PinRegistry, SpatialIndex};

/// The default number of members a group lists before the rest are counted (§34.2).
///
/// §34.2 budgets the *view* at roughly thirty nodes for a text map; a place view spends that
/// across its groups, so a group shows a handful and says how many more there are. `--all`
/// removes the bound, which §6.2 says "MAY be expensive".
pub const DEFAULT_GROUP_BUDGET: usize = 8;

/// What a caller wants to see around a place (§6.2's five options, §3.6's "current view purpose"
/// and "user filters").
#[derive(Debug, Clone, Default)]
pub struct NeighborhoodRequest {
    relation: Option<String>,
    object_type: Option<SpatialType>,
    changed_within: Option<Span>,
    limit: Option<usize>,
    all: bool,
    terminal_rows: Option<usize>,
}

impl NeighborhoodRequest {
    /// The default view: every relation, bounded.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `near <relation>` — one exit only (§6.2).
    #[must_use]
    pub fn along(mut self, relation: impl Into<String>) -> Self {
        self.relation = Some(relation.into());
        self
    }

    /// `near --type <type>` (§6.2).
    #[must_use]
    pub fn of_type(mut self, object_type: SpatialType) -> Self {
        self.object_type = Some(object_type);
        self
    }

    /// `near --changed [duration]` (§6.2, §24.3).
    #[must_use]
    pub fn changed_within(mut self, window: Span) -> Self {
        self.changed_within = Some(window);
        self
    }

    /// Whether a place of this type survives the `--type` filter (§6.2).
    #[must_use]
    pub fn accepts_type(&self, object_type: SpatialType) -> bool {
        self.object_type
            .is_none_or(|wanted| object_type.is_a(wanted))
    }

    /// Whether the caller asked for recently changed neighbours only (§6.2's `--changed`).
    #[must_use]
    pub fn wants_changed(&self) -> bool {
        self.changed_within.is_some()
    }

    /// `near --limit <n>` (§6.2).
    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// `near --all` — "the complete currently known one-hop neighborhood" (§6.2).
    #[must_use]
    pub fn all(mut self, all: bool) -> Self {
        self.all = all;
        self
    }

    /// How tall the terminal is, where the caller knows (§3.6's "terminal size").
    #[must_use]
    pub fn in_terminal_rows(mut self, rows: usize) -> Self {
        self.terminal_rows = Some(rows);
        self
    }

    /// Whether an exit labelled `label` survives `near <relation>` (§6.2).
    #[must_use]
    pub fn keeps_relation(&self, label: &str) -> bool {
        self.relation.as_ref().is_none_or(|wanted| wanted == label)
    }

    /// Whether the caller asked for the complete neighbourhood (`--all`, §6.2).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.all
    }

    /// The bound the caller stated with `--limit`, where they stated one (§6.2).
    #[must_use]
    pub fn stated_limit(&self) -> Option<usize> {
        self.limit
    }

    /// How many members one group may list.
    ///
    /// `--limit` is the user's word and wins outright. Otherwise the terminal decides, within the
    /// §34.2 budget: a place view spends about half the height on exits, across the groups it has.
    #[must_use]
    pub fn group_budget(&self, groups: usize) -> usize {
        if self.all {
            return usize::MAX;
        }
        if let Some(limit) = self.limit {
            return limit;
        }
        match self.terminal_rows {
            Some(rows) => (rows.saturating_sub(8) / groups.max(1)).clamp(1, DEFAULT_GROUP_BUDGET),
            None => DEFAULT_GROUP_BUDGET,
        }
    }
}

/// The bounded, ranked projection around `center` (§3.6).
///
/// The groups the index reports are filtered by the request, ranked, and then bounded; the
/// `hidden_count` and the `completeness` of what the bound left out are computed from the result
/// rather than asserted, so a caller cannot claim a completeness it does not have.
#[must_use]
pub fn neighborhood_of(
    index: &SpatialIndex,
    center: &SpatialId,
    request: &NeighborhoodRequest,
    pins: &PinRegistry,
    now: Timestamp,
) -> Neighborhood {
    // The index is read whole — it is already in memory (§33.1) — and bounded here, where the
    // request says what the bound is. Nothing is asked of a provider: `near` is a view of what is
    // known, and refreshing it is the session's business, not the query's (§33.2).
    let summary = index.relation_summary(center, usize::MAX, now);
    let object_type = index
        .get(center)
        .map(|entry| entry.object().object_type())
        .or_else(|| crate::resolve::space_of(center).map(|space| space.object_type));

    let mut groups: Vec<NeighborhoodGroup> = summary
        .into_iter()
        .filter(|group| keeps_group(group, request))
        .collect();
    rank_groups(&mut groups, object_type);

    let budget = request.group_budget(groups.len());
    let pinned: Vec<&SpatialId> = pins
        .pins()
        .map(ono_spatial_index::Pin::spatial_id)
        .collect();
    let groups: Vec<NeighborhoodGroup> = groups
        .into_iter()
        .map(|group| bound(index, group, request, &pinned, budget, now))
        .collect();

    let landmarks = landmarks_of(index, center, &groups, &pinned);
    Neighborhood::new(center.clone(), groups, now).with_landmarks(landmarks)
}

/// Whether a group survives the request's filters.
///
/// A withheld group survives every filter except the relation name, because §42.4 forbids denied
/// information from being reported as absence: a group hidden by a `--type` filter would be
/// exactly that.
fn keeps_group(group: &NeighborhoodGroup, request: &NeighborhoodRequest) -> bool {
    if let Some(wanted) = &request.relation
        && group.label() != wanted
        && group.relation().is_none_or(|relation| {
            let spec = relation.spec();
            relation.as_str() != wanted
                && spec.canonical_label != wanted
                && spec.inverse_label != wanted
        })
    {
        return false;
    }
    true
}

/// §3.6's "relationship relevance": the declared order of `relations.yaml`, with a cheap relation
/// ahead of an expensive one and a group that has something to show ahead of one that has not.
///
/// A withheld group keeps its declared position: it is information, and sorting it to the bottom
/// by "having nothing" would be the false-empty rendering §42.4 forbids.
fn rank_groups(groups: &mut [NeighborhoodGroup], object_type: Option<SpatialType>) {
    let order = |label: &str| -> (usize, usize) {
        let Some(object_type) = object_type else {
            return (0, usize::MAX);
        };
        relation::exits_from(object_type)
            .enumerate()
            .find(|(_, (declared, _))| *declared == label)
            .map_or((0, usize::MAX), |(position, (_, spec))| {
                (cost_rank(spec.cost_class), position)
            })
    };
    let mut indexed: Vec<(usize, (usize, usize), NeighborhoodGroup)> = groups
        .iter()
        .enumerate()
        .map(|(position, group)| (position, order(group.label()), group.clone()))
        .collect();
    indexed.sort_by(|a, b| {
        let interesting =
            |group: &NeighborhoodGroup| usize::from(group.total().is_none_or(|total| total == 0));
        (interesting(&a.2), a.1, a.0).cmp(&(interesting(&b.2), b.1, b.0))
    });
    for (slot, (_, _, group)) in indexed.into_iter().enumerate() {
        groups[slot] = group;
    }
}

fn cost_rank(cost: CostClass) -> usize {
    match cost {
        CostClass::Cheap => 0,
        CostClass::Normal => 1,
        CostClass::Expensive => 2,
        CostClass::Privileged => 3,
        CostClass::Remote => 4,
    }
}

/// Ranks a group's members and keeps the first `budget` of them, counting the rest.
///
/// A withheld group is returned untouched: it has no members to rank and no total to bound, and
/// giving it either would replace a §35.2 state with a number (§42.4).
fn bound(
    index: &SpatialIndex,
    group: NeighborhoodGroup,
    request: &NeighborhoodRequest,
    pinned: &[&SpatialId],
    budget: usize,
    now: Timestamp,
) -> NeighborhoodGroup {
    // A withheld group has no total (§2.17), no members to rank and nothing a budget could
    // bound: it is a §35.2 state, and it travels to the caller exactly as the index reported it.
    if group.total().is_none() {
        return group;
    }
    let relation = group.relation().cloned();
    let label = group.label().to_owned();
    let freshness = group.freshness();

    let mut members: Vec<SpatialId> = group
        .members()
        .iter()
        .filter(|id| keeps_member(index, id, request, now))
        .cloned()
        .collect();
    let total = members.len();
    members.sort_by_cached_key(|id| rank_of(index, id, pinned));
    members.truncate(budget);

    let mut ranked = NeighborhoodGroup::available(label, members)
        .of_total(total)
        .observed(freshness);
    if let Some(relation) = relation {
        ranked = ranked.along(relation);
    }
    ranked
}

/// Whether a member survives the request's `--type` and `--changed` filters.
pub(crate) fn keeps_member(
    index: &SpatialIndex,
    id: &SpatialId,
    request: &NeighborhoodRequest,
    now: Timestamp,
) -> bool {
    let Some(entry) = index.get(id) else {
        return false;
    };
    if let Some(wanted) = request.object_type
        && !entry.object().object_type().is_a(wanted)
    {
        return false;
    }
    if let Some(window) = request.changed_within {
        let Ok(edge) = now.checked_sub(window) else {
            return true;
        };
        if entry.observed_at() < edge {
            return false;
        }
    }
    true
}

/// The rank of one member: pinned first, then landmarked, then the most recently observed, then
/// by name so that two equal members always come out in the same order (§29.3).
pub(crate) fn rank_of(
    index: &SpatialIndex,
    id: &SpatialId,
    pinned: &[&SpatialId],
) -> (u8, u8, std::cmp::Reverse<i128>, String, String) {
    let entry = index.get(id);
    let is_pinned = u8::from(!pinned.contains(&id));
    let has_landmark = u8::from(entry.is_none_or(|entry| entry.landmarks().is_empty()));
    let observed = entry.map_or(0, |entry| entry.observed_at().as_nanosecond());
    let name = entry.map_or_else(String::new, |entry| {
        entry.object().display_name().to_ascii_lowercase()
    });
    (
        is_pinned,
        has_landmark,
        std::cmp::Reverse(observed),
        name,
        id.to_string(),
    )
}

/// The landmarks of a projection (§3.6's `landmarks`, §26.4).
///
/// The landmark *engine* — the rules and thresholds of §26.2 and §26.3 — is a later phase. What
/// exists now is what the index was told and what the user pinned, and that is what this returns:
/// the field is real, and nothing in it is invented (§2.16).
pub(crate) fn landmarks_of(
    index: &SpatialIndex,
    center: &SpatialId,
    groups: &[NeighborhoodGroup],
    pinned: &[&SpatialId],
) -> Vec<Landmark> {
    let mut landmarks: Vec<Landmark> = Vec::new();
    let mut subjects: Vec<SpatialId> = vec![center.clone()];
    subjects.extend(groups.iter().flat_map(|group| group.members().to_vec()));

    for subject in subjects {
        if let Some(entry) = index.get(&subject) {
            landmarks.extend(entry.landmarks().iter().cloned());
        }
        // §26.4: "User pins are landmarks." A pin outranks every heuristic, and it is the one
        // landmark reason this phase can state on its own, because the user stated it.
        if pinned.contains(&&subject)
            && !landmarks.iter().any(|landmark| {
                landmark.subject() == &subject && landmark.reason() == LandmarkReason::UserPinned
            })
        {
            let name = index.get(&subject).map_or_else(
                || subject.to_string(),
                |entry| entry.object().display_name().to_owned(),
            );
            landmarks.push(Landmark::built_in(
                subject.clone(),
                LandmarkReason::UserPinned,
                format!("pinned as `{name}`"),
            ));
        }
    }
    landmarks.sort_by_key(|landmark| {
        (
            landmark.reason(),
            landmark.subject().to_string(),
            landmark.evidence().to_owned(),
        )
    });
    landmarks.dedup_by(|a, b| {
        a.reason() == b.reason() && a.subject() == b.subject() && a.evidence() == b.evidence()
    });
    landmarks
}
