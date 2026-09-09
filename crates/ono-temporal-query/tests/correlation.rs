//! Correlation stays correlation (spec v0.5 §15.5, §15.6, §16.6, §55.3, §48.5 scenarios 24, 25).
//!
//! §55.3 names the failure mode: "A renderer or AI that says 'the config change caused the
//! outage' because it happened first violates the core contract." The last test in this file is
//! the falsification suite of §51's TEST-006 — generated event pairs with plausible timing and no
//! shared evidence, and the assertion that no rule ever emits a causal class for any of them.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

mod causal_fixture;

use causal_fixture::{EventBuilder, World, file, filesystem, instant, process, service};
use ono_spatial_core::SpatialType;
use ono_temporal_core::{CausalRelation, EventKind, EvidenceStrength, TemporalEvent, TimeRange};
use ono_temporal_query::causal::{CausalEngine, WhyOptions, WhyRequest};
use ono_value::Value;

/// A config change, a failure, and whether anything structural joins them (§15.5).
fn change_then_failure(
    related: bool,
    change_at: &str,
    failure_at: &str,
) -> (
    Vec<TemporalEvent>,
    ono_temporal_query::causal::CausalContext,
) {
    let mut world = World::new();
    let worker = process(1827);
    let conf = file("/etc/nginx/nginx.conf");

    if related {
        world.relation(
            "linux.procfs",
            "2026-08-31T14:00:00Z",
            &worker,
            "process.opened_file",
            &conf,
            TimeRange::since(instant("2026-08-31T14:00:00Z")),
            EvidenceStrength::Asserted,
        );
    }
    let edited = world.field(
        "ono.recorder",
        change_at,
        &conf,
        "mtime",
        Value::string(change_at),
        EvidenceStrength::Observational,
    );
    let failed = world.field(
        "linux.systemd-dbus",
        failure_at,
        &worker,
        "result",
        Value::string("exit-code"),
        EvidenceStrength::Asserted,
    );

    let change = EventBuilder::new(EventKind::ObjectChanged, change_at)
        .provider("ono.recorder")
        .subject(&conf, SpatialType::File, "nginx.conf")
        .changed("mtime", Value::Null, Value::string(change_at))
        .evidence(&edited)
        .seal();
    let failure = EventBuilder::new(EventKind::ActionFailed, failure_at)
        .provider("systemd")
        .subject(&worker, SpatialType::Process, "process/1827")
        .evidence(&failed)
        .seal();

    let context = world.context();
    (vec![change, failure], context)
}

#[test]
fn should_emit_correlation_only_when_a_change_precedes_a_related_failure() {
    let (events, context) =
        change_then_failure(true, "2026-08-31T14:03:06Z", "2026-08-31T14:03:17Z");
    let links = CausalEngine::builtin().links(&events, &context);

    assert!(
        !links.is_empty(),
        "§15.5's first example is a registered rule"
    );
    for link in &links {
        assert!(
            !link.is_causal(),
            "§15.5: a correlation rule emits `correlated_with` and nothing else"
        );
        assert_eq!(link.relation, CausalRelation::CorrelatedWith);
        assert_eq!(link.rule.as_str(), "ono.change-before-failure");
        assert!(
            link.strength <= EvidenceStrength::Correlated,
            "§7.2: association is never presented at a strength that reads as proof"
        );
    }
}

#[test]
fn should_emit_nothing_when_only_time_relates_a_change_to_a_failure() {
    let (events, context) =
        change_then_failure(false, "2026-08-31T14:03:06Z", "2026-08-31T14:03:17Z");
    let links = CausalEngine::builtin().links(&events, &context);
    assert!(
        links.is_empty(),
        "§15.5 asks for structural association; nine seconds is not one"
    );
}

#[test]
fn should_emit_nothing_when_the_change_falls_outside_the_declared_window() {
    let (events, context) =
        change_then_failure(true, "2026-08-31T13:50:00Z", "2026-08-31T14:03:17Z");
    let links = CausalEngine::builtin().links(&events, &context);
    assert!(
        links.is_empty(),
        "the rule declares a 5m window and keeps it"
    );
}

