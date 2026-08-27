//! Reading netlink messages and attributes out of a byte buffer.
//!
//! Everything the kernel sends arrives here first, and everything here treats it as untrusted
//! input (spec §35.6, ADR-0015 T7): every length is checked against what remains, every read is
//! a bounded slice rather than an index, and every step of an iterator consumes at least one
//! aligned header, so no malformed length can turn a walk into a loop.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ono_core::ErrorCode;
use ono_value::ErrorValue;

use crate::sys;

/// One netlink message: its type and the payload behind the header.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Message<'a> {
    pub(crate) kind: u16,
    pub(crate) payload: &'a [u8],
}

/// What the walk over a buffer produced: a message, or the reason it stopped.
#[derive(Debug)]
pub(crate) enum Frame<'a> {
    /// A message whose header was consistent with the bytes that followed it.
    Message(Message<'a>),
    /// The buffer ended inside a message, or a header claimed a length the buffer cannot hold.
    Malformed(ErrorValue),
}

/// Walks the netlink messages in `bytes`.
pub(crate) fn frames(bytes: &[u8]) -> Frames<'_> {
    Frames { rest: bytes }
}

/// The iterator [`frames`] returns.
#[derive(Debug)]
pub(crate) struct Frames<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Frames<'a> {
    type Item = Frame<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let remaining = self.rest.len();
        if remaining < sys::NLMSG_HEADER {
            self.rest = &[];
            return Some(Frame::Malformed(truncated(format!(
                "a netlink message header needs {} bytes but {remaining} remain",
                sys::NLMSG_HEADER
            ))));
        }

        let Some(length) = u32_at(self.rest, 0).map(|length| length as usize) else {
            self.rest = &[];
            return Some(Frame::Malformed(truncated(
                "a netlink message header ended before its length field".to_owned(),
            )));
        };
        if length < sys::NLMSG_HEADER {
            self.rest = &[];
            return Some(Frame::Malformed(truncated(format!(
                "a netlink message claims {length} bytes, less than its own {}-byte header",
                sys::NLMSG_HEADER
            ))));
        }
        if length > remaining {
            self.rest = &[];
            return Some(Frame::Malformed(truncated(format!(
                "a netlink message claims {length} bytes but only {remaining} remain"
            ))));
        }

        let kind = u16_at(self.rest, 4).unwrap_or(0);
        let payload = self.rest.get(sys::NLMSG_HEADER..length).unwrap_or(&[]);
        // `length` is at least the header size, so the cursor always moves forward.
        let advance = align(length).min(remaining);
        self.rest = self.rest.get(advance..).unwrap_or(&[]);
        Some(Frame::Message(Message { kind, payload }))
    }
}

/// One `rtattr`: its type and the payload behind the header.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Attribute<'a> {
    pub(crate) kind: u16,
    pub(crate) payload: &'a [u8],
}

/// Walks the attributes in `bytes`.
///
/// An attribute whose length field does not fit the buffer ends the walk rather than raising an
/// error: the attributes already read are still good, and the caller reports the ones it needed
/// and did not get as failed fields, which is what keeps a truncated message from erasing the
/// object it described.
pub(crate) fn attributes(bytes: &[u8]) -> Attributes<'_> {
    Attributes { rest: bytes }
}

/// The iterator [`attributes`] returns.
#[derive(Debug)]
pub(crate) struct Attributes<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Attributes<'a> {
    type Item = Attribute<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let remaining = self.rest.len();
        if remaining < sys::ATTR_HEADER {
            self.rest = &[];
            return None;
        }
        let length = u16_at(self.rest, 0)? as usize;
        if length < sys::ATTR_HEADER || length > remaining {
            self.rest = &[];
            return None;
        }
        let kind = u16_at(self.rest, 2)?;
        let payload = self.rest.get(sys::ATTR_HEADER..length).unwrap_or(&[]);
        let advance = align(length).min(remaining);
        self.rest = self.rest.get(advance..).unwrap_or(&[]);
        Some(Attribute { kind, payload })
    }
}

