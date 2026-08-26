//! The security model at the link layer of Phase H (spec §21.5, ADR-0015 T5/T6).
//!
//! A [`RemoteLink`] is opened through the same trust decision the raw protocol makes; these
//! suites prove the refusals still hold at this level, before any provider is mounted.

mod common;

use std::sync::Arc;

use common::fixture::{FixtureObserved, fixture_registry, fixture_schemas};
use common::within;
use ono_core::ErrorCode;
use ono_protocol::{ClientConfig, HostKey, Identity, PlainTransport, TrustPolicy, TrustStore};
use ono_remote::{AgentConfig, RemoteLink, serve_registry};
use ono_value::ErrorValue;

/// Attempts a connection whose transport authenticated `presents` about the peer.
async fn try_connect(
    client: ClientConfig,
    presents: Option<HostKey>,
) -> Result<RemoteLink, ErrorValue> {
    let (near, far) = tokio::io::duplex(16 * 1024);
    let registry = fixture_registry(Arc::new(FixtureObserved::default()));
    let config = AgentConfig::new(registry).with_identity(Identity::new("remote-user"));
    tokio::spawn(async move { serve_registry(PlainTransport::new(far), config).await });
    let mut transport = PlainTransport::new(near);
    if let Some(key) = presents {
        transport = transport.with_peer_key(key);
    }
    within(RemoteLink::connect(transport, client)).await
}

fn pinning_config(store: TrustStore) -> ClientConfig {
    ClientConfig::new("remhost")
        .with_schemas(fixture_schemas())
        .with_trust_store(store)
        .with_trust_policy(TrustPolicy::Required)
        .with_identity(Identity::new("tester"))
}

#[tokio::test]
async fn should_refuse_a_changed_host_key_with_the_stable_safety_code() {
    let store = TrustStore::in_memory();
    let pinned = HostKey::new("ed25519", *b"the-first-key-this-host-showed--");
    store.pin("remhost", &pinned).expect("the first key pins");

    let impostor = HostKey::new("ed25519", *b"a-completely-different-public-ky");
    let error = try_connect(pinning_config(store), Some(impostor))
        .await
        .expect_err("a peer presenting another key than the pinned one cannot be linked to");

    assert_eq!(
        error.code(),
        ErrorCode::RemoteHostKeyChanged,
        "ADR-0015 T6: a changed key is E0603, a safety refusal, not a transport hiccup"
    );
    assert!(
        error.retryable() != Some(true),
        "retrying would not make the key match; the refusal must say so"
    );
}

#[tokio::test]
async fn should_refuse_an_unauthenticated_transport_when_trust_is_required() {
    let error = try_connect(pinning_config(TrustStore::in_memory()), None)
        .await
        .expect_err("a transport that authenticated nobody cannot satisfy a required trust policy");

    assert_eq!(
        error.code(),
        ErrorCode::SafetyPolicyDenied,
        "ADR-0015 standing rule 4: the refusal is an error, never a prompt"
    );
}

#[tokio::test]
async fn should_link_and_answer_when_the_presented_key_matches_the_pinned_one() {
    let store = TrustStore::in_memory();
    let key = HostKey::new("ed25519", *b"the-first-key-this-host-showed--");
    store.pin("remhost", &key).expect("the first key pins");

    let link = try_connect(pinning_config(store), Some(key))
        .await
        .expect("a matching key establishes the link");

    assert!(
        !link.providers().is_empty(),
        "the trusted link goes on to negotiate providers"
    );
}
