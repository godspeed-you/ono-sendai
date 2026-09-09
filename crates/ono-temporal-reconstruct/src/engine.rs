//! The reconstruction engine of v0.5 §9.1.
//!
//! The six steps, in the order the specification gives them:
//!
//! 1. select the nearest trusted checkpoint at or before `T` for the relevant scope (§42.1);
//! 2. apply ordered compatible events from that checkpoint through `T` (§26, [`crate::replay`]);
//! 3. merge provider-owned historical answers that speak to fields at `T` (§21.4);
//! 4. reconcile identity and relations through the canonical v0.4 rules (§5);
//! 5. compute coverage and gaps (§8.5, §7.5);
//! 6. return typed reconstructed objects with provenance (§9.4).
//!
//! With no checkpoint it reconstructs from events alone "when coverage and event semantics
//! support it", and refuses when they do not — which is most of what the module is. Two rules do
//! that refusing:
//!
//! - a value observed before `T` holds *at* `T` only where a source's own coverage was complete
//!   over the stretch in between (§9.2). Otherwise the field is
//!   [`crate::FieldKnowledge::UnknownInInterval`], naming what was last and next seen.
//! - existence and relation presence follow the same rule, and an absence needs
//!   [`CoverageSummary::can_prove_absence`] (§7.4). Nothing observed is not nothing there.
//!
//! Nothing here reads a clock. `T` is a parameter and so is every window bound (§39.2), which is
//! what makes the same inputs produce the same output whenever the test runs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{Confidence, SpatialId, SpatialScope, SpatialType};
use ono_temporal_core::{
    ChangeCertainty, Checkpoint, CheckpointId, CoverageQuery, CoverageSummary, EventKind,
    EventQuery, EvidenceId, EvidenceSource, EvidenceStrength, GapReason, LedgerRead, SpatialRef,
    TemporalCompleteness, TemporalCoverage, TemporalEvent, TemporalGap, TimeRange, relation_of,
};
use ono_value::{ErrorValue, RecordValue, Value};

use crate::answers::HistoricalAnswers;
use crate::capability;
use crate::checkpoint::nearest_trusted;
use crate::collection::ReconstructedCollection;
use crate::field::{FieldKnowledge, Observation, Presence, ReconstructedField};
use crate::object::{ReconstructedObject, UnresolvedSubject};
use crate::relation::ReconstructedRelation;
use crate::replay::replay_order;
use crate::structure::{SourceMatrix, is_path_structure};

/// What a caller wants reconstructed, and for when (§9.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructionRequest {
    scope: SpatialScope,
    at: Timestamp,
    since: Option<Timestamp>,
}

impl ReconstructionRequest {
    /// The state of `scope` at `at`.
    #[must_use]
    pub const fn new(scope: SpatialScope, at: Timestamp) -> Self {
        Self {
            scope,
            at,
            since: None,
        }
    }

    /// The instant the coverage window opens at.
    ///
    /// Where a caller names none, the window opens at the chosen checkpoint, or at the earliest
    /// event at or before `at`, or at `at` itself — never earlier than there is evidence for,
    /// because a window is what coverage is composed over and an unbounded one would be a claim
    /// about all time.
    #[must_use]
    pub const fn since(mut self, from: Timestamp) -> Self {
        self.since = Some(from);
        self
    }

    /// The boundary being reconstructed.
    #[must_use]
    pub const fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// The instant being reconstructed.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }
}

/// The reconstruction engine (§9.1).
#[derive(Debug, Clone, Copy)]
pub struct Reconstructor<'a> {
    ledger: &'a dyn LedgerRead,
    answers: Option<&'a HistoricalAnswers>,
    sources: Option<&'a SourceMatrix>,
}

impl<'a> Reconstructor<'a> {
    /// A reconstruction over `ledger` and nothing else.
    #[must_use]
    pub const fn new(ledger: &'a dyn LedgerRead) -> Self {
        Self {
            ledger,
            answers: None,
            sources: None,
        }
    }

    /// The reconstruction, merging provider-owned historical answers as §9.1 step 3 asks.
    #[must_use]
    pub const fn with_answers(mut self, answers: &'a HistoricalAnswers) -> Self {
        self.answers = Some(answers);
        self
    }

    /// The reconstruction, knowing what each source declared it can answer (§21.1).
    ///
    /// §14.5's refusal reads this: without it, no source carries historical path structure.
    #[must_use]
    pub const fn with_sources(mut self, sources: &'a SourceMatrix) -> Self {
        self.sources = Some(sources);
        self
    }

