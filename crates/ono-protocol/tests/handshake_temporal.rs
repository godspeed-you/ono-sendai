//! Temporal capability negotiation and the clock identity a peer reports (v0.5 §24.1, §24.2,
//! §25.5).
//!
//! §24.1 lists four things a linked host may be — snapshots only, live events, persisted
//! history, no temporal support at all — and requires negotiation to report which. §24.5 turns
//! that report into a promise: a remote with no history must *say so* rather than let current
//! state stand in for past state. So the two properties this suite holds are that the report
//! arrives, and that its absence degrades the link instead of failing it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions; a failed precondition should abort loudly"
)]

mod common;

use common::{client_config, server_config, try_connect};
use ono_protocol::{PeerClock, ProviderDescriptor, RemoteTemporal};
use ono_provider_api::TemporalCapabilities;

/// A server whose journal answers about the past and whose procfs does not.
fn temporal_server() -> ono_protocol::ServerConfig {
    server_config()
        .with_provider(
            ProviderDescriptor::new("linux.journald")
                .with_targets(["journal-event"])
                .with_capabilities(["journal.read"])
                .with_temporal(TemporalCapabilities {
                    current_snapshot: false,
                    live_events: true,
                    historical_query: true,
                    exhaustive_events: false,
                    causal_tokens: false,
                    checkpointable: false,
                    retained_history: Some(ono_value::Duration::from_nanoseconds(
                        86_400_000_000_000,
                    )),
                }),
        )
        .with_clock(
            PeerClock::of("web01")
                .on_boot("boot-a")
                .reading_at("2026-08-31T12:00:00Z".parse().expect("a fixed instant"))
                .within(ono_value::Duration::from_nanoseconds(40_000_000)),
        )
}

#[tokio::test]
async fn should_report_what_the_peer_can_answer_about_the_past_when_a_link_is_negotiated() {
    let fixture = try_connect(client_config("web01"), temporal_server(), None)
        .await
        .expect("the handshake succeeds");
    let negotiated = fixture.link.negotiated();

    let journal = negotiated
        .providers()
        .iter()
        .find(|provider| provider.id() == "linux.journald")
        .expect("the peer offered its journal");
    assert!(
        journal.temporal().historical_query,
        "§24.1: negotiation MUST report that the peer keeps persisted history"
    );
    assert_eq!(
        journal.temporal().retained_history,
        Some(ono_value::Duration::from_nanoseconds(86_400_000_000_000)),
        "§21.1: how far back a source keeps material crosses the link with the claim"
    );
    assert_eq!(
        negotiated.temporal(),
        RemoteTemporal::PersistedHistory,
        "§24.1: the link reports the strongest thing the peer can answer"
    );
}

#[tokio::test]
async fn should_degrade_the_link_rather_than_refuse_it_when_the_peer_has_no_temporal_support() {
    let fixture = try_connect(client_config("testhost"), server_config(), None)
        .await
        .expect("§24.5: a peer with no history is still a peer");
    let negotiated = fixture.link.negotiated();

    assert_eq!(
        negotiated.temporal(),
        RemoteTemporal::None,
        "a peer that claims nothing about time is reported as claiming nothing"
    );
    for provider in negotiated.providers() {
        assert_eq!(
            provider.temporal(),
            TemporalCapabilities::none(),
            "silence is not coverage (§21.1)"
        );
    }
    assert!(
        !negotiated.providers().is_empty(),
        "the link still negotiated its providers"
    );
}

#[tokio::test]
async fn should_report_only_live_events_when_that_is_all_the_peer_claims() {
    let server = server_config().with_provider(
        ProviderDescriptor::new("linux.netlink")
            .with_targets(["interface"])
            .with_temporal(TemporalCapabilities {
                live_events: true,
                ..TemporalCapabilities::none()
            }),
    );
    let fixture = try_connect(client_config("testhost"), server, None)
        .await
        .expect("the handshake succeeds");

    assert_eq!(
        fixture.link.negotiated().temporal(),
        RemoteTemporal::LiveEvents,
        "§24.1's second case: events as they happen, and nothing about the past"
    );
}

#[tokio::test]
async fn should_report_snapshots_only_when_the_peer_claims_nothing_stronger() {
    let server = server_config().with_provider(
        ProviderDescriptor::new("linux.procfs.now")
            .with_targets(["process"])
            .with_temporal(TemporalCapabilities::snapshot_only()),
    );
    let fixture = try_connect(client_config("testhost"), server, None)
        .await
        .expect("the handshake succeeds");

    assert_eq!(
        fixture.link.negotiated().temporal(),
        RemoteTemporal::CurrentSnapshot,
        "§24.1's first case: what is true now, and nothing else"
    );
}

#[tokio::test]
async fn should_carry_the_peers_clock_identity_and_boot_when_a_link_is_negotiated() {
    let fixture = try_connect(client_config("web01"), temporal_server(), None)
        .await
        .expect("the handshake succeeds");
    let clock = fixture
        .link
        .negotiated()
        .peer()
        .clock()
        .expect("§24.2: a remote event preserves a source clock identity");

    assert_eq!(clock.clock_id(), "web01");
    assert_eq!(
        clock.boot_id(),
        Some("boot-a"),
        "§25.5: boot_id separates clock domains, so it crosses the link"
    );
    assert_eq!(
        clock.reading(),
        Some("2026-08-31T12:00:00Z".parse().expect("a fixed instant")),
        "the peer's own wall clock at the handshake is what an offset is measured against"
    );
    assert_eq!(
        clock.uncertainty(),
        Some(ono_value::Duration::from_nanoseconds(40_000_000)),
        "§24.4: uncertainty travels with the claim rather than being implied away"
    );
}

#[tokio::test]
async fn should_report_an_unknown_clock_when_the_peer_says_nothing_about_one() {
    let fixture = try_connect(client_config("testhost"), server_config(), None)
        .await
        .expect("the handshake succeeds");

    assert_eq!(
        fixture.link.negotiated().peer().clock(),
        None,
        "§35.3: a clock nobody reported is unknown, never a fabricated zero offset"
    );
}
