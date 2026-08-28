//! `find place` (spec v0.4 §6.8, §9.3, §27.4, §29.4; ADR-0124).
//!
//! §6.8: "`find` MUST search the spatial index and provider registries rather than blindly grep
//! rendered text", and "Results MUST include enough path/scope information to disambiguate
//! identical names."
//!
//! Both halves are here. The search runs over [`SpatialIndex`] entries — objects a provider
//! produced and the bridge placed — and every result carries the place path of §27.2, the scope
//! of §3.2 and the freshness and provenance of §27.4, because a result that came from a cache
//! must say so.
//!
//! Ranking is total and deterministic (§29.3): the same index answers the same search the same
//! way in every session, which is what makes `find place <x> | take 1 | enter` a defined
//! selection rather than a coin flip (§28.2).

use jiff::Timestamp;
use ono_spatial_core::{Freshness, SpatialId, SpatialScope, SpatialType};
use ono_spatial_index::{PinRegistry, SpatialIndex};
use ono_value::Provenance;

/// How many places a search answers with before the caller asks for more (§34's search budget).
///
/// `find place` is a stream and composes with `take`, so this is a bound on the *work*, not on
/// what a pipeline may see: `--all` removes it and `--limit` replaces it.
pub const DEFAULT_RESULT_BUDGET: usize = 100;

/// What a search is looking for (§6.8's four spellings, in ADR-0124's one).
#[derive(Debug, Clone, Default)]
pub struct FindRequest {
    text: Option<String>,
    object_type: Option<SpatialType>,
    near: Option<SpatialId>,
    limit: Option<usize>,
    all: bool,
    /// The places a predicate was evaluated against, each with the position the provider
    /// answered it in. §29.3 wants a deterministic answer, and the order the providers gave is
    /// the one a user already sees from `get process | where …` (§28, §29.4).
    subjects: Option<std::collections::BTreeMap<SpatialId, usize>>,
    here: Option<SpatialId>,
}

impl FindRequest {
    /// A search with no filters, which answers every place the index holds, ranked and bounded.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `find place <query>` — the name or alias to look for (§6.8).
    #[must_use]
    pub fn matching(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// `find place --type <type>` (ADR-0124; §6.8's `find <type> <query>`).
    #[must_use]
    pub fn of_type(mut self, object_type: SpatialType) -> Self {
        self.object_type = Some(object_type);
        self
    }

    /// `find place --near <place-selector>` — the anchor the search is measured from (§6.8).
    #[must_use]
    pub fn near(mut self, anchor: SpatialId) -> Self {
        self.near = Some(anchor);
        self
    }

    /// `find place --limit <n>`.
    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// `find place --all` — every match, however many (§6.2's `--all`, "MAY be expensive").
    #[must_use]
    pub fn all(mut self, all: bool) -> Self {
        self.all = all;
        self
    }

    /// Restricts the answer to the places these ids name.
    ///
    /// This is what a `--where` predicate needs. A predicate is evaluated against the objects a
    /// provider produced, and absorbing those objects also places the ones they *mention* — the
    /// pid namespace a process reports, the cgroup it belongs to, the far end of a connection
    /// (§42.3). Those places were never tested against the predicate, so they are not an answer
    /// to it: a search that returned them would be reporting objects nobody filtered.
    #[must_use]
    pub fn among(mut self, subjects: impl IntoIterator<Item = SpatialId>) -> Self {
        self.subjects = Some(
            subjects
                .into_iter()
                .enumerate()
                .map(|(position, id)| (id, position))
                .collect(),
        );
        self
    }

    /// Where the search is being made from, for the ranking of §27.1 and §3.6.
    ///
    /// It narrows nothing: §9.3 keeps `find` global. It decides which of two equally good
    /// answers is offered first.
    #[must_use]
    pub fn from_place(mut self, here: SpatialId) -> Self {
        self.here = Some(here);
        self
    }

    /// The text being searched for.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// The type filter.
    #[must_use]
    pub fn object_type(&self) -> Option<SpatialType> {
        self.object_type
    }

    /// The anchor.
    #[must_use]
    pub fn anchor(&self) -> Option<&SpatialId> {
        self.near.as_ref()
    }

    /// How many results the search answers with.
    #[must_use]
    pub fn budget(&self) -> usize {
        if self.all {
            usize::MAX
        } else {
            self.limit.unwrap_or(DEFAULT_RESULT_BUDGET)
        }
    }
}

/// One place a search found, with the context §6.8 and §27.4 require beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundPlace {
    spatial_id: SpatialId,
    name: String,
    object_type: SpatialType,
    schema: String,
    place_path: String,
    scope: SpatialScope,
    freshness: Freshness,
    provenance: Provenance,
    observed_at: Timestamp,
    pinned: bool,
}

impl FoundPlace {
    /// The place's identity (§3.1).
    #[must_use]
    pub fn spatial_id(&self) -> &SpatialId {
        &self.spatial_id
    }

    /// What a person calls it (§3.1's `display_name`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What kind of place it is (§3.3).
    #[must_use]
    pub fn object_type(&self) -> SpatialType {
        self.object_type
    }

