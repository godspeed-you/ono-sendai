//! The link map of §19.1: which hosts this session can stand on, and how current each one is.
//!
//! §19.1 puts the links into the local root's view with a state beside each one — `connected`,
//! `disconnected`, "last seen 3h ago" — and §35.2 requires those states to stay distinct from
//! "empty". A link is not an object a provider found (it is the session's own bookkeeping, which
//! is why `ono.link/1` is served by the session provider), so the facts a spatial view needs
//! about it live here, beside the spatial state that is likewise per process (§29.2, §46).
//!
//! Two different questions are answered, and conflating them is what makes a link map lie:
//!
//! - **Is the link established?** `link host` negotiated it, `remove link` ended it. This is the
//!   `ono.link/1` state, published by the session on every stage.
//! - **Is this session still following it?** `detach link` leaves the attachment without tearing
//!   the link down (v0.2 §9.1), so the connection may well still be open while nothing keeps
//!   what is behind it current. §35.2's word for that is `stale`, and it is a different answer
//!   from `connected` and from `disconnected` alike (§53: "Unknown/denied data? Distinct from
//!   empty").

use std::collections::BTreeMap;
use std::sync::RwLock;

use crate::session_provider::LinkRow;

/// What a spatial view knows about one link (§19.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkFacts {
    /// The link's name — the word `jump` takes and the scope a remote place belongs to.
    pub name: String,
    /// The host the link points at.
    pub host: String,
    /// How the bytes travel.
    pub transport: String,
    /// Whether the handshake succeeded and the connection is held.
    pub connected: bool,
    /// Whether this session still follows the link's space (§19.1, §35.2).
    pub following: bool,
    /// How long the handshake's exchange took, where one was made — §19.1's `12ms`, with the
    /// meaning this build can give it (ADR-0985).
    pub round_trip: Option<std::time::Duration>,
    /// When this session last held the link connected and followed it — §19.1's "last seen".
    /// `None` for a link that never was.
    pub last_seen: Option<jiff::Timestamp>,
    /// The boot of the kernel the far side runs, as its `kernel` target answered.
    pub boot_id: Option<String>,
    /// What probes the link while it is open, where the far side is an agent (ADR-1095).
    pub prober: Option<Prober>,
}

/// A handle that probes one link without holding it open (ADR-1095).
#[cfg(feature = "remote")]
pub type Prober = ono_protocol::Prober;

/// A build without links has nothing to probe (ADR-0910).
#[cfg(not(feature = "remote"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prober {}

/// How long one probe may wait for its answer (ADR-1095). A pass over every link probes them side
/// by side, so this is also what a whole pass can cost.
pub const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// How long a measurement is reused rather than repeated (ADR-1095): drawing many maps, or a watch
/// beside a map, sends at most one probe per link in this window.
pub const PROBE_REUSE: std::time::Duration = std::time::Duration::from_secs(5);

/// What a probe of a link found (ADR-1095): `ono.link-place/1`'s `latency_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyStatus {
    /// The far side answered; `latency` is the round trip.
    Measured,
    /// The far side was asked and did not answer within [`PROBE_TIMEOUT`].
    TimedOut,
    /// The far side does not announce the probe: an older agent, or no agent at all.
    NotOffered,
    /// The link is not connected and followed, or the probe failed.
    Unavailable,
}

impl LatencyStatus {
    /// The word `ono.link-place/1` spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::TimedOut => "timed_out",
            Self::NotOffered => "not_offered",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One probe's result, and when it was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    /// Which link was probed — a link made again is another link (ADR-1095).
    pub link: u64,
    /// The round trip, where the far side answered. Never a previous answer carried forward.
    pub latency: Option<std::time::Duration>,
    /// When the probe was made.
    pub at: jiff::Timestamp,
    /// What it found.
    pub status: LatencyStatus,
    /// The structured failure, where the probe failed: the reason a status is what it is.
    pub failure: Option<ono_core::ErrorCode>,
    /// The monotonic moment it was made, which decides reuse.
    taken: std::time::Instant,
}

fn measured() -> &'static std::sync::Mutex<BTreeMap<String, Measurement>> {
    static MEASURED: std::sync::Mutex<BTreeMap<String, Measurement>> =
        std::sync::Mutex::new(BTreeMap::new());
    &MEASURED
}

/// The latest probe of `name`'s current link, where one was made (ADR-1095).
///
/// `None` when this link — this connection, not an earlier one under the same name — has never
/// been probed.
#[must_use]
pub fn measurement(name: &str) -> Option<Measurement> {
    let facts = facts(name)?;
    let link = facts.prober.as_ref().map(prober_link)?;
    measured()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(name)
        .filter(|measurement| measurement.link == link)
        .cloned()
}

#[cfg(feature = "remote")]
fn prober_link(prober: &Prober) -> u64 {
    prober.link()
}

#[cfg(not(feature = "remote"))]
fn prober_link(prober: &Prober) -> u64 {
    match *prober {}
}

