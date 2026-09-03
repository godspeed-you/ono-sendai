//! What a listening agent will and will not hold (v0.4.1 spec §12, Appendix A; issues #51–#54,
//! #56, #57).
//!
//! §12 is one section with six parts, and they are one subject: an agent a person exposed on a
//! network is a service strangers can reach, so every dimension in which a stranger could make it
//! grow has a ceiling. §12.4 says where the ceilings live — *"Their defaults MUST be centralized
//! in one `Limits` contract"* — and §52.2 says why that matters:
//!
//! > A number such as `max_connections = 32` MUST not be independently typed into five files if
//! > one contract can generate the others.
//!
//! So this suite asks two kinds of question. The first is contractual: does the one contract carry
//! Appendix A's numbers, and can any constructor reach past them? The second is behavioural: with
//! the ceilings set low enough to reach in a test, does the agent refuse the connection over the
//! line, keep serving the ones under it, and give the slot back when one ends?
//!
//! Everything runs on the loopback interface with keys the test generates, so the suite needs no
//! network, no certificate authority and no fixture host.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_protocol::Limits;

/// The registry that owns every hardening number (`docs/spec/hardening/limits.yaml`).
fn limits_registry() -> serde_yaml_ng::Value {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/spec/hardening/limits.yaml"),
    )
    .expect("docs/spec/hardening/limits.yaml is the registry of v0.4.1 section 52.1");
    serde_yaml_ng::from_str(&text).expect("the registry is YAML")
}

/// The registry that says what enforces each connection ceiling
/// (`docs/spec/hardening/remote_limits.yaml`).
fn remote_registry() -> serde_yaml_ng::Value {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/spec/hardening/remote_limits.yaml"),
    )
    .expect("docs/spec/hardening/remote_limits.yaml is the `remote_limits` registry of §52.1");
    serde_yaml_ng::from_str(&text).expect("the registry is YAML")
}

/// One row of `limits.yaml`, by its key.
fn row(registry: &serde_yaml_ng::Value, key: &str) -> serde_yaml_ng::Value {
    registry["limits"]
        .as_sequence()
        .expect("the registry declares a `limits` sequence")
        .iter()
        .find(|entry| entry["key"].as_str() == Some(key))
        .unwrap_or_else(|| panic!("`{key}` is declared in docs/spec/hardening/limits.yaml"))
        .clone()
}

fn default_of(registry: &serde_yaml_ng::Value, key: &str) -> u64 {
    row(registry, key)["default"]
        .as_u64()
        .expect("a limit's default is a whole number in base units (Appendix A)")
}

fn max_of(registry: &serde_yaml_ng::Value, key: &str) -> u64 {
    row(registry, key)["max"]
        .as_u64()
        .expect("a limit declares the largest value it accepts (section 55.2)")
}

#[test]
fn should_read_every_connection_ceiling_from_the_one_limits_contract() {
    let registry = limits_registry();
    let limits = Limits::default();

    assert_eq!(
        u64::from(limits.max_connections()),
        default_of(&registry, "limits.remote_connections"),
        "v0.4.1 §12.1 and Appendix A fix the concurrent-connection ceiling, and §52.2 forbids a \
         second copy of it: the contract must answer what the registry declares"
    );
    assert_eq!(
        u64::from(limits.max_pending_handshakes()),
        default_of(&registry, "limits.remote_pending_handshakes"),
        "§12.2's pending-handshake ceiling comes from the registry"
    );
    assert_eq!(
        u64::from(limits.max_connections_per_client()),
        default_of(&registry, "limits.remote_connections_per_client"),
        "§12.3's per-fingerprint ceiling comes from the registry"
    );
    assert_eq!(
        u64::try_from(limits.handshake_timeout().as_millis()).unwrap(),
        default_of(&registry, "limits.remote_handshake_timeout_ms"),
        "§12.2's handshake timeout comes from the registry"
    );

    // §52.1 names `remote_limits` as a registry of its own. It carries the semantics of the
    // enforcement — what refuses, with which code, under which audit event — and deliberately no
    // numbers, because a number in two files is the defect §52.2 names.
    let remote = remote_registry();
    let ceilings = remote["ceilings"]
        .as_sequence()
        .expect("the remote registry declares a `ceilings` sequence")
        .clone();
    assert_eq!(
        ceilings.len(),
        4,
        "§12.1, §12.2 and §12.3 between them fix four connection ceilings"
    );
    for ceiling in &ceilings {
        let key = ceiling["limit_key"]
            .as_str()
            .expect("every ceiling names the `limits.*` key that holds its number");
        // Fails loudly if the key is not declared next door.
        let _ = row(&registry, key);
        assert!(
            ceiling.get("default").is_none() && ceiling.get("min").is_none(),
            "`{key}` must carry no number here: §52.2 gives a number one home and it is \
             limits.yaml"
        );
        assert_eq!(
            ceiling["enforced_by"].as_str(),
            Some("ono-remote"),
            "the registry says which component enforces the ceiling (§52.3)"
        );
    }
}

