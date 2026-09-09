//! Event search (spec v0.5 §20): `find event` over ordinary Ono expression semantics, so a user
//! discovers an event without already knowing when it happened.
//!
//! §20.3 is a constraint on what this module may be: `find event <predicate>` "reuses the
//! existing `find` verb and Ono expression semantics rather than inventing a new search
//! language". The expression language lives in `ono-parser` and `ono-command`, which this crate
//! does not depend on and must not, so what is here is the *planner*: the caller reads off the
//! predicate whatever it can see without evaluating it — the kinds, the subjects, the window —
//! and [`plan`] turns those [`SearchHints`] into one bounded [`EventQuery`]. Everything the
//! hints could not express is filtered by the same `where` a pipeline already uses. That split
//! is also what §32.3's 150 ms budget for an indexed predicate rests on.
//!
//! §11.6's short references live here too. The full [`EventId`] is a 24-digit digest, unusable at
//! a prompt, so [`EventReferences`] issues the shortest prefix that names one event in this
//! session and never lets an issued reference change meaning. A reference is resolvable outside
//! the session as well, by the ledger's own prefix lookup, which raises
//! `temporal.ambiguous_event` where a prefix has since come to name two events (§34).
//!
//! §14.4 is the subtle one. Searching for a place at a historical instant may legitimately use a
//! present-day name to reach a candidate, and doing so "MUST distinguish resolution aid from
//! historical existence evidence". [`ResolutionBasis`] is that distinction, and a match reached
//! only through today's index carries no anchor and no instant, because finding a name today is
//! not evidence the thing existed then.

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{PermissionState, SpatialId, SpatialScope, SpatialType};
use ono_temporal_core::{
    CoverageQuery, EventId, EventKind, EventQuery, LedgerRead, QueryOrder, SpatialRef,
    TemporalCompleteness, TemporalEvent, TimeRange, error,
};
use ono_value::ErrorValue;

use crate::relevance::{Horizon, classify, is_default_scope};

/// How many events a search returns when the caller names no ceiling.
pub const DEFAULT_LIMIT: usize = 500;

/// The shortest reference §11.6 issues, in hex digits.
///
/// Two digits is what `@e42` is, and it is enough for a session that shows a handful of events.
/// A session that shows more takes longer prefixes, one event at a time.
pub const MIN_REFERENCE_DIGITS: usize = 2;

/// What a caller could read off an ordinary Ono predicate without evaluating it (§20.3).
///
/// Everything here is a restriction the ledger can apply itself. A predicate the hints cannot
/// express is not lost: it is applied by the pipeline over the stream this planner produces,
/// which is what "reuses Ono expression semantics" means in practice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchHints {
    /// The boundary to search. `None` for every scope the reader may see.
    pub scope: Option<SpatialScope>,
    /// The subjects the predicate named, where it named any.
    pub subjects: Vec<SpatialId>,
    /// The kinds the predicate fixed, where it fixed any.
    pub kinds: Vec<EventKind>,
    /// The window the predicate implied.
    pub range: TimeRange,
    /// How many events to return. `None` takes [`DEFAULT_LIMIT`].
    pub limit: Option<usize>,
    /// Which way round the answer comes.
    pub order: QueryOrder,
}

/// The bounded ledger query `hints` become (§20.3, §32.3).
#[must_use]
pub fn plan(hints: &SearchHints) -> EventQuery {
    EventQuery {
        scope: hints.scope.clone(),
        subjects: hints.subjects.clone(),
        kinds: hints.kinds.clone(),
        range: hints.range,
        limit: Some(hints.limit.unwrap_or(DEFAULT_LIMIT)),
        order: hints.order,
    }
}

/// The events `hints` admit, as the stream §20.3 returns.
///
/// The predicate itself is the pipeline's to apply; this is the bounded set it applies to.
///
/// # Errors
///
/// Returns whatever §34 refusal the ledger raises.
pub fn find_events(
    ledger: &dyn LedgerRead,
    hints: &SearchHints,
) -> Result<Vec<TemporalEvent>, ErrorValue> {
    ledger.events(&plan(hints))
}

