//! Remote and distributed time across a real link (v0.5 §24, §25, §26, §30.6; §48.8).
//!
//! Both ends run in this process over an in-memory duplex, so the suite needs no network, no
//! container and no clock — which is exactly what makes §52.4's "two linked test hosts with
//! artificial wall-clock skew" a deterministic fixture rather than a flaky one. The skew is a
//! number the fixture chooses.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions; a failed precondition should abort loudly"
)]

mod common;

use std::sync::Arc;

use common::fixture::{FixtureObserved, HISTORICAL_TARGET, fixture_schemas, temporal_registry};
use common::within;
use ono_core::ErrorCode;
use ono_protocol::{ClientConfig, Identity, PeerClock, TrustPolicy, UnauthenticatedTransport};
use ono_provider_api::{Action, ObjectId, Provider, Query, TimeWindow};
use ono_remote::{AgentConfig, RemoteIngest, RemoteLink, serve_registry};
use ono_temporal_core::{Ordering, TemporalEvent, happens_before};
use ono_value::{SchemaId, Value};

/// The scope a fixture event is placed in: the far side, reached through a link (§19.1).
fn fixture_scope() -> ono_spatial_core::SpatialScope {
    ono_spatial_core::SpatialScope::remote_host(
        "web01",
        ono_spatial_core::BootIdentity::of("web01", Some("boot-a"), None),
    )
}

/// A fixed instant the whole suite navigates by.
fn instant(text: &str) -> jiff::Timestamp {
    text.parse().expect("a fixed instant")
}

/// A link to a fixture agent that announces `clock` and the temporal claims of `historical`.
async fn link_to(host: &str, clock: PeerClock, historical: bool) -> RemoteLink {
    let (near, far) = tokio::io::duplex(16 * 1024);
    let registry = temporal_registry(Arc::new(FixtureObserved::default()), historical);
    let config = AgentConfig::new(registry)
        .with_identity(Identity::new("remote-user"))
        .with_clock(clock);
    tokio::spawn(async move { serve_registry(UnauthenticatedTransport::new(far), config).await });
    within(RemoteLink::connect(
        UnauthenticatedTransport::new(near),
        ClientConfig::new(host)
            .with_schemas(fixture_schemas())
            .with_trust_policy(TrustPolicy::Unauthenticated)
            .with_identity(Identity::new("tester")),
    ))
    .await
    .expect("the fixture handshake succeeds")
}

/// The event `at`, as it lands over `link`, in that link's clock domain.
fn arriving(link: &RemoteLink, at: &str, sequence: u64, ingested: &str) -> TemporalEvent {
    let ingest = RemoteIngest::new(link.host(), link.negotiated().peer().clock().cloned());
    let record = common::fixture::fixture_record_observed(1, "nginx", instant(at));
    let event = ono_provider_api::ObjectEvent::changed(&record, ["name"]).with_sequence(sequence);
    ono_temporal_core::EventSeed {
        kind: ono_temporal_core::EventKind::ObjectChanged,
        subtype: None,
        scope: fixture_scope(),
        subject: None,
        related: Vec::new(),
        times: ingest.times_of_event(&event, instant(ingested)),
        before: None,
        after: None,
        changed_fields: Vec::new(),
        evidence: Vec::new(),
        causal_parents: Vec::new(),
        payload: None,
        provenance: record.provenance().clone(),
    }
    .seal()
}

#[tokio::test]
async fn should_keep_the_remote_source_time_and_the_local_ingest_time_distinct() {
    let link = link_to("web01", PeerClock::of("web01").on_boot("boot-a"), false).await;
    let ingest = RemoteIngest::new(link.host(), link.negotiated().peer().clock().cloned());
    let record =
        common::fixture::fixture_record_observed(7, "nginx", instant("2026-08-31T12:00:00Z"));

    let times = ingest.times_of_record(&record, instant("2026-08-31T12:00:04Z"));

    assert_eq!(
        times.source_time,
        Some(instant("2026-08-31T12:00:00Z")),
        "§24.2: the local ledger MUST NOT overwrite remote source time with ingest time"
    );
    assert_eq!(times.ingested_at, instant("2026-08-31T12:00:04Z"));
    assert_ne!(
        times.source_time,
        Some(times.ingested_at),
        "the two instants answer different questions and stay two fields (§3.3)"
    );
}

