//! The rule interface of spec v0.5 §41, and the two things a rule is given: the candidate events
//! and the evidence behind them.
//!
//! §41 states two obligations. "Rules MUST be deterministic for the same canonical input", which
//! is why [`EventSet`] arrives already in presentation order and why every iteration here is over
//! an ordered collection. And "AI-generated reasoning MUST NOT implement this trait in core
//! v0.5", which needs no code to enforce: a rule reaches evidence only through [`CausalContext`],
//! and a [`ono_temporal_core::Evidence`] record can only come from one of §7.1's nine closed
//! source classes.

use std::collections::BTreeMap;
use std::sync::Arc;

use ono_spatial_core::SpatialId;
use ono_temporal_core::{
    CausalRelation, CausalRuleId, CoverageSummary, EventId, EventKind, Evidence, EvidenceClaim,
    EvidenceId, EvidenceSource, EvidenceStrength, TemporalCapabilities, TemporalEvent,
    TemporalSourceDescription,
};
use ono_value::Duration;

use crate::causal::link::CausalFinding;

/// The minimum evidence strength each input event kind must carry (§15.8, §7.2).
///
/// A kind this does not name is a kind the rule does not read. There is no default minimum,
/// because a rule that accepted an undeclared input would be firing on evidence its registry row
/// never described.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EvidenceRequirements {
    minimums: Vec<(EventKind, EvidenceStrength)>,
}

impl EvidenceRequirements {
    /// The requirements, keyed by event kind.
    #[must_use]
    pub fn new(minimums: &[(EventKind, EvidenceStrength)]) -> Self {
        let mut minimums = minimums.to_vec();
        minimums.sort_by_key(|(kind, _)| *kind);
        minimums.dedup_by_key(|(kind, _)| *kind);
        Self { minimums }
    }

    /// The minimum strength `kind` must carry, or `None` where the rule does not read it.
    #[must_use]
    pub fn minimum_for(&self, kind: EventKind) -> Option<EvidenceStrength> {
        self.minimums
            .iter()
            .find(|(declared, _)| *declared == kind)
            .map(|(_, strength)| *strength)
    }

    /// The kinds the rule reads, in a stable order.
    pub fn kinds(&self) -> impl Iterator<Item = EventKind> + '_ {
        self.minimums.iter().map(|(kind, _)| *kind)
    }

    /// Whether evidence of `strength` satisfies what the rule needs from a `kind` input.
    #[must_use]
    pub fn satisfied_by(&self, kind: EventKind, strength: EvidenceStrength) -> bool {
        self.minimum_for(kind)
            .is_some_and(|minimum| strength >= minimum)
    }

    /// Whether the rule declares no input at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.minimums.is_empty()
    }
}

/// Which §7.1 sources may supply a rule's inputs (§15.8's provider/source constraints).
///
/// A constraint is either a source that names itself — `linux.systemd-dbus` — or one of §7.1's
/// composed forms written as the registry writes it, `adapter:<adapter-id>`. The angle brackets
/// are a placeholder for the composed segment and match any of them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceConstraint(Arc<str>);

impl SourceConstraint {
    /// The constraint, spelled the way `causality.yaml` spells it.
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        Self(Arc::from(pattern))
    }

    /// The pattern as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `source` is one this constraint admits.
    #[must_use]
    pub fn matches(&self, source: &EvidenceSource) -> bool {
        match self.0.split_once('<') {
            None => self.0.as_ref() == source.as_str(),
            Some((prefix, _)) => {
                !prefix.is_empty() && source.as_str().len() > prefix.len() && {
                    source.as_str().starts_with(prefix)
                }
            }
        }
    }
}

