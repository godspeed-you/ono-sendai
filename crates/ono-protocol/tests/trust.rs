//! Host identity: the trust store, the first-connection pin and the refusal that is never a
//! prompt (spec §21.5, §49; ADR-0015 T5 and T6).

mod common;

use common::{impostor_key, pinning_client_config, server_config, server_key, try_connect};
use ono_core::ErrorCode;
use ono_protocol::{HostKey, TrustDecision, TrustPolicy, TrustStore};
use ono_testkit::scratch;

#[tokio::test]
async fn should_pin_the_peer_key_on_the_first_connection() {
    let store = TrustStore::in_memory();
    let fixture = try_connect(
        pinning_client_config("db.example.com", store.clone()),
        server_config(),
        Some(server_key()),
    )
    .await
    .expect("a first connection is allowed and recorded");

    assert_eq!(
        fixture.link.negotiated().trust(),
        TrustDecision::NewlyPinned,
        "a user must be able to see that this was a first contact"
    );
    assert_eq!(
        store.fingerprint("db.example.com"),
        Some(server_key().fingerprint()),
        "the key is written down, so the next connection has something to check against"
    );
}

#[tokio::test]
async fn should_accept_a_later_connection_presenting_the_pinned_key() {
    let store = TrustStore::in_memory();
    store
        .pin("db.example.com", &server_key())
        .expect("pinning a key into an in-memory store succeeds");

    let fixture = try_connect(
        pinning_client_config("db.example.com", store.clone()),
        server_config(),
        Some(server_key()),
    )
    .await
    .expect("the pinned key is accepted");

    assert_eq!(fixture.link.negotiated().trust(), TrustDecision::Pinned);
}

#[tokio::test]
async fn should_refuse_a_peer_presenting_a_different_key_than_the_pinned_one() {
    let store = TrustStore::in_memory();
    store
        .pin("db.example.com", &server_key())
        .expect("pinning a key succeeds");

    let error = try_connect(
        pinning_client_config("db.example.com", store.clone()),
        server_config(),
        Some(impostor_key()),
    )
    .await
    .err()
    .expect("a changed host key is refused");

    assert_eq!(
        error.code(),
        ErrorCode::RemoteHostKeyChanged,
        "spec §43 gives this its own code, and ADR-0006 classifies it as a trust decision"
    );
    assert_eq!(
        error.code().kind(),
        ono_core::ErrorKind::Safety,
        "a changed key is not a transport hiccup"
    );
    assert_eq!(
        store.fingerprint("db.example.com"),
        Some(server_key().fingerprint()),
        "a refused connection must not quietly overwrite the pin it refused against"
    );
    let rendered = error.render_full();
    assert!(
        rendered.contains(&server_key().fingerprint().to_string())
            && rendered.contains(&impostor_key().fingerprint().to_string()),
        "the user must see both fingerprints to judge what happened: {rendered}"
    );
    assert!(
        !rendered.to_lowercase().contains("continue anyway")
            && !rendered.to_lowercase().contains("(y/n)"),
        "ADR-0015 standing rule 4: a refusal is never a prompt. Got: {rendered}"
    );
}

#[tokio::test]
async fn should_refuse_an_unknown_key_when_the_policy_requires_a_pin_to_exist_already() {
    let store = TrustStore::in_memory();
    let error = try_connect(
        pinning_client_config("db.example.com", store.clone())
            .with_trust_policy(TrustPolicy::Pinned),
        server_config(),
        Some(server_key()),
    )
    .await
    .err()
    .expect("an unknown key is refused under a strict policy");

    assert_eq!(error.code(), ErrorCode::SafetyPolicyDenied);
    assert_eq!(
        store.fingerprint("db.example.com"),
        None,
        "a refusal records nothing: trusting is a separate, deliberate act"
    );
}

