//! `why` (spec v0.5 §16, §48.5 scenarios 22, 23, 26, 27).
//!
//! The scenario is §17.7's: an operator restarts nginx through Ono, systemd queues a job, the
//! unit moves through its states, a worker goes and another appears. §16.5 says what the answer
//! looks like; these tests say what it contains.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

mod causal_fixture;

use std::sync::Arc;

use causal_fixture::{EventBuilder, World, instant, process, service};
use ono_core::ErrorCode;
use ono_spatial_core::SpatialType;
use ono_temporal_core::{
    ActionId, CausalRelation, ChangeCertainty, EventKind, EvidenceStrength, TemporalEvent,
};
use ono_temporal_query::causal::{
    CausalContext, CausalEngine, SYSTEMD_JOB_NEW, WhyOptions, WhyRequest,
};
use ono_value::Value;

/// The path systemd returned for the restart job.
const JOB: &str = "/org/freedesktop/systemd1/job/4821";

/// The identity the shell minted for `restart service nginx` (§17.3).
fn action() -> ActionId {
    ActionId::of(
        "session-1",
        instant("2026-08-31T14:03:11Z"),
        "restart",
        Some(&service("nginx.service")),
    )
}

/// §17.7's restart, as events and the evidence behind them.
fn restart() -> (Vec<TemporalEvent>, CausalContext) {
    let mut world = World::new();
    let unit = service("nginx.service");
    let action = action();
    let job_token = format!("systemd:{JOB};ono={}", action.as_str());

    let minted = world.transaction(
        "ono.session",
        "2026-08-31T14:03:11Z",
        Some(&unit),
        action.as_str(),
        EvidenceStrength::Authoritative,
    );
    let job = world.transaction(
        "linux.systemd-dbus",
        "2026-08-31T14:03:11Z",
        Some(&unit),
        &job_token,
        EvidenceStrength::Authoritative,
    );
    let deactivating = world.transition(
        "linux.systemd-dbus",
        "2026-08-31T14:03:11Z",
        &unit,
        "active_state",
        Value::string("active"),
        Value::string("deactivating"),
        EvidenceStrength::Authoritative,
    );
    let active = world.transition(
        "linux.systemd-dbus",
        "2026-08-31T14:03:13Z",
        &unit,
        "active_state",
        Value::string("activating"),
        Value::string("active"),
        EvidenceStrength::Authoritative,
    );

    let executed = EventBuilder::new(EventKind::ActionExecuted, "2026-08-31T14:03:11.002Z")
        .provider("ono.session")
        .related(&unit, SpatialType::Service, "nginx.service")
        .evidence(&minted)
        .seal();
    let queued = EventBuilder::new(EventKind::ProviderEvent, "2026-08-31T14:03:11.017Z")
        .subtype(SYSTEMD_JOB_NEW)
        .provider("systemd")
        .related(&unit, SpatialType::Service, "nginx.service")
        .payload("job_type", Value::string("restart"))
        .evidence(&job)
        .seal();
    let going = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:11.401Z")
        .provider("systemd")
        .subject(&unit, SpatialType::Service, "nginx.service")
        .changed(
            "active_state",
            Value::string("active"),
            Value::string("deactivating"),
        )
        .evidence(&job)
        .evidence(&deactivating)
        .seal();
    let up = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:13.002Z")
        .provider("systemd")
        .subject(&unit, SpatialType::Service, "nginx.service")
        .changed(
            "active_state",
            Value::string("activating"),
            Value::string("active"),
        )
        .evidence(&job)
        .evidence(&active)
        .seal();

    let context = world.context();
    (vec![executed, queued, going, up], context)
}

