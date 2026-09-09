//! `why` — the causal explanation of spec v0.5 §16.
//!
//! §16.1: "It is not an LLM prompt and MUST NOT require an AI model. The core implementation uses
//! registered causal rules, event relationships, evidence and coverage." Everything below is
//! those four things, and §55.8 makes the absence of a model a release criterion rather than a
//! convenience.
//!
//! Three properties are load-bearing:
//!
//! - **Unknown cause is a success.** §15.7 and §16.7: `cause: None` with evidence, correlations
//!   and gaps listed is a complete answer and carries no error code.
//! - **Causal and correlated stay in separate arrays.** §35.5 requires it, and §16.6 forbids
//!   moving a correlated event into the cause because it looks plausible.
//! - **Ambiguity is refused rather than resolved.** §16.3: where several transitions are equally
//!   relevant, `why` lists the event references and raises `temporal.ambiguous_event`.

use std::collections::BTreeSet;
use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{BootIdentity, SpatialId, SpatialScope};
use ono_temporal_core::{
    CausalLink, CausalRelation, CausalRuleId, ChangeCertainty, CoverageSummary, EventId, EventKind,
    Ordering, TemporalEvent, TemporalGap, error, happens_before, presentation_order, value,
};
use ono_value::{
    Duration, ErrorValue, MapValue, Provenance, RecordBuilder, RecordValue, SchemaId, Value,
    builtin_schemas,
};

use crate::causal::facts;
use crate::causal::registry::CausalEngine;
use crate::causal::rule::CausalContext;

/// How many causal hops the default text rendering shows (§16.7, `temporal.timeline.default_depth`).
pub const DEFAULT_DEPTH: usize = 3;

/// How many events bound the candidate set (`temporal.why.max_candidates`, §33).
pub const DEFAULT_MAX_CANDIDATES: usize = 1000;

/// The kinds §16.3 calls a notable state transition.
const NOTABLE: &[EventKind] = &[
    EventKind::ObjectAppeared,
    EventKind::ObjectChanged,
    EventKind::ObjectDisappeared,
    EventKind::ActionFailed,
    EventKind::ActionCompleted,
];

/// One of §16.2's three forms, and no others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhyRequest {
    /// `why <target> <selector>` — the most recent notable transition of an object (§16.3).
    Target {
        /// The canonical identity the selector resolved to.
        subject: SpatialId,
        /// What a person calls it, for the answer's own words.
        label: Arc<str>,
    },
    /// `why event <event-ref>` — one named event (§16.2).
    Event {
        /// The identity the reference resolved to.
        event: EventId,
    },
    /// `why field <field-name>` — the most recent supported change to a field (§16.2).
    Field {
        /// The spatial object the session is on.
        subject: SpatialId,
        /// What a person calls it.
        label: Arc<str>,
        /// The field being asked about.
        field: Arc<str>,
    },
}

impl WhyRequest {
    /// `why <target> <selector>`.
    #[must_use]
    pub fn target(subject: &SpatialId, label: &str) -> Self {
        Self::Target {
            subject: subject.clone(),
            label: Arc::from(label),
        }
    }

    /// `why event <event-ref>`.
    #[must_use]
    pub fn event(event: &EventId) -> Self {
        Self::Event {
            event: event.clone(),
        }
    }

    /// `why field <field-name>`.
    #[must_use]
    pub fn field(subject: &SpatialId, label: &str, field: &str) -> Self {
        Self::Field {
            subject: subject.clone(),
            label: Arc::from(label),
            field: Arc::from(field),
        }
    }
}

/// What bounds an explanation (§16.7, §32.3, §33).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhyOptions {
    /// The active temporal coordinate the question is asked at (§16.3).
    pub at: Timestamp,
    /// How many causal hops the typed answer carries (§16.7).
    pub depth: usize,
    /// The ceiling on the candidate set (`temporal.why.max_candidates`).
    pub max_candidates: usize,
}

impl WhyOptions {
    /// The defaults, at the active coordinate.
    #[must_use]
    pub fn at(at: Timestamp) -> Self {
        Self {
            at,
            depth: DEFAULT_DEPTH,
            max_candidates: DEFAULT_MAX_CANDIDATES,
        }
    }

