//! Selector resolution (spec v0.4 §27, §9, §40).
//!
//! §27.1 fixes the order a relative selector resolves in — visible child, visible neighbour,
//! canonical identifier, fuzzy visible match, current-host index, linked-host index — and §27.2
//! and §27.3 fix what happens when more than one answer, or only an approximate one, is found.
//!
//! Two rules decide the shape of this module:
//!
//! - **A fuzzy match never acts** (§27.3). It is offered — to a picker, or as the "did you mean"
//!   of a refusal — and it is never the answer a script gets, because a script has nobody to
//!   confirm it (§29.3).
//! - **Ambiguity is data, not a message** (§27.2). Every candidate carries the three columns
//!   §27.2's picker shows — name, type and place path — so the picker of §51 and the
//!   `spatial.ambiguous_selector` refusal of §40 are two renderings of one value.

use std::collections::BTreeSet;

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_spatial_core::{Freshness, SpatialId, SpatialType, canonical_parent, space, spaces};
use ono_spatial_index::SpatialIndex;
use ono_value::{ErrorValue, Provenance};

/// Which of §27.1's six steps found a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionStep {
    /// 1 — an exact visible child or group of the current place.
    VisibleChild,
    /// 2 — an exact visible neighbour of the current place.
    VisibleNeighbor,
    /// 3 — an exact canonical identifier in the current scope: a `SpatialId`, a canonical space
    ///     id, or a `<scope>:<key>` selector.
    CanonicalIdentifier,
    /// 4 — a fuzzy match among the visible places. Never acts alone (§27.3).
    FuzzyVisible,
    /// 5 — the current-host spatial index (§33.1).
    HostIndex,
    /// 6 — a linked host's index, only when explicitly requested or configured (§9.3, §35.4).
    LinkedHostIndex,
}

impl ResolutionStep {
    /// The six steps, in the order §27.1 gives them.
    pub const ALL: &'static [ResolutionStep] = &[
        ResolutionStep::VisibleChild,
        ResolutionStep::VisibleNeighbor,
        ResolutionStep::CanonicalIdentifier,
        ResolutionStep::FuzzyVisible,
        ResolutionStep::HostIndex,
        ResolutionStep::LinkedHostIndex,
    ];

    /// The name `explain` and a diagnostic spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ResolutionStep::VisibleChild => "visible_child",
            ResolutionStep::VisibleNeighbor => "visible_neighbor",
            ResolutionStep::CanonicalIdentifier => "canonical_identifier",
            ResolutionStep::FuzzyVisible => "fuzzy_visible",
            ResolutionStep::HostIndex => "host_index",
            ResolutionStep::LinkedHostIndex => "linked_host_index",
        }
    }

    /// Whether the step is an approximation, and so may never act on its own (§27.3).
    #[must_use]
    pub const fn is_fuzzy(self) -> bool {
        matches!(self, ResolutionStep::FuzzyVisible)
    }
}

/// One place a selector could mean, with the context §27.2 requires beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    spatial_id: SpatialId,
    /// The key a person types for this place, where it is not the name itself (§11.2, §27.2).
    key: Option<String>,
    name: String,
    object_type: SpatialType,
    place_path: String,
    step: ResolutionStep,
    freshness: Freshness,
    provenance: Option<Provenance>,
}

impl Candidate {
    /// The place.
    #[must_use]
    pub fn spatial_id(&self) -> &SpatialId {
        &self.spatial_id
    }

    /// What a person calls it — §27.2's first column.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What kind of place it is — §27.2's second column.
    #[must_use]
    pub fn object_type(&self) -> SpatialType {
        self.object_type
    }

    /// Where it sits in the canonical hierarchy — §27.2's third column, and the `path/scope`
    /// information §6.8 requires of a search result.
    #[must_use]
    pub fn place_path(&self) -> &str {
        &self.place_path
    }

    /// Which of §27.1's steps found it.
    #[must_use]
    pub fn step(&self) -> ResolutionStep {
        self.step
    }

    /// How current the answer about it is (§27.4).
    #[must_use]
    pub fn freshness(&self) -> Freshness {
        self.freshness
    }