    /// Reconstructs the state of the requested scope at the requested instant (§9.1).
    ///
    /// # Errors
    ///
    /// Returns whatever the ledger reports — a §34 store refusal.
    pub fn reconstruct(
        &self,
        request: &ReconstructionRequest,
    ) -> Result<ReconstructedWorld, ErrorValue> {
        let at = request.at;
        let checkpoint = nearest_trusted(self.ledger, &request.scope, at)?;
        let query_from = request
            .since
            .or_else(|| checkpoint.as_ref().map(|found| found.captured_at));

        let events = replay_order(self.ledger.events(&EventQuery {
            scope: Some(request.scope.clone()),
            range: TimeRange {
                from: query_from,
                until: None,
            },
            ..EventQuery::default()
        })?);

        let window_from = window_start(request, checkpoint.as_ref(), &events);
        let intervals = self.intervals(request, checkpoint.as_ref(), window_from)?;
        let window = TimeRange::between(window_from, at);
        let coverage = CoverageSummary::compose(&intervals, window);
        let strengths = self.strengths(&events)?;

        let mut state = State::new(request.scope.clone(), at, window_from);
        if let Some(found) = &checkpoint {
            state.seed_from(found);
        }
        state.replay(&events, &strengths);
        if let Some(answers) = self.answers {
            state.merge(answers);
        }
        state.finish(&intervals, &coverage, self.sources, checkpoint.as_ref())
    }

    /// The coverage intervals that bear on the window, plus the chosen checkpoint's own (§42.4).
    fn intervals(
        &self,
        request: &ReconstructionRequest,
        checkpoint: Option<&Checkpoint>,
        window_from: Timestamp,
    ) -> Result<Vec<TemporalCoverage>, ErrorValue> {
        let mut intervals = self.ledger.coverage(&CoverageQuery {
            scope: Some(request.scope.clone()),
            capabilities: Vec::new(),
            range: TimeRange::since(window_from),
        })?;
        if let Some(found) = checkpoint {
            for interval in &found.coverage {
                if !intervals.contains(interval) {
                    intervals.push(interval.clone());
                }
            }
        }
        Ok(intervals)
    }

    /// How strongly each cited evidence record supports its claim (§7.2).
    fn strengths(
        &self,
        events: &[TemporalEvent],
    ) -> Result<BTreeMap<EvidenceId, EvidenceStrength>, ErrorValue> {
        let ids: Vec<EvidenceId> = events
            .iter()
            .flat_map(|event| event.evidence.iter().cloned())
            .collect();
        if ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        Ok(self
            .ledger
            .evidence(&ids)?
            .into_iter()
            .map(|record| (record.evidence_id, record.strength))
            .collect())
    }
}

/// The instant the coverage window opens at.
fn window_start(
    request: &ReconstructionRequest,
    checkpoint: Option<&Checkpoint>,
    events: &[TemporalEvent],
) -> Timestamp {
    let from = request
        .since
        .or_else(|| checkpoint.map(|found| found.captured_at))
        .or_else(|| {
            events
                .iter()
                .map(|event| event.times.presentation_instant())
                .filter(|instant| *instant <= request.at)
                .min()
        })
        .unwrap_or(request.at);
    from.min(request.at)
}

/// One reading of one field, before the §9.2 rule decides what it supports.
#[derive(Debug, Clone)]
struct Reading {
    at: Timestamp,
    value: Value,
    source: EvidenceSource,
    strength: EvidenceStrength,
    valid_from: Option<Timestamp>,
    valid_until: Option<Timestamp>,
    /// Whether a provider's own historical answer produced it (§9.1 step 3).
    answered: bool,
}

/// What one observation says about whether something was there.
#[derive(Debug, Clone)]
struct Support {
    at: Timestamp,
    sequence: usize,
    present: bool,
    source: EvidenceSource,
    from_checkpoint: bool,
}

/// One object as replay is building it.
#[derive(Debug, Clone)]
struct Working {
    object_type: SpatialType,
    label: Arc<str>,
    supports: Vec<Support>,
    readings: BTreeMap<Arc<str>, Vec<Reading>>,
    record: Option<RecordValue>,
    record_at: Option<Timestamp>,
    sources: BTreeSet<EvidenceSource>,
}

impl Working {
    fn new(object_type: SpatialType, label: Arc<str>) -> Self {
        Self {
            object_type,
            label,
            supports: Vec::new(),
            readings: BTreeMap::new(),
            record: None,
            record_at: None,
            sources: BTreeSet::new(),
        }
    }

    fn observe_record(&mut self, record: &RecordValue, at: Timestamp) {
        if self.record_at.is_none_or(|held| at >= held) {
            self.record = Some(record.clone());
            self.record_at = Some(at);
        }
    }

    fn read(&mut self, field: &str, reading: Reading) {
        self.sources.insert(reading.source.clone());
        self.readings
            .entry(Arc::from(field))
            .or_default()
            .push(reading);
    }