    /// `--depth N` (§16.7).
    #[must_use]
    pub fn with_depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// A different candidate ceiling.
    #[must_use]
    pub fn with_max_candidates(mut self, max_candidates: usize) -> Self {
        self.max_candidates = max_candidates;
        self
    }
}

/// The immediate cause, where a registered rule found one (§16.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalNode {
    /// The event at the cause end.
    pub event: EventId,
    /// The class the rule emitted (§15.1).
    pub relation: CausalRelation,
    /// The rule that emitted it, so the claim can be inspected (§15.8).
    pub rule: CausalRuleId,
    /// What the cause event is, in words.
    pub summary: Arc<str>,
    /// When the cause event happened, so §16.5 can draw a clock beside it (§39.3).
    pub at: Option<Timestamp>,
}

/// One step of the chain from the explained event back towards its origin (§16.4, §16.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalStep {
    /// The edge, with its rule, source and evidence.
    pub link: CausalLink,
    /// How many hops from the explained event, counting from one.
    pub depth: usize,
    /// What the cause event is, in words.
    pub summary: Arc<str>,
    /// When the cause event happened (§16.5). `None` where it is outside the window.
    pub at: Option<Timestamp>,
}

/// An association that is not a causal claim (§15.5, §15.6, §16.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalAssociation {
    /// The other event.
    pub event: EventId,
    /// `correlated_with` or `preceded_by`; never a causal class.
    pub relation: CausalRelation,
    /// The correlation rule that found it, or `None` for plain temporal order (§15.6).
    pub rule: Option<CausalRuleId>,
    /// What the other event is, in words.
    pub summary: Arc<str>,
    /// How far it is from the explained event; negative when it is earlier.
    pub offset: Option<Duration>,
    /// When it happened (§16.6). `None` where it is outside the window.
    pub at: Option<Timestamp>,
}

/// Why a state or a change came about — the `ono.causal-explanation/1` value of §16.4.
///
/// `cause` is `None` for §15.7's unknown cause, which is a complete answer rather than an error:
/// the evidence, the correlations and the gaps are all still here.
#[derive(Debug, Clone, PartialEq)]
pub struct CausalExplanation {
    /// The identity the question was asked about. `None` where the question named an event.
    pub subject: Option<SpatialId>,
    /// The event being explained. `None` where nothing notable was recorded.
    pub explained_event: Option<EventId>,
    /// When the explained state or change happened (§16.5, §16.6).
    ///
    /// §16.5 renders `failed at 14:03:17.004` and §16.6 renders `11s before failure`, and §39.3
    /// forbids the renderer resolving an id to find the instant. So it travels here. `None`
    /// where nothing notable was recorded and there is no explained event.
    pub at: Option<Timestamp>,
    /// What is being explained, in words a person asked for.
    pub state_or_change: Arc<str>,
    /// The immediate cause, where a registered rule found one (§15.7).
    pub cause: Option<CausalNode>,
    /// The chain back towards the origin, nearest first (§16.7).
    pub causal_chain: Vec<CausalStep>,
    /// Associations a correlation rule found, kept apart from the chain by §35.5.
    pub correlations: Vec<TemporalAssociation>,
    /// Events the ordering model supports as earlier, with no claim beyond order (§15.6).
    pub preceding: Vec<TemporalAssociation>,
    /// The intervals that materially affect the answer (§7.5).
    pub gaps: Vec<TemporalGap>,
    /// The composed coverage the answer rests on (§8.5).
    pub coverage: CoverageSummary,
    /// Where the answer came from.
    pub provenance: Provenance,
}

impl CausalExplanation {
    /// Whether the answer is §15.7's `cause: unknown`, which is a success.
    #[must_use]
    pub fn is_unknown_cause(&self) -> bool {
        self.cause.is_none()
    }