/// A short event reference as §11.6 renders it — `@e42`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRef {
    short: Arc<str>,
    full: EventId,
}

impl EventRef {
    /// The reference without the `@` a renderer adds.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.short
    }

    /// The full identity it stands for.
    #[must_use]
    pub const fn event_id(&self) -> &EventId {
        &self.full
    }
}

impl std::fmt::Display for EventRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@{}", self.short)
    }
}

/// The short references one session has issued (§11.6).
///
/// A reference is a prefix of the event's own content digest, so it needs no counter, no shared
/// state and no allocation table in the ledger: it resolves in a later session, on another host,
/// against the same events. What the session table adds is *stability* — once a reference has
/// been shown to the reader, a later event that shares its prefix takes a longer one, and the
/// reference the reader can see keeps meaning the event they saw.
#[derive(Debug, Clone, Default)]
pub struct EventReferences {
    issued: Vec<EventRef>,
}

impl EventReferences {
    /// A session with nothing issued yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many references the session has issued.
    #[must_use]
    pub fn len(&self) -> usize {
        self.issued.len()
    }

    /// Whether the session has issued none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.issued.is_empty()
    }

    /// Every reference issued, in the order they were issued.
    pub fn issued(&self) -> impl Iterator<Item = &EventRef> {
        self.issued.iter()
    }

    /// The reference for `event`, minting one where the session has not shown it before.
    ///
    /// The same event always gets the same reference inside one session, so a reader who scrolls
    /// back sees the reference they were given.
    pub fn reference(&mut self, event: &TemporalEvent) -> EventRef {
        if let Some(held) = self.issued.iter().find(|held| held.full == event.event_id) {
            return held.clone();
        }
        // The identity reads `e` followed by hex, so a reference of `n` hex digits is the first
        // `n + 1` characters of it: `MIN_REFERENCE_DIGITS` of 2 is `@e42`.
        let identity = event.event_id.as_str();
        let mut length = (MIN_REFERENCE_DIGITS + 1).min(identity.len());
        while length < identity.len()
            && self
                .issued
                .iter()
                .any(|held| shares_prefix(held.as_str(), &identity[..length]))
        {
            length += 1;
        }
        let minted = EventRef {
            short: Arc::from(&identity[..length]),
            full: event.event_id.clone(),
        };
        self.issued.push(minted.clone());
        minted
    }

    /// The full identity a reference this session issued stands for.
    #[must_use]
    pub fn lookup(&self, reference: &str) -> Option<&EventId> {
        let bare = reference.strip_prefix('@').unwrap_or(reference);
        self.issued
            .iter()
            .find(|held| held.as_str() == bare)
            .map(EventRef::event_id)
    }

    /// The event `reference` names, in this session or in the ledger (§11.6).
    ///
    /// A reference the session issued resolves through the session first, so it keeps meaning the
    /// event the reader saw. Anything else is a prefix the ledger resolves. `Ok(None)` means the
    /// reference names no event held, which is a fact rather than a failure.
    ///
    /// # Errors
    ///
    /// Returns `temporal.invalid_time` where the text is not an event reference at all,
    /// `temporal.ambiguous_event` where a prefix names more than one, and whatever §34 refusal
    /// the ledger raises otherwise.
    pub fn resolve(
        &self,
        reference: &str,
        ledger: &dyn LedgerRead,
    ) -> Result<Option<TemporalEvent>, ErrorValue> {
        if let Some(full) = self.lookup(reference) {
            return ledger.event(full);
        }
        let parsed = EventId::parse(reference.trim()).ok_or_else(|| {
            error::invalid_time(reference, "an event reference is written `@e42` (§11.6)")
        })?;
        ledger.event(&parsed)
    }
}