#[tokio::test]
async fn should_refuse_a_transport_that_authenticates_nothing_unless_that_is_asked_for() {
    let store = TrustStore::in_memory();
    let error = try_connect(
        pinning_client_config("db.example.com", store),
        server_config(),
        None,
    )
    .await
    .err()
    .expect("a transport with no peer identity cannot satisfy a pinning policy");

    assert_eq!(
        error.code(),
        ErrorCode::SafetyPolicyDenied,
        "spec §21.5 requires authenticated encryption; a transport that authenticates nobody \
         cannot be trusted by default"
    );
}

#[test]
fn should_replace_a_pin_only_when_asked_to_deliberately() {
    let store = TrustStore::in_memory();
    store.pin("db.example.com", &server_key()).expect("pinned");

    let refused = store
        .pin("db.example.com", &impostor_key())
        .err()
        .expect("pinning over a different key is refused");
    assert_eq!(refused.code(), ErrorCode::RemoteHostKeyChanged);

    store
        .repin("db.example.com", &impostor_key())
        .expect("re-trusting is possible, but only by asking for it");
    assert_eq!(
        store.fingerprint("db.example.com"),
        Some(impostor_key().fingerprint())
    );
}

#[test]
fn should_write_a_trust_store_a_person_can_read_and_edit() {
    let scratch = scratch();
    let path = scratch.path().join("trust");

    let store = TrustStore::open(&path).expect("an absent store opens empty");
    store.pin("db.example.com", &server_key()).expect("pinned");
    store
        .pin("web.example.com", &impostor_key())
        .expect("pinned");

    let text = std::fs::read_to_string(&path).expect("the store is a file on disk");
    assert!(
        text.lines().any(|line| line.starts_with('#')),
        "the file explains itself, so a person editing it knows what it is: {text}"
    );
    assert!(
        text.contains(&format!(
            "db.example.com ed25519 {}",
            server_key().fingerprint()
        )),
        "one line per peer, host then algorithm then fingerprint: {text}"
    );

    let reopened = TrustStore::open(&path).expect("the store reads back");
    assert_eq!(
        reopened.fingerprint("db.example.com"),
        Some(server_key().fingerprint())
    );
    assert_eq!(
        reopened.fingerprint("web.example.com"),
        Some(impostor_key().fingerprint())
    );
}

#[test]
fn should_load_a_hand_written_trust_store() {
    let scratch = scratch();
    let path = scratch.write(
        "trust",
        format!(
            "# my hosts\n\n  \n# a comment\ndb.example.com ed25519 {}\n",
            server_key().fingerprint()
        ),
    );

    let store = TrustStore::open(&path).expect("a hand-written store loads");

    assert_eq!(
        store
            .verify("db.example.com", &server_key())
            .expect("the pinned key verifies"),
        TrustDecision::Pinned
    );
    assert_eq!(store.entries().len(), 1);
}

#[test]
fn should_refuse_a_trust_store_line_it_cannot_understand() {
    let scratch = scratch();
    let path = scratch.write("trust", "db.example.com ed25519\n");

    let error = TrustStore::open(&path)
        .err()
        .expect("a malformed store is refused rather than silently half-loaded");

    assert_eq!(error.code(), ErrorCode::ParseSyntax);
    assert!(
        error.message().contains('1') || error.help().is_some_and(|help| help.contains('1')),
        "the line number is what makes the file editable: {}",
        error.render_full()
    );
}

#[test]
fn should_derive_a_stable_fingerprint_from_the_key_material() {
    let key = HostKey::new("ed25519", *b"the-fixture-server-public-key---");

    let fingerprint = key.fingerprint();

    assert_eq!(fingerprint, server_key().fingerprint());
    assert_ne!(fingerprint, impostor_key().fingerprint());
    assert!(
        fingerprint.to_string().starts_with("sha256:"),
        "the fingerprint names its own hash, so it can never be confused with another kind"
    );
    assert_eq!(
        fingerprint.to_string().len(),
        "sha256:".len() + 64,
        "a full SHA-256, not a truncation a collision could be searched for"
    );
    assert_eq!(
        fingerprint.to_string().parse::<ono_protocol::Fingerprint>(),
        Ok(fingerprint),
        "what is written into the store reads back as the same fingerprint"
    );
}
