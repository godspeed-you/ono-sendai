//! The canonical event model of v0.5 §6: the seventeen kinds, the typed field changes and the
//! subject a source named but Ono could not always reconcile.

use std::sync::Arc;

use ono_spatial_core::{Confidence, SpatialId, SpatialScope, SpatialType};
use ono_value::{MapValue, Provenance, Value};

use crate::clock::EventTimes;
use crate::id::{CausalLinkId, EventId, EvidenceId};
use crate::source::EvidenceSource;

/// The top-level class of an event (§6.1).
///
/// The list is closed. "Providers and plugins MAY define namespaced subtypes, but the top-level
/// semantics MUST map to one of these classes", which is what keeps a timeline readable when
/// half of it comes from a package nobody has heard of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventKind {
    /// A source reported the object as present, without claiming it changed.
    ObjectObserved,
    /// Evidence supports that the object became observable (§6.3).
    ObjectAppeared,
    /// A field of the object changed (§6.2).
    ObjectChanged,
    /// Evidence supports that the object ceased to exist or to be represented (§6.3).
    ObjectDisappeared,
    /// A spatial relationship came into being (§6.4).
    RelationAdded,
    /// A spatial relationship ended (§6.4).
    RelationRemoved,
    /// An operator asked for a mutation through the shell (§17.2).
    ActionRequested,
    /// The mutation passed the authorisation gate (§17.2).
    ActionAuthorized,
    /// The mutation was carried out (§17.2).
    ActionExecuted,
    /// The mutation finished successfully (§17.2).
    ActionCompleted,
    /// The mutation did not succeed (§17.2).
    ActionFailed,
    /// Source-native information that does not yet map to a canonical object mutation (§6.5).
    ProviderEvent,
    /// A source began covering a capability over an interval (§8.1).
    CoverageStarted,
    /// A source stopped covering it, which is where a gap begins (§7.5).
    CoverageEnded,
    /// A bounded state projection was captured (§3.6).
    CheckpointCreated,
    /// A landmark became true for a place (§27.1).
    LandmarkAdded,
    /// A landmark stopped being true (§27.1).
    LandmarkRemoved,
}

impl EventKind {
    /// Every kind, in the order §6.1 lists them.
    pub const ALL: &'static [EventKind] = &[
        EventKind::ObjectObserved,
        EventKind::ObjectAppeared,
        EventKind::ObjectChanged,
        EventKind::ObjectDisappeared,
        EventKind::RelationAdded,
        EventKind::RelationRemoved,
        EventKind::ActionRequested,
        EventKind::ActionAuthorized,
        EventKind::ActionExecuted,
        EventKind::ActionCompleted,
        EventKind::ActionFailed,
        EventKind::ProviderEvent,
        EventKind::CoverageStarted,
        EventKind::CoverageEnded,
        EventKind::CheckpointCreated,
        EventKind::LandmarkAdded,
        EventKind::LandmarkRemoved,
    ];

    /// The name §6.1 and `ono.temporal-event/1` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            EventKind::ObjectObserved => "object.observed",
            EventKind::ObjectAppeared => "object.appeared",
            EventKind::ObjectChanged => "object.changed",
            EventKind::ObjectDisappeared => "object.disappeared",
            EventKind::RelationAdded => "relation.added",
            EventKind::RelationRemoved => "relation.removed",
            EventKind::ActionRequested => "action.requested",
            EventKind::ActionAuthorized => "action.authorized",
            EventKind::ActionExecuted => "action.executed",
            EventKind::ActionCompleted => "action.completed",
            EventKind::ActionFailed => "action.failed",
            EventKind::ProviderEvent => "provider.event",
            EventKind::CoverageStarted => "coverage.started",
            EventKind::CoverageEnded => "coverage.ended",
            EventKind::CheckpointCreated => "checkpoint.created",
            EventKind::LandmarkAdded => "landmark.added",
            EventKind::LandmarkRemoved => "landmark.removed",
        }
    }

    /// The kind with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_str() == name)
    }

    /// Whether this kind belongs to §17.2's action lifecycle.
    #[must_use]
    pub fn is_action(self) -> bool {
        matches!(
            self,
            EventKind::ActionRequested
                | EventKind::ActionAuthorized
                | EventKind::ActionExecuted
                | EventKind::ActionCompleted
                | EventKind::ActionFailed
        )
    }
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The class of a difference between two instants (§13.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeClass {
    /// The object did not exist at the earlier instant and did at the later one.
    Added,
    /// The object existed at the earlier instant and did not at the later one.
    Removed,
    /// The object existed at both, and a field differs.
    Changed,
    /// A relationship came into being between the two instants.
    RelationAdded,
    /// A relationship ended between the two instants.
    RelationRemoved,
}

impl ChangeClass {
    /// Every class, in the order §13.2 lists them.
    pub const ALL: &'static [ChangeClass] = &[
        ChangeClass::Added,
        ChangeClass::Removed,
        ChangeClass::Changed,
        ChangeClass::RelationAdded,
        ChangeClass::RelationRemoved,
    ];

    /// The name §13.2 and `ono.temporal-change/1` spell.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            ChangeClass::Added => "added",
            ChangeClass::Removed => "removed",
            ChangeClass::Changed => "changed",
            ChangeClass::RelationAdded => "relation_added",
            ChangeClass::RelationRemoved => "relation_removed",
        }
    }

    /// The class with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|class| class.as_str() == name)
    }
}

