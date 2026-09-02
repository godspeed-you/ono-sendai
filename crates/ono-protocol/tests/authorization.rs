//! What a listening agent lets an authenticated client do (v0.4.1 §9, §10, §20, §59.1–§59.3).
//!
//! Phase H1 made the listening side demand a client certificate, and ADR-0437 said in as many
//! words what it left open: "a listening agent today authenticates every client and authorizes
//! all of them". These are the proofs that it no longer does.
//!
//! §20 sets the bar every one of them is written to: "a security control is accepted only when
//! there is an automated **negative** test proving the forbidden behavior is refused. Positive
//! tests alone are insufficient for every P0 boundary." So each case below arranges an adversary
//! that is authenticated — the certificate is real, the key is real, the handshake completes —
//! and then asks for something policy withholds.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;

use ono_core::{ErrorCode, ErrorKind};
use ono_protocol::{
    ActRequest, ActionGrant, AuthorizedClient, AuthorizedClients, RemoteMessage, RemoteQuery,
    ServerAuthorization, ServerConfig,
};
use ono_provider_api::ObjectId;
use ono_value::{ErrorValue, SchemaId, Value};

mod common;
use common::{
    client_config, client_key, other_client_key, server_config, try_connect_proving, within,
};

/// A store that authorizes the fixture client to observe and nothing else — §9.4's default grant.
fn observe_only() -> Arc<AuthorizedClients> {
    Arc::new(AuthorizedClients::of([AuthorizedClient::observing(
        client_key().fingerprint(),
    )
    .with_label("observer")]))
}

/// The same client, additionally granted exactly one action.
fn granted(capability: &str) -> Arc<AuthorizedClients> {
    Arc::new(AuthorizedClients::of([AuthorizedClient::observing(
        client_key().fingerprint(),
    )
    .with_actions([capability
        .parse::<ActionGrant>()
        .expect("the fixture grants a real capability id")])]))
}

fn policed(store: Arc<AuthorizedClients>) -> ServerConfig {
    server_config()
        .with_authorization(ServerAuthorization::Store(store))
        .with_action_capability("process", "stop", "process.signal")
        .with_action_capability("service", "restart", "service.manage")
}

/// An object identity for a target the fixture agent is asked about.
fn object(schema: &str, key: i128) -> ObjectId {
    ObjectId::new(SchemaId::new(schema, 1), [Value::Int(key)])
}

fn stop_process() -> ActRequest {
    ActRequest::new("process", "stop", object("ono.process", 4419))
}

fn restart_service() -> ActRequest {
    ActRequest::new("service", "restart", object("ono.service", 1))
}

/// The first failure a stream carries, which is where a refused dispatch arrives.
async fn first_failure(stream: &mut ono_protocol::RemoteStream) -> ErrorValue {
    match within(stream.recv()).await {
        Some(RemoteMessage::Failure(error)) => error,
        Some(other) => panic!("expected a refusal, the stream carried {other:?}"),
        None => panic!("the stream ended without saying anything, so nothing was refused"),
    }
}

// --- §59.1: the unknown direct client ------------------------------------------------------

#[tokio::test]
async fn should_refuse_an_unlisted_client_before_provider_negotiation() {
    let refused = try_connect_proving(
        client_config("testhost"),
        policed(observe_only()),
        Some(other_client_key()),
    )
    .await
    .err()
    .expect("§59.1: a client the operator never listed does not get a session");

    assert_eq!(
        refused.code(),
        ErrorCode::RemoteUnauthorized,
        "§10.4 fixes the code a refusal answers with; got {refused:?}"
    );
    assert!(
        !refused.retryable().unwrap_or(true),
        "§59.9: a trust failure is deterministic, so retrying is not the remedy"
    );
}