    /// Where the fact came from, for a candidate that is an observed object (§27.4).
    ///
    /// `None` for a canonical space, which no provider observes.
    #[must_use]
    pub fn provenance(&self) -> Option<&Provenance> {
        self.provenance.as_ref()
    }

    /// The key a person would type for this place — a process's pid, a mount's target (§11.2).
    ///
    /// `None` for a canonical space, whose label is already the whole name.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// The row §27.2's picker shows: `nginx/1842   process   local/compute/processes`.
    ///
    /// §27.2's own example writes the first column as `<name>/<key>` for exactly the case that
    /// makes a picker necessary — two places a person calls the same thing. A row that repeated
    /// the ambiguous name three times would disambiguate nothing, which is what the section
    /// requires of it: "The picker MUST show disambiguating context."
    #[must_use]
    pub fn row(&self) -> String {
        let name = match &self.key {
            Some(key) if key != &self.name => format!("{}/{key}", self.name),
            _ => self.name.clone(),
        };
        format!(
            "{:<28} {:<12} {}",
            name,
            self.object_type.as_str(),
            self.place_path
        )
    }
}

/// What a selector resolved to (§27.1, §27.2, §27.3).
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// Exactly one place answers to the selector.
    Resolved(Box<Candidate>),
    /// Several exact matches. Interactively this opens the picker of §27.2; in a script it is
    /// `spatial.ambiguous_selector` and nothing else (§29.3).
    Ambiguous(Vec<Candidate>),
    /// Only approximate matches. They are offered, never taken (§27.3).
    Fuzzy(Vec<Candidate>),
    /// Nothing answers to it.
    NotFound,
}

impl Resolution {
    /// The candidates the resolution found, in ranked order.
    #[must_use]
    pub fn candidates(&self) -> &[Candidate] {
        match self {
            Resolution::Resolved(candidate) => std::slice::from_ref(candidate),
            Resolution::Ambiguous(candidates) | Resolution::Fuzzy(candidates) => candidates,
            Resolution::NotFound => &[],
        }
    }

    /// The one place the selector names, or the structured refusal §40 requires.
    ///
    /// # Errors
    ///
    /// - `spatial.ambiguous_selector` listing the candidates in §27.2's three columns;
    /// - `spatial.not_found`, naming the nearest approximate matches where there are any — which
    ///   is §40's "actionable next steps where deterministic", and is the only thing a fuzzy
    ///   match is ever used for outside an interactive picker (§27.3).
    pub fn require(self, selector: &str) -> Result<Candidate, ErrorValue> {
        match self {
            Resolution::Resolved(candidate) => Ok(*candidate),
            Resolution::Ambiguous(candidates) => Err(ErrorValue::new(
                ErrorCode::SpatialAmbiguousSelector,
                format!(
                    "`{selector}` names {} places:\n{}",
                    candidates.len(),
                    rows(&candidates)
                ),
            )
            .with_help(
                "name the exact spatial id, or select one from a stream — `find place \
                 <selector> | take 1 | enter` (spec v0.4 §29.3)",
            )),
            Resolution::Fuzzy(candidates) => Err(ErrorValue::new(
                ErrorCode::SpatialNotFound,
                format!("no place is called `{selector}`"),
            )
            .with_help(format!(
                "a fuzzy match is never followed on its own (spec v0.4 §27.3); did you mean \
                 one of these?\n{}",
                rows(&candidates)
            ))),
            Resolution::NotFound => Err(ErrorValue::new(
                ErrorCode::SpatialNotFound,
                format!("no place is called `{selector}`"),
            )
            .with_help("`find place <text>` searches the spatial index (spec v0.4 §6.8)")),
        }
    }
}