    /// The support that stands at `at`, by instant and then by replay position.
    fn last_support(&self, at: Timestamp) -> Option<&Support> {
        self.supports
            .iter()
            .filter(|support| support.at <= at)
            .max_by_key(|support| (support.at, support.sequence))
    }
}

/// One relationship as replay is building it.
#[derive(Debug, Clone)]
struct WorkingEdge {
    supports: Vec<Support>,
    confidence: Confidence,
    sources: BTreeSet<EvidenceSource>,
}

/// The working state of one reconstruction.
struct State {
    scope: SpatialScope,
    at: Timestamp,
    window_from: Timestamp,
    objects: BTreeMap<SpatialId, Working>,
    edges: BTreeMap<(SpatialId, SpatialId, Arc<str>), WorkingEdge>,
    unresolved: BTreeMap<(Arc<str>, Arc<str>), UnresolvedSubject>,
    sequence: usize,
}

impl State {
    fn new(scope: SpatialScope, at: Timestamp, window_from: Timestamp) -> Self {
        Self {
            scope,
            at,
            window_from,
            objects: BTreeMap::new(),
            edges: BTreeMap::new(),
            unresolved: BTreeMap::new(),
            sequence: 0,
        }
    }

    /// §9.1 step 1: the checkpoint's own objects and relations, as its sources observed them.
    fn seed_from(&mut self, checkpoint: &Checkpoint) {
        for object in &checkpoint.objects {
            let working = self
                .objects
                .entry(object.id.clone())
                .or_insert_with(|| Working::new(object.object_type, Arc::clone(&object.label)));
            working.supports.push(Support {
                at: object.observed_at,
                sequence: 0,
                present: true,
                source: object.source.clone(),
                from_checkpoint: true,
            });
            working.sources.insert(object.source.clone());
            working.observe_record(&object.record, object.observed_at);
            let fields: Vec<Arc<str>> = object
                .record
                .schema()
                .fields()
                .iter()
                .map(|field| Arc::from(field.name()))
                .collect();
            for name in fields {
                let Some(value) = object.record.get(&name).filter(|held| !held.is_null()) else {
                    continue;
                };
                working.read(
                    &name,
                    Reading {
                        at: object.observed_at,
                        value: value.clone(),
                        source: object.source.clone(),
                        strength: EvidenceStrength::Observational,
                        valid_from: None,
                        valid_until: None,
                        answered: false,
                    },
                );
            }
        }
        for edge in &checkpoint.relations {
            let key = (
                edge.from.clone(),
                edge.to.clone(),
                Arc::clone(&edge.relation),
            );
            let held = self.edges.entry(key).or_insert_with(|| WorkingEdge {
                supports: Vec::new(),
                confidence: edge.confidence,
                sources: BTreeSet::new(),
            });
            held.supports.push(Support {
                at: edge.observed_at,
                sequence: 0,
                present: true,
                source: edge.source.clone(),
                from_checkpoint: true,
            });
            held.sources.insert(edge.source.clone());
        }
    }

    /// §9.1 step 2 and step 4: the ordered events, reconciled onto canonical identities.
    fn replay(
        &mut self,
        events: &[TemporalEvent],
        strengths: &BTreeMap<EvidenceId, EvidenceStrength>,
    ) {
        let at = self.at;
        for event in events {
            if event.times.presentation_instant() <= at {
                self.sequence += 1;
                self.apply(event, strengths);
            }
        }
        // Readings after `T` are what §9.2's "next observed" names. They create nothing: an
        // object whose only evidence is later than `T` did not exist at `T` as far as this
        // reconstruction knows.
        for event in events {
            if event.times.presentation_instant() > at {
                self.apply_future(event, strengths);
            }
        }
    }

    fn apply(&mut self, event: &TemporalEvent, strengths: &BTreeMap<EvidenceId, EvidenceStrength>) {
        let at = event.times.presentation_instant();
        let source = source_of(event);
        let strength = strength_of(event, strengths);
        match event.kind {
            EventKind::RelationAdded | EventKind::RelationRemoved => {
                self.apply_relation(event, at, &source);
                return;
            }
            _ => {}
        }
        let Some(subject) = &event.subject else {
            return;
        };
        let Some((id, object_type, label)) = resolved_parts(subject) else {
            self.record_unresolved(subject, event);
            return;
        };
        let sequence = self.sequence;
        let working = self
            .objects
            .entry(id)
            .or_insert_with(|| Working::new(object_type, label));
        working.sources.insert(source.clone());
        match event.kind {
            EventKind::ObjectObserved | EventKind::ObjectAppeared | EventKind::ObjectChanged => {
                working.supports.push(Support {
                    at,
                    sequence,
                    present: true,
                    source: source.clone(),
                    from_checkpoint: false,
                });
            }
            EventKind::ObjectDisappeared => {
                working.supports.push(Support {
                    at,
                    sequence,
                    present: false,
                    source: source.clone(),
                    from_checkpoint: false,
                });
            }
            _ => return,
        }
        read_event_fields(working, event, at, &source, strength, false);
    }

