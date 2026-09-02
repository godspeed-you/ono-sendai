//! What a listening agent writes down about who reached it (v0.4.1 §14.1, §14.2).
//!
//! §14.1 lists the events, §14.2 lists the fields, and the last sentence of §14.2 is the one that
//! decides the shape of the record: events "MUST NOT include private keys, full secret environment
//! values or unredacted credentials from provider payloads". So the last case here feeds the agent
//! a provider payload with a credential in it and greps the whole audit stream for the secret.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::{Arc, Mutex};

use ono_core::ErrorCode;
use ono_protocol::{
    ActRequest, ActionGrant, AuditEvent, AuditKind, AuditSink, AuthorizedClient, AuthorizedClients,
    ClientConfig, HostKey, Identity, RemoteMessage, RemoteQuery, ServerAuthorization, TrustPolicy,
    UnauthenticatedTransport,
};
use ono_remote::{AgentConfig, RemoteLink, serve_registry};
use ono_value::SchemaId;

mod common;
use common::fixture::{FixtureObserved, fixture_registry, fixture_schemas};
use common::within;

/// The audit stream, kept in memory so a test can read it back.
#[derive(Debug, Default)]
struct Recorded {
    lines: Mutex<Vec<String>>,
    events: Mutex<Vec<(AuditKind, String, Option<String>, Option<ErrorCode>)>>,
}

impl Recorded {
    fn lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .expect("the recorder is not poisoned")
            .clone()
    }

    fn kinds(&self) -> Vec<AuditKind> {
        self.events
            .lock()
            .expect("the recorder is not poisoned")
            .iter()
            .map(|(kind, _, _, _)| *kind)
            .collect()
    }

    fn of(&self, kind: AuditKind) -> Vec<(String, Option<String>, Option<ErrorCode>)> {
        self.events
            .lock()
            .expect("the recorder is not poisoned")
            .iter()
            .filter(|(recorded, _, _, _)| *recorded == kind)
            .map(|(_, id, capability, code)| (id.clone(), capability.clone(), *code))
            .collect()
    }
}

impl AuditSink for Recorded {
    fn record(&self, event: &AuditEvent) {
        self.lines
            .lock()
            .expect("the recorder is not poisoned")
            .push(event.render());
        self.events
            .lock()
            .expect("the recorder is not poisoned")
            .push((
                event.kind(),
                event.connection_id().to_owned(),
                event.requested_capability().map(ToOwned::to_owned),
                event.error_code(),
            ));
    }
}

fn client_key() -> HostKey {
    HostKey::new("ed25519", *b"the-audit-suite-client-pub-key--")
}

fn stranger_key() -> HostKey {
    HostKey::new("ed25519", *b"a-stranger-nobody-authorized----")
}

fn client_config() -> ClientConfig {
    ClientConfig::new("remhost")
        .with_schemas(fixture_schemas())
        .with_trust_policy(TrustPolicy::Unauthenticated)
        .with_identity(Identity::new("tester"))
}

/// A listening agent whose policy is `store`, serving one connection from a peer proving `proves`.
struct Watched {
    link: Option<RemoteLink>,
    audit: Arc<Recorded>,
    refusal: Option<ono_value::ErrorValue>,
}

async fn connect_watched(store: AuthorizedClients, proves: Option<HostKey>) -> Watched {
    let (near, far) = tokio::io::duplex(16 * 1024);
    let audit = Arc::new(Recorded::default());
    let registry = fixture_registry(Arc::new(FixtureObserved::default()));
    let config = AgentConfig::new(registry)
        .with_identity(Identity::new("remote-user"))
        .with_authorization(ServerAuthorization::Store(Arc::new(store)))
        .with_action_capability("process", "stop", "process.signal")
        .with_audit(Arc::clone(&audit) as ono_protocol::Audit)
        .with_source_address("127.0.0.1:54321");
    let mut listening = UnauthenticatedTransport::new(far);
    if let Some(key) = proves {
        listening = listening.with_peer_key(key);
    }
    tokio::spawn(async move { serve_registry(listening, config).await });
    let outcome = within(RemoteLink::connect(
        UnauthenticatedTransport::new(near),
        client_config(),
    ))
    .await;
    match outcome {
        Ok(link) => Watched {
            link: Some(link),
            audit,
            refusal: None,
        },
        Err(refusal) => Watched {
            link: None,
            audit,
            refusal: Some(refusal),
        },
    }
}

