//! Causality across a host boundary (spec v0.5 §26.3, §26.4, §24.3, §55.6).
//!
//! §26.4: "A causal explanation can cross hosts only when the evidence chain actually crosses the
//! boundary… Mere temporal proximity between events on different hosts is correlation at most."
//! §55.6 states the same prohibition from the other side: sorting remote events by timestamp and
//! treating that order as causal truth is forbidden.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

mod causal_fixture;

use causal_fixture::{EventBuilder, World, endpoint, instant, remote_domain, remote_scope, socket};
use ono_spatial_core::SpatialType;
use ono_temporal_core::{EventKind, EvidenceStrength, Ordering, TimeRange, happens_before};
use ono_temporal_query::causal::CausalEngine;
use ono_value::Value;

/// The transaction the remote provider published on both sides of the link (§26.4).
const REQUEST: &str = "req-42";

#[test]
fn should_claim_nothing_when_two_hosts_share_only_a_plausible_gap() {
    let mut world = World::new();
    let here = socket(4001);
    let there = endpoint("10.0.0.7:5432");
    let local = world.field(
        "linux.netlink",
        "2026-08-31T14:03:11Z",
        &here,
        "retries",
        Value::Int(41),
        EvidenceStrength::Observational,
    );
    let remote = world.field(
        "remote:db01/linux.procfs",
        "2026-08-31T14:03:10Z",
        &there,
        "state",
        Value::string("gone"),
        EvidenceStrength::Asserted,
    );

    let retry = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:11Z")
        .provider("linux.netlink")
        .subject(&here, SpatialType::Socket, "socket/4001")
        .changed("retries", Value::Int(3), Value::Int(41))
        .evidence(&local)
        .seal();
    let vanished = EventBuilder::new(EventKind::ObjectDisappeared, "2026-08-31T14:03:10Z")
        .provider("db01")
        .on_host(remote_domain(), remote_scope())
        .subject(&there, SpatialType::Endpoint, "10.0.0.7:5432")
        .evidence(&remote)
        .seal();

    assert_eq!(
        happens_before(&vanished, &retry).0,
        Ordering::Concurrent,
        "§26.1: two clock domains with no shared evidence are concurrent"
    );

    let links = CausalEngine::builtin().links(&[retry, vanished], &world.context());
    assert!(
        links.iter().all(|link| !link.is_causal()),
        "§26.4: temporal proximity between hosts is correlation at most"
    );
}

#[test]
fn should_offer_correlation_when_a_connection_joins_the_two_hosts() {
    let mut world = World::new();
    let here = socket(4001);
    let there = endpoint("10.0.0.7:5432");
    world.relation(
        "linux.netlink",
        "2026-08-31T14:00:00Z",
        &here,
        "socket.connected_to",
        &there,
        TimeRange::since(instant("2026-08-31T14:00:00Z")),
        EvidenceStrength::Asserted,
    );
    let local = world.field(
        "linux.netlink",
        "2026-08-31T14:03:11Z",
        &here,
        "retries",
        Value::Int(41),
        EvidenceStrength::Observational,
    );
    let remote = world.field(
        "remote:db01/linux.procfs",
        "2026-08-31T14:03:10Z",
        &there,
        "state",
        Value::string("gone"),
        EvidenceStrength::Asserted,
    );

    let retry = EventBuilder::new(EventKind::ObjectChanged, "2026-08-31T14:03:11Z")
        .provider("linux.netlink")
        .subject(&here, SpatialType::Socket, "socket/4001")
        .changed("retries", Value::Int(3), Value::Int(41))
        .evidence(&local)
        .seal();
    let vanished = EventBuilder::new(EventKind::ObjectDisappeared, "2026-08-31T14:03:10Z")
        .provider("db01")
        .on_host(remote_domain(), remote_scope())
        .subject(&there, SpatialType::Endpoint, "10.0.0.7:5432")
        .evidence(&remote)
        .seal();

    let links = CausalEngine::builtin().links(&[retry, vanished], &world.context());
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].rule.as_str(), "ono.remote-endpoint-retry-spike");
    assert!(!links[0].is_causal(), "§15.5, §26.4");
    assert!(links[0].strength <= EvidenceStrength::Correlated);
}