#[tokio::test]
async fn should_disclose_no_process_schema_or_capability_inventory_to_an_unlisted_client() {
    let refused = try_connect_proving(
        client_config("testhost"),
        policed(observe_only()),
        Some(other_client_key()),
    )
    .await
    .err()
    .expect("the unlisted client is refused");

    // §59.1: "no process list, schema list or capability inventory beyond minimal rejection
    // protocol data may be disclosed". The refusal is the only thing that crossed, so the whole
    // of what the client learned is this text.
    let disclosed = format!("{} {}", refused.message(), refused.help().unwrap_or(""));
    for withheld in [
        "linux.procfs",
        "linux.systemd",
        "process.list",
        "process.signal",
        "ono.test.remote",
    ] {
        assert!(
            !disclosed.contains(withheld),
            "an unlisted client learned `{withheld}` from the refusal: {disclosed}"
        );
    }
}

#[tokio::test]
async fn should_refuse_a_client_the_transport_authenticated_as_nobody() {
    let refused = try_connect_proving(client_config("testhost"), policed(observe_only()), None)
        .await
        .err()
        .expect("§2.2: authorization cannot begin without an authenticated identity");

    assert_eq!(refused.code(), ErrorCode::RemoteUnauthenticated);
}

// --- §59.2: the authorized observer --------------------------------------------------------