/// The payload of the first attribute of type `kind`, if the buffer holds one.
pub(crate) fn attribute(bytes: &[u8], kind: u16) -> Option<&[u8]> {
    attributes(bytes)
        .find(|attribute| attribute.kind == kind)
        .map(|attribute| attribute.payload)
}

/// Rounds up to the netlink alignment.
fn align(value: usize) -> usize {
    value.div_ceil(sys::ALIGN) * sys::ALIGN
}

/// The `u8` at `offset`, if the buffer reaches that far.
pub(crate) fn u8_at(bytes: &[u8], offset: usize) -> Option<u8> {
    bytes.get(offset).copied()
}

/// The native-endian `u16` at `offset`, if the buffer reaches that far.
pub(crate) fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes
        .get(offset..)
        .and_then(<[u8]>::first_chunk::<2>)
        .copied()
        .map(u16::from_ne_bytes)
}

/// The native-endian `u32` at `offset`, if the buffer reaches that far.
pub(crate) fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..)
        .and_then(<[u8]>::first_chunk::<4>)
        .copied()
        .map(u32::from_ne_bytes)
}

/// The native-endian `i32` at `offset`, if the buffer reaches that far.
pub(crate) fn i32_at(bytes: &[u8], offset: usize) -> Option<i32> {
    u32_at(bytes, offset).map(|value| value as i32)
}

/// The native-endian `u64` at `offset`, if the buffer reaches that far.
pub(crate) fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    bytes
        .get(offset..)
        .and_then(<[u8]>::first_chunk::<8>)
        .copied()
        .map(u64::from_ne_bytes)
}

/// The big-endian `u16` at `offset` — netlink carries ports in network order.
pub(crate) fn be16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes
        .get(offset..)
        .and_then(<[u8]>::first_chunk::<2>)
        .copied()
        .map(u16::from_be_bytes)
}

/// An attribute payload read as a native-endian `u32`.
pub(crate) fn attribute_u32(bytes: &[u8], kind: u16) -> Option<u32> {
    attribute(bytes, kind).and_then(|payload| u32_at(payload, 0))
}

/// An address of `family` read from the first bytes of `payload`.
pub(crate) fn address(payload: &[u8], family: u8) -> Option<IpAddr> {
    match family {
        sys::AF_INET => payload
            .first_chunk::<4>()
            .copied()
            .map(|octets| IpAddr::V4(Ipv4Addr::from(octets))),
        sys::AF_INET6 => payload
            .first_chunk::<16>()
            .copied()
            .map(|octets| IpAddr::V6(Ipv6Addr::from(octets))),
        _ => None,
    }
}

/// A link-layer address as the colon-separated hexadecimal a person reads.
///
/// An address of all zeroes is `None`: interfaces that have no hardware address — loopback, tun,
/// most tunnels — report one, and spec §10.5 wants that reported as unknown rather than as a
/// MAC nobody has.
pub(crate) fn hardware_address(payload: &[u8]) -> Option<String> {
    if payload.is_empty() || payload.iter().all(|byte| *byte == 0) {
        return None;
    }
    let mut out = String::with_capacity(payload.len() * 3);
    for (index, byte) in payload.iter().enumerate() {
        if index > 0 {
            out.push(':');
        }
        out.push_str(&format!("{byte:02x}"));
    }
    Some(out)
}

/// A NUL-terminated string attribute.
pub(crate) fn text(payload: &[u8]) -> Option<String> {
    let end = payload
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(payload.len());
    let slice = payload.get(..end)?;
    if slice.is_empty() {
        return None;
    }
    std::str::from_utf8(slice).ok().map(ToOwned::to_owned)
}