#[test]
fn should_cross_the_boundary_when_one_transaction_identity_crosses_it() {
    // §26.4's own example: `web01 request @r42 -> network connection identity -> db01 accepted
    // connection @r42`. One provider published one token on both sides and numbered its own
    // stream, so the evidence chain crosses the boundary and the causal chain may follow it.
    let mut world = World::new();
    let here = socket(4001);
    let there = endpoint("10.0.0.7:5432");
    let sent = world.transaction(
        "remote:db01/linux.procfs",
        "2026-08-31T14:03:11Z",
        Some(&here),
        REQUEST,
        EvidenceStrength::Asserted,
    );
    let accepted = world.transaction(
        "remote:db01/linux.procfs",
        "2026-08-31T14:03:10Z",
        Some(&there),
        REQUEST,
        EvidenceStrength::Asserted,
    );

    let request = EventBuilder::new(EventKind::ProviderEvent, "2026-08-31T14:03:11Z")
        .provider("db01")
        .sequence(1)
        .subject(&here, SpatialType::Socket, "socket/4001")
        .evidence(&sent)
        .seal();
    // The wall clock on the far host reads *earlier*, which is exactly the case §55.6 forbids
    // deciding from. The sequence the provider stamped is what orders the two.
    let response = EventBuilder::new(EventKind::ProviderEvent, "2026-08-31T14:03:10Z")
        .provider("db01")
        .on_host(remote_domain(), remote_scope())
        .sequence(2)
        .subject(&there, SpatialType::Endpoint, "10.0.0.7:5432")
        .evidence(&accepted)
        .seal();

    let links =
        CausalEngine::builtin().links(&[request.clone(), response.clone()], &world.context());
    let crossing = links
        .iter()
        .find(|link| link.is_causal())
        .expect("§26.4: the chain follows the evidence across the boundary");

    assert_eq!(crossing.rule.as_str(), "ono.provider-causal-token");
    assert_eq!(crossing.cause, request.event_id);
    assert_eq!(crossing.effect, response.event_id);
    assert_eq!(
        happens_before(&request, &response).0,
        Ordering::Concurrent,
        "§26.3: the ordering model itself still claims nothing between the two clock domains"
    );
}

#[test]
fn should_claim_nothing_when_the_shared_token_has_no_sequence_behind_it() {
    let mut world = World::new();
    let here = socket(4001);
    let there = endpoint("10.0.0.7:5432");
    let sent = world.transaction(
        "remote:db01/linux.procfs",
        "2026-08-31T14:03:11Z",
        Some(&here),
        REQUEST,
        EvidenceStrength::Asserted,
    );
    let accepted = world.transaction(
        "remote:db01/linux.procfs",
        "2026-08-31T14:03:12Z",
        Some(&there),
        REQUEST,
        EvidenceStrength::Asserted,
    );

    let request = EventBuilder::new(EventKind::ProviderEvent, "2026-08-31T14:03:11Z")
        .provider("db01")
        .subject(&here, SpatialType::Socket, "socket/4001")
        .evidence(&sent)
        .seal();
    let response = EventBuilder::new(EventKind::ProviderEvent, "2026-08-31T14:03:12Z")
        .provider("db01")
        .on_host(remote_domain(), remote_scope())
        .subject(&there, SpatialType::Endpoint, "10.0.0.7:5432")
        .evidence(&accepted)
        .seal();

    let links = CausalEngine::builtin().links(&[request, response], &world.context());
    assert!(
        links.is_empty(),
        "§26.1 and §55.6: a wall clock a second apart across two hosts orders nothing"
    );
}

#[test]
fn should_cap_a_plugin_transaction_at_asserted() {
    // §37.4: "plugin causal strength MUST NOT exceed `asserted`".
    let mut world = World::new();
    let here = socket(4001);
    let there = socket(4002);
    let first = world.transaction(
        "kuang:dev.example.packet-eye/flows",
        "2026-08-31T14:03:11Z",
        Some(&here),
        REQUEST,
        EvidenceStrength::Authoritative,
    );
    let second = world.transaction(
        "kuang:dev.example.packet-eye/flows",
        "2026-08-31T14:03:12Z",
        Some(&there),
        REQUEST,
        EvidenceStrength::Authoritative,
    );

    let cause = EventBuilder::new(EventKind::ProviderEvent, "2026-08-31T14:03:11Z")
        .provider("packet-eye")
        .sequence(1)
        .subject(&here, SpatialType::Socket, "socket/4001")
        .evidence(&first)
        .seal();
    let effect = EventBuilder::new(EventKind::ProviderEvent, "2026-08-31T14:03:12Z")
        .provider("packet-eye")
        .sequence(2)
        .subject(&there, SpatialType::Socket, "socket/4002")
        .evidence(&second)
        .seal();

    let links = CausalEngine::builtin().links(&[cause, effect], &world.context());
    let link = links
        .first()
        .expect("the plugin's own transaction is a link");
    assert_eq!(link.strength, EvidenceStrength::Asserted);
}