    /// The `ono.causal-explanation/1` record of §16.4 and §35.5.
    ///
    /// §35.5 fixes the shape: causal, correlated and preceding stay three arrays, so nothing in
    /// a renderer can promote a correlation into a cause. Every nested piece goes through
    /// `ono_temporal_core::value`, which is the one place a temporal field name is spelled.
    ///
    /// # Errors
    ///
    /// Returns `ono.provider_schema_violation` where the contract is not in this build, which
    /// the `ono-value` contract test and `cargo xtask spec-check` both prevent from shipping.
    pub fn to_record(&self) -> Result<RecordValue, ErrorValue> {
        let schema_id = SchemaId::new("ono.causal-explanation", 1);
        let schema = builtin_schemas().get(&schema_id).ok_or_else(|| {
            ErrorValue::new(
                ono_core::ErrorCode::ProviderSchemaViolation,
                "the `ono.causal-explanation/1` contract is not in this build".to_owned(),
            )
        })?;
        let mut chain = Vec::with_capacity(self.causal_chain.len());
        for step in &self.causal_chain {
            chain.push(step_value(step)?);
        }
        let mut gaps = Vec::with_capacity(self.gaps.len());
        for gap in &self.gaps {
            gaps.push(Value::Record(Arc::new(value::gap_record(gap)?)));
        }
        let builder = RecordValue::builder(schema, self.provenance.clone());
        let builder = put(
            builder,
            "subject",
            self.subject
                .as_ref()
                .map_or(Value::Null, |id| Value::string(id.as_str())),
        );
        let builder = put(
            builder,
            "explained_event",
            self.explained_event
                .as_ref()
                .map_or(Value::Null, |id| Value::string(id.as_str())),
        );
        let builder = put(builder, "at", self.at.map_or(Value::Null, Value::Timestamp));
        let builder = put(
            builder,
            "state_or_change",
            Value::string(&self.state_or_change),
        );
        let builder = put(
            builder,
            "cause",
            self.cause.as_ref().map_or(Value::Null, cause_value),
        );
        let builder = put(builder, "causal_chain", Value::list(chain));
        let builder = put(
            builder,
            "correlations",
            Value::list(
                self.correlations
                    .iter()
                    .map(association_value)
                    .collect::<Vec<_>>(),
            ),
        );
        let builder = put(
            builder,
            "preceding",
            Value::list(
                self.preceding
                    .iter()
                    .map(association_value)
                    .collect::<Vec<_>>(),
            ),
        );
        let builder = put(builder, "gaps", Value::list(gaps));
        let builder = put(
            builder,
            "coverage",
            value::coverage_summary(&self.coverage)?,
        );
        let builder = put(builder, "provenance", provenance_value(&self.provenance));
        Ok(builder.build())
    }
}

/// §16.4's `cause`: the event, the class, the rule that emitted it and the words for it.
///
/// `is_causal` and the inverse label travel with the class because §15.6 forbids a renderer
/// re-deriving from the class name whether an edge asserts causation.
fn cause_value(cause: &CausalNode) -> Value {
    let mut map = MapValue::new();
    map.insert("event".into(), Value::string(cause.event.as_str()));
    map.insert("relation".into(), Value::string(cause.relation.as_str()));
    map.insert(
        "inverse".into(),
        Value::string(cause.relation.inverse_label()),
    );
    map.insert("is_causal".into(), Value::Bool(cause.relation.is_causal()));
    map.insert("rule".into(), Value::string(cause.rule.as_str()));
    map.insert("summary".into(), Value::string(&cause.summary));
    map.insert("at".into(), cause.at.map_or(Value::Null, Value::Timestamp));
    Value::Map(Arc::new(map))
}

/// One chain step: the `ono.causal-link/1` the engine emitted, with its depth and its words.
fn step_value(step: &CausalStep) -> Result<Value, ErrorValue> {
    let mut map = MapValue::new();
    map.insert(
        "link".into(),
        Value::Record(Arc::new(value::causal_link_record(&step.link)?)),
    );
    map.insert(
        "depth".into(),
        Value::Int(i128::try_from(step.depth).unwrap_or(i128::MAX)),
    );
    map.insert("event".into(), Value::string(step.link.cause.as_str()));
    map.insert("summary".into(), Value::string(&step.summary));
    map.insert("at".into(), step.at.map_or(Value::Null, Value::Timestamp));
    Ok(Value::Map(Arc::new(map)))
}