/// One candidate completion for `at event @`, `why event @` or `inspect event @` (§20.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventCompletion {
    /// The reference to insert.
    pub reference: EventRef,
    /// When it happened, so a reader can tell two candidates apart.
    pub at: Timestamp,
    /// What kind of event it is.
    pub kind: EventKind,
    /// What a person calls the subject, or the kind where there is no subject.
    pub label: Arc<str>,
}

/// The completions §20.4 offers for an event reference at `horizon`, up to `before`.
///
/// Two rules bound the answer. Relevance decides the order: the candidates are the events
/// §11.3's default scope would have shown, ranked by §18.4's priority and then by recency, so a
/// node appearing at the current place outranks a byte counter moving. Permission decides
/// membership: an event inside a scope whose coverage says this reader may not be told is left
/// out entirely, because §20.4 forbids completion from enumerating hidden or unauthorized
/// history — a candidate list is a disclosure.
///
/// # Errors
///
/// Returns whatever §34 refusal the ledger raises.
pub fn completions(
    ledger: &dyn LedgerRead,
    references: &mut EventReferences,
    horizon: &Horizon,
    before: Timestamp,
    limit: usize,
) -> Result<Vec<EventCompletion>, ErrorValue> {
    let window = TimeRange::until(before);
    let found = ledger.events(&EventQuery {
        scope: Some(horizon.scope().clone()),
        subjects: horizon.subjects(),
        kinds: Vec::new(),
        range: window,
        limit: Some(limit.saturating_mul(4).max(limit)),
        order: QueryOrder::Descending,
    })?;

    let withheld = withheld_scopes(ledger, horizon, window)?;
    let mut candidates: Vec<TemporalEvent> = found
        .into_iter()
        .filter(|event| is_default_scope(event, horizon))
        .filter(|event| !withheld.iter().any(|denied| denied.contains(&event.scope)))
        .collect();
    candidates.sort_by(|a, b| {
        classify(a, horizon)
            .sort_key()
            .cmp(&classify(b, horizon).sort_key())
            .then_with(|| {
                b.times
                    .presentation_instant()
                    .cmp(&a.times.presentation_instant())
            })
            .then_with(|| a.event_id.as_str().cmp(b.event_id.as_str()))
    });
    candidates.truncate(limit);

    Ok(candidates
        .iter()
        .map(|event| EventCompletion {
            reference: references.reference(event),
            at: event.times.presentation_instant(),
            kind: event.kind,
            label: event
                .subject
                .as_ref()
                .map_or_else(|| Arc::from(event.kind.as_str()), |s| Arc::from(s.label())),
        })
        .collect())
}

/// The scopes whose history this reader may not be told about over `window` (§20.4, §34).
fn withheld_scopes(
    ledger: &dyn LedgerRead,
    horizon: &Horizon,
    window: TimeRange,
) -> Result<Vec<SpatialScope>, ErrorValue> {
    let intervals = ledger.coverage(&CoverageQuery {
        scope: Some(horizon.scope().clone()),
        capabilities: Vec::new(),
        range: window,
    })?;
    let mut denied: Vec<SpatialScope> = Vec::new();
    for interval in intervals {
        let refused = interval.completeness == TemporalCompleteness::PermissionDenied
            || interval.permission == PermissionState::PermissionDenied;
        if refused && !denied.contains(&interval.scope) {
            denied.push(interval.scope);
        }
    }
    Ok(denied)
}

/// Why a historical place resolved (§14.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionBasis {
    /// An event at or before the active time supports the object having been there.
    HistoricalEvidence,
    /// A present-day name reached the candidate, and nothing says it existed then.
    ///
    /// §14.4: this "MUST" stay distinguishable from historical existence evidence. A reader who
    /// cannot tell the two apart is being told the past looked like the present.
    ResolutionAid,
}

impl ResolutionBasis {
    /// The name a renderer and `inspect` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ResolutionBasis::HistoricalEvidence => "historical_evidence",
            ResolutionBasis::ResolutionAid => "resolution_aid",
        }
    }
}

