//! The links a listening agent holds of its own: `ono --agent --listen … --link <name>=<address>`
//! (v0.4 §19.3, ADR-1097).
//!
//! §19.3's federated map draws `prod/web01 ----- prod/db01`, a link between two hosts that are
//! both remote. For the far side to have links to tell about, something on it has to hold them;
//! an `ono --agent` reached over stdio serves one caller and holds nothing, so the listening
//! agent — the long-lived process an operator runs on a host — is what holds them, declared on
//! its command line. Each declared link is made over the authenticated transport, with this
//! agent's own identity and its own trust store (the next hop must be pinned here, and must
//! authorize this agent), and is made again when it goes down.
//!
//! What a peer that may observe this agent is told is exactly what this agent observed: each
//! link's state and since when, the key the next hop proved, the boot it runs, the handshake's
//! round trip, and a latency probed when the table is asked for — on demand, bounded and reused
//! by the same pass the shell's own link map uses (ADR-1095). Nothing is sent to the next hop
//! while nobody asks, except the remaking of a link that went down.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ono_value::ErrorValue;

/// The first wait before a declared link is made again; doubled after each failure.
const RETRY_FIRST: Duration = Duration::from_secs(1);

/// The longest wait between two attempts at a declared link.
const RETRY_CEILING: Duration = Duration::from_secs(30);

/// How long one attempt at a declared link may take, handshake included.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);

/// The most links one agent may be told to hold — the table's own bound (ADR-1097).
pub const MAX_HELD_LINKS: usize = ono_protocol::MAX_LINK_TABLE_ENTRIES;

/// One `--link <name>=<address>` as the command line wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// The agent's name for the link.
    pub name: String,
    /// `host:port` of the next hop's listening agent.
    pub address: String,
}

impl Declared {
    /// Reads `<name>=<host:port>`.
    ///
    /// # Errors
    ///
    /// A usage message when the shape is not that.
    pub fn parse(text: &str) -> Result<Self, String> {
        let Some((name, address)) = text.split_once('=') else {
            return Err(format!(
                "--link takes <name>=<host:port>, not `{text}` (ADR-1097)"
            ));
        };
        let port = address
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse::<u16>().ok());
        if name.is_empty()
            || port.is_none()
            || name.len() > ono_protocol::MAX_LINK_FIELD_BYTES
            || address.len() > ono_protocol::MAX_LINK_FIELD_BYTES
        {
            return Err(format!(
                "--link takes <name>=<host:port> with a port, not `{text}` (ADR-1097)"
            ));
        }
        Ok(Self {
            name: name.to_owned(),
            address: address.to_owned(),
        })
    }
}

/// What this agent knows about one held link right now.
#[derive(Debug)]
struct Held {
    declared: Declared,
    state: Mutex<HeldState>,
}

#[derive(Debug, Default)]
struct HeldState {
    link: Option<Arc<ono_remote::RemoteLink>>,
    since: Option<jiff::Timestamp>,
    boot_id: Option<String>,
}

/// The links this agent holds, and the table it tells a peer that may observe it.
#[derive(Debug, Default)]
pub struct HeldLinks {
    links: Vec<Arc<Held>>,
}

impl HeldLinks {
    /// No links: what an agent that serves one caller over stdio holds, and says it holds.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Holds every declared link, each made — and made again when it goes down — by a task of
    /// its own on the current runtime, with `identity` as this agent's proof and the trust store
    /// of `sources` deciding whom it believes.
    #[must_use]
    pub fn hold(
        declared: Vec<Declared>,
        identity: &Arc<ono_remote::PeerIdentity>,
        sources: &crate::hosts::HostSources,
    ) -> Self {
        let links: Vec<Arc<Held>> = declared
            .into_iter()
            .map(|declared| {
                Arc::new(Held {
                    declared,
                    state: Mutex::new(HeldState::default()),
                })
            })
            .collect();
        for held in &links {
            let held = Arc::clone(held);
            let identity = Arc::clone(identity);
            let sources = sources.clone();
            tokio::spawn(async move { keep(held, identity, sources).await });
        }
        Self { links }
    }
}

