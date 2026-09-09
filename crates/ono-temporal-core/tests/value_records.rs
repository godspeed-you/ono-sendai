//! The value bridge: every temporal type that has a schema in v0.5 §35 becomes a record, and
//! every record validates against the contract that defines it. This is the one place a temporal
//! value becomes an Ono value, so §36.4's drift check has one thing to compare.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_spatial_core::PermissionState;
use ono_temporal_core::{
    CausalLink, CausalLinkId, CausalRelation, CausalRuleId, CoverageSummary, EventKind,
    EvidenceSource, EvidenceStrength, GapReason, TemporalCapabilities, TemporalCompleteness,
    TemporalContext, TemporalCoverage, TemporalGap, TemporalSourceDescription, TimeSelector, value,
};
use ono_value::{Value, builtin_schemas};

use common::{event, instant, scope, subject, systemd, window};

fn coverage_interval() -> TemporalCoverage {
    TemporalCoverage {
        scope: scope(),
        capability: "service.state".into(),
        from: instant("2026-08-31T12:00:00Z"),
        until: instant("2026-08-31T13:00:00Z"),
        completeness: TemporalCompleteness::Complete,
        sampling_interval: None,
        source: systemd(),
        permission: PermissionState::Available,
    }
}

fn gap() -> TemporalGap {
    TemporalGap {
        scope: scope(),
        from: instant("2026-08-31T12:20:00Z"),
        until: instant("2026-08-31T12:24:12Z"),
        capability: "service.state".into(),
        reason: GapReason::ProviderUnavailable,
        source: EvidenceSource::recorder(),
        detail: Some("recorder offline".into()),
    }
}

fn assert_valid(record: &ono_value::RecordValue) {
    record.validate().unwrap_or_else(|error| {
        panic!("{} must validate: {}", record.schema_id(), error.message())
    });
    assert!(
        builtin_schemas().get(record.schema_id()).is_some(),
        "{} must be an embedded contract",
        record.schema_id()
    );
}

#[test]
fn should_validate_against_its_contract_when_an_event_becomes_a_record() {
    let event = event(
        EventKind::ObjectChanged,
        "2026-08-31T12:00:00Z",
        "linux.systemd-dbus",
    );
    let record = value::event_record(&event).expect("an event becomes a record");
    assert_valid(&record);
    assert_eq!(
        record.get("event_id"),
        Some(&Value::string(event.event_id.as_str()))
    );
    assert_eq!(record.get("kind"), Some(&Value::string("object.changed")));
}

#[test]
fn should_keep_the_three_timestamps_apart_when_an_event_becomes_a_record() {
    let mut seed = common::seed(
        EventKind::ObjectChanged,
        common::times("2026-08-31T12:00:01Z"),
        "linux.journald",
    );
    seed.times.source_time = Some(instant("2026-08-31T11:59:00Z"));
    seed.times.ingested_at = instant("2026-08-31T12:00:09Z");
    let record = value::event_record(&seed.seal()).expect("an event becomes a record");
    assert_valid(&record);
    assert_eq!(
        record.get("source_time"),
        Some(&Value::Timestamp(instant("2026-08-31T11:59:00Z")))
    );
    assert_eq!(
        record.get("observed_at"),
        Some(&Value::Timestamp(instant("2026-08-31T12:00:01Z")))
    );
    assert_eq!(
        record.get("ingested_at"),
        Some(&Value::Timestamp(instant("2026-08-31T12:00:09Z"))),
        "§3.3: the three timestamps MUST NOT be silently collapsed into one field"
    );
}

#[test]
fn should_validate_against_its_contract_when_a_context_becomes_a_record() {
    let present =
        value::context_record(&TemporalContext::Present).expect("the present is a record");
    assert_valid(&present);
    assert_eq!(present.get("mode"), Some(&Value::string("present")));
    assert_eq!(present.get("marker"), Some(&Value::Null));

    let historical = value::context_record(&TemporalContext::Historical {
        requested: TimeSelector::parse("-10m").expect("a selector"),
        requested_text: "-10m".into(),
        resolved_at: instant("2026-08-31T12:07:14Z"),
        coverage: CoverageSummary::compose(&[coverage_interval()], window()),
        anchor_event: None,
    })
    .expect("a historical context is a record");
    assert_valid(&historical);
    assert_eq!(historical.get("mode"), Some(&Value::string("historical")));
    assert_eq!(historical.get("marker"), Some(&Value::string("[PAST]")));
}

#[test]
fn should_validate_against_its_contract_when_a_coverage_interval_becomes_a_record() {
    let record = value::coverage_record(&coverage_interval()).expect("coverage becomes a record");
    assert_valid(&record);
    assert_eq!(record.get("completeness"), Some(&Value::string("complete")));
    assert_eq!(
        record.get("source"),
        Some(&Value::string("linux.systemd-dbus"))
    );
}