#[test]
fn should_offer_no_production_constructor_that_leaves_a_limit_unbounded() {
    // §12.4: "No code path may construct an effectively unlimited `Limits` instance for a network
    // listener in production." The guarantee that needs no reviewer is the one the type makes:
    // every setter clamps into the range the registry declares, so the unlimited value cannot be
    // written down (ADR-0453's pattern).
    let registry = limits_registry();

    let reached_for_the_sky = Limits::default()
        .with_max_connections(u32::MAX)
        .with_max_pending_handshakes(u32::MAX)
        .with_max_connections_per_client(u32::MAX)
        .with_handshake_timeout(Duration::MAX)
        .with_max_frame_payload(usize::MAX)
        .with_max_value_depth(usize::MAX)
        .with_max_streams(usize::MAX)
        .with_max_credit(u32::MAX);

    assert_eq!(
        u64::from(reached_for_the_sky.max_connections()),
        max_of(&registry, "limits.remote_connections")
    );
    assert_eq!(
        u64::from(reached_for_the_sky.max_pending_handshakes()),
        max_of(&registry, "limits.remote_pending_handshakes")
    );
    assert_eq!(
        u64::from(reached_for_the_sky.max_connections_per_client()),
        max_of(&registry, "limits.remote_connections_per_client")
    );
    assert_eq!(
        u64::try_from(reached_for_the_sky.handshake_timeout().as_millis()).unwrap(),
        max_of(&registry, "limits.remote_handshake_timeout_ms")
    );

    // And the wire bounds of §12.4's first sentence, which stay enforced: a frame ceiling of
    // four gibibytes is unlimited in every sense that matters to a listener.
    assert!(
        reached_for_the_sky.max_frame_payload() <= ono_protocol::MAX_FRAME_PAYLOAD,
        "a frame ceiling cannot be raised past the one the protocol declares"
    );
    assert!(reached_for_the_sky.max_value_depth() <= ono_protocol::MAX_VALUE_DEPTH);
    assert!(reached_for_the_sky.max_streams() <= ono_protocol::MAX_STREAMS);
    assert!(reached_for_the_sky.max_credit() <= ono_protocol::MAX_CREDIT);

    // A floor as well as a ceiling: a zero connection limit would turn the listener off silently,
    // and §2.3 wants a boundary that refuses rather than one that disappears (ADR-0456).
    let floored = Limits::default()
        .with_max_connections(0)
        .with_max_pending_handshakes(0)
        .with_max_connections_per_client(0)
        .with_handshake_timeout(Duration::ZERO);
    assert_eq!(floored.max_connections(), 1);
    assert_eq!(floored.max_pending_handshakes(), 1);
    assert_eq!(floored.max_connections_per_client(), 1);
    assert!(floored.handshake_timeout() >= Duration::from_millis(100));
}

// --- the listening agent, under its ceilings (§12.1, §12.2, §12.3, §12.5, §12.6) --------------

mod common;

use std::sync::Arc;

use common::fixture::{FixtureObserved, fixture_registry, fixture_schemas};
use common::within;
use ono_core::ErrorCode;
use ono_protocol::{
    AuthorizedClient, AuthorizedClients, ClientConfig, Identity, TrustPolicy, TrustStore,
};
use ono_remote::{
    AgentConfig, ConnectionRegistry, ListeningAgent, PeerIdentity, RemoteLink, TlsListener,
    tls_connect,
};
use tokio::io::AsyncReadExt as _;