/// The identity of one process on the fixture's far side.
fn process_object(pid: i128) -> ono_provider_api::ObjectId {
    ono_provider_api::ObjectId::new(
        SchemaId::new("ono.process", 1),
        [ono_value::Value::Int(pid)],
    )
}

fn observer() -> AuthorizedClients {
    AuthorizedClients::of([
        AuthorizedClient::observing(client_key().fingerprint()).with_label("watcher")
    ])
}

fn actor() -> AuthorizedClients {
    AuthorizedClients::of([AuthorizedClient::observing(client_key().fingerprint())
        .with_label("deployer")
        .with_actions(["process.signal".parse::<ActionGrant>().unwrap()])])
}

#[tokio::test]
async fn should_emit_a_structured_event_for_every_connection_lifecycle_step() {
    // Accepted, an action requested, an authorization denial, and the disconnect: four of
    // §14.1's eight on one connection, driven end to end rather than constructed by hand.
    let watched = connect_watched(actor(), Some(client_key())).await;
    let link = watched.link.expect("the authorized client is served");

    let _ = link
        .act(
            &ActRequest::new("process", "stop", process_object(4419))
                .with_argument("signal", ono_value::Value::string("TERM")),
        )
        .await;
    drop(link);
    common::settle().await;

    let kinds = watched.audit.kinds();
    assert!(
        kinds.contains(&AuditKind::ConnectionAccepted),
        "§14.1: a successful authenticated connection is recorded, got {kinds:?}"
    );
    assert!(
        kinds.contains(&AuditKind::ActionRequested),
        "§14.1: an action execution request for an authorized action is recorded, got {kinds:?}"
    );
    assert!(
        kinds.contains(&AuditKind::ClientDisconnected),
        "§14.1: a client disconnect is recorded, got {kinds:?}"
    );

    // §14.2's field set, on every line, with `-` where a field does not apply — a shape a script
    // can cut without knowing which event it is reading.
    for line in watched.audit.lines() {
        for field in [
            "event=",
            "connection_id=",
            "peer_fingerprint=",
            "peer_label=",
            "source_address=",
            "protocol_version=",
            "requested_capability=",
            "result=",
            "error_code=",
            "timestamp=",
        ] {
            assert!(line.contains(field), "§14.2 requires `{field}`, got {line}");
        }
        assert!(
            line.contains("source_address=127.0.0.1:54321"),
            "the source address travels from the listener into every event, got {line}"
        );
    }
}

#[tokio::test]
async fn should_record_the_refusal_of_a_client_nobody_authorized() {
    let watched = connect_watched(observer(), Some(stranger_key())).await;
    assert_eq!(
        watched
            .refusal
            .as_ref()
            .expect("the stranger is refused")
            .code(),
        ErrorCode::RemoteUnauthorized
    );

    let refused = watched.audit.of(AuditKind::UnknownClientRefused);
    assert_eq!(
        refused.len(),
        1,
        "§14.1: an unknown/unapproved client refusal is one event, got {refused:?}"
    );
    assert_eq!(refused[0].2, Some(ErrorCode::RemoteUnauthorized));
    assert!(
        watched
            .audit
            .lines()
            .iter()
            .any(|line| line.contains(&stranger_key().fingerprint().to_string())),
        "§53.3: the fingerprint is public identity material, and it is what an operator adds"
    );
}