impl std::fmt::Display for ResolutionBasis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One answer from `find place` in historical context (§14.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalPlaceMatch {
    /// The canonical identity.
    pub id: SpatialId,
    /// What kind of object it is.
    pub object_type: SpatialType,
    /// What a person calls it.
    pub label: Arc<str>,
    /// Why it resolved (§14.4).
    pub basis: ResolutionBasis,
    /// The event that supports it having been there, for `at event` and `why event` (§27.3).
    pub anchor: Option<EventId>,
    /// The instant that event places it at. `None` for a resolution aid.
    pub existed_at: Option<Timestamp>,
}

impl HistoricalPlaceMatch {
    /// Whether the match is evidence the object existed at the active time (§14.4).
    #[must_use]
    pub const fn is_evidence_of_existence(&self) -> bool {
        matches!(self.basis, ResolutionBasis::HistoricalEvidence)
    }
}

/// A present-day name index — the live spatial index, seen from here (§14.4).
///
/// The caller owns it, because this crate reaches for no provider and no live state (§39.3).
pub trait PresentAliases {
    /// The present-day objects whose name matches `text`.
    fn resolve_alias(&self, text: &str) -> Vec<(SpatialId, SpatialType, Arc<str>)>;
}

/// `find place` at a historical instant (§14.4).
///
/// The historical index is searched first: an event at or before `at` naming a subject whose
/// label matches is evidence the object was there, and the match carries the event so `at event`
/// and `why event` reach it. Present-day names are consulted second, and what they contribute is
/// a candidate rather than a claim: a match reached only that way is a
/// [`ResolutionBasis::ResolutionAid`] with no anchor and no instant.
///
/// # Errors
///
/// Returns whatever §34 refusal the ledger raises.
pub fn find_place_at(
    ledger: &dyn LedgerRead,
    scope: &SpatialScope,
    at: Timestamp,
    query: &str,
    aliases: &dyn PresentAliases,
) -> Result<Vec<HistoricalPlaceMatch>, ErrorValue> {
    let events = ledger.events(&EventQuery {
        scope: Some(scope.clone()),
        subjects: Vec::new(),
        kinds: Vec::new(),
        range: TimeRange::until(at),
        limit: None,
        order: QueryOrder::Ascending,
    })?;

    let needle = query.to_lowercase();
    let mut found: Vec<HistoricalPlaceMatch> = Vec::new();
    for event in &events {
        for subject in event.subject.iter().chain(event.related.iter()) {
            let SpatialRef::Resolved {
                id,
                object_type,
                label,
            } = subject
            else {
                continue;
            };
            if !label.to_lowercase().contains(&needle) {
                continue;
            }
            let instant = event.times.presentation_instant();
            match found.iter_mut().find(|held| &held.id == id) {
                Some(held) => {
                    if held.existed_at.is_none_or(|held_at| instant > held_at) {
                        held.existed_at = Some(instant);
                        held.anchor = Some(event.event_id.clone());
                    }
                }
                None => found.push(HistoricalPlaceMatch {
                    id: id.clone(),
                    object_type: *object_type,
                    label: Arc::clone(label),
                    basis: ResolutionBasis::HistoricalEvidence,
                    anchor: Some(event.event_id.clone()),
                    existed_at: Some(instant),
                }),
            }
        }
    }

    for (id, object_type, label) in aliases.resolve_alias(query) {
        if found.iter().any(|held| held.id == id) {
            continue;
        }
        found.push(HistoricalPlaceMatch {
            id,
            object_type,
            label,
            basis: ResolutionBasis::ResolutionAid,
            anchor: None,
            existed_at: None,
        });
    }

    found.sort_by(|a, b| {
        a.basis
            .cmp(&b.basis)
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.id.as_str().cmp(b.id.as_str()))
    });
    Ok(found)
}

/// Whether either reference is a prefix of the other, which is what makes one ambiguous.
fn shares_prefix(a: &str, b: &str) -> bool {
    a.starts_with(b) || b.starts_with(a)
}