/// Makes `held`'s link, waits for it to go down, and makes it again, backing off while it fails.
async fn keep(
    held: Arc<Held>,
    identity: Arc<ono_remote::PeerIdentity>,
    sources: crate::hosts::HostSources,
) {
    let mut wait = RETRY_FIRST;
    loop {
        let attempt = tokio::time::timeout(
            ATTEMPT_TIMEOUT,
            connect(&held.declared, &identity, &sources),
        )
        .await;
        match attempt {
            Ok(Ok((link, boot_id))) => {
                let link = Arc::new(link);
                {
                    let mut state = lock(&held.state);
                    state.link = Some(Arc::clone(&link));
                    state.since = Some(jiff::Timestamp::now());
                    state.boot_id = boot_id;
                }
                wait = RETRY_FIRST;
                // On stderr, beside the audit trail: an operator reads which held link is up.
                ono_core::diagnostic!(
                    "{}: held link `{}` connected to {}",
                    ono_core::SHORT_NAME,
                    held.declared.name,
                    held.declared.address
                );
                link.protocol().closed().await;
                ono_core::diagnostic!(
                    "{}: held link `{}` to {} went down",
                    ono_core::SHORT_NAME,
                    held.declared.name,
                    held.declared.address
                );
                let mut state = lock(&held.state);
                state.link = None;
                state.since = Some(jiff::Timestamp::now());
            }
            Ok(Err(refusal)) => {
                ono_core::diagnostic!(
                    "{}: the held link `{}` to {} could not be made: {}",
                    ono_core::SHORT_NAME,
                    held.declared.name,
                    held.declared.address,
                    refusal.message()
                );
                mark_down(&held);
            }
            Err(_elapsed) => mark_down(&held),
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(RETRY_CEILING);
    }
}

fn mark_down(held: &Held) {
    let mut state = lock(&held.state);
    if state.link.is_some() || state.since.is_none() {
        state.since = Some(jiff::Timestamp::now());
    }
    state.link = None;
}

fn lock(state: &Mutex<HeldState>) -> std::sync::MutexGuard<'_, HeldState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One attempt at a declared link over the authenticated transport, pinned (spec §21.5).
async fn connect(
    declared: &Declared,
    identity: &ono_remote::PeerIdentity,
    sources: &crate::hosts::HostSources,
) -> Result<(ono_remote::RemoteLink, Option<String>), ErrorValue> {
    let store = crate::trust::open(sources)?;
    let (host, _) = ono_remote::split_address(&declared.address);
    let config = ono_protocol::ClientConfig::new(&host)
        .with_schemas(Arc::new(ono_value::builtin_schemas().clone()))
        .with_trust_store(store)
        .with_trust_policy(ono_protocol::TrustPolicy::Pinned)
        .with_identity(ono_protocol::Identity::new(
            std::env::var("USER").unwrap_or_else(|_| "ono".to_owned()),
        ));
    let transport = ono_remote::tls_connect(&declared.address, identity).await?;
    let link = ono_remote::RemoteLink::connect(transport, config).await?;
    let mut registry = ono_provider_api::ProviderRegistry::new();
    link.register_into(&mut registry);
    let boot_id = crate::context::far_boot(&registry).await;
    Ok((link, boot_id))
}

#[async_trait::async_trait]
impl ono_remote::LinkTableSource for HeldLinks {
    async fn table(&self) -> ono_protocol::LinkTable {
        let current: Vec<(Arc<Held>, Option<Arc<ono_remote::RemoteLink>>)> = self
            .links
            .iter()
            .map(|held| (Arc::clone(held), lock(&held.state).link.clone()))
            .collect();
        // The latency is measured now, by the pass the shell's link map uses: side by side,
        // bounded, reused (ADR-1095).
        let due: Vec<(String, ono_protocol::Prober)> = current
            .iter()
            .filter_map(|(held, link)| {
                let link = link.as_ref()?;
                Some((held.declared.name.clone(), link.protocol().prober()))
            })
            .collect();
        crate::spatial::links::measure_links(due).await;

        let entries = current
            .iter()
            .map(|(held, link)| entry(held, link.as_deref()))
            .collect();
        ono_protocol::LinkTable::of(entries)
    }
}

/// One held link as this agent observed it.
fn entry(held: &Held, link: Option<&ono_remote::RemoteLink>) -> ono_protocol::LinkEntry {
    let state = lock(&held.state);
    let alive = link.filter(|link| link.protocol().is_alive());
    let measured = alive.and_then(|link| {
        crate::spatial::links::measurement_of(&held.declared.name, link.protocol().prober().link())
    });
    let nanos = |taken: Duration| u64::try_from(taken.as_nanos()).ok();
    ono_protocol::LinkEntry {
        name: held.declared.name.clone(),
        host: held.declared.address.clone(),
        transport: "tcp".to_owned(),
        state: if alive.is_some() {
            "connected".to_owned()
        } else {
            "disconnected".to_owned()
        },
        fingerprint: alive.and_then(|link| {
            link.negotiated()
                .fingerprint()
                .map(|fingerprint| fingerprint.to_string())
        }),
        boot_id: alive.and(state.boot_id.clone()),
        round_trip_ns: alive.and_then(|link| nanos(link.protocol().handshake_round_trip())),
        latency_ns: measured
            .as_ref()
            .and_then(|measured| measured.latency)
            .and_then(nanos),
        measured_at: measured.as_ref().map(|measured| measured.at.to_string()),
        since: state.since.map(|since| since.to_string()),
    }
}
