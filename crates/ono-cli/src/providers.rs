//! Which providers this shell can ask, and the runtime they run on.
//!
//! Both are built on first use rather than at startup. A shell that runs `echo hi` should not
//! have paid for a thread pool and a netlink socket to do it, and spec §34's cold-start budget is
//! measured on exactly that command.

use std::sync::Arc;

use ono_provider_api::ProviderRegistry;

/// Every provider this build knows about, in the order they are asked.
///
/// Registration order decides which provider answers a target that two claim, which is how a
/// KUANG/11 package will later extend a target without displacing what is already there
/// (spec §31.23).
pub fn registry(environment: impl IntoIterator<Item = (String, String)>) -> ProviderRegistry {
    let environment: Vec<(String, String)> = environment.into_iter().collect();
    let env = Arc::new(ono_provider_linux::EnvProvider::new(
        environment
            .iter()
            .map(|(name, value)| ono_provider_linux::EnvBinding::inherited(name, value)),
    ));
    registry_with_tables(environment, Arc::default(), env)
}

/// The same registry, with the shell's own tables (`ono.shell`) answering from `tables` — the
/// job and link tables the session publishes before each pipeline runs (spec §18.4, §21;
/// ADR-0090, ADR-0103), the host sources the environment points at, and `env` answering `get env`
/// from the bindings the session publishes to it.
pub fn registry_with_tables(
    environment: impl IntoIterator<Item = (String, String)>,
    tables: Arc<std::sync::Mutex<crate::session_provider::SessionTables>>,
    env: Arc<ono_provider_linux::EnvProvider>,
) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    let environment: Vec<(String, String)> = environment.into_iter().collect();
    // The host sources of spec §9.1 are where the environment says they are (ADR-0103).
    let sources = crate::hosts::HostSources::from_environment(
        environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
    );

    ono_provider_linux::register_with_env(&mut registry, env);
    // The container runtime is found the way `docker` and `podman` find it: through
    // DOCKER_HOST / CONTAINER_HOST, or the well-known sockets (ADR-0112).
    registry.register(Arc::new(
        ono_provider_container::ContainerProvider::from_environment(
            environment
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        ),
    ));

    registry.register(Arc::new(ono_provider_netlink::InterfaceProvider::new()));
    registry.register(Arc::new(ono_provider_netlink::RouteProvider::new()));
    registry.register(Arc::new(ono_provider_netlink::NeighborProvider::new()));
    registry.register(Arc::new(ono_provider_netlink::SocketProvider::new()));
    registry.register(Arc::new(ono_provider_systemd::JournalProvider::new()));

    registry.register(Arc::new(crate::session_provider::SessionProvider::new(
        tables, sources,
    )));
    registry.register(Arc::new(ono_provider_net::DnsProvider::new()));
    registry.register(Arc::new(ono_provider_net::PortProvider::new()));

    registry
}

/// Adds the providers that have to be reached asynchronously.
///
/// systemd is behind D-Bus, so building its provider is an `await`. It is registered separately
/// rather than being made synchronous, because pretending an I/O-bound constructor is not one is
/// how a shell acquires a hang at startup.
pub async fn register_async(registry: &mut ProviderRegistry) {
    // Two connections to the same bus, each a handshake and a round trip to its manager, and
    // neither waiting on the other: opened side by side, they cost one of them (spec §34).
    // Registration order is what decides which provider answers a target (see `registry`), and
    // it is kept.
    let (systemd, logind) = tokio::join!(
        ono_provider_systemd::SystemdProvider::connect(),
        ono_provider_systemd::SessionProvider::connect(),
    );
    registry.register(Arc::new(systemd));
    registry.register(Arc::new(logind));
}