#[test]
fn should_return_a_causal_chain_when_an_action_restarted_a_service() {
    let (events, context) = restart();
    let up = events
        .last()
        .expect("the fixture ends with the unit becoming active");
    let engine = CausalEngine::builtin();

    let explanation = engine
        .explain(
            &WhyRequest::event(&up.event_id),
            &events,
            &context,
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect("an explanation");

    let cause = explanation.cause.as_ref().expect("a known cause");
    assert_eq!(cause.rule.as_str(), "ono.systemd-job-to-unit-state");
    assert_eq!(cause.relation, CausalRelation::CausedBy);
    assert!(!explanation.causal_chain.is_empty());
    assert!(
        explanation
            .causal_chain
            .iter()
            .any(|step| step.depth == 2 && step.link.cause == events[0].event_id),
        "§16.8: the chain reaches back to the operator action"
    );
}

#[test]
fn should_name_its_rule_source_and_evidence_when_every_causal_edge_is_read() {
    let (events, context) = restart();
    let up = events
        .last()
        .expect("the fixture ends with the unit becoming active");
    let engine = CausalEngine::builtin();

    let explanation = engine
        .explain(
            &WhyRequest::event(&up.event_id),
            &events,
            &context,
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect("an explanation");

    for step in &explanation.causal_chain {
        assert!(
            step.link.rule.is_builtin(),
            "§48.5 scenario 27: every edge names its rule"
        );
        assert!(
            !step.link.evidence.is_empty(),
            "§48.5 scenario 27: every edge names its evidence"
        );
        for id in &step.link.evidence {
            assert!(
                context.evidence(id).is_some(),
                "the evidence an edge names resolves to a record"
            );
        }
        assert!(!step.link.source.as_str().is_empty());
    }
}

#[test]
fn should_show_three_hops_by_default_and_more_when_a_depth_is_given() {
    let (events, context) = restart();
    let up = events.last().expect("the unit became active");
    let engine = CausalEngine::builtin();
    let at = instant("2026-08-31T14:05:00Z");

    let shallow = engine
        .explain(
            &WhyRequest::event(&up.event_id),
            &events,
            &context,
            &WhyOptions::at(at).with_depth(1),
        )
        .expect("an explanation");
    assert!(
        shallow.causal_chain.iter().all(|step| step.depth == 1),
        "§16.7: --depth bounds the typed answer"
    );

    let deep = engine
        .explain(
            &WhyRequest::event(&up.event_id),
            &events,
            &context,
            &WhyOptions::at(at).with_depth(8),
        )
        .expect("an explanation");
    assert!(deep.causal_chain.len() > shallow.causal_chain.len());
}

#[test]
fn should_answer_with_a_null_cause_when_no_registered_rule_explains_the_failure() {
    // §16.6's picture: nginx failed, a config file changed eleven seconds earlier, and nothing
    // states that one produced the other. §15.7 makes that a successful answer.
    let mut world = World::new();
    let worker = process(1827);
    let conf = causal_fixture::file("/etc/nginx/nginx.conf");

    let opened = world.relation(
        "linux.procfs",
        "2026-08-31T14:00:00Z",
        &worker,
        "process.opened_file",
        &conf,
        ono_temporal_core::TimeRange::since(instant("2026-08-31T14:00:00Z")),
        EvidenceStrength::Asserted,
    );
    let edited = world.field(
        "ono.recorder",
        "2026-08-31T14:03:06Z",
        &conf,
        "mtime",
        Value::string("2026-08-31T14:03:06Z"),
        EvidenceStrength::Observational,
    );
    let failed = world.field(
        "linux.systemd-dbus",
        "2026-08-31T14:03:17Z",
        &worker,
        "result",
        Value::string("exit-code"),
        EvidenceStrength::Asserted,
    );

    let change = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:06Z")
        .provider("ono.recorder")
        .subject(&conf, SpatialType::File, "nginx.conf")
        .changed("mtime", Value::Null, Value::string("2026-08-31T14:03:06Z"))
        .evidence(&edited)
        .seal();
    let failure = EventBuilder::new(EventKind::ActionFailed, "2026-08-31T14:03:17Z")
        .provider("systemd")
        .subject(&worker, SpatialType::Process, "process/1827")
        .evidence(&failed)
        .seal();
    let _ = opened;

    let events = vec![change.clone(), failure.clone()];
    let context = world.context();
    let engine = CausalEngine::builtin();

    let explanation = engine
        .explain(
            &WhyRequest::event(&failure.event_id),
            &events,
            &context,
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect("§15.7: unknown cause is an outcome, not an error");

    assert!(explanation.is_unknown_cause(), "§16.6: cause is unknown");
    assert!(explanation.causal_chain.is_empty());
    assert!(
        explanation
            .correlations
            .iter()
            .any(|association| association.event == change.event_id),
        "§16.6: the change is listed, under correlated"
    );
    assert_eq!(explanation.coverage, *context.coverage());
}

#[test]
fn should_refuse_ambiguity_when_two_transitions_are_equally_relevant() {
    let mut world = World::new();
    let unit = service("nginx.service");
    let one = world.field(
        "linux.systemd-dbus",
        "2026-08-31T14:03:13Z",
        &unit,
        "active_state",
        Value::string("active"),
        EvidenceStrength::Authoritative,
    );
    let two = world.field(
        "linux.systemd-dbus",
        "2026-08-31T14:03:13Z",
        &unit,
        "sub_state",
        Value::string("running"),
        EvidenceStrength::Authoritative,
    );
    let first = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:13Z")
        .subject(&unit, SpatialType::Service, "nginx.service")
        .changed(
            "active_state",
            Value::string("activating"),
            Value::string("active"),
        )
        .evidence(&one)
        .seal();
    let second = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:13Z")
        .subject(&unit, SpatialType::Service, "nginx.service")
        .changed(
            "sub_state",
            Value::string("start-post"),
            Value::string("running"),
        )
        .evidence(&two)
        .seal();

    let engine = CausalEngine::builtin();
    let refusal = engine
        .explain(
            &WhyRequest::target(&unit, "nginx.service"),
            &[first.clone(), second.clone()],
            &world.context(),
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect_err("§16.3: Ono refuses ambiguity rather than choosing");

    assert_eq!(refusal.code(), ErrorCode::TemporalAmbiguousEvent);
    let listed = refusal
        .metadata()
        .get("candidates")
        .expect("the refusal lists the event references")
        .to_string();
    assert!(listed.contains(first.event_id.as_str()));
    assert!(listed.contains(second.event_id.as_str()));
}

#[test]
fn should_explain_the_most_recent_supported_change_when_a_field_is_named() {
    let mut world = World::new();
    let unit = service("nginx.service");
    let earlier = world.field(
        "linux.systemd-dbus",
        "2026-08-31T14:00:00Z",
        &unit,
        "active_state",
        Value::string("active"),
        EvidenceStrength::Authoritative,
    );
    let later = world.field(
        "linux.systemd-dbus",
        "2026-08-31T14:03:13Z",
        &unit,
        "active_state",
        Value::string("failed"),
        EvidenceStrength::Authoritative,
    );

    let first = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:00:00Z")
        .subject(&unit, SpatialType::Service, "nginx.service")
        .changed(
            "active_state",
            Value::string("activating"),
            Value::string("active"),
        )
        .evidence(&earlier)
        .seal();
    let recent = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:13Z")
        .subject(&unit, SpatialType::Service, "nginx.service")
        .changed(
            "active_state",
            Value::string("active"),
            Value::string("failed"),
        )
        .evidence(&later)
        .seal();
    // A change with no evidence on one side is not a supported change (§6.2).
    let unsupported = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:04:00Z")
        .subject(&unit, SpatialType::Service, "nginx.service")
        .changed_with(
            "active_state",
            None,
            Some(Value::string("active")),
            ChangeCertainty::Unknown,
        )
        .evidence(&later)
        .seal();

    let engine = CausalEngine::builtin();
    let explanation = engine
        .explain(
            &WhyRequest::field(&unit, "nginx.service", "active_state"),
            &[first, recent.clone(), unsupported],
            &world.context(),
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect("an explanation");

    assert_eq!(explanation.explained_event, Some(recent.event_id));
    assert_eq!(explanation.subject, Some(unit));
}

#[test]
fn should_answer_about_a_coordinate_in_the_past_when_one_is_given() {
    let (events, context) = restart();
    let engine = CausalEngine::builtin();
    let unit = service("nginx.service");

    let explanation = engine
        .explain(
            &WhyRequest::target(&unit, "nginx.service"),
            &events,
            &context,
            &WhyOptions::at(instant("2026-08-31T14:03:12Z")),
        )
        .expect("an explanation");

    assert_eq!(
        explanation.explained_event,
        Some(events[2].event_id.clone()),
        "§16.3: the most recent transition at or before the active coordinate"
    );
}

#[test]
fn should_answer_when_the_window_holds_nothing_about_the_subject() {
    let engine = CausalEngine::builtin();
    let unit = service("nginx.service");
    let explanation = engine
        .explain(
            &WhyRequest::target(&unit, "nginx.service"),
            &[],
            &CausalContext::default(),
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect("§1.4: not knowing is an answer");

    assert!(explanation.explained_event.is_none());
    assert!(explanation.is_unknown_cause());
    assert_eq!(
        explanation.state_or_change,
        Arc::<str>::from(
            "nginx.service has no recorded state transition at or before the coordinate"
        )
    );
}

#[test]
fn should_state_the_explained_instant_when_an_explanation_becomes_a_record() {
    let (events, context) = restart();
    let up = events
        .last()
        .expect("the fixture ends with the unit becoming active");
    let engine = CausalEngine::builtin();

    let explanation = engine
        .explain(
            &WhyRequest::event(&up.event_id),
            &events,
            &context,
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect("an explanation");
    let record = explanation.to_record().expect("an explanation is a record");

    record
        .validate()
        .expect("§35.5's contract is what the answer is built against");
    // §16.5 renders `failed at 14:03:17.004`; §39.3 forbids the renderer resolving an id to find
    // the instant, so the producer states it.
    assert_eq!(
        record.get("at"),
        Some(&Value::Timestamp(instant("2026-08-31T14:03:13.002Z")))
    );
    assert_eq!(
        record.get("explained_event"),
        Some(&Value::string(up.event_id.as_str()))
    );
}

#[test]
fn should_keep_causal_correlated_and_preceding_in_three_arrays_when_an_explanation_becomes_a_record()
 {
    let (events, context) = restart();
    let up = events
        .last()
        .expect("the fixture ends with the unit becoming active");
    let engine = CausalEngine::builtin();

    let explanation = engine
        .explain(
            &WhyRequest::event(&up.event_id),
            &events,
            &context,
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect("an explanation");
    let record = explanation.to_record().expect("an explanation is a record");
    record.validate().expect("the contract holds");

    // §35.5: "MUST preserve causal vs correlated associations as separate arrays."
    for name in ["causal_chain", "correlations", "preceding"] {
        assert!(
            matches!(record.get(name), Some(Value::List(_))),
            "§35.5: `{name}` is an array of its own"
        );
    }
    let Some(Value::List(chain)) = record.get("causal_chain") else {
        panic!("§16.4: the chain is a list");
    };
    assert_eq!(chain.len(), explanation.causal_chain.len());
    let Some(Value::Map(step)) = chain.first() else {
        panic!("a chain step is a sub-record");
    };
    assert!(
        matches!(step.get("link"), Some(Value::Record(_))),
        "§15.8: the link is the `ono.causal-link/1` the engine emitted, spelled once"
    );
    assert!(
        matches!(step.get("at"), Some(Value::Timestamp(_))),
        "§16.5 draws a clock beside every node of the chain"
    );
    let cause = record.get("cause").expect("the fixture has a known cause");
    assert!(
        !cause.is_null(),
        "§16.5: a known cause is rendered from this field"
    );
}

#[test]
fn should_carry_the_gaps_and_the_coverage_when_an_unknown_cause_becomes_a_record() {
    let engine = CausalEngine::builtin();
    let unit = service("nginx.service");
    let explanation = engine
        .explain(
            &WhyRequest::target(&unit, "nginx.service"),
            &[],
            &CausalContext::default(),
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect("§1.4: not knowing is an answer");
    let record = explanation.to_record().expect("an explanation is a record");

    record.validate().expect("§15.7's answer is a valid record");
    assert_eq!(record.get("cause"), Some(&Value::Null));
    assert_eq!(
        record.get("at"),
        Some(&Value::Null),
        "nothing was recorded, so there is no instant to state"
    );
    assert!(matches!(record.get("coverage"), Some(Value::Map(_))));
    assert!(matches!(record.get("gaps"), Some(Value::List(_))));
}