/// One association: an event, the class it is held under, and how far it is from the explained
/// event (§16.6). No causal word appears here, because §15.5 gives it none to appear under.
fn association_value(association: &TemporalAssociation) -> Value {
    let mut map = MapValue::new();
    map.insert("event".into(), Value::string(association.event.as_str()));
    map.insert(
        "relation".into(),
        Value::string(association.relation.as_str()),
    );
    map.insert(
        "inverse".into(),
        Value::string(association.relation.inverse_label()),
    );
    map.insert(
        "is_causal".into(),
        Value::Bool(association.relation.is_causal()),
    );
    map.insert(
        "rule".into(),
        association
            .rule
            .as_ref()
            .map_or(Value::Null, |rule| Value::string(rule.as_str())),
    );
    map.insert("summary".into(), Value::string(&association.summary));
    map.insert(
        "offset".into(),
        association.offset.map_or(Value::Null, Value::Duration),
    );
    map.insert(
        "at".into(),
        association.at.map_or(Value::Null, Value::Timestamp),
    );
    Value::Map(Arc::new(map))
}

/// Sets a field the schema declares; an undeclared name is a bug here rather than a caller's.
fn put(builder: RecordBuilder, name: &str, value: Value) -> RecordBuilder {
    let fallback = builder.clone();
    builder.set(name, value).unwrap_or(fallback)
}

/// Provenance as the nested record every schema of §35 declares (v0.2 §25.2).
fn provenance_value(provenance: &Provenance) -> Value {
    let mut map = MapValue::new();
    map.insert("provider".into(), Value::string(provenance.provider()));
    map.insert(
        "observed".into(),
        provenance.observed().map_or(Value::Null, Value::Timestamp),
    );
    map.insert(
        "source".into(),
        provenance.source().map_or(Value::Null, Value::string),
    );
    map.insert("link".into(), Value::string(&provenance.link().to_string()));
    map.insert(
        "schema".into(),
        Value::string(&provenance.schema().to_string()),
    );
    Value::Map(Arc::new(map))
}

