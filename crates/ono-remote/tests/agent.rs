//! The agent end of spec §21.4: provider negotiation from a real registry, bounded answers,
//! and a clean end of session (spec §37 Phase H: "agent", "provider negotiation").

mod common;

use common::fixture::fixture_schema_id;
use common::{connect, within};
use ono_protocol::{ProviderDescriptor, RemoteMessage, RemoteQuery};
use ono_provider_api::Risk;

#[tokio::test]
async fn should_negotiate_the_registry_providers_with_their_availability() {
    let connected = connect().await;
    let negotiated = connected.link.negotiated();

    let usable: Vec<&str> = negotiated
        .providers()
        .iter()
        .filter(|provider| provider.is_available())
        .map(ProviderDescriptor::id)
        .collect();
    assert_eq!(
        usable,
        ["fixture.demo"],
        "the agent announces exactly the providers its registry can answer with (spec §21.2)"
    );

    let demo = negotiated
        .providers()
        .iter()
        .find(|provider| provider.id() == "fixture.demo")
        .expect("the working provider is announced");
    assert_eq!(
        demo.targets(),
        ["process", "tick", "flaky"],
        "the provider's targets cross the link, so `get process` can be routed"
    );

    let absent = negotiated
        .providers()
        .iter()
        .find(|provider| provider.id() == "fixture.absent")
        .expect("an unavailable provider is still announced");
    assert_eq!(
        absent.unavailable_reason(),
        Some("no service manager in this fixture"),
        "spec §21.3/§35.3: a capability that is missing must be visibly missing"
    );
}

#[tokio::test]
async fn should_announce_capabilities_with_the_risk_the_provider_declares() {
    let connected = connect().await;
    let demo = connected
        .link
        .negotiated()
        .providers()
        .iter()
        .find(|provider| provider.id() == "fixture.demo")
        .expect("the working provider is announced");

    let signal = demo
        .capabilities()
        .iter()
        .find(|capability| capability.id() == "process.signal")
        .expect("the mutating capability is announced");
    let projected = signal.to_capability();
    assert_eq!(
        projected.risk(),
        Risk::Mutate,
        "spec §17.1 computes risk from what the provider declares, on both ends of a link"
    );
    assert!(projected.needs_elevation());
}

#[tokio::test]
async fn should_announce_the_schemas_the_registry_produces() {
    let connected = connect().await;
    let schemas = connected.link.negotiated().schemas();
    assert!(
        schemas
            .iter()
            .any(|id| id == &fixture_schema_id().to_string()),
        "the provider's schema is negotiated, so the caller knows what it will receive: {schemas:?}"
    );
}

#[tokio::test]
async fn should_answer_at_most_the_limit_the_query_asked_for() {
    let connected = connect().await;
    let mut stream = connected
        .link
        .protocol()
        .query(&RemoteQuery::target("tick").limit(5))
        .expect("a query opens a stream");

    let mut values = 0;
    within(async {
        while let Some(message) = stream.recv().await {
            if let RemoteMessage::Value(_) = message {
                values += 1;
            }
        }
    })
    .await;

    assert_eq!(
        values, 5,
        "the agent bounds an endless target by the query's limit rather than streaming forever"
    );
}

#[tokio::test]
async fn should_end_the_agent_loop_cleanly_when_the_caller_disconnects() {
    let connected = connect().await;
    drop(connected.link);

    let outcome = within(connected.agent)
        .await
        .expect("the agent task does not panic");
    assert_eq!(
        outcome,
        Ok(()),
        "a caller hanging up is a successful end of session for the agent, not a failure"
    );
}