    /// The v0.2 schema of the object behind the place — `ono.process/1` — which is what makes a
    /// found place recognisable to the object pipeline it flows into (§28, §37.1).
    ///
    /// It is the schema of the *object*, not of whatever record named it: a pid namespace
    /// composed from a process's `/proc/<pid>/ns/pid` is an `ono.namespace/1`, because that is
    /// what it is (§42.3).
    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Where it sits in the canonical hierarchy (§27.2, §6.8).
    #[must_use]
    pub fn place_path(&self) -> &str {
        &self.place_path
    }

    /// The boundary it belongs to (§3.2).
    #[must_use]
    pub fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// How current the answer is (§27.4, §33.4).
    #[must_use]
    pub fn freshness(&self) -> Freshness {
        self.freshness
    }

    /// Where the fact came from (§27.4).
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// When the provider last saw it.
    #[must_use]
    pub fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    /// Whether the user pinned it (§20.4, §26.4).
    #[must_use]
    pub fn is_pinned(&self) -> bool {
        self.pinned
    }
}

/// The places the index holds that answer `request`, ranked and bounded (§6.8).
///
/// Ranking, in order: a pin outranks everything the user did not choose (§26.4); an exact name
/// match outranks a partial one, because a user who typed the whole name meant it; a place nearer
/// the anchor outranks one further away (§6.8's `--near`); a landmark outranks a plain object
/// (§3.7); and everything else is ordered by name and then by identity, so the order is total
/// and a script sees the same answer twice (§29.3).
#[must_use]
pub fn find_places(
    index: &SpatialIndex,
    request: &FindRequest,
    pins: &PinRegistry,
    now: Timestamp,
) -> Vec<FoundPlace> {
    let needle = request.text.as_ref().map(|text| text.to_ascii_lowercase());
    let pinned: Vec<&SpatialId> = pins
        .pins()
        .map(ono_spatial_index::Pin::spatial_id)
        .collect();
    let anchor_path = request
        .near
        .as_ref()
        .map(|anchor| crate::resolve::place_path(index, anchor));

    // §27.1 "prioritizes local orientation over surprising global jumps", and §3.6 counts the
    // current view purpose among what a ranking must consider. A global search stays global —
    // §9.3 — and the places under where the user is standing simply answer first.
    let here = request
        .here
        .as_ref()
        .map(|here| crate::resolve::place_path(index, here));

    let mut found: Vec<(Rank, FoundPlace)> = Vec::new();
    for entry in index.entries() {
        let object = entry.object();
        let id = object.spatial_id();
        if let Some(subjects) = &request.subjects
            && !subjects.contains_key(id)
        {
            continue;
        }
        if let Some(wanted) = request.object_type
            && !object.object_type().is_a(wanted)
        {
            continue;
        }
        let matched = match &needle {
            None => Match::Everything,
            Some(needle) => match name_match(entry.aliases(), needle) {
                Some(matched) => matched,
                None => continue,
            },
        };
        let place_path = crate::resolve::place_path(index, id);
        if let Some(anchor) = &anchor_path
            && !place_path.starts_with(anchor.as_str())
            && !anchor.starts_with(place_path.as_str())
        {
            continue;
        }
        let place = FoundPlace {
            spatial_id: id.clone(),
            name: object.display_name().to_owned(),
            object_type: object.object_type(),
            schema: object.canonical_ref().id().schema().to_string(),
            place_path,
            scope: object.scope().clone(),
            freshness: index.freshness(id, now),
            provenance: object.provenance().clone(),
            observed_at: entry.observed_at(),
            pinned: pinned.contains(&id),
        };
        let rank = Rank {
            unpinned: u8::from(!place.pinned),
            distant: u8::from(
                !here
                    .as_ref()
                    .is_none_or(|here| place.place_path.starts_with(here.as_str())),
            ),
            match_quality: matched,
            plain: u8::from(entry.landmarks().is_empty()),
            order: request
                .subjects
                .as_ref()
                .and_then(|subjects| subjects.get(id).copied())
                .unwrap_or(usize::MAX),
            name: place.name.to_ascii_lowercase(),
            id: id.to_string(),
        };
        found.push((rank, place));
    }

    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.truncate(request.budget());
    found.into_iter().map(|(_, place)| place).collect()
}

/// How well a name answered the search, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Match {
    /// The whole alias is the search text.
    Exact,
    /// An alias starts with it.
    Prefix,
    /// An alias contains it.
    Contains,
    /// Nothing was searched for, so everything answers equally.
    Everything,
}

fn name_match(aliases: &std::collections::BTreeSet<String>, needle: &str) -> Option<Match> {
    let mut best = None;
    for alias in aliases {
        let quality = if alias == needle {
            Match::Exact
        } else if alias.starts_with(needle) {
            Match::Prefix
        } else if alias.contains(needle) {
            Match::Contains
        } else {
            continue;
        };
        best = Some(best.map_or(quality, |known: Match| known.min(quality)));
    }
    best
}

/// The total order results come out in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    unpinned: u8,
    distant: u8,
    match_quality: Match,
    plain: u8,
    order: usize,
    name: String,
    id: String,
}
