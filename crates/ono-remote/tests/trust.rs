//! The security model at the link layer of Phase H (spec §21.5, ADR-0015 T5/T6).
//!
//! A [`RemoteLink`] is opened through the same trust decision the raw protocol makes; these
//! suites prove the refusals still hold at this level, before any provider is mounted.

mod common;

use std::sync::Arc;

use common::fixture::{FixtureObserved, fixture_registry, fixture_schemas};
use common::within;
use ono_core::ErrorCode;
use ono_protocol::{
    ClientConfig, HostKey, Identity, TrustPolicy, TrustStore, UnauthenticatedTransport,
};
use ono_remote::{AgentConfig, RemoteLink, serve_registry};
use ono_value::ErrorValue;

/// Attempts a connection whose transport authenticated `presents` about the peer.
async fn try_connect(
    client: ClientConfig,
    presents: Option<HostKey>,
) -> Result<RemoteLink, ErrorValue> {
    try_connect_to(client, presents, Identity::new("remote-user")).await
}

/// The same, with the far side reporting `claims` about who it is running as.
///
/// The two arguments are the two identities of v0.4.1 §7.3: `presents` is what the transport
/// proved, `claims` is what the peer said. Every suite that varies one and holds the other still
/// is asking whether the second can influence a decision that belongs to the first.
async fn try_connect_to(
    client: ClientConfig,
    presents: Option<HostKey>,
    claims: Identity,
) -> Result<RemoteLink, ErrorValue> {
    let (near, far) = tokio::io::duplex(16 * 1024);
    let registry = fixture_registry(Arc::new(FixtureObserved::default()));
    let config = AgentConfig::new(registry).with_identity(claims);
    tokio::spawn(async move { serve_registry(UnauthenticatedTransport::new(far), config).await });
    let mut transport = UnauthenticatedTransport::new(near);
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

/// v0.4.1 §7.3: "the runtime identity is useful context but MUST NOT grant authority", and §2.1:
/// "self-reported fields such as user name, UID, operating system, architecture or elevation
/// status MUST NOT satisfy this invariant".
///
/// The most authority-shaped claim a peer can make is `root`, uid 0, elevated. It changes
/// nothing: the decision is about the key, and this peer's key is not pinned.
#[tokio::test]
async fn should_refuse_an_unpinned_peer_however_privileged_it_says_it_is() {
    let unpinned = HostKey::new("ed25519", *b"a key nobody ever decided about-");

    let refusal = try_connect_to(
        pinning_config(TrustStore::in_memory()).with_trust_policy(TrustPolicy::Pinned),
        Some(unpinned),
        Identity::new("root").with_uid(0).elevated(),
    )
    .await
    .expect_err("an unpinned key is refused whatever the peer says about itself");

    assert_eq!(
        refusal.code(),
        ErrorCode::SafetyPolicyDenied,
        "the trust decision reads the key and nothing else; a peer that calls itself root has          only called itself root"
    );
}

/// And the same in the other direction, which is the half that would go unnoticed: a peer whose
/// key *is* pinned is accepted however unprivileged it says it is, and what it said is carried
/// beside the decision as context rather than folded into it.
#[tokio::test]
async fn should_carry_what_the_peer_says_about_itself_beside_the_decision_about_its_key() {
    let store = TrustStore::in_memory();
    let key = HostKey::new("ed25519", *b"the-first-key-this-host-showed--");
    store.pin("remhost", &key).expect("the first key pins");

    let link = try_connect_to(
        pinning_config(store).with_trust_policy(TrustPolicy::Pinned),
        Some(key),
        Identity::new("nobody").with_uid(65_534),
    )
    .await
    .expect("a pinned key is what establishes the link");

    assert_eq!(
        link.negotiated().trust(),
        ono_protocol::TrustDecision::Pinned,
        "the decision came from the key"
    );
    assert_eq!(
        link.negotiated().peer().identity().user(),
        "nobody",
        "v0.4.1 §7.3 keeps the runtime identity as a separate field, so it stays visible"
    );
    assert_eq!(
        link.negotiated().peer().identity().uid(),
        Some(65_534),
        "and stays exactly what the peer said, neither believed nor discarded"
    );
}