/// How well a field change is known (§6.2).
///
/// §6.2 forbids overloading a null side to mean "arbitrary unknown", so the reason a side is
/// null travels with the change instead of being inferred from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeCertainty {
    /// Both sides came from evidence.
    Observed,
    /// One side came from a documented deterministic rule.
    Derived,
    /// Reconstruction filled one side from an interval rather than from a reading (§9.2).
    Inferred,
    /// One side has no evidence at all, so the change is a change only in the reading.
    Unknown,
}

impl ChangeCertainty {
    /// Every certainty, strongest first.
    pub const ALL: &'static [ChangeCertainty] = &[
        ChangeCertainty::Observed,
        ChangeCertainty::Derived,
        ChangeCertainty::Inferred,
        ChangeCertainty::Unknown,
    ];

    /// The name `ono.temporal-event/1` and `ono.temporal-change/1` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeCertainty::Observed => "observed",
            ChangeCertainty::Derived => "derived",
            ChangeCertainty::Inferred => "inferred",
            ChangeCertainty::Unknown => "unknown",
        }
    }

    /// The certainty with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|certainty| certainty.as_str() == name)
    }
}

/// One typed field change (§6.2).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldChange {
    /// The field that changed, under the name its schema gives it.
    pub field: Arc<str>,
    /// What it was. `None` where nothing observed the earlier side.
    pub before: Option<Value>,
    /// What it became. `None` where nothing observed the later side.
    pub after: Option<Value>,
    /// How well the change is known (§6.2).
    pub certainty: ChangeCertainty,
}

/// What an event happened to.
///
/// A resolved subject carries the canonical identity and what it was called, so a renderer never
/// has to resolve an id to draw a row. An unresolved one carries what the source said and stays
/// visibly unresolved: §5.5 requires an event Ono could not reconcile to be "rendered as such
/// rather than attached to a guessed object".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpatialRef {
    /// The canonical identity, its type and its label.
    Resolved {
        /// The v0.4 identity the rest of the shell navigates by (§5.1).
        id: SpatialId,
        /// What kind of object it is.
        object_type: SpatialType,
        /// What a person calls it.
        label: Arc<str>,
    },
    /// A source named something that could not be reconciled to a canonical identity (§5.5).
    Unresolved {
        /// The source that named it.
        source: EvidenceSource,
        /// What the source called it — `pid 1842`, `nginx.conf`.
        described: Arc<str>,
    },
}

impl SpatialRef {
    /// Whether the subject reached a canonical identity (§5.5).
    #[must_use]
    pub fn is_resolved(&self) -> bool {
        matches!(self, SpatialRef::Resolved { .. })
    }

    /// The canonical identity, or `None` for a subject that never reached one.
    #[must_use]
    pub fn spatial_id(&self) -> Option<&SpatialId> {
        match self {
            SpatialRef::Resolved { id, .. } => Some(id),
            SpatialRef::Unresolved { .. } => None,
        }
    }

    /// What to draw in a timeline row, resolved or not.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            SpatialRef::Resolved { label, .. } => label,
            SpatialRef::Unresolved { described, .. } => described,
        }
    }
}

/// A typed temporal record describing a meaningful change, observation or action (§3.3).
///
/// "An event MUST have stable identity independent from its rendered timeline row", and
/// [`EventId`] is that identity. Persisted events are append-only (§6.7): a correction is a new
/// event or a new evidence record referring to this one, never an edit.
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalEvent {
    /// The content identity of §3.3.
    pub event_id: EventId,
    /// The top-level class (§6.1).
    pub kind: EventKind,
    /// A provider's or plugin's namespaced refinement of the kind (§6.1).
    pub subtype: Option<Arc<str>>,
    /// The v0.4 boundary the event belongs to (§3.2).
    pub scope: SpatialScope,
    /// What it happened to. `None` for an event about no single object.
    pub subject: Option<SpatialRef>,
    /// Further subjects the event touches — both ends of a relation event (§6.4).
    pub related: Vec<SpatialRef>,
    /// When it happened, was seen and was stored (§3.3).
    pub times: EventTimes,
    /// The subject's value before the change, where the evidence gives one.
    pub before: Option<Value>,
    /// The subject's value after the change, where the evidence gives one.
    pub after: Option<Value>,
    /// The typed field changes (§6.2).
    pub changed_fields: Vec<FieldChange>,
    /// The evidence that supports the event (§3.4).
    pub evidence: Vec<EvidenceId>,
    /// The causal links whose effect is this event (§15).
    pub causal_parents: Vec<CausalLinkId>,
    /// The kind-specific body, typed and namespaced.
    ///
    /// §6.5's source-native information for `provider.event`, and the fields the other kinds
    /// need beside the common ones: the relation type and confidence of §6.4, the `action_id`
    /// and lifecycle detail of §17.2, the capability and completeness of §8.1.
    /// `docs/contracts/temporal/events.yaml` states which per kind.
    pub payload: Option<Value>,
    /// Where the record came from (v0.2 §25.2).
    pub provenance: Provenance,
}