#[tokio::test]
async fn should_preserve_the_remote_observation_when_a_record_crosses_the_link() {
    let link = link_to("web01", PeerClock::of("web01").on_boot("boot-a"), false).await;
    let observed = instant("2026-08-31T12:00:00Z");
    let record = common::fixture::fixture_record_observed(7, "nginx", observed);

    let Value::Record(retagged) = link.retag(record.into_value()) else {
        panic!("a record retags as a record");
    };

    assert_eq!(
        retagged.provenance().observed(),
        Some(observed),
        "§24.2: arrival rewrites the link and nothing else"
    );
    assert_eq!(
        retagged.provenance().link(),
        &ono_value::Link::Remote("web01".into()),
        "the record says which machine it was observed on"
    );
}

#[tokio::test]
async fn should_answer_concurrent_when_two_hosts_wall_clocks_are_all_that_differ() {
    // §52.4's fixture: two linked hosts with an artificial ninety-second wall-clock skew.
    let web = link_to("web01", PeerClock::of("web01").on_boot("boot-a"), false).await;
    let db = link_to("db01", PeerClock::of("db01").on_boot("boot-b"), false).await;

    let request = arriving(&web, "2026-08-31T12:00:00Z", 1, "2026-08-31T12:00:01Z");
    let accept = arriving(&db, "2026-08-31T11:58:30Z", 1, "2026-08-31T12:00:01Z");

    let (ordering, evidence) = happens_before(&request, &accept);
    assert_eq!(
        ordering,
        Ordering::Concurrent,
        "§24.3: two hosts' events are not ordered because their wall clocks differ"
    );
    assert_eq!(
        evidence, None,
        "§26.3: `inspect` MUST NOT claim semantic ordering it has no evidence for"
    );
}

#[tokio::test]
async fn should_order_two_events_of_one_remote_stream_by_the_sequence_the_source_gave() {
    let web = link_to("web01", PeerClock::of("web01").on_boot("boot-a"), false).await;

    let first = arriving(&web, "2026-08-31T12:00:00Z", 1, "2026-08-31T12:00:01Z");
    let second = arriving(&web, "2026-08-31T12:00:02Z", 2, "2026-08-31T12:00:03Z");

    let (ordering, evidence) = happens_before(&first, &second);
    assert_eq!(
        ordering,
        Ordering::Before,
        "§24.3: same-source sequence continuity is strong ordering evidence"
    );
    assert_eq!(
        evidence,
        Some(ono_temporal_core::OrderEvidence::SourceSequence),
        "the answer names what it rests on"
    );
}

#[tokio::test]
async fn should_carry_the_clock_uncertainty_the_peer_declared_onto_an_arriving_event() {
    let link = link_to(
        "web01",
        PeerClock::of("web01")
            .on_boot("boot-a")
            .within(ono_value::Duration::from_nanoseconds(40_000_000)),
        false,
    )
    .await;
    let ingest = RemoteIngest::new(link.host(), link.negotiated().peer().clock().cloned());

    let times = ingest.times(
        Some(instant("2026-08-31T12:00:00Z")),
        instant("2026-08-31T12:00:01Z"),
    );

    assert_eq!(
        times.clock_uncertainty,
        Some(ono_value::Duration::from_nanoseconds(40_000_000)),
        "§24.4: uncertainty travels with the event rather than being implied away"
    );
}

#[tokio::test]
async fn should_leave_the_uncertainty_unknown_when_the_peer_measured_none() {
    let link = link_to("web01", PeerClock::of("web01").on_boot("boot-a"), false).await;
    let ingest = RemoteIngest::new(link.host(), link.negotiated().peer().clock().cloned());

    let times = ingest.times(
        Some(instant("2026-08-31T12:00:00Z")),
        instant("2026-08-31T12:00:01Z"),
    );

    assert_eq!(
        times.clock_uncertainty, None,
        "§35.3: unmeasured is unknown, never a measured zero"
    );
}