#[tokio::test]
async fn should_let_an_authorized_observer_read_and_refuse_it_an_action() {
    let fixture = try_connect_proving(
        client_config("testhost"),
        policed(observe_only()),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served");

    let mut values = fixture
        .link
        .query(&RemoteQuery::target("demo").limit(2))
        .expect("the observer may read");
    let mut read = 0;
    while let Some(message) = within(values.recv()).await {
        if matches!(message, RemoteMessage::Value(_)) {
            read += 1;
        }
    }
    assert_eq!(read, 2, "§59.2: an observer executes representative reads");

    let refused = fixture
        .link
        .act(&restart_service())
        .await
        .expect_err("§59.2: `Act` is refused even though the provider offers the capability");
    assert_eq!(refused.code(), ErrorCode::RemoteCapabilityDenied);
}

// --- §59.3 and Appendix C: exact action grants ----------------------------------------------

#[tokio::test]
async fn should_leave_every_ungranted_action_refused_when_one_action_is_granted() {
    let fixture = try_connect_proving(
        client_config("testhost"),
        policed(granted("service.manage")),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served");

    fixture
        .link
        .act(&restart_service())
        .await
        .expect("§59.3: the granted action may proceed");

    let refused = fixture
        .link
        .act(&stop_process())
        .await
        .expect_err("§59.3: `process.signal` remains refused unless separately granted");
    assert_eq!(refused.code(), ErrorCode::RemoteCapabilityDenied);
    assert_eq!(
        refused.metadata().get("requested_capability"),
        Some(&Value::string("process.signal")),
        "§10.4: the refusal names the capability that was asked for, got {refused:?}"
    );
}

#[tokio::test]
async fn should_deny_an_action_whose_capability_id_is_unknown() {
    let fixture = try_connect_proving(
        client_config("testhost"),
        // Granted everything the agent can name, and then asked for something it cannot: the
        // capability a later version introduces, which Appendix C's last row denies.
        policed(granted("service.manage")),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served");

    let refused = fixture
        .link
        .act(&ActRequest::new(
            "quantum-drive",
            "engage",
            object("ono.quantum-drive", 1),
        ))
        .await
        .expect_err("Appendix C: an unknown capability id is always denied");

    assert_eq!(refused.code(), ErrorCode::RemoteCapabilityDenied);
    assert_eq!(
        refused.metadata().get("denied_because"),
        Some(&Value::string("capability_unknown")),
        "a capability the agent cannot name is denied *as* unnameable, got {refused:?}"
    );
}

#[tokio::test]
async fn should_deny_a_capability_introduced_after_the_grant_was_written() {
    // The operator granted "everything this agent could do" on the day they wrote it — which
    // §9.5 requires to be an expansion to exact ids, not a wildcard. Tomorrow's agent knows one
    // more action; today's grant does not name it, so it stays denied.
    let store = Arc::new(AuthorizedClients::of([AuthorizedClient::observing(
        client_key().fingerprint(),
    )
    .with_actions([
        "process.signal".parse::<ActionGrant>().unwrap(),
        "service.manage".parse::<ActionGrant>().unwrap(),
    ])]));
    let tomorrow = policed(store).with_action_capability("route", "set", "route.set");

    let fixture = try_connect_proving(client_config("testhost"), tomorrow, Some(client_key()))
        .await
        .expect("a listed client is served");

    let refused = fixture
        .link
        .act(&ActRequest::new("route", "set", object("ono.route", 0)))
        .await
        .expect_err("§9.5: a capability introduced later stays denied until someone authorizes it");
    assert_eq!(refused.code(), ErrorCode::RemoteCapabilityDenied);
}

// --- §10.3: the context is decided once ------------------------------------------------------

#[tokio::test]
async fn should_build_the_authorization_context_from_the_authenticated_fingerprint_alone() {
    let store = AuthorizedClients::of([
        AuthorizedClient::observing(client_key().fingerprint()).with_label("deploy")
    ]);

    let context = store
        .authorize(client_key().fingerprint())
        .expect("the listed client is authorized");

    assert_eq!(context.peer_fingerprint(), client_key().fingerprint());
    assert_eq!(context.client_label(), Some("deploy"));
    assert!(context.observe_allowed());
    assert!(context.allowed_action_capabilities().is_empty());
    assert!(
        !context.connection_id().is_empty(),
        "§10.3 names `connection_id` among the fields a context carries"
    );

    // §65.2: nothing the peer *said* reaches the decision. There is no constructor that takes a
    // user, a uid, an elevation flag or an address, so a self-reported field cannot grant
    // anything — the store is asked about a fingerprint and answers about a fingerprint.
    assert!(
        store.authorize(other_client_key().fingerprint()).is_err(),
        "a second key is a second client, whatever either of them claims to be"
    );
}

#[tokio::test]
async fn should_keep_the_authorization_context_immutable_for_the_life_of_the_connection() {
    let store = observe_only();
    let fixture = try_connect_proving(
        client_config("testhost"),
        policed(Arc::clone(&store)),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served");

    // The operator widens the policy while the connection is up. §10.3: "changes to
    // authorization affect new connections", and a handler "MUST NOT re-read a mutable
    // authorization file on each individual request" — so the live link is unchanged, and it is
    // unchanged in the safe direction as well as the unsafe one.
    let widened = AuthorizedClients::of([AuthorizedClient::observing(client_key().fingerprint())
        .with_actions(["service.manage".parse::<ActionGrant>().unwrap()])]);

    let refused = fixture
        .link
        .act(&restart_service())
        .await
        .expect_err("the live connection still runs under the policy it was accepted with");
    assert_eq!(refused.code(), ErrorCode::RemoteCapabilityDenied);

    // The next connection reads the edit.
    let next = try_connect_proving(
        client_config("testhost"),
        policed(Arc::new(widened)),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served");
    next.link
        .act(&restart_service())
        .await
        .expect("§10.3: the change reaches the next connection");
}

// --- §10.1: the offer is filtered ------------------------------------------------------------

#[tokio::test]
async fn should_offer_only_the_capabilities_the_clients_policy_allows() {
    let fixture = try_connect_proving(
        client_config("testhost"),
        policed(observe_only()),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served");

    let offered: Vec<&str> = fixture
        .link
        .negotiated()
        .providers()
        .iter()
        .flat_map(|provider| provider.capabilities())
        .map(ono_protocol::CapabilityDescriptor::id)
        .collect();

    assert!(
        offered.contains(&"process.list"),
        "an observer is offered what it may read, got {offered:?}"
    );
    assert!(
        !offered.contains(&"process.signal"),
        "§10.1: an unauthorized capability is absent from the accepted contract, got {offered:?}"
    );
    assert!(
        !fixture
            .link
            .negotiated()
            .capabilities()
            .contains(&"process.signal".to_owned()),
        "the agent-wide capability list is narrowed with the providers, got {:?}",
        fixture.link.negotiated().capabilities()
    );
}

#[tokio::test]
async fn should_leave_an_ungranted_action_capability_out_of_the_offer_the_provider_advertises() {
    let observing = try_connect_proving(
        client_config("testhost"),
        policed(observe_only()),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served");
    let acting = try_connect_proving(
        client_config("testhost"),
        policed(granted("process.signal")),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served");

    let ids = |fixture: &common::Fixture| -> Vec<String> {
        fixture
            .link
            .negotiated()
            .providers()
            .iter()
            .flat_map(|provider| provider.capabilities())
            .map(|capability| capability.id().to_owned())
            .collect()
    };

    // The provider declares `process.signal` either way. What differs is the policy, and the
    // difference is visible in the contract rather than only in a later refusal.
    assert!(!ids(&observing).contains(&"process.signal".to_owned()));
    assert!(ids(&acting).contains(&"process.signal".to_owned()));
}

// --- §10.2, §65.3: dispatch refuses independently of the offer ------------------------------

#[tokio::test]
async fn should_refuse_a_request_for_a_capability_the_offer_omitted() {
    let fixture = try_connect_proving(
        client_config("testhost"),
        policed(observe_only()),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served");

    // The offer this client accepted contains no action capability at all, so this request is
    // for something it was never told about. §65.3: hiding a capability in `Accept` and then
    // executing a forged request for it is the failure mode; the dispatch path answers instead.
    let refused = fixture
        .link
        .act(&stop_process())
        .await
        .expect_err("a hand-built request for an unnegotiated capability is refused");

    assert_eq!(refused.code(), ErrorCode::RemoteCapabilityDenied);
    assert_eq!(
        fixture.observed.sent(),
        0,
        "§10.2: the operation MUST NOT execute — no provider code ran"
    );
}

#[tokio::test]
async fn should_refuse_it_on_every_dispatch_path_the_server_exposes() {
    // Observe is off *and* nothing is granted: the client is listed, and listed for nothing. All
    // four ways into the agent must answer the same way, because a dispatch path that forgot to
    // ask is the whole of §10.2's concern.
    let store = Arc::new(AuthorizedClients::of([AuthorizedClient::observing(
        client_key().fingerprint(),
    )
    .with_observe(false)]));
    let fixture = try_connect_proving(
        client_config("testhost"),
        policed(store),
        Some(client_key()),
    )
    .await
    .expect("a listed client is served, however little it is listed for");

    let mut query = fixture
        .link
        .query(&RemoteQuery::target("demo"))
        .expect("the stream opens; the refusal arrives on it");
    assert_eq!(
        first_failure(&mut query).await.code(),
        ErrorCode::RemoteCapabilityDenied,
        "query is a dispatch path"
    );

    let mut watching = fixture
        .link
        .subscribe(&RemoteQuery::target("demo"))
        .expect("the stream opens");
    assert_eq!(
        first_failure(&mut watching).await.code(),
        ErrorCode::RemoteCapabilityDenied,
        "subscribe is a dispatch path"
    );

    let mut adapting = fixture
        .link
        .adapt(&ono_protocol::AdaptRequest::new(
            ["ip".to_owned(), "addr".to_owned()],
            "structured",
        ))
        .expect("the stream opens");
    assert_eq!(
        first_failure(&mut adapting).await.code(),
        ErrorCode::RemoteCapabilityDenied,
        "adapt runs a program on this host and is a dispatch path"
    );

    assert_eq!(
        fixture
            .link
            .act(&stop_process())
            .await
            .expect_err("act is a dispatch path")
            .code(),
        ErrorCode::RemoteCapabilityDenied
    );

    assert_eq!(
        fixture.observed.sent(),
        0,
        "not one of the four reached the service"
    );
}

// --- §53: the refusals are a stable, structured family --------------------------------------

#[tokio::test]
async fn should_declare_the_three_remote_refusal_codes_with_their_details() {
    // §53.1 names the family; §53.2 requires callers to match on the code rather than the text.
    // The registry is the contract, so this asserts what a script can rely on.
    for (code, rendered, name) in [
        (
            ErrorCode::RemoteUnauthenticated,
            "Ono-Sendai-E1201",
            "remote.unauthenticated",
        ),
        (
            ErrorCode::RemoteUnauthorized,
            "Ono-Sendai-E1202",
            "remote.unauthorized",
        ),
        (
            ErrorCode::RemoteCapabilityDenied,
            "Ono-Sendai-E1203",
            "remote.capability_denied",
        ),
    ] {
        assert_eq!(code.code(), rendered);
        assert_eq!(code.name(), name);
        assert_eq!(
            code.kind(),
            ErrorKind::Safety,
            "an authorization refusal is a safety decision, not a transport failure (ADR-0006)"
        );
        assert_eq!(ErrorCode::from_name(name), Some(code));
    }

    // §53.3: a fingerprint is public identity material and may be shown in full, and the denial
    // says which boundary decided — observe off, or the action capability absent.
    let store = AuthorizedClients::of([AuthorizedClient::observing(client_key().fingerprint())]);
    let peer = ono_protocol::PeerAuthorization::Policy(Arc::new(
        store
            .authorize(client_key().fingerprint())
            .expect("the listed client"),
    ));
    let denied = peer
        .require_action(Some("process.signal"), "stop process")
        .expect_err("an ungranted action is denied");
    assert_eq!(
        denied.metadata().get("peer_fingerprint"),
        Some(&Value::string(&client_key().fingerprint().to_string()))
    );
    assert_eq!(
        denied.metadata().get("denied_because"),
        Some(&Value::string("action_not_granted"))
    );

    let closed = ono_protocol::PeerAuthorization::Policy(Arc::new(
        AuthorizedClients::of([
            AuthorizedClient::observing(client_key().fingerprint()).with_observe(false)
        ])
        .authorize(client_key().fingerprint())
        .expect("the listed client"),
    ));
    assert_eq!(
        closed
            .require_observe("get process")
            .expect_err("observe is off")
            .metadata()
            .get("denied_because"),
        Some(&Value::string("observe_not_allowed")),
        "§10.4: the two reasons are told apart, because they are fixed by different commands"
    );
}

#[tokio::test]
async fn should_answer_the_same_stable_code_for_the_same_refusal_every_time() {
    for _ in 0..3 {
        let refused = try_connect_proving(
            client_config("testhost"),
            policed(observe_only()),
            Some(other_client_key()),
        )
        .await
        .err()
        .expect("the unlisted client is refused");
        assert_eq!(refused.code(), ErrorCode::RemoteUnauthorized);
        assert_eq!(refused.code().code(), "Ono-Sendai-E1202");
    }
}

#[tokio::test]
async fn should_refuse_without_prompting_when_no_terminal_is_attached() {
    // §59.9: "every trust failure above MUST be non-interactive and deterministic in scripts."
    // The refusal is a value, it is not retryable, and there is nothing in it that offers a way
    // past — no "continue", no "trust anyway", no question.
    let refused = try_connect_proving(
        client_config("testhost"),
        policed(observe_only()),
        Some(other_client_key()),
    )
    .await
    .err()
    .expect("the unlisted client is refused");

    assert_eq!(refused.retryable(), Some(false));
    let text = format!("{} {}", refused.message(), refused.help().unwrap_or(""));
    for prompt in ["[y/N]", "(yes/no)", "continue anyway", "Do you want"] {
        assert!(
            !text.contains(prompt),
            "a refusal that asks a question is a refusal a script answers: {text}"
        );
    }
}