#[test]
fn should_carry_the_evidence_source_class_when_an_events_provenance_names_one() {
    let record = value::event_record(&event(
        EventKind::ObjectChanged,
        "2026-08-31T12:18:02.044Z",
        "linux.systemd-dbus",
    ))
    .expect("an event becomes a record");
    assert_valid(&record);
    // §11.5's `[systemd]` tag abbreviates the §7.1 class and nothing else, so the class is a
    // declared field rather than something a renderer reads out of free-text provenance.
    assert_eq!(
        record.get("source"),
        Some(&Value::string("linux.systemd-dbus"))
    );
}

#[test]
fn should_leave_the_source_null_when_the_provenance_names_no_evidence_class() {
    let record = value::event_record(&event(
        EventKind::ObjectChanged,
        "2026-08-31T12:18:02.044Z",
        "linux.sock-diag",
    ))
    .expect("an event becomes a record");
    assert_valid(&record);
    assert_eq!(
        record.get("source"),
        Some(&Value::Null),
        "§7.1 is a closed list: a provider name outside it is not an evidence source class"
    );
}

#[test]
fn should_carry_the_short_form_when_a_session_minted_a_reference_for_an_event() {
    let event = event(
        EventKind::ObjectChanged,
        "2026-08-31T12:18:02.044Z",
        "linux.systemd-dbus",
    );
    let record =
        value::event_record_with_reference(&event, Some("e42")).expect("an event becomes a record");
    assert_valid(&record);
    // §11.6: the rendered reference must be usable in `at event`, `inspect event` and `why
    // event`, so it is the string the session's own resolver accepts.
    assert_eq!(record.get("reference"), Some(&Value::string("e42")));
}

#[test]
fn should_leave_the_reference_null_when_no_session_minted_one() {
    let record = value::event_record(&event(
        EventKind::ObjectChanged,
        "2026-08-31T12:18:02.044Z",
        "linux.systemd-dbus",
    ))
    .expect("an event becomes a record");
    assert_valid(&record);
    assert_eq!(record.get("reference"), Some(&Value::Null));
}

#[test]
fn should_validate_against_its_contract_when_a_gap_becomes_a_record() {
    let record = value::gap_record(&gap()).expect("a gap becomes a record");
    assert_valid(&record);
    assert_eq!(
        record.get("reason"),
        Some(&Value::string("provider_unavailable"))
    );
}

#[test]
fn should_carry_the_producers_words_when_a_gap_states_a_detail() {
    let record = value::gap_record(&gap()).expect("a gap becomes a record");
    assert_valid(&record);
    // §11.7's own worked example is this field: `---- coverage gap: recorder offline 4m12s ----`.
    // A renderer that only had `reason` would write `provider unavailable` instead.
    assert_eq!(
        record.get("detail"),
        Some(&Value::string("recorder offline"))
    );
}

#[test]
fn should_leave_the_detail_null_when_the_reason_says_everything() {
    let bare = TemporalGap {
        detail: None,
        ..gap()
    };
    let record = value::gap_record(&bare).expect("a gap becomes a record");
    assert_valid(&record);
    assert_eq!(record.get("detail"), Some(&Value::Null));
}

#[test]
fn should_validate_against_its_contract_when_evidence_becomes_a_record() {
    let record = value::evidence_record(&common::evidence()).expect("evidence becomes a record");
    assert_valid(&record);
    assert_eq!(
        record.get("strength"),
        Some(&Value::string("authoritative"))
    );
    assert_eq!(
        record.get("claim_kind"),
        Some(&Value::string("field_value"))
    );
}

#[test]
fn should_validate_against_its_contract_when_a_causal_link_becomes_a_record() {
    let cause = event(
        EventKind::ActionExecuted,
        "2026-08-31T14:03:11Z",
        "ono.session",
    );
    let effect = event(
        EventKind::ObjectChanged,
        "2026-08-31T14:03:13Z",
        "linux.systemd-dbus",
    );
    let link = CausalLink {
        link_id: CausalLinkId::of(
            &CausalRuleId::new("ono.action-to-job"),
            CausalRelation::CausedBy,
            &cause.event_id,
            &effect.event_id,
        ),
        relation: CausalRelation::CausedBy,
        cause: cause.event_id.clone(),
        effect: effect.event_id.clone(),
        rule: CausalRuleId::new("ono.action-to-job"),
        evidence: Vec::new(),
        strength: EvidenceStrength::Authoritative,
        source: EvidenceSource::session(),
    };
    let record = value::causal_link_record(&link).expect("a link becomes a record");
    assert_valid(&record);
    assert_eq!(record.get("relation"), Some(&Value::string("caused_by")));
    assert_eq!(record.get("inverse"), Some(&Value::string("caused")));
    assert_eq!(record.get("is_causal"), Some(&Value::Bool(true)));
}