    /// An event after `T`: its readings answer §9.2's "next observed" and nothing else.
    fn apply_future(
        &mut self,
        event: &TemporalEvent,
        strengths: &BTreeMap<EvidenceId, EvidenceStrength>,
    ) {
        let at = event.times.presentation_instant();
        let source = source_of(event);
        let strength = strength_of(event, strengths);
        let Some(subject) = &event.subject else {
            return;
        };
        let Some((id, _, _)) = resolved_parts(subject) else {
            return;
        };
        let Some(working) = self.objects.get_mut(&id) else {
            return;
        };
        read_event_fields(working, event, at, &source, strength, true);
    }

    fn apply_relation(&mut self, event: &TemporalEvent, at: Timestamp, source: &EvidenceSource) {
        let Some((relation, confidence)) = relation_of(event) else {
            return;
        };
        let (Some(from), Some(to)) = (
            event.subject.as_ref().and_then(SpatialRef::spatial_id),
            event.related.first().and_then(SpatialRef::spatial_id),
        ) else {
            return;
        };
        let sequence = self.sequence;
        let held = self
            .edges
            .entry((from.clone(), to.clone(), relation))
            .or_insert_with(|| WorkingEdge {
                supports: Vec::new(),
                confidence,
                sources: BTreeSet::new(),
            });
        held.confidence = confidence;
        held.sources.insert(source.clone());
        held.supports.push(Support {
            at,
            sequence,
            present: event.kind == EventKind::RelationAdded,
            source: source.clone(),
            from_checkpoint: false,
        });
    }

    fn record_unresolved(&mut self, subject: &SpatialRef, event: &TemporalEvent) {
        let SpatialRef::Unresolved { source, described } = subject else {
            return;
        };
        let key = (Arc::from(source.as_str()), Arc::clone(described));
        self.unresolved
            .entry(key)
            .or_insert_with(|| UnresolvedSubject {
                source: source.clone(),
                described: Arc::clone(described),
                events: Vec::new(),
            })
            .events
            .push(event.event_id.clone());
    }

    /// §9.1 step 3: what a provider answered directly about `T`.
    fn merge(&mut self, answers: &HistoricalAnswers) {
        for answer in answers.iter() {
            if answer.observed_at > self.at && !answer.speaks_to(self.at) {
                continue;
            }
            let sequence = self.sequence;
            let working = self
                .objects
                .entry(answer.subject.clone())
                .or_insert_with(|| Working::new(answer.object_type, Arc::clone(&answer.label)));
            working.sources.insert(answer.source.clone());
            if answer.observed_at <= self.at {
                working.supports.push(Support {
                    at: answer.observed_at,
                    sequence,
                    present: true,
                    source: answer.source.clone(),
                    from_checkpoint: false,
                });
                working.observe_record(&answer.record, answer.observed_at);
            }
            let fields: Vec<Arc<str>> = answer
                .record
                .schema()
                .fields()
                .iter()
                .map(|field| Arc::from(field.name()))
                .collect();
            for name in fields {
                let Some(value) = answer.record.get(&name).filter(|held| !held.is_null()) else {
                    continue;
                };
                working.read(
                    &name,
                    Reading {
                        at: answer.observed_at,
                        value: value.clone(),
                        source: answer.source.clone(),
                        strength: answer.strength,
                        valid_from: answer.valid_from,
                        valid_until: answer.valid_until,
                        answered: answer.speaks_to(self.at),
                    },
                );
            }
        }
    }