/// Probes every connected, followed agent link whose last measurement is older than
/// [`PROBE_REUSE`], side by side, each within [`PROBE_TIMEOUT`] (ADR-1095).
///
/// Passes are serialized process-wide, so a second caller waits for the first and then finds its
/// measurements fresh: no probe storm however many views are drawn at once. Nothing is probed
/// that is not connected and followed — a detached link is not this session's to keep asking.
pub async fn measure() {
    #[cfg(feature = "remote")]
    {
        static PASS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _pass = PASS.lock().await;
        let due: Vec<(String, Prober)> = all()
            .into_iter()
            .filter(LinkFacts::reachable)
            .filter_map(|facts| {
                let prober = facts.prober.clone()?;
                let fresh = measured()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&facts.name)
                    .is_some_and(|known| {
                        known.link == prober.link() && known.taken.elapsed() < PROBE_REUSE
                    });
                (!fresh).then_some((facts.name, prober))
            })
            .collect();
        let probes = due.into_iter().map(|(name, prober)| async move {
            let at = jiff::Timestamp::now();
            let taken = std::time::Instant::now();
            let outcome = prober.probe(PROBE_TIMEOUT).await;
            let (latency, status, failure) = match outcome {
                Ok(latency) => (Some(latency), LatencyStatus::Measured, None),
                Err(error) => {
                    let status = if !prober.is_offered() {
                        LatencyStatus::NotOffered
                    } else if error.metadata().get("timeout_ms").is_some() {
                        LatencyStatus::TimedOut
                    } else {
                        LatencyStatus::Unavailable
                    };
                    (None, status, Some(error.code()))
                }
            };
            (
                name,
                Measurement {
                    link: prober.link(),
                    latency,
                    at,
                    status,
                    failure,
                    taken,
                },
            )
        });
        let results = futures::future::join_all(probes).await;
        let mut known = measured()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (name, measurement) in results {
            known.insert(name, measurement);
        }
    }
}

impl LinkFacts {
    /// The §19.1 state a link map shows, in §35.2's vocabulary.
    ///
    /// `connected` only when the link is both established and followed: a detached link may
    /// still hold its socket, but nothing is keeping the places behind it current, and saying
    /// `connected` would promise a freshness nobody delivers (§2.17, §25.3).
    #[must_use]
    pub fn state(&self) -> &'static str {
        match (self.connected, self.following) {
            (true, true) => "connected",
            (true, false) => "stale",
            (false, _) => "disconnected",
        }
    }

    /// Whether a spatial command may reach across the link right now (§35.4).
    #[must_use]
    pub fn reachable(&self) -> bool {
        self.connected && self.following
    }
}

fn links() -> &'static RwLock<BTreeMap<String, LinkFacts>> {
    static LINKS: RwLock<BTreeMap<String, LinkFacts>> = RwLock::new(BTreeMap::new());
    &LINKS
}

/// The links this session has detached from, by name.
///
/// Kept apart from the table above because the two are written at different times: the table is
/// republished from the session whenever a pipeline starts, and a `detach link` may well be the
/// first statement that ever mentions the link. The intent outlives the republish.
fn detached() -> &'static RwLock<BTreeMap<String, jiff::Timestamp>> {
    static DETACHED: RwLock<BTreeMap<String, jiff::Timestamp>> = RwLock::new(BTreeMap::new());
    &DETACHED
}

/// Replaces what is known about the established links with what the session published.
///
/// Whether a link is *followed* is not in the published row: it is this session's own answer to
/// `detach link`, kept here across republishes.
pub fn publish(rows: &[LinkRow]) {
    let Ok(mut links) = links().write() else {
        return;
    };
    let left = detached().read().ok();
    let now = jiff::Timestamp::now();
    let mut next = BTreeMap::new();
    for row in rows {
        let left_at = left
            .as_ref()
            .and_then(|left| left.get(row.name.as_str()).copied());
        let following = left_at.is_none();
        let connected = row.state == "connected";
        // A link held and followed was seen just now; one that is not keeps the last moment it
        // was, which is what §19.1's "last seen" reports — the moment the session stopped
        // following it, where it was connected until then.
        let last_seen = if connected && following {
            Some(now)
        } else {
            links
                .get(&row.name)
                .and_then(|known| known.last_seen)
                .or(left_at.filter(|_| connected))
        };
        next.insert(
            row.name.clone(),
            LinkFacts {
                name: row.name.clone(),
                host: row.host.clone(),
                transport: row.transport.clone(),
                connected,
                following,
                round_trip: row.round_trip,
                last_seen,
                boot_id: row.boot_id.clone(),
                #[cfg(feature = "remote")]
                prober: row.prober.clone(),
                #[cfg(not(feature = "remote"))]
                prober: None,
            },
        );
    }
    *links = next;
}

/// Records that this session has stopped following `name`'s space (v0.2 §9.1, §19.1, §35.2).
pub fn detach(name: &str) {
    if let Ok(mut left) = detached().write() {
        left.entry(name.to_owned())
            .or_insert_with(jiff::Timestamp::now);
    }
    if let Ok(mut links) = links().write()
        && let Some(facts) = links.get_mut(name)
    {
        facts.following = false;
    }
}

/// Records that this session follows `name`'s space again — `enter link`, or a `jump` across it.
pub fn attach(name: &str) {
    if let Ok(mut left) = detached().write() {
        left.remove(name);
    }
    if let Ok(mut links) = links().write()
        && let Some(facts) = links.get_mut(name)
    {
        facts.following = true;
    }
}

/// What is known about the named link, or `None` where this session holds no such link.
#[must_use]
pub fn facts(name: &str) -> Option<LinkFacts> {
    links()
        .read()
        .ok()
        .and_then(|links| links.get(name).cloned())
}

/// Every link this session holds, in name order (§19.1's link map).
#[must_use]
pub fn all() -> Vec<LinkFacts> {
    links()
        .read()
        .ok()
        .map(|links| links.values().cloned().collect())
        .unwrap_or_default()
}

/// Whether a spatial command standing on `name` may reach across it right now.
///
/// A link this session no longer holds at all is not reachable either, and the difference is
/// visible: [`facts`] answers `None` rather than a state (§2.17).
#[must_use]
pub fn reachable(name: &str) -> bool {
    facts(name).is_some_and(|facts| facts.reachable())
}