/// The seven fields §15.8 requires a rule to record, as the engine holds them.
///
/// This is what makes a rule inspectable: `why` names the rule behind every edge, and a reader
/// who follows the name arrives here and at the matching row of
/// `docs/contracts/temporal/causality.yaml`. A test in this crate holds the two against each
/// other in both directions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDescription {
    /// The registered id (§15.8). `ono.*` is reserved to the project.
    pub rule_id: CausalRuleId,
    /// The event kinds the rule reads.
    pub input_event_kinds: Vec<EventKind>,
    /// The minimum evidence strength each input must carry (§7.2).
    pub required_evidence: EvidenceRequirements,
    /// What must be *equal* for the rule to fire. The substance of a causal rule (§15.2).
    pub identity_constraints: &'static str,
    /// The ordering or bounding constraint, or `None` where the identity join stands alone.
    pub time_constraints: Option<&'static str>,
    /// The §15.1 class the rule emits.
    pub output_relation: CausalRelation,
    /// Which §7.1 sources may supply the inputs.
    pub source_constraints: Vec<SourceConstraint>,
    /// The window a correlation rule declares (§15.5). `None` for a causal rule.
    pub window: Option<Duration>,
}

/// A registered causal rule (§41).
///
/// The three accessors §41 names are derived from [`CausalRule::describe`], so a rule states its
/// seven fields once and cannot describe itself differently from how it behaves.
pub trait CausalRule: std::fmt::Debug + Send + Sync {
    /// The seven fields of §15.8.
    fn describe(&self) -> RuleDescription;

    /// The registered id (§41).
    fn id(&self) -> CausalRuleId {
        self.describe().rule_id
    }

    /// The class the rule emits (§41).
    fn relation(&self) -> CausalRelation {
        self.describe().output_relation
    }

    /// What each input must carry (§41).
    fn required_evidence(&self) -> EvidenceRequirements {
        self.describe().required_evidence
    }

    /// The links this rule finds in `candidate`, given `context` (§41).
    ///
    /// Deterministic for the same canonical input: `candidate` arrives in presentation order and
    /// nothing here may consult a clock, a random source or the order evidence was inserted in.
    fn evaluate(&self, candidate: &EventSet<'_>, context: &CausalContext) -> Vec<CausalFinding>;
}

/// The candidate events a rule is asked about (§41).
///
/// The slice is in the presentation order [`ono_temporal_core::presentation_order`] produces, so
/// two runs over the same events visit them the same way. §26.3 makes that order a display
/// convention rather than an ordering claim, and no rule here treats it as one.
#[derive(Debug, Clone, Copy)]
pub struct EventSet<'a> {
    events: &'a [TemporalEvent],
}

impl<'a> EventSet<'a> {
    /// The set over `events`.
    #[must_use]
    pub fn new(events: &'a [TemporalEvent]) -> Self {
        Self { events }
    }

    /// Every candidate.
    #[must_use]
    pub fn events(&self) -> &'a [TemporalEvent] {
        self.events
    }

    /// The candidates of one kind.
    pub fn of_kind(&self, kind: EventKind) -> impl Iterator<Item = &'a TemporalEvent> {
        self.events.iter().filter(move |event| event.kind == kind)
    }

    /// The candidates of any of several kinds.
    pub fn of_kinds(&self, kinds: &'a [EventKind]) -> impl Iterator<Item = &'a TemporalEvent> {
        self.events
            .iter()
            .filter(move |event| kinds.contains(&event.kind))
    }

    /// The candidate with this identity.
    #[must_use]
    pub fn get(&self, id: &EventId) -> Option<&'a TemporalEvent> {
        self.events.iter().find(|event| &event.event_id == id)
    }

    /// How many candidates there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Everything a rule may consult beside the events themselves (§41).
///
/// Evidence, the temporal capabilities each source advertises (§21.1) and the composed coverage
/// the answer rests on (§8.5). There is no clock here and no store: §39.2 keeps the system clock
/// out of pure logic, and a rule that could read one would be a rule that could reason about
/// "now".
#[derive(Debug, Clone, Default)]
pub struct CausalContext {
    evidence: BTreeMap<EvidenceId, Evidence>,
    capabilities: BTreeMap<EvidenceSource, TemporalCapabilities>,
    coverage: CoverageSummary,
}

impl CausalContext {
    /// The context over `evidence`.
    #[must_use]
    pub fn new(evidence: Vec<Evidence>) -> Self {
        Self {
            evidence: evidence
                .into_iter()
                .map(|record| (record.evidence_id.clone(), record))
                .collect(),
            capabilities: BTreeMap::new(),
            coverage: CoverageSummary::default(),
        }
    }

    /// Records what each source advertises about time (§21.1).
    #[must_use]
    pub fn with_sources(mut self, sources: &[TemporalSourceDescription]) -> Self {
        for description in sources {
            self.capabilities
                .insert(description.source.clone(), description.capabilities.clone());
        }
        self
    }