/// Everything an event is, before it has an identity.
///
/// An ingest path builds one of these and [seals](Self::seal) it; the identity is then a
/// function of the content rather than of the order things arrived in, which is what lets two
/// sources report one observation and produce one event (§6.8).
#[derive(Debug, Clone, PartialEq)]
pub struct EventSeed {
    /// The top-level class (§6.1).
    pub kind: EventKind,
    /// A provider's or plugin's namespaced refinement of the kind (§6.1).
    pub subtype: Option<Arc<str>>,
    /// The v0.4 boundary the event belongs to (§3.2).
    pub scope: SpatialScope,
    /// What it happened to.
    pub subject: Option<SpatialRef>,
    /// Further subjects the event touches.
    pub related: Vec<SpatialRef>,
    /// When it happened, was seen and was stored (§3.3).
    pub times: EventTimes,
    /// The subject's value before the change.
    pub before: Option<Value>,
    /// The subject's value after the change.
    pub after: Option<Value>,
    /// The typed field changes (§6.2).
    pub changed_fields: Vec<FieldChange>,
    /// The evidence that supports the event (§3.4).
    pub evidence: Vec<EvidenceId>,
    /// The causal links whose effect is this event (§15).
    pub causal_parents: Vec<CausalLinkId>,
    /// The kind-specific body, typed and namespaced (§6.4, §6.5, §8.1, §17.2).
    pub payload: Option<Value>,
    /// Where the record came from (v0.2 §25.2).
    pub provenance: Provenance,
}

impl EventSeed {
    /// Computes the identity and turns the seed into an event.
    #[must_use]
    pub fn seal(self) -> TemporalEvent {
        let event_id = EventId::of(&self);
        TemporalEvent {
            event_id,
            kind: self.kind,
            subtype: self.subtype,
            scope: self.scope,
            subject: self.subject,
            related: self.related,
            times: self.times,
            before: self.before,
            after: self.after,
            changed_fields: self.changed_fields,
            evidence: self.evidence,
            causal_parents: self.causal_parents,
            payload: self.payload,
            provenance: self.provenance,
        }
    }
}

/// The payload key naming which relation a `relation.added` or `relation.removed` event is about.
pub const RELATION_KEY: &str = "relation";

/// The payload key carrying the edge's confidence (v0.4 §11.5).
pub const CONFIDENCE_KEY: &str = "confidence";

/// The payload `docs/contracts/temporal/events.yaml` requires of a relation event (§6.4).
///
/// §6.4 makes `relation` and `confidence` required fields of both relation kinds, and the two
/// ends travel as the event's subject and its one related reference. This is where the body is
/// written and [`relation_of`] is where it is read, so an ingest path and a reconstruction
/// cannot disagree about the spelling.
#[must_use]
pub fn relation_payload(relation: &str, confidence: Confidence) -> Value {
    let mut map = MapValue::new();
    map.insert(RELATION_KEY.into(), Value::string(relation));
    map.insert(CONFIDENCE_KEY.into(), Value::string(confidence.as_str()));
    Value::Map(Arc::new(map))
}

/// The relation and confidence a relation event names, or `None` where it names none.
///
/// A relation event without both ends and a relation type "is not renderable"
/// (`docs/contracts/temporal/events.yaml`), so an event that carries no relation contributes no
/// edge rather than an edge with a guessed type.
#[must_use]
pub fn relation_of(event: &TemporalEvent) -> Option<(Arc<str>, Confidence)> {
    let payload = event.payload.as_ref()?.as_map().ok()?;
    let Value::String(relation) = payload.get(RELATION_KEY)? else {
        return None;
    };
    let confidence = match payload.get(CONFIDENCE_KEY) {
        Some(Value::String(name)) => Confidence::from_name(name).unwrap_or(Confidence::Unknown),
        _ => Confidence::Unknown,
    };
    Some((Arc::clone(relation), confidence))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_name_the_action_lifecycle_when_a_kind_is_asked_whether_it_is_one() {
        assert!(EventKind::ActionRequested.is_action());
        assert!(EventKind::ActionFailed.is_action());
        assert!(!EventKind::ObjectChanged.is_action());
        assert_eq!(EventKind::ALL.len(), 17, "§6.1 defines seventeen kinds");
    }

    #[test]
    fn should_read_every_vocabulary_word_back_when_it_is_rendered() {
        for class in ChangeClass::ALL {
            assert_eq!(ChangeClass::from_name(class.as_str()), Some(*class));
        }
        for certainty in ChangeCertainty::ALL {
            assert_eq!(
                ChangeCertainty::from_name(certainty.as_str()),
                Some(*certainty)
            );
        }
        assert_eq!(ChangeClass::from_name("wobbled"), None);
    }
}
