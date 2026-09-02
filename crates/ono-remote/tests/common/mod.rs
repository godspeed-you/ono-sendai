//! Helpers shared by the remote-machinery suites.
//!
//! Every suite runs offline: the transport is an in-memory duplex or a local child process, so
//! no test needs a network, real ssh, a container or a clock (AGENTS.md §11).

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a shared test fixture states preconditions the same way a #[test] body does, and \
              each test binary uses a different subset of the helpers"
)]

pub mod fixture;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use ono_protocol::{ClientConfig, Identity, TrustPolicy, UnauthenticatedTransport};
use ono_remote::{AgentConfig, PeerIdentity, RemoteLink, TlsListener, serve_registry};
use ono_value::ErrorValue;

use fixture::{FixtureObserved, fixture_registry, fixture_schemas};

/// A test that waits must fail rather than stall the suite, so every await is bounded.
pub const LIMIT: Duration = Duration::from_secs(20);

/// Runs `future` under a hard timeout.
pub async fn within<F: Future>(future: F) -> F::Output {
    match tokio::time::timeout(LIMIT, future).await {
        Ok(output) => output,
        Err(_) => panic!("the link did not finish within {LIMIT:?}: it hung"),
    }
}

/// Lets every task that can make progress make it, without touching the clock.
pub async fn settle() {
    for _ in 0..5_000 {
        tokio::task::yield_now().await;
    }
}

/// The client configuration the suites open links with.
///
/// The transport is an in-memory duplex protected by nothing, so the policy must say so by
/// name; the trust suites use their own configurations.
pub fn client_config() -> ClientConfig {
    ClientConfig::new("remhost")
        .with_schemas(fixture_schemas())
        .with_trust_policy(TrustPolicy::Unauthenticated)
        .with_identity(Identity::new("tester"))
}

/// A connected client, what the fixture observed, and the agent task serving it.
pub struct Connected {
    /// The client end.
    pub link: RemoteLink,
    /// What the fixture provider observed while it ran.
    pub observed: Arc<FixtureObserved>,
    /// The task running the agent end.
    pub agent: tokio::task::JoinHandle<Result<(), ErrorValue>>,
}

/// Connects a [`RemoteLink`] to a fixture agent over an in-memory duplex.
pub async fn connect() -> Connected {
    let (near, far) = tokio::io::duplex(16 * 1024);
    let observed = Arc::new(FixtureObserved::default());
    let registry = fixture_registry(Arc::clone(&observed));
    let config = AgentConfig::new(registry).with_identity(Identity::new("remote-user"));
    let agent =
        tokio::spawn(
            async move { serve_registry(UnauthenticatedTransport::new(far), config).await },
        );
    let link = within(RemoteLink::connect(
        UnauthenticatedTransport::new(near),
        client_config(),
    ))
    .await
    .expect("the fixture handshake succeeds");
    Connected {
        link,
        observed,
        agent,
    }
}

/// An agent listening on a loopback port the system chooses, serving the fixture registry.
pub async fn listening(identity: PeerIdentity) -> String {
    let listener = TlsListener::bind("127.0.0.1:0", &identity)
        .await
        .expect("a loopback listener binds");
    let address = listener
        .local_addr()
        .expect("the system reports the port it chose")
        .to_string();
    tokio::spawn(async move {
        while let Ok(transport) = listener.accept().await {
            let registry = fixture_registry(Arc::new(FixtureObserved::default()));
            let config = AgentConfig::new(registry).with_identity(Identity::new("remote-user"));
            tokio::spawn(async move {
                let _ = serve_registry(transport, config).await;
            });
        }
    });
    address
}