    /// Records that these sources advertise `causal_tokens` (§21.6).
    ///
    /// §15.8's `ono.provider-causal-token` rule fires only for a source that claims the
    /// capability: "a token from a source that does not claim the capability is not a token".
    #[must_use]
    pub fn with_causal_token_sources(mut self, sources: &[EvidenceSource]) -> Self {
        for source in sources {
            let entry = self.capabilities.entry(source.clone()).or_default();
            entry.causal_tokens = true;
        }
        self
    }

    /// Records the coverage an explanation rests on (§8.5).
    #[must_use]
    pub fn with_coverage(mut self, coverage: CoverageSummary) -> Self {
        self.coverage = coverage;
        self
    }

    /// One evidence record.
    #[must_use]
    pub fn evidence(&self, id: &EvidenceId) -> Option<&Evidence> {
        self.evidence.get(id)
    }

    /// The evidence behind `event`, in the order the event lists it, skipping what the context
    /// does not hold.
    pub fn evidence_of<'a>(
        &'a self,
        event: &'a TemporalEvent,
    ) -> impl Iterator<Item = &'a Evidence> {
        event.evidence.iter().filter_map(|id| self.evidence.get(id))
    }

    /// What `source` advertises about time (§21.1).
    #[must_use]
    pub fn capabilities_of(&self, source: &EvidenceSource) -> Option<&TemporalCapabilities> {
        self.capabilities.get(source)
    }

    /// Whether `source` advertises explicit causal transaction identifiers (§21.6).
    #[must_use]
    pub fn publishes_causal_tokens(&self, source: &EvidenceSource) -> bool {
        self.capabilities_of(source)
            .is_some_and(|capabilities| capabilities.causal_tokens)
    }

    /// The coverage the answer rests on (§8.5).
    #[must_use]
    pub fn coverage(&self) -> &CoverageSummary {
        &self.coverage
    }

    /// Every evidence record, in identity order, so a scan is deterministic.
    pub fn records(&self) -> impl Iterator<Item = &Evidence> {
        self.evidence.values()
    }

    /// The evidence stating that `from` held `relation` to `to`, if a source observed it (§6.4).
    ///
    /// The direction matters: `process.parent_of` read backwards would attribute a parent to its
    /// child. [`Self::relation_touching`] is the reading for a bidirectional relation.
    #[must_use]
    pub fn relation_between(
        &self,
        relation: &str,
        from: &SpatialId,
        to: &SpatialId,
    ) -> Option<&Evidence> {
        self.records().find(|record| {
            record.subject.as_ref() == Some(from) && claims_relation(record, relation, to)
        })
    }

    /// The evidence stating that `a` and `b` are joined by `relation`, in either direction.
    ///
    /// For a relation §11 declares bidirectional — `socket.connected_to` — neither end is the
    /// origin, so neither reading is the wrong one.
    #[must_use]
    pub fn relation_touching(
        &self,
        relation: &str,
        a: &SpatialId,
        b: &SpatialId,
    ) -> Option<&Evidence> {
        self.relation_between(relation, a, b)
            .or_else(|| self.relation_between(relation, b, a))
    }

    /// Any evidence joining `a` and `b` by a spatial relation, in either direction (§15.5).
    ///
    /// This is the *structural* association a correlation rule needs before it may say anything
    /// at all. Without it, two events that happened near each other are two events that happened
    /// near each other.
    #[must_use]
    pub fn any_relation_touching(&self, a: &SpatialId, b: &SpatialId) -> Option<&Evidence> {
        self.records().find(|record| {
            let subject = record.subject.as_ref();
            match &record.claim {
                EvidenceClaim::RelationHeld { other, .. } => {
                    (subject == Some(a) && other == b) || (subject == Some(b) && other == a)
                }
                _ => false,
            }
        })
    }
}

/// Whether `record` claims `relation` to `other`.
fn claims_relation(record: &Evidence, relation: &str, other: &SpatialId) -> bool {
    match &record.claim {
        EvidenceClaim::RelationHeld {
            relation: held,
            other: end,
            ..
        } => held.as_ref() == relation && end == other,
        _ => false,
    }
}
