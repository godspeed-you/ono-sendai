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
//! a prompt, so [`EventReferences`] issues the shortest prefix that names one event *in the
//! ledger it was printed from*, and never lets an issued reference change meaning inside the
//! session. Both halves are needed, and the ledger half is the load-bearing one: "rendered events
//! MUST expose stable references usable in subsequent commands" (§11.6), and the next command
//! runs in a shell whose session table is empty, so it resolves the printed prefix through the
//! ledger's own prefix lookup. A prefix that named one event only among the handful this session
//! had shown would meet `temporal.ambiguous_event` there (§34, ADR-0783).
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
/// Two digits is what `@e42` is, and it is the floor rather than the answer: a reference is
/// spelled this short only where the ledger holds no second event whose identity begins the same
/// way. Anything the ledger cannot tell apart at two digits is spelled longer (ADR-0783).
pub const MIN_REFERENCE_DIGITS: usize = 2;

/// How many hex digits a reference carries beyond the shortest one the ledger can tell apart.
///
/// The ledger is append-only and alive: the shell that printed the row records its own coverage
/// and action events as it exits, and the shell the reader types the reference into records more
/// before it resolves anything. A prefix that named one event the instant it was printed can
/// therefore be taken back by an event nobody had seen yet, and the reader meets
/// `temporal.ambiguous_event` for a reference that was correct on screen. One digit of headroom
/// divides that chance by sixteen and costs one character (ADR-0783).
pub const REFERENCE_HEADROOM: usize = 1;

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
/// against the same events. Two rules decide how long a prefix is, and they are checked in this
/// order (ADR-0783):
///
/// 1. it names at most one event in the ledger it is minted from, which is what makes it usable
///    in the *subsequent command* §11.6 promises, run from a shell with an empty session table;
/// 2. it carries [`REFERENCE_HEADROOM`] beyond that, because the ledger is append-only and alive;
/// 3. it collides with no reference this session has already shown, which is what makes it
///    *stable* — a reference the reader can still see on screen never changes meaning.
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
    /// back sees the reference they were given, and `ledger` is asked how short that reference
    /// may be spelled without naming a second retained event. Rendering several events at once
    /// goes through [`EventReferences::reference_all`], which asks once for all of them.
    pub fn reference(&mut self, ledger: &dyn LedgerRead, event: &TemporalEvent) -> EventRef {
        if let Some(held) = self.held(&event.event_id) {
            return held;
        }
        let length = self.unique_lengths(ledger, std::slice::from_ref(event));
        self.mint(event, length.first().copied())
    }

    /// The references for `events`, asking the ledger once for the whole rendering (§32.3).
    ///
    /// A timeline mints one reference per rendered row, up to §11.4's limit, so the lengths come
    /// back in one batched lookup rather than one lookup per row per lengthening step.
    pub fn reference_all(
        &mut self,
        ledger: &dyn LedgerRead,
        events: &[TemporalEvent],
    ) -> Vec<EventRef> {
        let unseen: Vec<&TemporalEvent> = events
            .iter()
            .filter(|event| self.held(&event.event_id).is_none())
            .collect();
        let lengths = self.unique_lengths_of(ledger, unseen.iter().map(|event| &event.event_id));
        let mut lengths = unseen
            .iter()
            .map(|event| event.event_id.clone())
            .zip(lengths)
            .collect::<Vec<_>>();
        lengths.dedup_by(|left, right| left.0 == right.0);
        events
            .iter()
            .map(|event| match self.held(&event.event_id) {
                Some(held) => held,
                None => {
                    let length = lengths
                        .iter()
                        .find(|(id, _)| id == &event.event_id)
                        .map(|(_, length)| *length);
                    self.mint(event, length)
                }
            })
            .collect()
    }

    /// The reference this session already shows for `id`, where it shows one.
    fn held(&self, id: &EventId) -> Option<EventRef> {
        self.issued.iter().find(|held| &held.full == id).cloned()
    }

    /// How long a prefix of each event's identity the ledger needs to tell it apart (§11.6).
    fn unique_lengths(&self, ledger: &dyn LedgerRead, events: &[TemporalEvent]) -> Vec<usize> {
        self.unique_lengths_of(ledger, events.iter().map(|event| &event.event_id))
    }

    /// The same question, for identities the caller has already gathered.
    ///
    /// A store that cannot answer leaves the answer empty, and [`EventReferences::mint`] then
    /// spells the whole identity: long, and still exactly one event.
    fn unique_lengths_of<'a>(
        &self,
        ledger: &dyn LedgerRead,
        ids: impl Iterator<Item = &'a EventId>,
    ) -> Vec<usize> {
        let ids: Vec<EventId> = ids.cloned().collect();
        ledger
            .shortest_unique_prefixes(&ids, MIN_REFERENCE_DIGITS + 1)
            .unwrap_or_default()
    }

    /// Issues the reference for `event`, `length` being the shortest the ledger can tell apart.
    ///
    /// What is issued is that length plus [`REFERENCE_HEADROOM`], lengthened again where the
    /// session has already shown a reference that shares it.
    ///
    /// `None` is the answer of a ledger that could not be asked, and it spells the whole identity.
    fn mint(&mut self, event: &TemporalEvent, length: Option<usize>) -> EventRef {
        // The identity reads `e` followed by hex, so a reference of `n` hex digits is the first
        // `n + 1` characters of it: `MIN_REFERENCE_DIGITS` of 2 is `@e42`.
        let identity = event.event_id.as_str();
        let floor = (MIN_REFERENCE_DIGITS + 1).min(identity.len());
        let mut length = length
            .map_or(identity.len(), |length| length + REFERENCE_HEADROOM)
            .clamp(floor, identity.len());
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

    let minted = references.reference_all(ledger, &candidates);
    Ok(candidates
        .iter()
        .zip(minted)
        .map(|(event, reference)| EventCompletion {
            reference,
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
