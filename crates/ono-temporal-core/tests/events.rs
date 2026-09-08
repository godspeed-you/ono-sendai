//! The canonical event model: the seventeen kinds of v0.5 §6.1, the typed field changes of §6.2
//! and the rule that "`before == null` and `after == null` MUST NOT be overloaded to mean
//! arbitrary unknown", and the unresolved temporal subject of §5.5.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_temporal_core::{
    CausalRelation, ChangeCertainty, ChangeClass, EventKind, FieldChange, SpatialRef,
};
use ono_value::{FieldAccess, Value};

use common::{source, subject};

#[test]
fn should_name_every_kind_of_section_six_one_when_the_vocabulary_is_read() {
    let names: Vec<&str> = EventKind::ALL.iter().map(|kind| kind.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "object.observed",
            "object.appeared",
            "object.changed",
            "object.disappeared",
            "relation.added",
            "relation.removed",
            "action.requested",
            "action.authorized",
            "action.executed",
            "action.completed",
            "action.failed",
            "provider.event",
            "coverage.started",
            "coverage.ended",
            "checkpoint.created",
            "landmark.added",
            "landmark.removed",
        ],
        "§6.1's list is closed and ordered"
    );
    for kind in EventKind::ALL {
        assert_eq!(EventKind::from_name(kind.as_str()), Some(*kind));
    }
    assert_eq!(EventKind::from_name("object.wobbled"), None);
}

#[test]
fn should_keep_unknown_apart_from_absent_when_a_field_change_has_no_evidence_on_one_side() {
    let change = FieldChange {
        field: "active_state".into(),
        before: None,
        after: Some(Value::string("active")),
        certainty: ChangeCertainty::Unknown,
    };
    assert_eq!(
        change.certainty,
        ChangeCertainty::Unknown,
        "§6.2: a side with no evidence is unknown, and says so, rather than reading as a value"
    );
    assert!(
        change.before.is_none(),
        "the missing side is absent from the change rather than filled with a zero"
    );
}

#[test]
fn should_read_back_as_null_when_a_field_change_with_no_evidence_becomes_a_value() {
    let change = FieldChange {
        field: "active_state".into(),
        before: None,
        after: Some(Value::string("active")),
        certainty: ChangeCertainty::Unknown,
    };
    let Value::Map(rendered) = ono_temporal_core::value::field_change(&change) else {
        panic!("a field change renders as a sub-record");
    };
    assert_eq!(
        rendered.get("before"),
        Some(&Value::Null),
        "v0.2 §10.5: a value that is not known is null and never a zero"
    );
    assert_eq!(
        rendered.get("certainty"),
        Some(&Value::string("unknown")),
        "the certainty says why it is null"
    );
}

#[test]
fn should_distinguish_a_change_observed_on_both_sides_from_one_that_was_inferred() {
    let observed = FieldChange {
        field: "active_state".into(),
        before: Some(Value::string("activating")),
        after: Some(Value::string("active")),
        certainty: ChangeCertainty::Observed,
    };
    let inferred = FieldChange {
        certainty: ChangeCertainty::Inferred,
        ..observed.clone()
    };
    assert_ne!(observed.certainty, inferred.certainty);
    let names: Vec<&str> = ChangeCertainty::ALL
        .iter()
        .map(|certainty| certainty.as_str())
        .collect();
    assert_eq!(names, vec!["observed", "derived", "inferred", "unknown"]);
}

#[test]
fn should_stay_visibly_unresolved_when_a_source_named_something_ono_could_not_reconcile() {
    let unresolved = SpatialRef::Unresolved {
        source: source(),
        described: "pid 1842".into(),
    };
    assert!(
        !unresolved.is_resolved(),
        "§5.5: an event Ono could not attach to an identity is rendered as unresolved"
    );
    assert_eq!(unresolved.label(), "pid 1842");
    assert_eq!(unresolved.spatial_id(), None);

    let resolved = SpatialRef::Resolved {
        id: subject(1842),
        object_type: ono_spatial_core::SpatialType::Process,
        label: "nginx".into(),
    };
    assert!(resolved.is_resolved());
    assert_eq!(resolved.spatial_id(), Some(&subject(1842)));
}

#[test]
fn should_name_every_change_class_when_the_vocabulary_is_read() {
    let names: Vec<&str> = ChangeClass::ALL.iter().map(ChangeClass::as_str).collect();
    assert_eq!(
        names,
        vec![
            "added",
            "removed",
            "changed",
            "relation_added",
            "relation_removed"
        ],
        "§13.2's list is closed"
    );
}

#[test]
fn should_keep_causal_classes_apart_from_ordering_when_a_renderer_asks() {
    for (relation, inverse, causal) in [
        (CausalRelation::CausedBy, "caused", true),
        (CausalRelation::TriggeredBy, "triggered", true),
        (CausalRelation::ResultedIn, "result", true),
        (CausalRelation::CorrelatedWith, "correlated_with", false),
        (CausalRelation::PrecededBy, "followed_by", false),
    ] {
        assert_eq!(
            relation.inverse_label(),
            inverse,
            "§15.1 fixes the inverse label"
        );
        assert_eq!(
            relation.is_causal(),
            causal,
            "§15.6: `preceded_by` states only order, and correlation is not causation (§15.5)"
        );
        assert_eq!(CausalRelation::from_name(relation.as_str()), Some(relation));
    }
}

#[test]
fn should_keep_unknown_apart_from_absent_when_an_event_record_is_read() {
    // v0.5 §13.4 and v0.2 §10.5: there is no Unknown value; a field with no evidence is null,
    // and the three-way distinction lives at field access rather than in the data.
    let event = common::event(
        EventKind::ObjectObserved,
        "2026-08-31T12:00:00Z",
        "linux.procfs",
    );
    let record = ono_temporal_core::value::event_record(&event).expect("an event becomes a value");
    assert_eq!(
        record.access("payload"),
        FieldAccess::Unknown,
        "an observation carries no provider body, and unknown is visible"
    );
    assert_eq!(record.access("nowhere"), FieldAccess::Absent);
    assert_eq!(
        record.access("kind"),
        FieldAccess::Known(Value::string("object.observed"))
    );
}
