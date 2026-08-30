//! Decoding `sock_diag` replies into `ono.socket/1` (spec §23.2, §28.4).

use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_value::{ErrorValue, MapValue, RecordValue, Schema, Value};

use crate::decoded::{Decoded, Item, build};
use crate::interface::{short, unexpected};
use crate::owners::SocketOwners;
use crate::schema::{endpoint_schema, socket_schema};
use crate::wire::{self, Frame};
use crate::{SOCK_DIAG_PROVIDER, sys};

/// Which transport a `sock_diag` dump was asked about.
///
/// The reply does not carry the protocol — `inet_diag_msg` has no field for it — so it is an
/// input to the decoder rather than something read out of the bytes. Guessing it from the state
/// would be exactly the kind of invention spec §35.3 forbids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SocketProtocol {
    /// `IPPROTO_TCP`.
    Tcp,
    /// `IPPROTO_UDP`.
    Udp,
}

impl SocketProtocol {
    /// The name `ono.socket/1` gives the protocol.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SocketProtocol::Tcp => "tcp",
            SocketProtocol::Udp => "udp",
        }
    }

    /// The `IPPROTO_*` number to ask `sock_diag` for.
    pub(crate) const fn number(self) -> u8 {
        match self {
            SocketProtocol::Tcp => sys::IPPROTO_TCP,
            SocketProtocol::Udp => sys::IPPROTO_UDP,
        }
    }

    /// Whether the protocol has a connection state of its own.
    ///
    /// UDP does not. The kernel reuses the TCP state constants for it, and copying that reuse
    /// into a field a user filters on would mean answering `established` about a datagram socket.
    pub(crate) const fn has_state(self) -> bool {
        matches!(self, SocketProtocol::Tcp)
    }
}

/// Where an IP socket record says it came from.
pub(crate) fn inet_source(protocol: SocketProtocol) -> String {
    format!(
        "NETLINK_SOCK_DIAG SOCK_DIAG_BY_FAMILY({})",
        protocol.as_str()
    )
}

/// Where a Unix socket record says it came from.
pub(crate) const UNIX_SOURCE: &str = "NETLINK_SOCK_DIAG SOCK_DIAG_BY_FAMILY(AF_UNIX)";

/// Decodes an `inet_diag` dump of `protocol` into socket records.
///
/// `owners` is the result of one procfs scan, or `None` when the caller did not ask for the
/// owning process. Without it the `process` field is null — the socket's owner was not looked
/// up, which is not the same as it having none.
///
/// ```
/// use ono_provider_netlink::{SocketProtocol, decode_inet_sockets};
/// let decoded = decode_inet_sockets(&[], SocketProtocol::Tcp, None);
/// assert!(decoded.records().is_empty());
/// ```
#[must_use]
pub fn decode_inet_sockets(
    bytes: &[u8],
    protocol: SocketProtocol,
    owners: Option<&SocketOwners>,
) -> Decoded {
    Decoded::from_items(inet_sockets(bytes, protocol, owners))
}

/// Walks an `inet_diag` dump one message at a time.
///
/// The whole dump is in memory — the kernel answered it in one go — but nothing in it becomes a
/// record until the caller asks for the next one, so a consumer that wanted three sockets pays
/// for three (ADR-0418).
pub(crate) fn inet_sockets<'a>(
    bytes: &'a [u8],
    protocol: SocketProtocol,
    owners: Option<&'a SocketOwners>,
) -> InetSockets<'a> {
    InetSockets {
        frames: wire::frames(bytes),
        protocol,
        owners,
        schema: socket_schema(),
        source: inet_source(protocol),
        finished: false,
    }
}

/// The iterator [`inet_sockets`] returns.
pub(crate) struct InetSockets<'a> {
    frames: wire::Frames<'a>,
    protocol: SocketProtocol,
    owners: Option<&'a SocketOwners>,
    schema: Arc<Schema>,
    source: String,
    finished: bool,
}