#[test]
fn should_keep_a_correlated_change_out_of_the_known_cause_when_why_is_asked() {
    let (events, context) =
        change_then_failure(true, "2026-08-31T14:03:06Z", "2026-08-31T14:03:17Z");
    let failure = &events[1];
    let explanation = CausalEngine::builtin()
        .explain(
            &WhyRequest::event(&failure.event_id),
            &events,
            &context,
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect("an explanation");

    assert!(
        explanation.is_unknown_cause(),
        "§16.6: the renderer must not move the config change into the known cause"
    );
    assert!(explanation.causal_chain.is_empty());
    assert_eq!(explanation.correlations.len(), 1);
    assert_eq!(
        explanation.correlations[0].relation,
        CausalRelation::CorrelatedWith
    );
}

#[test]
fn should_list_an_unrelated_earlier_event_under_order_alone() {
    let mut world = World::new();
    let unit = service("nginx.service");
    let unrelated = process(999);
    let first = world.field(
        "linux.systemd-dbus",
        "2026-08-31T14:03:10Z",
        &unrelated,
        "state",
        Value::string("sleeping"),
        EvidenceStrength::Asserted,
    );
    let second = world.field(
        "linux.systemd-dbus",
        "2026-08-31T14:03:13Z",
        &unit,
        "active_state",
        Value::string("failed"),
        EvidenceStrength::Authoritative,
    );

    let earlier = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:10Z")
        .provider("systemd")
        .sequence(1)
        .subject(&unrelated, SpatialType::Process, "process/999")
        .changed("state", Value::string("running"), Value::string("sleeping"))
        .evidence(&first)
        .seal();
    let later = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:13Z")
        .provider("systemd")
        .sequence(2)
        .subject(&unit, SpatialType::Service, "nginx.service")
        .changed(
            "active_state",
            Value::string("active"),
            Value::string("failed"),
        )
        .evidence(&second)
        .seal();

    let context = world.context();
    let explanation = CausalEngine::builtin()
        .explain(
            &WhyRequest::event(&later.event_id),
            &[earlier.clone(), later],
            &context,
            &WhyOptions::at(instant("2026-08-31T14:05:00Z")),
        )
        .expect("an explanation");

    assert!(
        explanation.is_unknown_cause(),
        "§48.5 scenario 25: a preceding unrelated event never appears under known cause"
    );
    assert!(explanation.correlations.is_empty());
    assert_eq!(explanation.preceding.len(), 1);
    assert_eq!(explanation.preceding[0].event, earlier.event_id);
    assert_eq!(
        explanation.preceding[0].relation,
        CausalRelation::PrecededBy
    );
    assert!(
        explanation.preceding[0].rule.is_none(),
        "§15.6: order comes from the ordering model, not from a registered rule"
    );
}

/// The kinds the built-in rules read, so the sweep below spends every world on a pair one of
/// them could plausibly fire on.
const READ_BY_RULES: &[EventKind] = &[
    EventKind::ActionExecuted,
    EventKind::ActionFailed,
    EventKind::ProviderEvent,
    EventKind::ObjectChanged,
    EventKind::ObjectAppeared,
    EventKind::ObjectDisappeared,
];

/// Every §7.1 source class, so no rule is excluded by its source constraint.
const EVERY_SOURCE: &[&str] = &[
    "ono.session",
    "ono.recorder",
    "linux.procfs",
    "linux.netlink",
    "linux.systemd-dbus",
    "linux.journald",
    "adapter:ps",
    "remote:db01/linux.procfs",
    "kuang:dev.example.packet-eye/flows",
];

/// One world: two events a rule reads, at plausible instants.
///
/// `shared` is the single bit the falsification suite turns: with it off the two events publish
/// no identity that joins them, and with it on the effect carries the cause's own systemd job
/// path and the action id the shell minted. Everything else about the two worlds is equal, so
/// the difference in what the engine emits is the difference the evidence makes.
fn world_of(
    cause_kind: EventKind,
    effect_kind: EventKind,
    source: &str,
    strength: EvidenceStrength,
    same_subject: bool,
    subtype: Option<&str>,
    gap_seconds: u64,
    shared: bool,
) -> (
    Vec<TemporalEvent>,
    ono_temporal_query::causal::CausalContext,
) {
    let mut world = World::new();
    let unit = service("nginx.service");
    let worker = process(1827);
    let other = if same_subject {
        worker.clone()
    } else {
        process(2741)
    };
    let action = ono_temporal_core::ActionId::of(
        "session-1",
        instant("2026-08-31T14:03:11Z"),
        "restart",
        Some(&unit),
    );

    let cause_at = "2026-08-31T14:03:11Z".to_owned();
    let effect_at = format!("2026-08-31T14:03:{:02}Z", 11 + gap_seconds);

    // The cause publishes a real Ono action id and a real systemd job path. The effect publishes
    // a token of its own. Nothing states that the two are the same transaction.
    let minted = world.transaction(source, &cause_at, Some(&worker), action.as_str(), strength);
    let job = world.transaction(
        source,
        &cause_at,
        Some(&worker),
        "/org/freedesktop/systemd1/job/4821",
        strength,
    );
    let elsewhere = world.transaction(
        source,
        &effect_at,
        Some(&other),
        "/org/freedesktop/systemd1/job/9134",
        strength,
    );
    let observed = world.transition(
        source,
        &effect_at,
        &other,
        "active_state",
        Value::string("activating"),
        Value::string("active"),
        strength,
    );

    let mut cause = EventBuilder::new(cause_kind, &cause_at)
        .provider(source)
        .subject(&worker, SpatialType::Process, "process/1827")
        .related(&unit, SpatialType::Service, "nginx.service")
        .sequence(1)
        .payload("job_type", Value::string("restart"))
        .evidence(&minted)
        .evidence(&job);
    if let Some(subtype) = subtype {
        cause = cause.subtype(subtype);
    }
    let mut effect = EventBuilder::new(effect_kind, &effect_at)
        .provider(source)
        .subject(&other, SpatialType::Process, "process/2741")
        .related(&unit, SpatialType::Service, "nginx.service")
        .sequence(2)
        .changed(
            "active_state",
            Value::string("activating"),
            Value::string("active"),
        )
        .evidence(&elsewhere)
        .evidence(&observed);
    if shared {
        let same_job = world.transaction(
            source,
            &effect_at,
            Some(&other),
            "/org/freedesktop/systemd1/job/4821",
            strength,
        );
        let carries_action = world.transaction(
            source,
            &effect_at,
            Some(&other),
            &format!(
                "systemd:/org/freedesktop/systemd1/job/4821;ono={}",
                action.as_str()
            ),
            strength,
        );
        effect = effect.evidence(&same_job).evidence(&carries_action);
    }

    let context = world.context();
    (vec![cause.seal(), effect.seal()], context)
}

#[test]
fn should_never_emit_a_causal_class_when_two_events_share_no_evidence() {
    // §51 TEST-006, and the property §2 invariant 7 states: temporal proximity alone MUST NEVER
    // create a causal relation. Every world below is built to look like a cause and an effect —
    // adjacent instants, a real action id, a real systemd job path, systemd job subtypes, a unit
    // state transition, source sequences, and half the time the very same subject — and to
    // publish no identity that joins the two events.
    let engine = CausalEngine::builtin();
    let subtypes = [
        None,
        Some(ono_temporal_query::causal::SYSTEMD_JOB_NEW),
        Some(ono_temporal_query::causal::SYSTEMD_JOB_REMOVED),
    ];
    let mut worlds = 0_u32;
    let mut joined = 0_u32;

    for cause_kind in READ_BY_RULES {
        for effect_kind in READ_BY_RULES {
            for source in EVERY_SOURCE {
                for strength in EvidenceStrength::ALL {
                    for same_subject in [false, true] {
                        for subtype in subtypes {
                            for gap in [1_u64, 45] {
                                let (events, context) = world_of(
                                    *cause_kind,
                                    *effect_kind,
                                    source,
                                    *strength,
                                    same_subject,
                                    subtype,
                                    gap,
                                    false,
                                );
                                worlds += 1;
                                for link in engine.links(&events, &context) {
                                    assert!(
                                        !link.is_causal(),
                                        "`{}` claimed {} between a {cause_kind} and a \
                                         {effect_kind} that share no published identity — v0.5 \
                                         §2 invariant 7 and §15.2",
                                        link.rule,
                                        link.relation
                                    );
                                }

                                // The same world, with the one difference that matters.
                                let (events, context) = world_of(
                                    *cause_kind,
                                    *effect_kind,
                                    source,
                                    *strength,
                                    same_subject,
                                    subtype,
                                    gap,
                                    true,
                                );
                                joined += u32::from(
                                    engine
                                        .links(&events, &context)
                                        .iter()
                                        .any(ono_temporal_core::CausalLink::is_causal),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(worlds > 5_000, "the sweep is wide enough to be evidence");
    assert!(
        joined > 0,
        "the same worlds with a shared transaction produced no causal link either, so the sweep \
         proves nothing: it has to be able to say `caused` before its silence means anything"
    );
}

#[test]
fn should_emit_a_causal_link_when_the_same_world_gains_one_shared_transaction() {
    // The control for the falsification suite: the only difference between this world and the
    // near misses above is a token both events carry, and that difference is what a causal claim
    // rests on.
    let mut world = World::new();
    let unit = service("nginx.service");
    let token = "/org/freedesktop/systemd1/job/4821";
    let job = world.transaction(
        "linux.systemd-dbus",
        "2026-08-31T14:03:11Z",
        Some(&unit),
        token,
        EvidenceStrength::Authoritative,
    );
    let carried = world.transaction(
        "linux.systemd-dbus",
        "2026-08-31T14:03:13Z",
        Some(&unit),
        token,
        EvidenceStrength::Authoritative,
    );
    let observed = world.transition(
        "linux.systemd-dbus",
        "2026-08-31T14:03:13Z",
        &unit,
        "active_state",
        Value::string("activating"),
        Value::string("active"),
        EvidenceStrength::Authoritative,
    );

    let queued = EventBuilder::new(EventKind::ProviderEvent, "2026-08-31T14:03:11Z")
        .subtype(ono_temporal_query::causal::SYSTEMD_JOB_REMOVED)
        .provider("systemd")
        .related(&unit, SpatialType::Service, "nginx.service")
        .evidence(&job)
        .seal();
    let became = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:13Z")
        .provider("systemd")
        .subject(&unit, SpatialType::Service, "nginx.service")
        .changed(
            "active_state",
            Value::string("activating"),
            Value::string("active"),
        )
        .evidence(&carried)
        .evidence(&observed)
        .seal();

    let context = world.context();
    let links = CausalEngine::builtin().links(&[queued, became], &context);
    assert!(
        links
            .iter()
            .any(|link| link.is_causal() && link.rule.as_str() == "ono.systemd-job-result"),
        "one authoritative source stating both ends is §15.2's strongest example"
    );
}

#[test]
fn should_stay_correlation_when_pressure_and_a_failure_merely_overlap() {
    let mut world = World::new();
    let root = filesystem("/");
    let worker = process(1827);
    let pressure = world.field(
        "linux.procfs",
        "2026-08-31T14:03:10Z",
        &root,
        "available",
        Value::Int(4096),
        EvidenceStrength::Observational,
    );
    let failed = world.field(
        "ono.recorder",
        "2026-08-31T14:03:40Z",
        &worker,
        "result",
        Value::string("no-space"),
        EvidenceStrength::Asserted,
    );
    let change = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:10Z")
        .provider("linux.procfs")
        .subject(&root, SpatialType::Filesystem, "/")
        .changed("available", Value::Int(40960), Value::Int(4096))
        .evidence(&pressure)
        .seal();
    let failure = EventBuilder::new(EventKind::ActionFailed, "2026-08-31T14:03:40Z")
        .provider("ono.recorder")
        .subject(&worker, SpatialType::Process, "process/1827")
        .evidence(&failed)
        .seal();

    let context = world.context();
    let links = CausalEngine::builtin().links(&[change, failure], &context);

    assert!(!links.is_empty());
    for link in &links {
        assert_eq!(link.rule.as_str(), "ono.resource-pressure-overlap");
        assert!(!link.is_causal(), "§15.5: overlap is association");
    }
}
