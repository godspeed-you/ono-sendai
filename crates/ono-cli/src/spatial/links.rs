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

use std::collections::{BTreeMap, BTreeSet};
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
fn detached() -> &'static RwLock<BTreeSet<String>> {
    static DETACHED: RwLock<BTreeSet<String>> = RwLock::new(BTreeSet::new());
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
    let mut next = BTreeMap::new();
    for row in rows {
        let following = left
            .as_ref()
            .is_none_or(|left| !left.contains(row.name.as_str()));
        next.insert(
            row.name.clone(),
            LinkFacts {
                name: row.name.clone(),
                host: row.host.clone(),
                transport: row.transport.clone(),
                connected: row.state == "connected",
                following,
            },
        );
    }
    *links = next;
}

/// Records that this session has stopped following `name`'s space (v0.2 §9.1, §19.1, §35.2).
pub fn detach(name: &str) {
    if let Ok(mut left) = detached().write() {
        left.insert(name.to_owned());
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