#[tokio::test]
async fn should_record_a_client_that_proved_no_key_as_a_verification_failure() {
    let watched = connect_watched(observer(), None).await;
    assert_eq!(
        watched
            .refusal
            .as_ref()
            .expect("a peer that proved nothing is refused")
            .code(),
        ErrorCode::RemoteUnauthenticated
    );
    assert_eq!(
        watched.audit.of(AuditKind::ClientVerificationFailed).len(),
        1,
        "§14.1: a client-certificate verification failure is its own event class"
    );
}

#[tokio::test]
async fn should_carry_the_fingerprint_and_the_decision_on_every_authorization_event() {
    let watched = connect_watched(observer(), Some(client_key())).await;
    let link = watched.link.expect("the observer is served");

    let refused = link
        .act(&ActRequest::new("process", "stop", process_object(4419)))
        .await
        .expect_err("an observer may not act");
    assert_eq!(refused.code(), ErrorCode::RemoteCapabilityDenied);
    common::settle().await;

    let denials = watched.audit.of(AuditKind::AuthorizationDenied);
    assert_eq!(denials.len(), 1, "one denial, one event, got {denials:?}");
    assert_eq!(
        denials[0].1.as_deref(),
        Some("process.signal"),
        "§14.2 names `requested_capability` among the fields, got {denials:?}"
    );
    assert_eq!(denials[0].2, Some(ErrorCode::RemoteCapabilityDenied));

    let expected = client_key().fingerprint().to_string();
    for line in watched.audit.lines() {
        assert!(
            line.contains(&expected) || line.contains("peer_fingerprint=-"),
            "an event about a connection carries the key that connection proved, got {line}"
        );
        assert!(
            line.contains("peer_label=watcher") || line.contains("peer_label=-"),
            "§14.2: the peer label travels where one is known, got {line}"
        );
    }
}

#[tokio::test]
async fn should_never_write_key_material_or_payload_bytes_into_an_audit_event() {
    // Two kinds of secret meet the agent on one connection: a value the *caller* sent, in an
    // action argument, and the values the *provider* produced, in the records it answered with.
    // §14.2 forbids both from reaching an audit event, and the record type has no field either
    // could occupy — which is what makes the grep below worth running.
    let watched = connect_watched(actor(), Some(client_key())).await;
    let link = watched.link.expect("the authorized client is served");

    let mut processes = link
        .protocol()
        .query(&RemoteQuery::target("process"))
        .expect("the client may read");
    let mut seen = 0;
    while let Some(message) = within(processes.recv()).await {
        if matches!(message, RemoteMessage::Value(_)) {
            seen += 1;
        }
    }
    assert!(
        seen > 0,
        "the fixture answered nothing, so nothing was audited about it"
    );

    let _ = link
        .act(
            &ActRequest::new("process", "stop", process_object(4419))
                .with_argument("signal", ono_value::Value::string("hunter2")),
        )
        .await;
    drop(link);
    common::settle().await;

    let stream = watched.audit.lines().join("\n");
    for secret in [
        // The caller's argument, the provider's payload, and what key material would look like.
        "hunter2",
        "nginx",
        "portd",
        "BEGIN PRIVATE KEY",
        "the-audit-suite-client-pub-key",
    ] {
        assert!(
            !stream.contains(secret),
            "§14.2: an audit event carries no payload, no argument and no key material, and \
             `{secret}` appeared in:\n{stream}"
        );
    }
    assert!(
        stream.contains("event=connection.accepted") && stream.contains("event=action.requested"),
        "the stream is not empty, so the grep above tested something: {stream}"
    );
}

#[tokio::test]
async fn should_name_every_event_class_the_specification_lists() {
    // §14.1's eight bullets are a closed set, and the names are what an operator greps for.
    let names: Vec<&str> = AuditKind::ALL.iter().map(|kind| kind.as_str()).collect();
    assert_eq!(
        names,
        [
            "connection.accepted",
            "connection.unknown_client_refused",
            "connection.client_verification_failed",
            "authorization.denied",
            "connection.limit_denied",
            "connection.protocol_mismatch",
            "connection.disconnected",
            "action.requested",
        ]
    );
}