    /// §9.1 steps 5 and 6: coverage, gaps, and the typed objects that carry them.
    fn finish(
        self,
        intervals: &[TemporalCoverage],
        coverage: &CoverageSummary,
        sources: Option<&SourceMatrix>,
        checkpoint: Option<&Checkpoint>,
    ) -> Result<ReconstructedWorld, ErrorValue> {
        let window = TimeRange::between(self.window_from, self.at);
        let empty = SourceMatrix::new();
        let matrix = sources.unwrap_or(&empty);
        let mut objects = Vec::with_capacity(self.objects.len());
        let mut refusals = Vec::new();

        for (id, working) in &self.objects {
            let Some(last) = working.last_support(self.at) else {
                continue;
            };
            if is_path_structure(working.object_type)
                && !working.supports.iter().any(|support| {
                    matrix
                        .structure_support(&support.source, support.from_checkpoint)
                        .is_some()
                })
            {
                // §14.5: "no generic v0.5 implementation may pretend that current directory
                // contents represent the past". The refusal is a gap, so it is visible.
                refusals.push(TemporalGap {
                    scope: self.scope.clone(),
                    from: self.window_from,
                    until: self.at,
                    capability: capability::existence(working.object_type),
                    reason: GapReason::Unsupported,
                    source: last.source.clone(),
                    detail: Some(Arc::from("no historical structure evidence")),
                });
                continue;
            }

            let existence = capability::existence(working.object_type);
            let presence = presence_at(last, self.at, intervals, &existence);
            let mut names: Vec<Arc<str>> = working.readings.keys().cloned().collect();
            names.sort_unstable();
            let fields: Vec<ReconstructedField> = names
                .iter()
                .map(|name| {
                    let readings = working.readings.get(name).map_or(&[][..], Vec::as_slice);
                    reconstruct_field(
                        Arc::clone(name),
                        working.object_type,
                        readings,
                        self.at,
                        intervals,
                        window,
                    )
                })
                .collect();

            let mut capabilities: BTreeSet<Arc<str>> = fields
                .iter()
                .map(|field| capability::field(working.object_type, field.name()))
                .collect();
            capabilities.insert(Arc::clone(&existence));
            let own: Vec<TemporalCoverage> = intervals
                .iter()
                .filter(|interval| capabilities.contains(&interval.capability))
                .cloned()
                .collect();
            let own_coverage = CoverageSummary::compose(&own, window);
            let gaps = own_coverage.gaps().to_vec();

            objects.push(ReconstructedObject {
                id: id.clone(),
                object_type: working.object_type,
                label: Arc::clone(&working.label),
                scope: self.scope.clone(),
                presence,
                record: working
                    .record
                    .as_ref()
                    .map(|record| rebuild(record, &fields)),
                fields,
                observed_at: working.record_at.unwrap_or(last.at),
                as_of: self.at,
                lifetime_from: working
                    .supports
                    .iter()
                    .filter(|support| support.present && support.at <= self.at)
                    .map(|support| support.at)
                    .min(),
                lifetime_until: (!last.present).then_some(last.at),
                sources: working.sources.iter().cloned().collect(),
                coverage: own_coverage,
                gaps,
            });
        }

        let mut relations = Vec::new();
        for ((from, to, relation), edge) in &self.edges {
            let Some(last) = edge
                .supports
                .iter()
                .filter(|s| s.at <= self.at)
                .max_by_key(|s| (s.at, s.sequence))
            else {
                continue;
            };
            let name = capability::relation(relation);
            let presence = presence_at(last, self.at, intervals, &name);
            let own: Vec<TemporalCoverage> = intervals
                .iter()
                .filter(|interval| interval.capability == name)
                .cloned()
                .collect();
            let composed = CoverageSummary::compose(&own, window);
            relations.push(ReconstructedRelation::new(
                from.clone(),
                to.clone(),
                Arc::clone(relation),
                presence,
                edge.supports
                    .iter()
                    .filter(|support| support.present && support.at <= self.at)
                    .map(|support| support.at)
                    .min(),
                (!last.present).then_some(last.at),
                edge.confidence,
                composed
                    .completeness_of(&name)
                    .unwrap_or(TemporalCompleteness::Unknown),
                edge.sources.iter().cloned().collect(),
            ));
        }

        let mut gaps = coverage.gaps().to_vec();
        gaps.extend(refusals);

        Ok(ReconstructedWorld {
            scope: self.scope,
            as_of: self.at,
            window,
            objects,
            relations,
            unresolved: self.unresolved.into_values().collect(),
            coverage: coverage.clone(),
            gaps,
            checkpoint: checkpoint.map(|found| found.checkpoint_id.clone()),
        })
    }
}

/// Whether the object or edge that `last` supports was there at `at` (§7.4, §9.5).
///
/// A support at `at` itself is a reading at that instant, which §8.4 says a point sample can
/// carry. A support before it holds forward only where a source's own coverage was complete over
/// the stretch in between; otherwise nothing observed the interval and the answer is unknown.
fn presence_at(
    last: &Support,
    at: Timestamp,
    intervals: &[TemporalCoverage],
    capability: &str,
) -> Presence {
    if last.at == at || complete_span(intervals, capability, last.at, at).is_some() {
        return if last.present {
            Presence::Present
        } else {
            Presence::Absent
        };
    }
    Presence::Unknown
}