/// The audit sink a test reads, so a refusal is proved to be recorded and not only made.
#[derive(Debug, Default)]
struct Recording {
    lines: std::sync::Mutex<Vec<String>>,
}

impl ono_protocol::AuditSink for Recording {
    fn record(&self, event: &ono_protocol::AuditEvent) {
        self.lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event.render());
    }
}

impl Recording {
    /// Every line recorded so far.
    fn lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// What a test drives: where the agent is, what it is holding, and what it may be told to serve.
struct Agent {
    address: String,
    connections: Arc<ConnectionRegistry>,
    /// What the agent wrote down about the decisions it made (§14.1).
    audit: Arc<Recording>,
    /// The store the agent re-reads: replacing it is how a test revokes a client (§12.5).
    store: Arc<std::sync::Mutex<AuthorizedClients>>,
    host: ono_protocol::HostKey,
    task: tokio::task::JoinHandle<ono_value::ErrorValue>,
}

impl Drop for Agent {
    /// Every listener a test starts is stopped by the test: a leaked accept loop outlives the
    /// suite that made it.
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// An agent on a loopback port the system chooses, serving the fixture registry under `limits`.
async fn listening(limits: ono_protocol::Limits, authorized: &[&PeerIdentity]) -> Agent {
    let identity = PeerIdentity::generate().expect("a host identity is generated");
    let host = identity.peer_key();
    let listener = TlsListener::bind("127.0.0.1:0", &identity)
        .await
        .expect("a loopback listener binds");
    let address = listener
        .local_addr()
        .expect("the system reports the port it chose")
        .to_string();
    let store = Arc::new(std::sync::Mutex::new(store_of(authorized)));
    let reading = Arc::clone(&store);
    let audit = Arc::new(Recording::default());
    let registry = fixture_registry(Arc::new(FixtureObserved::default()));
    let agent = ListeningAgent::new(
        listener,
        AgentConfig::new(registry).with_identity(Identity::new("remote-user")),
    )
    .with_limits(limits)
    .with_audit(Arc::clone(&audit) as ono_protocol::Audit)
    .with_authorization_source(move || {
        Ok(reading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    });
    let connections = agent.connections();
    let task = tokio::spawn(agent.run());
    Agent {
        address,
        connections,
        audit,
        store,
        host,
        task,
    }
}

/// The store that authorizes exactly these client keys to observe (§9.4's default grant).
fn store_of(authorized: &[&PeerIdentity]) -> AuthorizedClients {
    AuthorizedClients::of(
        authorized
            .iter()
            .map(|identity| AuthorizedClient::observing(identity.fingerprint())),
    )
}

/// Opens one authenticated link to `agent` as `client`.
async fn link(agent: &Agent, client: &PeerIdentity) -> Result<RemoteLink, ono_value::ErrorValue> {
    let transport = tls_connect(&agent.address, client).await?;
    let store = TrustStore::in_memory();
    store
        .pin("testbox", &agent.host)
        .expect("the key is pinned");
    RemoteLink::connect(
        transport,
        ClientConfig::new("testbox")
            .with_schemas(fixture_schemas())
            .with_trust_store(store)
            .with_trust_policy(TrustPolicy::Pinned)
            .with_identity(Identity::new("tester")),
    )
    .await
}

/// Whether `link` can still read objects from the far side.
async fn still_serves(link: &RemoteLink) -> bool {
    let mut registry = ono_provider_api::ProviderRegistry::new();
    link.register_into(&mut registry);
    let Ok(mut stream) = registry.snapshot(&ono_provider_api::Query::target("process")) else {
        return false;
    };
    let mut values = 0;
    while let Some(event) = within(stream.recv()).await {
        if let ono_pipeline::StreamEvent::Value(_) = event {
            values += 1;
        }
    }
    values > 0
}

/// Waits for a condition the agent reaches on its own, or fails the test.
///
/// A condition rather than a duration: ADR-0459 and ADR-0252 both say what a timing assertion is
/// worth on a loaded machine. What is asserted is *that* the agent lets the slot go, never how
/// many milliseconds it took.
async fn until(what: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..2_000 {
        if condition() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the agent never reached the state this test is about: {what}");
}

#[tokio::test(flavor = "multi_thread")]
async fn should_refuse_the_connection_past_the_global_ceiling_and_keep_serving_the_rest() {
    // Appendix A's own figure, unchanged: thirty-two. Two client keys share it, because the
    // per-client ceiling is a different rule with a test of its own and would otherwise be the
    // one that refused first.
    let ceiling = ono_protocol::MAX_CONNECTIONS as usize;
    let first = PeerIdentity::generate().expect("a client identity is generated");
    let second = PeerIdentity::generate().expect("a client identity is generated");
    let agent = listening(
        ono_protocol::Limits::default().with_max_connections_per_client(u32::MAX),
        &[&first, &second],
    )
    .await;

    let mut held = Vec::new();
    for index in 0..ceiling {
        let client = if index % 2 == 0 { &first } else { &second };
        held.push(
            within(link(&agent, client))
                .await
                .unwrap_or_else(|error| panic!("connection {index} is under the ceiling: {error}")),
        );
    }
    assert_eq!(agent.connections.live(), ceiling);

    let refused = within(link(&agent, &first))
        .await
        .expect_err("v0.4.1 §12.1: the thirty-third concurrent connection is refused");

    assert_eq!(
        refused.code(),
        ErrorCode::RemoteConnectionLimit,
        "§53.1 and §53.2: the refusal is a stable code a caller matches on, not a message, got \
         {refused:?}"
    );
    assert!(
        refused.message().contains(&ceiling.to_string()),
        "§54.1: a refusal says which boundary decided and what it was set to, got {refused:?}"
    );
    assert_eq!(
        agent.connections.live(),
        ceiling,
        "the refusal must not disturb the connections already established"
    );
    assert!(
        still_serves(&held[0]).await,
        "§12.1: the thirty-two established sessions keep serving while the thirty-third is \
         refused"
    );

    // §14.1's fifth bullet — "connection-limit denial" — declared by phase H2 and raised by
    // nothing until there was a ceiling to reach. The line carries the code the peer received,
    // so an operator reading the trail and a script reading the refusal see one decision.
    let denials: Vec<String> = agent
        .audit
        .lines()
        .into_iter()
        .filter(|line| line.contains("event=connection.limit_denied"))
        .collect();
    assert_eq!(
        denials.len(),
        1,
        "§14.1: the refusal is recorded once, as a connection-limit denial, got {:?}",
        agent.audit.lines()
    );
    assert!(
        denials[0].contains("error_code=remote.connection_limit"),
        "§14.2: the audit record carries the code the decision answered with, got {:?}",
        denials[0]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn should_release_a_slot_when_a_connection_closes() {
    let client = PeerIdentity::generate().expect("a client identity is generated");
    let agent = listening(
        ono_protocol::Limits::default()
            .with_max_connections(2)
            .with_max_connections_per_client(u32::MAX),
        &[&client],
    )
    .await;

    let first = within(link(&agent, &client)).await.expect("the first fits");
    let second = within(link(&agent, &client))
        .await
        .expect("the second fits");
    within(link(&agent, &client))
        .await
        .expect_err("the third is over the ceiling");

    drop(first);
    until("the closed connection's slot is given back", || {
        agent.connections.live() == 1
    })
    .await;

    let third = within(link(&agent, &client))
        .await
        .expect("a ceiling that never released would be a listener that dies of its own traffic");
    assert!(still_serves(&third).await);
    assert!(still_serves(&second).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn should_refuse_a_seventeenth_pending_handshake() {
    // Appendix A's figure, and the cheapest attack there is: sixteen peers that complete TCP and
    // say nothing. Nothing here spends a byte of cryptography, which is exactly why the ceiling
    // is applied before any is spent (§12.2).
    let pending = ono_protocol::MAX_PENDING_HANDSHAKES as usize;
    let agent = listening(ono_protocol::Limits::default(), &[]).await;

    let mut silent = Vec::new();
    for _ in 0..pending {
        silent.push(
            tokio::net::TcpStream::connect(&agent.address)
                .await
                .expect("the agent accepts the connection"),
        );
    }
    until("sixteen peers are negotiating", || {
        agent.connections.pending() == pending
    })
    .await;

    let mut seventeenth = tokio::net::TcpStream::connect(&agent.address)
        .await
        .expect("TCP is accepted by the kernel before Ono sees it");
    let mut nothing = [0u8; 1];
    let read = within(seventeenth.read(&mut nothing))
        .await
        .expect("the agent closes the connection rather than answering it");

    assert_eq!(
        read, 0,
        "§12.2: the seventeenth peer is dropped, because a peer that has not completed TLS has no \
         authenticated channel to be told anything over (§13.1)"
    );
    assert_eq!(
        agent.connections.pending(),
        pending,
        "the sixteen that were negotiating are still negotiating: the refusal is of the \
         seventeenth, not of everybody"
    );
    drop(silent);
}

#[tokio::test(flavor = "multi_thread")]
async fn should_drop_a_handshake_that_has_not_completed_within_the_timeout() {
    // The timeout and the ceiling are two protections, not one. With a ceiling of one and a peer
    // that never speaks, the ceiling alone would close this agent to everyone for ever; the
    // timeout is what gives the slot back.
    let client = PeerIdentity::generate().expect("a client identity is generated");
    let agent = listening(
        ono_protocol::Limits::default()
            .with_max_pending_handshakes(1)
            .with_handshake_timeout(std::time::Duration::from_millis(200)),
        &[&client],
    )
    .await;

    let mut silent = tokio::net::TcpStream::connect(&agent.address)
        .await
        .expect("the agent accepts the connection");
    until("the silent peer is counted as negotiating", || {
        agent.connections.pending() == 1
    })
    .await;

    let mut nothing = [0u8; 1];
    let read = within(silent.read(&mut nothing))
        .await
        .expect("a peer that says nothing is disconnected rather than held");
    assert_eq!(
        read, 0,
        "§12.2: a peer that does not complete TLS plus Ono negotiation within the timeout MUST be \
         disconnected"
    );
    until("the timed-out peer's slot is given back", || {
        agent.connections.pending() == 0
    })
    .await;

    // And the slot is really usable again, which is the half a dropped socket does not prove.
    let served = within(link(&agent, &client))
        .await
        .expect("a legitimate client is not starved by a peer that said nothing");
    assert!(still_serves(&served).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn should_refuse_a_fifth_connection_from_one_authenticated_fingerprint() {
    let per_client = ono_protocol::MAX_CONNECTIONS_PER_CLIENT as usize;
    let client = PeerIdentity::generate().expect("a client identity is generated");
    let agent = listening(ono_protocol::Limits::default(), &[&client]).await;

    let mut held = Vec::new();
    for index in 0..per_client {
        held.push(
            within(link(&agent, &client))
                .await
                .unwrap_or_else(|error| panic!("connection {index} is under the ceiling: {error}")),
        );
    }

    let refused = within(link(&agent, &client))
        .await
        .expect_err("v0.4.1 §12.3: one fingerprint gets four connections");

    assert_eq!(refused.code(), ErrorCode::RemoteConnectionLimit);
    assert!(
        refused.message().contains(&per_client.to_string()),
        "a refusal says the figure it enforced, got {refused:?}"
    );
    assert!(
        still_serves(&held[0]).await,
        "the four this key already holds are undisturbed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn should_key_the_per_client_ceiling_on_the_fingerprint_rather_than_the_address() {
    // §11.3: "Loopback, RFC1918/private address space, Unix user identity inferred from source
    // port, source IP allowlists, or 'same LAN' MUST NOT substitute for cryptographic client
    // authentication." Both clients here dial from 127.0.0.1, on different source ports the
    // kernel chose. If the ceiling were keyed on where a connection came from, the second client
    // would be refused for the first client's traffic — and one client with two addresses would
    // walk past it.
    let first = PeerIdentity::generate().expect("a client identity is generated");
    let second = PeerIdentity::generate().expect("a client identity is generated");
    let agent = listening(
        ono_protocol::Limits::default().with_max_connections_per_client(1),
        &[&first, &second],
    )
    .await;

    let held = within(link(&agent, &first))
        .await
        .expect("the first key's one connection");
    let refused = within(link(&agent, &first))
        .await
        .expect_err("the same key's second connection is over its own ceiling");
    assert_eq!(refused.code(), ErrorCode::RemoteConnectionLimit);

    let other = within(link(&agent, &second))
        .await
        .expect("a different key from the same address is a different client (§11.3, §12.3)");

    assert!(still_serves(&other).await);
    assert!(still_serves(&held).await);
    assert_eq!(agent.connections.live(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn should_keep_accepting_after_one_connection_fails() {
    // §12.6's three shapes of failure, all of them still in flight, and then the question that
    // matters: is the listener still a listener *while* they are?
    //
    // The stalling peer is what makes this a test rather than a formality. It holds a
    // pre-negotiation slot for the full ten-second handshake budget, so an agent that did its
    // TLS handshakes on the accept loop would serve nobody until it gave up on this one. The
    // assertion is not that the legitimate client was served quickly — it is that the stalling
    // peer was *still stalling* when the legitimate client was served.
    let client = PeerIdentity::generate().expect("a client identity is generated");
    let stranger = PeerIdentity::generate().expect("an unauthorized client identity");
    let agent = listening(ono_protocol::Limits::default(), &[&client]).await;

    // A peer that completes TCP and then stalls for its whole handshake budget.
    let staller = tokio::net::TcpStream::connect(&agent.address)
        .await
        .expect("the agent accepts the connection");
    until("the stalling peer is counted as negotiating", || {
        agent.connections.pending() == 1
    })
    .await;

    // A peer that speaks nonsense where a TLS ClientHello belongs.
    {
        let mut garbage = tokio::net::TcpStream::connect(&agent.address)
            .await
            .expect("the agent accepts the connection");
        tokio::io::AsyncWriteExt::write_all(&mut garbage, b"GET / HTTP/1.1\r\n\r\n")
            .await
            .expect("the bytes are sent");
        let mut nothing = [0u8; 1];
        let _ = within(garbage.read(&mut nothing)).await;
    }

    // A peer that authenticates perfectly and is authorized for nothing.
    let refused = within(link(&agent, &stranger))
        .await
        .expect_err("an unlisted client is refused (§9.1)");
    assert_eq!(refused.code(), ErrorCode::RemoteUnauthorized);

    let served = within(link(&agent, &client)).await.expect(
        "§12.6: one malformed, unauthorized or slow client MUST NOT terminate the listener",
    );
    assert!(
        agent.connections.pending() >= 1,
        "the stalling peer must still have been stalling: a listener that had to finish with it \
         first is a listener one silent peer can close (§12.6)"
    );
    assert!(
        still_serves(&served).await,
        "the legitimate peer is served while three others are failing, which is the whole of §12.6"
    );
    drop(staller);
    until("every failed connection is let go of", || {
        agent.connections.live() == 1
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn should_leave_every_other_session_intact_when_one_connection_is_aborted() {
    let client = PeerIdentity::generate().expect("a client identity is generated");
    let agent = listening(ono_protocol::Limits::default(), &[&client]).await;

    let doomed = within(link(&agent, &client)).await.expect("the first link");
    let survivor = within(link(&agent, &client))
        .await
        .expect("the second link");
    assert_eq!(agent.connections.live(), 2);

    // Gone without a goodbye: the transport is dropped mid-session, which is what a peer whose
    // process was killed looks like from here.
    drop(doomed);
    until("the aborted session is reaped", || {
        agent.connections.live() == 1
    })
    .await;

    assert!(
        still_serves(&survivor).await,
        "§12.6: the other sessions are intact, and the task count returns to baseline"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn should_terminate_an_established_session_when_its_authorization_is_revoked() {
    // §12.5: "Removing an authorized client MUST prevent all new connections immediately. The
    // reference implementation SHOULD also close existing direct-TCP connections for that
    // fingerprint within 5 seconds."
    let revoked = PeerIdentity::generate().expect("a client identity is generated");
    let kept = PeerIdentity::generate().expect("a client identity is generated");
    let agent = listening(ono_protocol::Limits::default(), &[&revoked, &kept]).await;

    let doomed = within(link(&agent, &revoked))
        .await
        .expect("the first link");
    let survivor = within(link(&agent, &kept)).await.expect("the second link");
    assert_eq!(agent.connections.live(), 2);

    *agent
        .store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = store_of(&[&kept]);

    until("the revoked client's live session is closed", || {
        agent.connections.live() == 1
    })
    .await;
    assert!(
        !still_serves(&doomed).await,
        "the session the operator revoked is over, not merely marked"
    );
    assert!(
        still_serves(&survivor).await,
        "revoking one client is not revoking the others"
    );
    within(link(&agent, &revoked))
        .await
        .expect_err("and the next connection from that key is refused (§12.5's MUST)");
}