impl Iterator for InetSockets<'_> {
    type Item = Item;

    fn next(&mut self) -> Option<Item> {
        while !self.finished {
            let message = match self.frames.next()? {
                Frame::Message(message) => message,
                Frame::Malformed(error) => {
                    self.finished = true;
                    return Some(Item::Failure(error));
                }
            };
            match message.kind {
                sys::NLMSG_DONE => {
                    self.finished = true;
                    return None;
                }
                sys::NLMSG_ERROR => {
                    return Some(Item::Failure(wire::error_message(message.payload)));
                }
                kind if wire::control(kind) => continue,
                sys::SOCK_DIAG_BY_FAMILY => {}
                other => return Some(Item::Failure(unexpected(other, "a socket"))),
            }
            return Some(self.socket(message.payload));
        }
        None
    }
}

impl InetSockets<'_> {
    /// One `inet_diag_msg` as a record, or the reason it could not become one.
    fn socket(&self, payload: &[u8]) -> Item {
        if payload.len() < sys::INET_DIAG_MSG {
            return Item::Failure(short("inet_diag_msg", payload.len(), sys::INET_DIAG_MSG));
        }

        let family = wire::u8_at(payload, 0).unwrap_or(0);
        let Some(family_name) = inet_family_name(family) else {
            return Item::Failure(unexpected_family(family));
        };
        let state = wire::u8_at(payload, 1).unwrap_or(0);
        let source_port = wire::be16_at(payload, 4).unwrap_or(0);
        let destination_port = wire::be16_at(payload, 6).unwrap_or(0);
        let source_address = wire::address(payload.get(8..).unwrap_or(&[]), family);
        let destination_address = wire::address(payload.get(24..).unwrap_or(&[]), family);
        let cookie = cookie_at(payload, 44);
        let rqueue = wire::u32_at(payload, 56).unwrap_or(0);
        let wqueue = wire::u32_at(payload, 60).unwrap_or(0);
        let uid = wire::u32_at(payload, 64).unwrap_or(0);
        let inode = wire::u32_at(payload, 68).unwrap_or(0);

        let local = match endpoint(source_address, Some(source_port), None) {
            Ok(record) => record,
            Err(error) => return Item::Failure(error),
        };
        // A peer of `0.0.0.0:0` is how the kernel says there is no peer, and a listening socket
        // genuinely has none. Reporting an all-zero endpoint would turn "none" into "this one".
        let has_peer = destination_port != 0
            || destination_address.is_some_and(|address| !address.is_unspecified());
        let remote = if has_peer {
            match endpoint(destination_address, Some(destination_port), None) {
                Ok(record) => record,
                Err(error) => return Item::Failure(error),
            }
        } else {
            Value::Null
        };

        item(build(
            &self.schema,
            &self.source,
            SOCK_DIAG_PROVIDER,
            vec![
                ("protocol", Value::string(self.protocol.as_str())),
                ("family", Value::string(family_name)),
                ("local", local),
                ("remote", remote),
                (
                    "state",
                    if self.protocol.has_state() {
                        Value::string(sys::tcp_state(state))
                    } else {
                        Value::Null
                    },
                ),
                ("process", owner_of(self.owners, u64::from(inode))),
                ("user", Value::Int(i128::from(uid))),
                ("inode", inode_value(inode)),
            ],
            vec![
                ("netlink.rx_queue", Value::Int(i128::from(rqueue))),
                ("netlink.tx_queue", Value::Int(i128::from(wqueue))),
                ("netlink.cookie", Value::Int(i128::from(cookie))),
            ],
        ))
    }
}

/// Decodes a `unix_diag` dump into socket records.
///
/// ```
/// use ono_provider_netlink::decode_unix_sockets;
/// let decoded = decode_unix_sockets(&[], None);
/// assert!(decoded.records().is_empty());
/// ```
#[must_use]
pub fn decode_unix_sockets(bytes: &[u8], owners: Option<&SocketOwners>) -> Decoded {
    Decoded::from_items(unix_sockets(bytes, owners))
}