impl CausalEngine {
    /// The explanation for one of §16.2's three questions.
    ///
    /// # Errors
    ///
    /// `temporal.ambiguous_event` where several transitions are equally relevant (§16.3), and
    /// `temporal.not_recorded` where a named event is not in the window at or before the active
    /// coordinate.
    pub fn explain(
        &self,
        request: &WhyRequest,
        events: &[TemporalEvent],
        context: &CausalContext,
        options: &WhyOptions,
    ) -> Result<CausalExplanation, ErrorValue> {
        let mut candidates: Vec<TemporalEvent> = events
            .iter()
            .filter(|event| facts::instant(event) <= options.at)
            .cloned()
            .collect();
        presentation_order(&mut candidates);
        if candidates.len() > options.max_candidates {
            let excess = candidates.len() - options.max_candidates;
            candidates.drain(..excess);
        }

        let explained = match request {
            WhyRequest::Event { event } => Some(
                candidates
                    .iter()
                    .find(|candidate| &candidate.event_id == event)
                    .ok_or_else(|| {
                        error::not_recorded(&scope_hint(&candidates), options.at, &[])
                    })?,
            ),
            WhyRequest::Target { subject, .. } => most_recent(&candidates, |event| {
                NOTABLE.contains(&event.kind)
                    && facts::mentions(event, subject)
                    && !event.evidence.is_empty()
            })?,
            WhyRequest::Field { subject, field, .. } => most_recent(&candidates, |event| {
                facts::mentions(event, subject)
                    && !event.evidence.is_empty()
                    && event.changed_fields.iter().any(|change| {
                        change.field.as_ref() == field.as_ref() && supported(change.certainty)
                    })
            })?,
        };

        let subject = match request {
            WhyRequest::Event { .. } => explained.and_then(facts::subject_id).cloned(),
            WhyRequest::Target { subject, .. } | WhyRequest::Field { subject, .. } => {
                Some(subject.clone())
            }
        };

        let Some(explained) = explained else {
            return Ok(CausalExplanation {
                subject,
                explained_event: None,
                at: None,
                state_or_change: Arc::from(nothing_recorded(request).as_str()),
                cause: None,
                causal_chain: Vec::new(),
                correlations: Vec::new(),
                preceding: Vec::new(),
                gaps: context.coverage().gaps().to_vec(),
                coverage: context.coverage().clone(),
                provenance: provenance(),
            });
        };

        let links = self.links(&candidates, context);
        let index = CausalEngine::by_effect(&links);
        let describe = |id: &EventId| -> Arc<str> {
            candidates
                .iter()
                .find(|event| &event.event_id == id)
                .map_or_else(|| Arc::from("unknown event"), summarise)
        };
        // §16.5 draws a clock beside every node, and §39.3 forbids the renderer resolving an id
        // to find one, so the instant is read here where the events are and travels with the
        // node. `None` for a cause outside the window, which stays visibly undated.
        let dated = |id: &EventId| -> Option<Timestamp> {
            candidates
                .iter()
                .find(|event| &event.event_id == id)
                .map(facts::instant)
        };

        // The causal chain: a bounded walk backwards along causal edges only (§16.7).
        let mut causal_chain = Vec::new();
        let mut visited: BTreeSet<EventId> = BTreeSet::new();
        visited.insert(explained.event_id.clone());
        let mut frontier = vec![explained.event_id.clone()];
        for depth in 1..=options.depth {
            let mut next = Vec::new();
            for effect in &frontier {
                for link in index.get(effect).into_iter().flatten() {
                    if !link.relation.is_causal() {
                        continue;
                    }
                    causal_chain.push(CausalStep {
                        link: (*link).clone(),
                        depth,
                        summary: describe(&link.cause),
                        at: dated(&link.cause),
                    });
                    if visited.insert(link.cause.clone()) {
                        next.push(link.cause.clone());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }

        let cause = causal_chain
            .iter()
            .filter(|step| step.depth == 1)
            // Strongest evidence first; then the more direct claim, because §15.2's `caused_by`
            // says the source stated the effect and §15.3's `triggered_by` says it started
            // something that reached it; then the rule id, so the choice never depends on the
            // order the rules happened to run in.
            .max_by(|left, right| {
                left.link
                    .strength
                    .cmp(&right.link.strength)
                    .then_with(|| right.link.relation.cmp(&left.link.relation))
                    .then_with(|| right.link.rule.cmp(&left.link.rule))
                    .then_with(|| right.link.cause.cmp(&left.link.cause))
            })
            .map(|step| CausalNode {
                event: step.link.cause.clone(),
                relation: step.link.relation,
                rule: step.link.rule.clone(),
                summary: Arc::clone(&step.summary),
                at: step.at,
            });

        // §35.5: correlations are a separate array, and nothing ever moves between the two.
        let anchor = facts::instant(explained);
        let mut correlations: Vec<TemporalAssociation> = links
            .iter()
            .filter(|link| link.relation == CausalRelation::CorrelatedWith)
            .filter_map(|link| {
                let other = if link.effect == explained.event_id {
                    &link.cause
                } else if link.cause == explained.event_id {
                    &link.effect
                } else {
                    return None;
                };
                Some(TemporalAssociation {
                    event: other.clone(),
                    relation: CausalRelation::CorrelatedWith,
                    rule: Some(link.rule.clone()),
                    summary: describe(other),
                    offset: offset_between(&candidates, other, anchor),
                    at: dated(other),
                })
            })
            .collect();
        correlations.sort_by(|left, right| left.event.cmp(&right.event));
        correlations.dedup_by(|left, right| left.event == right.event && left.rule == right.rule);

        // §15.6: order and nothing more. `happens_before` answers from a source sequence or a
        // monotonic reading, never from wall time, so an event that only *looks* earlier is not
        // here at all.
        let named: BTreeSet<&EventId> = causal_chain
            .iter()
            .map(|step| &step.link.cause)
            .chain(correlations.iter().map(|association| &association.event))
            .collect();
        let mut preceding: Vec<TemporalAssociation> = candidates
            .iter()
            .filter(|event| event.event_id != explained.event_id)
            .filter(|event| !named.contains(&event.event_id))
            .filter(|event| happens_before(event, explained).0 == Ordering::Before)
            .map(|event| TemporalAssociation {
                event: event.event_id.clone(),
                relation: CausalRelation::PrecededBy,
                rule: None,
                summary: summarise(event),
                offset: Some(span(facts::instant(event), anchor)),
                at: Some(facts::instant(event)),
            })
            .collect();
        preceding.sort_by(|left, right| {
            right
                .offset
                .cmp(&left.offset)
                .then_with(|| left.event.cmp(&right.event))
        });

        Ok(CausalExplanation {
            subject,
            explained_event: Some(explained.event_id.clone()),
            at: Some(anchor),
            state_or_change: summarise(explained),
            cause,
            causal_chain,
            correlations,
            preceding,
            gaps: context.coverage().gaps().to_vec(),
            coverage: context.coverage().clone(),
            provenance: provenance(),
        })
    }
}

/// Whether a field change is one §16.2 calls supported.
///
/// §6.2 keeps the reason a side is null with the change. A change whose certainty is `unknown`
/// has a side with no evidence at all, and an `inferred` one was filled from an interval by
/// reconstruction (§9.2); neither is a change a source stated, so `why field` does not explain
/// one as though it were.
const fn supported(certainty: ChangeCertainty) -> bool {
    matches!(
        certainty,
        ChangeCertainty::Observed | ChangeCertainty::Derived
    )
}

/// The most recent event matching `select`, refusing where several are equally recent (§16.3).
fn most_recent(
    candidates: &[TemporalEvent],
    select: impl Fn(&TemporalEvent) -> bool,
) -> Result<Option<&TemporalEvent>, ErrorValue> {
    let matching: Vec<&TemporalEvent> = candidates.iter().filter(|event| select(event)).collect();
    let Some(latest) = matching.iter().map(|event| facts::instant(event)).max() else {
        return Ok(None);
    };
    let equally_relevant: Vec<&TemporalEvent> = matching
        .into_iter()
        .filter(|event| facts::instant(event) == latest)
        .collect();
    if equally_relevant.len() > 1 {
        let references: Vec<EventId> = equally_relevant
            .iter()
            .map(|event| event.event_id.clone())
            .collect();
        return Err(error::ambiguous_event(&references));
    }
    Ok(equally_relevant.into_iter().next())
}

/// What an event is, in the words an explanation uses.
fn summarise(event: &TemporalEvent) -> Arc<str> {
    let label = event
        .subject
        .as_ref()
        .map_or("the session", ono_temporal_core::SpatialRef::label);
    let detail = event
        .changed_fields
        .first()
        .map(|change| {
            format!(
                " {} {} -> {}",
                change.field,
                render(change.before.as_ref()),
                render(change.after.as_ref())
            )
        })
        .unwrap_or_default();
    Arc::from(format!("{label} {}{detail}", event.kind).as_str())
}

/// A field side, or `unknown` where nothing observed it (§6.2, §35.3).
fn render(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "unknown".to_owned(),
        Some(Value::String(text)) => text.to_string(),
        Some(other) => other.to_string(),
    }
}

/// The words for a question nothing answers.
fn nothing_recorded(request: &WhyRequest) -> String {
    match request {
        WhyRequest::Target { label, .. } => {
            format!("{label} has no recorded state transition at or before the coordinate")
        }
        WhyRequest::Field { label, field, .. } => {
            format!("{label} has no supported change to {field} at or before the coordinate")
        }
        WhyRequest::Event { event } => format!("{event} is not in the window"),
    }
}

/// The signed distance from `anchor` to the event named `id`.
fn offset_between(
    candidates: &[TemporalEvent],
    id: &EventId,
    anchor: Timestamp,
) -> Option<Duration> {
    candidates
        .iter()
        .find(|event| &event.event_id == id)
        .map(|event| span(facts::instant(event), anchor))
}

/// How far `at` is from `anchor`; negative when it is earlier.
fn span(at: Timestamp, anchor: Timestamp) -> Duration {
    Duration::from_nanoseconds(at.as_nanosecond() - anchor.as_nanosecond())
}

/// The scope a refusal names when the window holds no event to take one from.
fn scope_hint(candidates: &[TemporalEvent]) -> SpatialScope {
    candidates.first().map_or_else(
        || SpatialScope::host("localhost", BootIdentity::unknown_boot("localhost")),
        |event| event.scope.clone(),
    )
}

/// Where an explanation came from.
fn provenance() -> Provenance {
    Provenance::local(
        "ono.temporal-query",
        SchemaId::new("ono.causal-explanation", 1),
    )
}