#[test]
fn should_validate_against_its_contract_when_an_action_becomes_a_record() {
    let record = value::action_record(&common::action(Some(
        "systemd:/org/freedesktop/systemd1/job/4821",
    )))
    .expect("an action becomes a record");
    assert_valid(&record);
    assert!(
        !record
            .get("command")
            .and_then(|value| match value {
                Value::String(text) => Some(text.to_string()),
                _ => None,
            })
            .unwrap_or_default()
            .contains("hunter2"),
        "§17.5: the raw text never reaches the ledger"
    );
}

#[test]
fn should_validate_against_its_contract_when_a_source_becomes_a_record() {
    let description = TemporalSourceDescription {
        source: systemd(),
        provider: "linux.systemd".into(),
        capabilities: TemporalCapabilities {
            current_snapshot: true,
            live_events: true,
            historical_query: false,
            exhaustive_events: false,
            causal_tokens: true,
            checkpointable: true,
            retained_history: None,
        },
        availability: PermissionState::Available,
        detail: None,
    };
    let record = value::source_record(&description).expect("a source becomes a record");
    assert_valid(&record);
    assert_eq!(record.get("live_events"), Some(&Value::Bool(true)));
    assert_eq!(
        record.get("exhaustive_events"),
        Some(&Value::Bool(false)),
        "§21.5: a polled source does not claim exhaustive delivery"
    );
    assert_eq!(record.get("retained_history"), Some(&Value::Null));
}

#[test]
fn should_carry_its_coverage_and_gaps_when_a_reconstructed_object_gains_temporal_metadata() {
    let metadata = value::temporal_metadata(
        instant("2026-08-31T12:07:14Z"),
        &CoverageSummary::compose(&[coverage_interval()], window()),
        true,
        &[systemd()],
        &[gap()],
    )
    .expect("the metadata is a value");
    let Value::Map(map) = metadata else {
        panic!("§9.4's `temporal` is a nested record field");
    };
    assert_eq!(
        map.get("as_of"),
        Some(&Value::Timestamp(instant("2026-08-31T12:07:14Z")))
    );
    assert_eq!(map.get("reconstructed"), Some(&Value::Bool(true)));
    let Some(Value::List(gaps)) = map.get("gaps") else {
        panic!("the gaps are a list");
    };
    assert_eq!(
        gaps.len(),
        1,
        "§7.5: gaps travel with the object they affect"
    );
    let Some(Value::Record(gap)) = gaps.first() else {
        panic!("each gap is an `ono.temporal-gap/1`");
    };
    assert_valid(gap);
}

#[test]
fn should_expose_source_level_detail_when_a_coverage_summary_becomes_a_value() {
    let summary = CoverageSummary::compose(&[coverage_interval()], window());
    let Value::Map(map) = value::coverage_summary(&summary).expect("a summary is a value") else {
        panic!("a summary is a nested record");
    };
    assert_eq!(map.get("headline"), Some(&Value::string("complete")));
    let Some(Value::Map(capabilities)) = map.get("capabilities") else {
        panic!("§8.5 exposes the composition per capability");
    };
    assert_eq!(
        capabilities.get("service.state"),
        Some(&Value::string("complete"))
    );
    let Some(Value::List(sources)) = map.get("sources") else {
        panic!("§8.5: `inspect` MUST expose source-level detail");
    };
    assert_eq!(sources.len(), 1);
}

#[test]
fn should_name_the_subject_when_an_event_has_one_a_renderer_can_draw() {
    let mut seed = common::seed(
        EventKind::ObjectAppeared,
        common::times("2026-08-31T12:00:00Z"),
        "linux.procfs",
    );
    seed.subject = Some(ono_temporal_core::SpatialRef::Resolved {
        id: subject(2741),
        object_type: ono_spatial_core::SpatialType::Process,
        label: "nginx".into(),
    });
    let record = value::event_record(&seed.seal()).expect("an event becomes a record");
    assert_valid(&record);
    let Some(Value::Map(subject)) = record.get("subject") else {
        panic!("the subject is a nested record");
    };
    assert_eq!(subject.get("label"), Some(&Value::string("nginx")));
    assert_eq!(subject.get("resolved"), Some(&Value::Bool(true)));
}

#[test]
fn should_stay_visibly_unresolved_when_an_event_subject_could_not_be_reconciled() {
    let mut seed = common::seed(
        EventKind::ObjectAppeared,
        common::times("2026-08-31T12:00:00Z"),
        "linux.journald",
    );
    seed.subject = Some(ono_temporal_core::SpatialRef::Unresolved {
        source: EvidenceSource::parse("linux.journald").expect("a §7.1 class"),
        described: "pid 1842".into(),
    });
    let record = value::event_record(&seed.seal()).expect("an event becomes a record");
    assert_valid(&record);
    let Some(Value::Map(subject)) = record.get("subject") else {
        panic!("the subject is a nested record");
    };
    assert_eq!(
        subject.get("resolved"),
        Some(&Value::Bool(false)),
        "§5.5: it MUST be rendered as unresolved rather than attached to a guessed object"
    );
    assert_eq!(subject.get("spatial_id"), Some(&Value::Null));
    assert_eq!(subject.get("described"), Some(&Value::string("pid 1842")));
}