/// Walks a `unix_diag` dump one message at a time, on the terms [`inet_sockets`] describes.
pub(crate) fn unix_sockets<'a>(
    bytes: &'a [u8],
    owners: Option<&'a SocketOwners>,
) -> UnixSockets<'a> {
    UnixSockets {
        frames: wire::frames(bytes),
        owners,
        schema: socket_schema(),
        finished: false,
    }
}

/// The iterator [`unix_sockets`] returns.
pub(crate) struct UnixSockets<'a> {
    frames: wire::Frames<'a>,
    owners: Option<&'a SocketOwners>,
    schema: Arc<Schema>,
    finished: bool,
}

impl Iterator for UnixSockets<'_> {
    type Item = Item;

    fn next(&mut self) -> Option<Item> {
        while !self.finished {
            let message = match self.frames.next()? {
                Frame::Message(message) => message,
                Frame::Malformed(error) => {
                    self.finished = true;
                    return Some(Item::Failure(error));
                }
            };
            match message.kind {
                sys::NLMSG_DONE => {
                    self.finished = true;
                    return None;
                }
                sys::NLMSG_ERROR => {
                    return Some(Item::Failure(wire::error_message(message.payload)));
                }
                kind if wire::control(kind) => continue,
                sys::SOCK_DIAG_BY_FAMILY => {}
                other => return Some(Item::Failure(unexpected(other, "a socket"))),
            }
            return Some(self.socket(message.payload));
        }
        None
    }
}

impl UnixSockets<'_> {
    /// One `unix_diag_msg` as a record, or the reason it could not become one.
    fn socket(&self, payload: &[u8]) -> Item {
        if payload.len() < sys::UNIX_DIAG_MSG {
            return Item::Failure(short("unix_diag_msg", payload.len(), sys::UNIX_DIAG_MSG));
        }

        let family = wire::u8_at(payload, 0).unwrap_or(0);
        if family != sys::AF_UNIX {
            return Item::Failure(unexpected_family(family));
        }
        let kind = wire::u8_at(payload, 1).unwrap_or(0);
        let state = wire::u8_at(payload, 2).unwrap_or(0);
        let inode = wire::u32_at(payload, 4).unwrap_or(0);
        let cookie = cookie_at(payload, 8);
        let attributes = payload.get(sys::UNIX_DIAG_MSG..).unwrap_or(&[]);

        let path = wire::attribute(attributes, sys::UNIX_DIAG_NAME).and_then(unix_path);
        let local = match path {
            None => Value::Null,
            Some(path) => match endpoint(None, None, Some(&path)) {
                Ok(record) => record,
                Err(error) => return Item::Failure(error),
            },
        };
        let peer = wire::attribute_u32(attributes, sys::UNIX_DIAG_PEER);

        item(build(
            &self.schema,
            UNIX_SOURCE,
            SOCK_DIAG_PROVIDER,
            vec![
                ("protocol", Value::string("unix")),
                ("family", Value::string("unix")),
                ("local", local),
                // A Unix socket's peer is another socket, named by inode rather than by address.
                // `ono.endpoint/1` has nowhere to put that, so it travels as an extension and
                // `remote` stays honestly null.
                ("remote", Value::Null),
                ("state", Value::string(sys::tcp_state(state))),
                ("process", owner_of(self.owners, u64::from(inode))),
                ("user", Value::Null),
                ("inode", inode_value(inode)),
            ],
            vec![
                (
                    "netlink.socket_type",
                    Value::string(sys::unix_socket_type(kind)),
                ),
                (
                    "netlink.peer_inode",
                    peer.map_or(Value::Null, |peer| Value::Int(i128::from(peer))),
                ),
                ("netlink.cookie", Value::Int(i128::from(cookie))),
            ],
        ))
    }
}