#[tokio::test]
async fn should_say_the_remote_keeps_no_history_when_a_historical_question_is_asked() {
    let link = link_to("web01", PeerClock::of("web01").on_boot("boot-a"), false).await;
    let provider = link
        .providers()
        .iter()
        .find(|provider| provider.targets().contains(&"process"))
        .expect("the link mounted the fixture's process target")
        .clone();

    let refusal = provider
        .history(
            &Query::target("process"),
            &TimeWindow::since(instant("2026-08-31T11:50:00Z")),
        )
        .expect_err("§24.5: a remote with no history says so");

    assert_eq!(
        refusal.code(),
        ErrorCode::TemporalUnsupportedSource,
        "the refusal is the temporal family's own, not a generic provider error"
    );
    assert!(
        refusal.message().contains("web01"),
        "it names the host that cannot answer, so the user knows where the gap is: {}",
        refusal.message()
    );
    assert!(
        !provider.temporal().historical_query,
        "and the claim the refusal rests on is the one negotiation reported"
    );
}

#[tokio::test]
async fn should_answer_a_historical_question_when_the_remote_declared_persisted_history() {
    let link = link_to("web01", PeerClock::of("web01").on_boot("boot-a"), true).await;
    let provider = link
        .providers()
        .iter()
        .find(|provider| provider.targets().contains(&HISTORICAL_TARGET))
        .expect("the link mounted the fixture's historical target")
        .clone();

    assert!(
        provider.temporal().historical_query,
        "§24.1: negotiation reported that the peer keeps history"
    );
    let mut stream = provider
        .history(
            &Query::target(HISTORICAL_TARGET),
            &TimeWindow::since(instant("2026-08-31T11:50:00Z")),
        )
        .expect("the remote answers about the past");

    let mut observed = Vec::new();
    while let Some(event) = stream.recv().await {
        if let ono_pipeline::StreamEvent::Value(Value::Record(record)) = event {
            observed.push(record.provenance().observed());
        }
    }
    assert_eq!(
        observed,
        vec![
            Some(instant("2026-08-31T11:55:00Z")),
            Some(instant("2026-08-31T11:56:00Z"))
        ],
        "the records arrive with the source's own instants, in the window that was asked for"
    );
}

#[tokio::test]
async fn should_refuse_a_remote_mutation_attempted_from_a_historical_session() {
    let link = link_to("web01", PeerClock::of("web01").on_boot("boot-a"), false).await;

    let refusal = link
        .act(
            &ono_protocol::ActRequest::new(
                "process",
                "stop",
                ObjectId::new(SchemaId::new("ono.test.remote-fixture", 1), [Value::Int(2)]),
            )
            .with_argument("signal", Value::string("SIGTERM"))
            .attempted_at(instant("2026-08-31T11:50:00Z")),
        )
        .await
        .expect_err("§4.7: past context is read-only, and §30.6 puts the decision at the agent");

    assert_eq!(refusal.code(), ErrorCode::TemporalReadOnly);
    assert_eq!(
        refusal.metadata().get("denied_because"),
        Some(&Value::string("historical_context")),
        "the refusal carries its own discriminator beside the existing ones"
    );
}

#[tokio::test]
async fn should_still_perform_a_mutation_asked_for_in_the_present() {
    let link = link_to("web01", PeerClock::of("web01").on_boot("boot-a"), false).await;

    let outcome = link
        .act(&ono_protocol::ActRequest::from_action(
            &Action::new(
                "process",
                "stop",
                ObjectId::new(SchemaId::new("ono.test.remote-fixture", 1), [Value::Int(2)]),
            )
            .with("signal", Value::string("SIGTERM")),
        ))
        .await
        .expect("a present-tense mutation is unaffected");

    assert_eq!(outcome.status(), ono_value::ActionStatus::Success);
}