fn rows(candidates: &[Candidate]) -> String {
    candidates
        .iter()
        .map(|candidate| format!("  {}", candidate.row()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where a selector is being resolved from, and what it is allowed to reach (§27.1).
#[derive(Debug, Clone, Default)]
pub struct SelectorContext {
    from: Option<SpatialId>,
    object_type: Option<SpatialType>,
    remote: bool,
}

impl SelectorContext {
    /// A resolution from the current place `from`.
    #[must_use]
    pub fn at(from: SpatialId) -> Self {
        Self {
            from: Some(from),
            object_type: None,
            remote: false,
        }
    }

    /// A resolution with no current place — the shape a script has before it has moved.
    #[must_use]
    pub fn anywhere() -> Self {
        Self::default()
    }

    /// Restricts the answer to one kind of place (`--type`, §6.2, ADR-0124).
    #[must_use]
    pub fn of_type(mut self, object_type: SpatialType) -> Self {
        self.object_type = Some(object_type);
        self
    }

    /// Allows step 6, the linked-host index. Off by default, because §9.3 and §35.4 require a
    /// jump across a link to be asked for and never guessed (`spatial.remote_search = explicit`).
    #[must_use]
    pub fn across_links(mut self, allowed: bool) -> Self {
        self.remote = allowed;
        self
    }

    /// The place the resolution starts from.
    #[must_use]
    pub fn from(&self) -> Option<&SpatialId> {
        self.from.as_ref()
    }
}

/// Resolves `selector` against the index, in the order of §27.1.
///
/// Exact matches are collected step by step: an exact answer at any step wins over an approximate
/// answer at an earlier one, because §27.3 forbids a fuzzy match from acting and a resolution
/// that stopped at step 4 would therefore have no answer at all. Only when no step matched
/// exactly are the approximate matches reported — as [`Resolution::Fuzzy`], which never acts.
#[must_use]
pub fn resolve(
    index: &SpatialIndex,
    selector: &str,
    context: &SelectorContext,
    now: Timestamp,
) -> Resolution {
    let selector = selector.trim();
    if selector.is_empty() {
        return Resolution::NotFound;
    }

    let mut exact: Vec<Candidate> = Vec::new();
    let mut seen: BTreeSet<SpatialId> = BTreeSet::new();

    for step in [
        ResolutionStep::VisibleChild,
        ResolutionStep::VisibleNeighbor,
        ResolutionStep::CanonicalIdentifier,
        ResolutionStep::HostIndex,
        ResolutionStep::LinkedHostIndex,
    ] {
        if step == ResolutionStep::LinkedHostIndex && !context.remote {
            continue;
        }
        for candidate in step_matches(index, selector, context, step, now) {
            if seen.insert(candidate.spatial_id.clone()) {
                exact.push(candidate);
            }
        }
        if !exact.is_empty() {
            break;
        }
    }

    match exact.len() {
        1 => return Resolution::Resolved(Box::new(exact.remove(0))),
        0 => {}
        _ => {
            exact.sort_by(|a, b| {
                (a.step, &a.name, &a.spatial_id).cmp(&(b.step, &b.name, &b.spatial_id))
            });
            return Resolution::Ambiguous(exact);
        }
    }

    let mut fuzzy = fuzzy_matches(index, selector, context, now);
    if fuzzy.is_empty() {
        return Resolution::NotFound;
    }
    fuzzy.sort_by(|a, b| (a.step, &a.name, &a.spatial_id).cmp(&(b.step, &b.name, &b.spatial_id)));
    Resolution::Fuzzy(fuzzy)
}

/// The exact matches one step of §27.1 finds.
fn step_matches(
    index: &SpatialIndex,
    selector: &str,
    context: &SelectorContext,
    step: ResolutionStep,
    now: Timestamp,
) -> Vec<Candidate> {
    match step {
        ResolutionStep::VisibleChild => visible_children(index, context)
            .into_iter()
            .filter(|id| names(index, id, selector))
            .filter_map(|id| candidate(index, &id, step, now))
            .filter(|candidate| context.accepts(candidate))
            .collect(),
        ResolutionStep::VisibleNeighbor => visible_neighbors(index, context, now)
            .into_iter()
            .filter(|id| names(index, id, selector))
            .filter_map(|id| candidate(index, &id, step, now))
            .filter(|candidate| context.accepts(candidate))
            .collect(),
        ResolutionStep::CanonicalIdentifier => canonical_identifier(index, selector, now)
            .into_iter()
            .filter(|candidate| context.accepts(candidate))
            .collect(),
        // The linked-host index is a place this build cannot reach yet: §19's remote root places
        // arrive with the federation phase, and answering from nothing would be worse than
        // answering nothing.
        ResolutionStep::HostIndex | ResolutionStep::LinkedHostIndex => index
            .by_alias(selector)
            .into_iter()
            .filter_map(|entry| candidate(index, entry.object().spatial_id(), step, now))
            .filter(|candidate| context.accepts(candidate))
            .filter(|_| step == ResolutionStep::HostIndex)
            .collect(),
        ResolutionStep::FuzzyVisible => Vec::new(),
    }
}

/// The approximate matches of §27.3: visible first, then the rest of the index.
fn fuzzy_matches(
    index: &SpatialIndex,
    selector: &str,
    context: &SelectorContext,
    now: Timestamp,
) -> Vec<Candidate> {
    let needle = selector.to_ascii_lowercase();
    let mut visible: BTreeSet<SpatialId> = visible_children(index, context).into_iter().collect();
    visible.extend(visible_neighbors(index, context, now));

    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in index.search(&needle) {
        let id = entry.object().spatial_id();
        let step = if visible.contains(id) {
            ResolutionStep::FuzzyVisible
        } else {
            ResolutionStep::HostIndex
        };
        let Some(candidate) = candidate(index, id, step, now) else {
            continue;
        };
        if !context.accepts(&candidate) || !seen.insert(id.clone()) {
            continue;
        }
        found.push(candidate);
    }
    // A canonical space answers to a prefix of its own label the same way an object does, so
    // `enter compu` offers COMPUTE rather than nothing.
    for space in spaces() {
        if space.is_served()
            && space.label.to_ascii_lowercase().contains(&needle)
            && seen.insert(space.spatial_id())
        {
            found.push(space_candidate(space, ResolutionStep::FuzzyVisible));
        }
    }
    found
}

impl SelectorContext {
    /// Whether a candidate passes the `--type` filter the caller set.
    fn accepts(&self, candidate: &Candidate) -> bool {
        self.object_type
            .is_none_or(|wanted| candidate.object_type.is_a(wanted))
    }
}

/// Whether `id` answers exactly to `selector` (§27.1's "exact"), case-insensitively.
fn names(index: &SpatialIndex, id: &SpatialId, selector: &str) -> bool {
    if let Some(space) = space_of(id) {
        return space.label.eq_ignore_ascii_case(selector)
            || space.id.eq_ignore_ascii_case(selector);
    }
    index
        .get(id)
        .is_some_and(|entry| entry.aliases().contains(&selector.to_ascii_lowercase()))
}

/// Step 3: an exact canonical identifier in the current scope.
///
/// Three spellings are canonical identifiers rather than names: the opaque [`SpatialId`] itself,
/// the dotted id of a canonical space (`compute.services`), and the `<scope>:<key>` form the
/// trail and `jump` use (`storage:/data`).
fn canonical_identifier(index: &SpatialIndex, selector: &str, now: Timestamp) -> Vec<Candidate> {
    if let Some(id) = SpatialId::parse(selector) {
        if let Some(space) = space_of(&id) {
            return vec![space_candidate(space, ResolutionStep::CanonicalIdentifier)];
        }
        return candidate(index, &id, ResolutionStep::CanonicalIdentifier, now)
            .into_iter()
            .collect();
    }
    if let Some(space) = space::space(&selector.to_ascii_lowercase()) {
        return vec![space_candidate(space, ResolutionStep::CanonicalIdentifier)];
    }
    // `<type>/<key>` — `process/1842`, `service/nginx.service`. §11.2 and §27.2 write a place
    // this way whenever the bare key would be ambiguous, and the trail and the map print it.
    if let Some((kind, key)) = selector.split_once('/')
        && !key.is_empty()
        && let Some(wanted) = SpatialType::ALL
            .iter()
            .copied()
            .find(|known| known.as_str().eq_ignore_ascii_case(kind))
    {
        let found: Vec<Candidate> = index
            .by_alias(key)
            .into_iter()
            .filter(|entry| entry.object().object_type().is_a(wanted))
            .filter_map(|entry| {
                candidate(
                    index,
                    entry.object().spatial_id(),
                    ResolutionStep::CanonicalIdentifier,
                    now,
                )
            })
            .collect();
        if !found.is_empty() {
            return found;
        }
    }
    // `<domain>:<key>` — `storage:/data`, `compute:1842`. The domain narrows the search to the
    // types that domain holds, which is what makes the same key unambiguous in two domains.
    if let Some((domain, key)) = selector.split_once(':')
        && let Some(space) = space::space(&domain.to_ascii_lowercase())
        && !key.is_empty()
    {
        let held: Vec<SpatialType> = held_types(space.id);
        return index
            .by_alias(key)
            .into_iter()
            .filter(|entry| {
                held.iter()
                    .any(|kind| entry.object().object_type().is_a(*kind))
            })
            .filter_map(|entry| {
                candidate(
                    index,
                    entry.object().spatial_id(),
                    ResolutionStep::CanonicalIdentifier,
                    now,
                )
            })
            .collect();
    }
    Vec::new()
}

/// The spatial types a domain or collection holds, directly or through its collections.
fn held_types(space_id: &str) -> Vec<SpatialType> {
    let mut held = Vec::new();
    if let Some(space) = space::space(space_id)
        && let Some(member) = space.member_type
    {
        held.push(member);
    }
    for child in space::children(space_id) {
        held.extend(held_types(child.id));
    }
    held
}

/// The places directly inside the current place (§27.1 step 1).
fn visible_children(index: &SpatialIndex, context: &SelectorContext) -> Vec<SpatialId> {
    let Some(here) = context.from() else {
        // With no current place the root's children are what is visible: the six domains are the
        // orientation anchors a session starts with (§5, §7.1).
        return space::children(space::root().id)
            .filter(|space| space.is_served())
            .map(|space| space.spatial_id())
            .collect();
    };
    let mut children: Vec<SpatialId> = Vec::new();
    if let Some(space) = space_of(here) {
        children.extend(
            space::children(space.id)
                .filter(|space| space.is_served())
                .map(|space| space.spatial_id()),
        );
    }
    children.extend(
        index
            .entries()
            .filter(|entry| {
                entry
                    .canonical_parent()
                    .is_some_and(|edge| edge.parent() == here)
            })
            .map(|entry| entry.object().spatial_id().clone()),
    );
    children
}

/// The places one relation away from the current place (§27.1 step 2).
fn visible_neighbors(
    index: &SpatialIndex,
    context: &SelectorContext,
    now: Timestamp,
) -> Vec<SpatialId> {
    let Some(here) = context.from() else {
        return Vec::new();
    };
    index
        .relation_summary(here, usize::MAX, now)
        .into_iter()
        .flat_map(|group| group.members().to_vec())
        .collect()
}

/// The candidate for an index entry, or `None` where the index does not hold it.
fn candidate(
    index: &SpatialIndex,
    id: &SpatialId,
    step: ResolutionStep,
    now: Timestamp,
) -> Option<Candidate> {
    if let Some(space) = space_of(id) {
        return Some(space_candidate(space, step));
    }
    let entry = index.get(id)?;
    // The first identity field is the key a person types: a process's pid, a socket's inode, a
    // mount's target (§11.2, and the same rule the trail writes a step's reference by).
    let key = entry
        .canonical_ref()
        .id()
        .values()
        .first()
        .and_then(|value| ono_value::canonical_text(value).ok());
    Some(Candidate {
        spatial_id: id.clone(),
        key,
        name: entry.object().display_name().to_owned(),
        object_type: entry.object().object_type(),
        place_path: place_path(index, id),
        step,
        freshness: index.freshness(id, now),
        provenance: Some(entry.object().provenance().clone()),
    })
}

fn space_candidate(
    space: &'static ono_spatial_core::CanonicalSpace,
    step: ResolutionStep,
) -> Candidate {
    Candidate {
        spatial_id: space.spatial_id(),
        key: None,
        name: space.label.to_owned(),
        object_type: space.object_type,
        place_path: space_path(space.id),
        step,
        // The geography is not observed; it is declared, and it is as current as the build.
        freshness: Freshness::Live,
        provenance: None,
    }
}

/// The canonical space an id names, where it names one.
#[must_use]
pub fn space_of(id: &SpatialId) -> Option<&'static ono_spatial_core::CanonicalSpace> {
    spaces().iter().find(|space| &space.spatial_id() == id)
}

/// The place as §21.2 asks the prompt to write it: `local`, `local/compute`, `local/process/nginx`.
///
/// §21.2 forbids rendering the whole navigation trail — "Ono MUST NOT blindly render the entire
/// navigation trail in the prompt" — and gives the shape instead: `<host>/<current-place-kind>/
/// <display-name>`. A canonical space *is* its path, so it needs no kind and no name added; an
/// observed object is written under the link it belongs to, its type and what a person calls it,
/// because its canonical parent chain is `place_path`'s answer to a different question (§27.2).
#[must_use]
pub fn concise_path(index: &SpatialIndex, id: &SpatialId) -> String {
    if let Some(space) = space_of(id) {
        return space_path(space.id);
    }
    let Some(entry) = index.get(id) else {
        return locality(None).to_owned();
    };
    let link = locality(Some(entry.object().scope()));
    let kind = entry.object().object_type().as_str().to_ascii_lowercase();
    format!("{link}/{kind}/{}", entry.object().display_name())
}

/// The canonical hierarchy path a place sits at — §27.2's third column and §6.8's `path/scope`
/// information: `local/compute/processes`.
///
/// It names the place's canonical parent chain, not the place itself, because that is what
/// disambiguates two objects with the same name (§27.2's own example lists `nginx.service` and
/// `nginx/1842` by the paths they sit at).
#[must_use]
pub fn place_path(index: &SpatialIndex, id: &SpatialId) -> String {
    if let Some(space) = space_of(id) {
        return space_path(space.id);
    }
    let Some(entry) = index.get(id) else {
        return locality(None).to_owned();
    };
    let scope = entry.object().scope();
    let parent = entry.canonical_parent().map(|edge| edge.parent().clone());
    let mut path = String::from(locality(Some(scope)));
    if let Some(parent) = parent {
        let rest = match space_of(&parent) {
            Some(space) => space_path(space.id)
                .split_once('/')
                .map(|(_, rest)| rest.to_owned())
                .unwrap_or_default(),
            None => index.get(&parent).map_or_else(String::new, |parent| {
                let above = place_path(index, parent.object().spatial_id());
                let above = above.split_once('/').map_or("", |(_, rest)| rest);
                if above.is_empty() {
                    parent.object().display_name().to_owned()
                } else {
                    format!("{above}/{}", parent.object().display_name())
                }
            }),
        };
        if !rest.is_empty() {
            path.push('/');
            path.push_str(&rest);
        }
    }
    path
}

/// The path of a canonical space, from the host down: `local/compute/processes`.
fn space_path(space_id: &str) -> String {
    let mut parts = vec!["local".to_owned()];
    parts.extend(
        ono_spatial_core::path_to_space(space_id)
            .into_iter()
            .filter(|space| space.parent.is_some())
            .map(|space| space.label.to_ascii_lowercase()),
    );
    parts.join("/")
}

/// What §27.2 writes as the first segment of a place path: `local` for this host, the host's own
/// name for anywhere else (§19).
fn locality(scope: Option<&ono_spatial_core::SpatialScope>) -> &str {
    match scope {
        Some(scope) if scope.is_remote() => scope.host_scope().id(),
        _ => "local",
    }
}

/// The canonical parent of an object the index holds, recomputed from what it knows.
///
/// Exposed because `up` and the place path need the same answer, and asking the index twice for
/// two different answers is how a hierarchy stops being one (§11.3).
#[must_use]
pub fn parent_of(index: &SpatialIndex, id: &SpatialId) -> Option<SpatialId> {
    if let Some(space) = space_of(id) {
        return ono_spatial_core::parent_of_space(space.id).map(|edge| edge.parent().clone());
    }
    let entry = index.get(id)?;
    entry
        .canonical_parent()
        .map(|edge| edge.parent().clone())
        .or_else(|| {
            canonical_parent(id, entry.object().object_type(), entry.edges())
                .map(|edge| edge.parent().clone())
        })
}