/// The `NLMSG_ERROR` payload as the structured error it describes.
pub(crate) fn error_message(payload: &[u8]) -> ErrorValue {
    let errno = i32_at(payload, 0).unwrap_or(0);
    errno_error(-errno)
}

/// Turns a positive errno into the structured error spec §43 requires.
pub(crate) fn errno_error(errno: i32) -> ErrorValue {
    let code = match errno {
        1 | 13 => ErrorCode::IoPermissionDenied,
        // `ESRCH` is what the routing code answers for a route that is not there, and
        // `ENODEV` what the link code answers for an interface that is not.
        2 | 3 | 19 => ErrorCode::IoNotFound,
        17 => ErrorCode::IoAlreadyExists,
        _ => ErrorCode::ProviderUnavailable,
    };
    ErrorValue::new(
        code,
        format!(
            "the kernel refused the netlink request: {}",
            strerror(errno)
        ),
    )
}

/// The name of the errno, for the few this crate can actually meet.
fn strerror(errno: i32) -> String {
    match errno {
        0 => "no error".to_owned(),
        1 => "operation not permitted (EPERM)".to_owned(),
        2 => "no such file or directory (ENOENT)".to_owned(),
        3 => "no such process (ESRCH)".to_owned(),
        13 => "permission denied (EACCES)".to_owned(),
        17 => "file exists (EEXIST)".to_owned(),
        19 => "no such device (ENODEV)".to_owned(),
        22 => "invalid argument (EINVAL)".to_owned(),
        95 => "operation not supported (EOPNOTSUPP)".to_owned(),
        101 => "network is unreachable (ENETUNREACH)".to_owned(),
        92 => "protocol not available (ENOPROTOOPT)".to_owned(),
        93 => "protocol not supported (EPROTONOSUPPORT)".to_owned(),
        other => format!("errno {other}"),
    }
}

/// A message this crate could not read at all.
fn truncated(message: String) -> ErrorValue {
    ErrorValue::new(ErrorCode::ProviderSchemaViolation, message).with_help(
        "the kernel's reply ended inside a message. The objects decoded before it are still \
         good; this reports the ones that were not.",
    )
}

/// Whether `kind` is one of the control messages every dump can carry.
pub(crate) fn control(kind: u16) -> bool {
    matches!(
        kind,
        sys::NLMSG_NOOP | sys::NLMSG_DONE | sys::NLMSG_ERROR | sys::NLMSG_OVERRUN
    )
}

/// Appends one `rtattr` — header, payload, padding to the netlink alignment — to `into`.
///
/// The encoder is the mirror of [`attributes`]: a request is built exactly the way a reply is
/// read, so the kernel sees the layout its own headers describe.
pub(crate) fn push_attribute(into: &mut Vec<u8>, kind: u16, payload: &[u8]) {
    let length = sys::ATTR_HEADER + payload.len();
    let declared = u16::try_from(length).unwrap_or(u16::MAX);
    into.extend_from_slice(&declared.to_ne_bytes());
    into.extend_from_slice(&kind.to_ne_bytes());
    into.extend_from_slice(payload);
    into.resize(into.len() + (align(length) - length), 0);
}

/// Appends one nested `rtattr` whose payload is itself a run of attributes.
pub(crate) fn push_nested(into: &mut Vec<u8>, kind: u16, build: impl FnOnce(&mut Vec<u8>)) {
    let mut payload = Vec::new();
    build(&mut payload);
    push_attribute(into, kind, &payload);
}

/// The bytes of an address as netlink carries them: four for IPv4, sixteen for IPv6.
pub(crate) fn address_bytes(address: IpAddr) -> Vec<u8> {
    match address {
        IpAddr::V4(v4) => v4.octets().to_vec(),
        IpAddr::V6(v6) => v6.octets().to_vec(),
    }
}

/// The netlink family number of an address.
pub(crate) fn family_of(address: IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => sys::AF_INET,
        IpAddr::V6(_) => sys::AF_INET6,
    }
}