/// A built record, or the reason it could not be built, as one item.
fn item(built: Result<RecordValue, ErrorValue>) -> Item {
    match built {
        Ok(record) => Item::Record(record),
        Err(error) => Item::Failure(error),
    }
}

/// One end of a socket as an `ono.endpoint/1` record, or the reason it could not be built.
fn endpoint(
    address: Option<IpAddr>,
    port: Option<u16>,
    path: Option<&Path>,
) -> Result<Value, ErrorValue> {
    let record = build(
        &endpoint_schema(),
        "NETLINK_SOCK_DIAG inet_diag_sockid",
        SOCK_DIAG_PROVIDER,
        vec![
            ("address", address.map_or(Value::Null, Value::Ip)),
            ("port", port.map_or(Value::Null, Value::Port)),
            (
                "path",
                path.map_or(Value::Null, |path| Value::Path(path.into())),
            ),
            // Reverse resolution is derived data (spec §22.2) and can block; it is never done on
            // the path of an enumeration.
            ("host", Value::Null),
        ],
        Vec::new(),
    );
    record.map(|record| Value::Record(Arc::new(record)))
}

/// The owning process as the identity map the `process` field carries.
fn owner_of(owners: Option<&SocketOwners>, inode: u64) -> Value {
    let Some(owners) = owners else {
        // Nobody looked. The field is unknown, and spec §35.3 spells an unknown null.
        return Value::Null;
    };
    let Some(owner) = owners.owner(inode) else {
        // The scan ran and did not attribute this inode. Whether that means "nobody holds it" or
        // "the process that holds it is not yours to read" is the scan's own answer, and v0.4
        // §35.2 keeps the two apart: a refusal is denied, never absent.
        return match owners.refusal() {
            Some(refusal) => refusal.into_value(),
            None => Value::Null,
        };
    };
    let mut identity = MapValue::new();
    identity.insert("pid".into(), Value::Int(i128::from(owner.pid())));
    identity.insert(
        "name".into(),
        owner
            .name()
            .map_or(Value::Null, |name| Value::String(name.into())),
    );
    Value::Map(Arc::new(identity))
}

/// The socket inode, or null where the kernel has none to give.
///
/// A socket in `time-wait` has no inode: the kernel has already released it. The identity is then
/// unknown rather than zero, and `netlink.cookie` carries the kernel's own handle for it.
fn inode_value(inode: u32) -> Value {
    if inode == 0 {
        Value::Null
    } else {
        Value::Int(i128::from(inode))
    }
}

/// The two halves of a `sock_diag` cookie as one number.
fn cookie_at(payload: &[u8], offset: usize) -> u64 {
    let low = u64::from(wire::u32_at(payload, offset).unwrap_or(0));
    let high = u64::from(wire::u32_at(payload, offset + 4).unwrap_or(0));
    low | (high << 32)
}

/// A Unix socket's name, with the leading NUL of an abstract name rendered as `@`.
fn unix_path(payload: &[u8]) -> Option<std::path::PathBuf> {
    if payload.is_empty() {
        return None;
    }
    let (prefix, rest) = if payload.first() == Some(&0) {
        ("@", payload.get(1..).unwrap_or(&[]))
    } else {
        ("", payload)
    };
    let end = rest
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(rest.len());
    let name = std::str::from_utf8(rest.get(..end)?).ok()?;
    if prefix.is_empty() && name.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(format!("{prefix}{name}")))
}

/// The name `ono.socket/1` gives an address family.
fn inet_family_name(family: u8) -> Option<&'static str> {
    match family {
        sys::AF_INET => Some("inet"),
        sys::AF_INET6 => Some("inet6"),
        _ => None,
    }
}

/// A diag reply about a family the request did not ask for.
fn unexpected_family(family: u8) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderSchemaViolation,
        format!("a sock_diag reply describes address family {family}, which was not requested"),
    )
}
