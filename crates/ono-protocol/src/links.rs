//! A peer's link table, as one agent tells another (`ono.link.links/1`, v0.4 §19.3, ADR-1097).
//!
//! §19.3's federated map draws `prod/web01 ----- prod/db01`: a link between two hosts that are
//! both remote. The far side's links are the far side's information, so this is a request like
//! any read — authorized on the agent as `get link` is (§10.2), refused with
//! `remote.capability_denied` where the connection may not observe — and the answer is bounded
//! on both ends: the agent sends at most [`MAX_LINK_TABLE_ENTRIES`] entries and says when it left
//! some out, and the client refuses a table that is larger or whose fields are longer than
//! [`MAX_LINK_FIELD_BYTES`] as a protocol mismatch, before anything of it is believed.
//!
//! Every entry is what the *far side* observed about its own link — who it reaches, whether the
//! link is up, how far it is, which key the next hop proved — and is carried as that: a report,
//! never as something this side verified.

use serde::{Deserialize, Serialize};

use crate::{FrameKind, ProtocolError};

/// The most links one table may carry.
///
/// A host that holds more links than this is a host whose map no reader takes in at once (v0.4
/// §34.2's node budget is thirty); the agent says it left some out rather than growing the frame.
pub const MAX_LINK_TABLE_ENTRIES: usize = 64;

/// The longest any text field of an entry may be, in bytes.
pub const MAX_LINK_FIELD_BYTES: usize = 256;

/// How a far link's bytes travel. A closed set: a value the decoder does not know refuses the
/// table (review ARC-6) rather than flowing into a map as text nobody defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkTransport {
    /// Ono's own authenticated transport.
    Tcp,
    /// Carried over OpenSSH.
    Ssh,
    /// A local agent process.
    Local,
}

impl LinkTransport {
    /// The word `ono.link/1` spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Ssh => "ssh",
            Self::Local => "local",
        }
    }
}

/// Whether a far link is up, as the far side last saw it. A closed set, like [`LinkTransport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    /// The far side holds the link established.
    Connected,
    /// The far side holds the definition, not a connection.
    Disconnected,
}

impl LinkState {
    /// The word `ono.link/1` spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
        }
    }
}

/// One link the far side holds, as the far side observed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkEntry {
    /// The far side's name for the link.
    pub name: String,
    /// The address the link reaches (`host:port` for the authenticated transport).
    pub host: String,
    /// How the bytes travel.
    pub transport: LinkTransport,
    /// Whether the link is up, as the far side last saw it.
    pub state: LinkState,
    /// The fingerprint of the key the next hop proved on this link, where it proved one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// The boot of the kernel the next hop runs, as its `kernel` target answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_id: Option<String>,
    /// The next hop's handshake round trip, in nanoseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_trip_ns: Option<u64>,
    /// The far side's latest probe of the link, in nanoseconds (ADR-1095).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ns: Option<u64>,
    /// When that probe was made (RFC 3339 on the wire).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured_at: Option<jiff::Timestamp>,
    /// Since when the link has been in `state` (RFC 3339 on the wire).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<jiff::Timestamp>,
}

/// A far side's link table: its links, and whether it left any out.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LinkTable {
    /// The links, as the far side observed them.
    pub links: Vec<LinkEntry>,
    /// Whether the far side holds more links than it sent.
    #[serde(default)]
    pub truncated: bool,
    /// When the far side assembled the table (RFC 3339 on the wire).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<jiff::Timestamp>,
}

impl LinkTable {
    /// A table of `links` this side may send as it is: cut to [`MAX_LINK_TABLE_ENTRIES`], and
    /// marked when it was cut.
    ///
    /// What an agent puts in its table came partly from its own next hops — a boot id, a
    /// fingerprint — and an honest agent must not send a table its peer is bound to refuse
    /// (review COR-4, SEC-4). So an optional field longer than [`MAX_LINK_FIELD_BYTES`] is left
    /// out (unknown, never cut into something else), and an entry whose name or host is longer
    /// is left out and the table marked truncated.
    #[must_use]
    pub fn of(links: Vec<LinkEntry>) -> Self {
        let long = |text: &str| text.len() > MAX_LINK_FIELD_BYTES;
        let mut truncated = false;
        let mut kept = Vec::new();
        for mut entry in links {
            if long(&entry.name) || long(&entry.host) {
                truncated = true;
                continue;
            }
            if entry.fingerprint.as_deref().is_some_and(long) {
                entry.fingerprint = None;
            }
            if entry.boot_id.as_deref().is_some_and(long) {
                entry.boot_id = None;
            }
            kept.push(entry);
        }
        truncated |= kept.len() > MAX_LINK_TABLE_ENTRIES;
        kept.truncate(MAX_LINK_TABLE_ENTRIES);
        Self {
            links: kept,
            truncated,
            observed_at: Some(jiff::Timestamp::now()),
        }
    }

    /// Refuses a table this side must not believe: too many entries, or a field too long.
    ///
    /// # Errors
    ///
    /// [`ProtocolError::MalformedPayload`] naming what was out of bounds.
    pub(crate) fn check(&self) -> Result<(), ProtocolError> {
        let refuse = |detail: String| ProtocolError::MalformedPayload {
            kind: FrameKind::LinkTable,
            detail,
        };
        if self.links.len() > MAX_LINK_TABLE_ENTRIES {
            return Err(refuse(format!(
                "a link table carries at most {MAX_LINK_TABLE_ENTRIES} links, this one {}",
                self.links.len()
            )));
        }
        let long = |text: &str| text.len() > MAX_LINK_FIELD_BYTES;
        for entry in &self.links {
            let texts = [
                Some(entry.name.as_str()),
                Some(entry.host.as_str()),
                entry.fingerprint.as_deref(),
                entry.boot_id.as_deref(),
            ];
            if texts.into_iter().flatten().any(long) {
                return Err(refuse(format!(
                    "a link table field is longer than {MAX_LINK_FIELD_BYTES} bytes"
                )));
            }
        }
        Ok(())
    }
}