/// The complete-coverage span for `capability` that reaches from `from` to `until`, if any.
///
/// §9.2's "if an exhaustive event stream proves no transition occurred until 12:08, the stronger
/// coverage may narrow the uncertainty": this is where that stream is looked for, and its own
/// end is what §9.3's `valid_until` is taken from.
fn complete_span(
    intervals: &[TemporalCoverage],
    capability: &str,
    from: Timestamp,
    until: Timestamp,
) -> Option<(Timestamp, Timestamp)> {
    let mut spans: Vec<(Timestamp, Timestamp)> = intervals
        .iter()
        .filter(|interval| {
            &*interval.capability == capability
                && interval.completeness == TemporalCompleteness::Complete
        })
        .map(|interval| (interval.from, interval.until))
        .collect();
    spans.sort_unstable();
    let mut union: Vec<(Timestamp, Timestamp)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match union.last_mut() {
            Some(held) if start <= held.1 => held.1 = held.1.max(end),
            _ => union.push((start, end)),
        }
    }
    union
        .into_iter()
        .find(|(start, end)| *start <= from && *end >= until)
}

/// §9.2 and §9.3 for one field: what a reading supports at `at`, and over which interval.
fn reconstruct_field(
    name: Arc<str>,
    object_type: SpatialType,
    readings: &[Reading],
    at: Timestamp,
    intervals: &[TemporalCoverage],
    window: TimeRange,
) -> ReconstructedField {
    let capability = capability::field(object_type, &name);
    let own: Vec<TemporalCoverage> = intervals
        .iter()
        .filter(|interval| interval.capability == capability)
        .cloned()
        .collect();
    let completeness = CoverageSummary::compose(&own, window)
        .completeness_of(&capability)
        .unwrap_or(TemporalCompleteness::Unknown);
    let sources: Vec<EvidenceSource> = readings
        .iter()
        .map(|reading| reading.source.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut past: Vec<&Reading> = readings.iter().filter(|reading| reading.at <= at).collect();
    past.sort_by(|left, right| {
        left.strength
            .cmp(&right.strength)
            .then_with(|| left.answered.cmp(&right.answered))
            .then_with(|| left.at.cmp(&right.at))
            .then_with(|| left.source.cmp(&right.source))
    });
    let chosen = past.last().copied();
    let next = readings
        .iter()
        .filter(|reading| reading.at > at)
        .min_by(|left, right| {
            left.at
                .cmp(&right.at)
                .then_with(|| left.source.cmp(&right.source))
        });

    let Some(chosen) = chosen else {
        return ReconstructedField::new(
            name,
            FieldKnowledge::Unobserved,
            completeness,
            ChangeCertainty::Unknown,
            EvidenceStrength::Observational,
            sources,
        );
    };

    // A provider's own validity interval is source semantics, which §9.3 asks for by name.
    if chosen.answered {
        return ReconstructedField::new(
            name,
            FieldKnowledge::Known {
                value: chosen.value.clone(),
                valid_from: chosen.valid_from,
                valid_until: chosen.valid_until,
            },
            completeness,
            ChangeCertainty::Observed,
            chosen.strength,
            sources,
        );
    }

    if chosen.at == at {
        return ReconstructedField::new(
            name,
            FieldKnowledge::Known {
                value: chosen.value.clone(),
                valid_from: None,
                valid_until: None,
            },
            completeness,
            ChangeCertainty::Observed,
            chosen.strength,
            sources,
        );
    }

    if let Some((_, end)) = complete_span(intervals, &capability, chosen.at, at) {
        let valid_until = next.map_or(end, |reading| reading.at.min(end));
        return ReconstructedField::new(
            name,
            FieldKnowledge::Known {
                value: chosen.value.clone(),
                valid_from: Some(chosen.at),
                valid_until: Some(valid_until),
            },
            completeness,
            ChangeCertainty::Derived,
            chosen.strength,
            sources,
        );
    }

    ReconstructedField::new(
        name,
        FieldKnowledge::UnknownInInterval {
            last: Some(Observation {
                value: chosen.value.clone(),
                at: chosen.at,
                source: chosen.source.clone(),
            }),
            next: next.map(|reading| Observation {
                value: reading.value.clone(),
                at: reading.at,
                source: reading.source.clone(),
            }),
        },
        completeness,
        ChangeCertainty::Unknown,
        chosen.strength,
        sources,
    )
}

/// The archived record with the reconstructed values in it.
///
/// A field the reconstruction cannot support becomes [`Value::Null`], which reads back as
/// unknown (v0.2 §10.5) — never a carried-forward value dressed as a reading. The schema's
/// identity fields are the exception and keep what the archive held: an identity component is
/// what makes this object *this* object across time (§5.1), and blanking it would make the
/// record project to a different place than the one it came from.
fn rebuild(record: &RecordValue, fields: &[ReconstructedField]) -> RecordValue {
    let identity: BTreeSet<&str> = record
        .schema()
        .identity()
        .iter()
        .chain(record.schema().identity_fallback())
        .map(|name| &**name)
        .collect();
    let mut builder =
        RecordValue::builder(Arc::clone(record.schema()), record.provenance().clone());
    for declared in record.schema().fields() {
        let name = declared.name();
        let held = record.get(name).cloned();
        let value = match fields.iter().find(|field| field.name() == name) {
            Some(field) if field.value().is_some() => field.to_value(),
            Some(_) if !identity.contains(name) => Value::Null,
            _ => held.unwrap_or(Value::Null),
        };
        builder = builder.set(name, value).unwrap_or_else(|_| {
            RecordValue::builder(Arc::clone(record.schema()), record.provenance().clone())
        });
    }
    for (key, value) in record.extra().iter() {
        builder = builder.set_extra(key, value.clone());
    }
    builder.build()
}

/// The identity, type and label a resolved subject carries (§5.5).
fn resolved_parts(subject: &SpatialRef) -> Option<(SpatialId, SpatialType, Arc<str>)> {
    match subject {
        SpatialRef::Resolved {
            id,
            object_type,
            label,
        } => Some((id.clone(), *object_type, Arc::clone(label))),
        SpatialRef::Unresolved { .. } => None,
    }
}

/// Reads an event's field observations into the working object.
fn read_event_fields(
    working: &mut Working,
    event: &TemporalEvent,
    at: Timestamp,
    source: &EvidenceSource,
    strength: EvidenceStrength,
    future: bool,
) {
    if let Some(Value::Record(record)) = &event.after {
        if !future {
            working.observe_record(record, at);
        }
        let names: Vec<Arc<str>> = record
            .schema()
            .fields()
            .iter()
            .map(|field| Arc::from(field.name()))
            .collect();
        for name in names {
            let Some(value) = record.get(&name).filter(|held| !held.is_null()) else {
                continue;
            };
            working.read(
                &name,
                Reading {
                    at,
                    value: value.clone(),
                    source: source.clone(),
                    strength,
                    valid_from: None,
                    valid_until: None,
                    answered: false,
                },
            );
        }
    }
    for change in &event.changed_fields {
        let Some(value) = change.after.as_ref().filter(|held| !held.is_null()) else {
            continue;
        };
        working.read(
            &change.field,
            Reading {
                at,
                value: value.clone(),
                source: source.clone(),
                strength,
                valid_from: None,
                valid_until: None,
                answered: false,
            },
        );
    }
}

/// The §7.1 source behind an event.
///
/// The evidence it cites owns the answer where it cites any; the provenance's provider is the
/// next honest thing, because that is who reported the record. Where neither names a §7.1 class,
/// the event reached the ledger through this shell and says so rather than borrowing a name.
fn source_of(event: &TemporalEvent) -> EvidenceSource {
    EvidenceSource::parse(event.provenance.provider()).unwrap_or_else(EvidenceSource::session)
}

/// The weakest strength among the evidence an event cites (§7.2).
fn strength_of(
    event: &TemporalEvent,
    strengths: &BTreeMap<EvidenceId, EvidenceStrength>,
) -> EvidenceStrength {
    event
        .evidence
        .iter()
        .filter_map(|id| strengths.get(id).copied())
        .reduce(EvidenceStrength::weakest_of)
        .unwrap_or(EvidenceStrength::Observational)
}

/// The reconstructed state of one scope at one instant (§9.1 step 6).
#[derive(Debug, Clone, PartialEq)]
pub struct ReconstructedWorld {
    scope: SpatialScope,
    as_of: Timestamp,
    window: TimeRange,
    objects: Vec<ReconstructedObject>,
    relations: Vec<ReconstructedRelation>,
    unresolved: Vec<UnresolvedSubject>,
    coverage: CoverageSummary,
    gaps: Vec<TemporalGap>,
    checkpoint: Option<CheckpointId>,
}

impl ReconstructedWorld {
    /// The boundary reconstructed.
    #[must_use]
    pub const fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// The instant reconstructed (§9.4's `as_of`).
    #[must_use]
    pub const fn as_of(&self) -> Timestamp {
        self.as_of
    }

    /// The window coverage was composed over.
    #[must_use]
    pub const fn window(&self) -> TimeRange {
        self.window
    }

    /// Every object with support at or before the requested instant.
    pub fn objects(&self) -> impl Iterator<Item = &ReconstructedObject> {
        self.objects.iter()
    }

    /// One object by identity.
    #[must_use]
    pub fn object(&self, id: &SpatialId) -> Option<&ReconstructedObject> {
        self.objects.iter().find(|object| object.spatial_id() == id)
    }

    /// Every object of one type that reconstruction supports (§9.6).
    pub fn objects_of(
        &self,
        object_type: SpatialType,
    ) -> impl Iterator<Item = &ReconstructedObject> {
        self.objects
            .iter()
            .filter(move |object| object.object_type() == object_type)
    }

    /// Whether an object was there at the requested instant, whether or not it is in the answer.
    ///
    /// §9.7 asks for exactly this distinction: a place that is not in the reconstruction is
    /// `absent` where coverage could have proven it and `unknown` where it could not, and a
    /// caller reporting "place not known at requested time" needs to say which.
    #[must_use]
    pub fn presence_of(&self, id: &SpatialId, object_type: SpatialType) -> Presence {
        if let Some(object) = self.object(id) {
            return object.presence();
        }
        if self.can_prove_absence(&capability::existence(object_type)) {
            Presence::Absent
        } else {
            Presence::Unknown
        }
    }

    /// The edges reconstruction supports at the requested instant (§14.3).
    ///
    /// Only supported edges: "an exit shown by `look` or `near` at time `T` MUST correspond to a
    /// relation or hierarchy supported at `T`", so a caller drawing whatever this yields cannot
    /// leak a current-only exit into a historical neighbourhood.
    pub fn relations(&self) -> impl Iterator<Item = &ReconstructedRelation> {
        self.relations
            .iter()
            .filter(|edge| edge.presence().is_present())
    }

    /// Every edge with support at or before the requested instant, whatever it composed to.
    pub fn all_relations(&self) -> impl Iterator<Item = &ReconstructedRelation> {
        self.relations.iter()
    }

    /// Whether one relation held at the requested instant (§9.5).
    #[must_use]
    pub fn relation(&self, from: &SpatialId, to: &SpatialId, relation: &str) -> Presence {
        if let Some(edge) = self
            .relations
            .iter()
            .find(|edge| edge.from() == from && edge.to() == to && edge.relation() == relation)
        {
            return edge.presence();
        }
        if self.can_prove_absence(&capability::relation(relation)) {
            Presence::Absent
        } else {
            Presence::Unknown
        }
    }

    /// The subjects a source named that Ono could not reconcile (§5.5).
    pub fn unresolved(&self) -> impl Iterator<Item = &UnresolvedSubject> {
        self.unresolved.iter()
    }

    /// The composed coverage of the whole reconstruction (§8.5).
    #[must_use]
    pub const fn coverage(&self) -> &CoverageSummary {
        &self.coverage
    }

    /// Every gap that materially affects the answer (§7.5, §11.7).
    #[must_use]
    pub fn gaps(&self) -> &[TemporalGap] {
        &self.gaps
    }

    /// Whether Ono may claim something of this capability did not exist (§7.4).
    #[must_use]
    pub fn can_prove_absence(&self, capability: &str) -> bool {
        self.coverage.can_prove_absence(capability)
    }

    /// The checkpoint the reconstruction started from, where it found one (§9.1 step 1).
    #[must_use]
    pub const fn checkpoint(&self) -> Option<&CheckpointId> {
        self.checkpoint.as_ref()
    }

    /// Always true: everything here was reconstructed rather than read from the present (§9.4).
    #[must_use]
    pub const fn is_reconstructed(&self) -> bool {
        true
    }

    /// The collection of one type, with the collection-level coverage §9.6 requires.
    #[must_use]
    pub fn collection(&self, object_type: SpatialType) -> ReconstructedCollection {
        let capability = capability::existence(object_type);
        let members = self
            .objects_of(object_type)
            .filter(|object| object.presence().is_present())
            .map(|object| object.spatial_id().clone())
            .collect();
        ReconstructedCollection {
            object_type,
            completeness: self
                .coverage
                .completeness_of(&capability)
                .unwrap_or(TemporalCompleteness::Unknown),
            enumeration_proven: self.coverage.can_prove_absence(&capability),
            gaps: self
                .gaps
                .iter()
                .filter(|gap| gap.capability == capability)
                .cloned()
                .collect(),
            capability,
            members,
        }
    }

    /// §9.4's `temporal` sub-record for the reconstruction as a whole.
    ///
    /// # Errors
    ///
    /// Returns `ono.provider_schema_violation` where the gap contract is not in this build.
    pub fn temporal_metadata(&self) -> Result<Value, ErrorValue> {
        let sources: Vec<EvidenceSource> = self.coverage.sources().cloned().collect();
        ono_temporal_core::value::temporal_metadata(
            self.as_of,
            &self.coverage,
            true,
            &sources,
            &self.gaps,
        )
    }
}
