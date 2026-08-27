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
    registry_with_tables(environment, Arc::default())
}

/// The same registry, with the shell's own tables (`ono.shell`) answering from `tables` — the
/// job table the session publishes before each pipeline runs (spec §18.4, ADR-0090).
pub fn registry_with_tables(
    environment: impl IntoIterator<Item = (String, String)>,
    tables: Arc<std::sync::Mutex<crate::session_provider::SessionTables>>,
) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();

    ono_provider_linux::register(
        &mut registry,
        environment
            .into_iter()
            .map(|(name, value)| ono_provider_linux::EnvBinding::inherited(name, value)),
    );

    registry.register(Arc::new(ono_provider_netlink::InterfaceProvider::new()));
    registry.register(Arc::new(ono_provider_netlink::RouteProvider::new()));
    registry.register(Arc::new(ono_provider_netlink::NeighborProvider::new()));
    registry.register(Arc::new(ono_provider_netlink::SocketProvider::new()));
    registry.register(Arc::new(ono_provider_systemd::JournalProvider::new()));

    registry.register(Arc::new(crate::session_provider::SessionProvider::new(
        tables,
    )));

    registry
}

/// Adds the providers that have to be reached asynchronously.
///
/// systemd is behind D-Bus, so building its provider is an `await`. It is registered separately
/// rather than being made synchronous, because pretending an I/O-bound constructor is not one is
/// how a shell acquires a hang at startup.
pub async fn register_async(registry: &mut ProviderRegistry) {
    registry.register(Arc::new(
        ono_provider_systemd::SystemdProvider::connect().await,
    ));
}
