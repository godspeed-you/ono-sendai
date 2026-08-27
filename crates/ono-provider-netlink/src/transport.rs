//! The netlink socket itself: one request, one dump, bounded and never blocking for ever.
//!
//! This is deliberately the thinnest layer in the crate. Everything that reads a byte lives in
//! the decoders, which are pure functions over a buffer and are therefore testable without a
//! kernel; this module only obtains the buffer.

use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicU32, Ordering};

use nix::sys::socket::{
    AddressFamily, MsgFlags, NetlinkAddr, SockFlag, SockProtocol, SockType, bind, recv, send,
    setsockopt, socket, sockopt,
};
use nix::sys::time::{TimeVal, TimeValLike};
use ono_core::ErrorCode;
use ono_value::ErrorValue;

use crate::sys;
use crate::wire::{self, Frame};

/// How long a single `recv` may wait before the provider gives up on the kernel.
///
/// A netlink dump is a local operation that answers in microseconds; anything approaching this
/// means the socket will not answer at all. A bound is required rather than merely prudent: spec
/// §34 makes latency a product property, and an interactive shell must never hang on a provider.
const RECEIVE_TIMEOUT_SECONDS: i64 = 2;

/// The buffer handed to each `recv`. The kernel sizes a netlink batch to fit the reader's buffer,
/// and 64 KiB is large enough that no single message is ever split.
const RECEIVE_BUFFER: usize = 64 * 1024;

/// The most a single dump may accumulate before the provider refuses it.
///
/// ADR-0015 T7: every decoder in this crate is bounded, and so is the buffer they decode. A
/// machine with a genuinely enormous socket table gets an error naming the limit rather than a
/// shell that grows until the kernel kills it.
const MAXIMUM_DUMP: usize = 64 * 1024 * 1024;

/// Numbers requests within this process, so a reply can be matched to what asked for it.
static SEQUENCE: AtomicU32 = AtomicU32::new(1);

/// A bound netlink socket of one protocol family.
#[derive(Debug)]
pub(crate) struct NetlinkSocket {
    fd: OwnedFd,
    family: &'static str,
}

impl NetlinkSocket {
    /// Opens `NETLINK_ROUTE`, the family that answers about links, addresses, routes and
    /// neighbours.
    pub(crate) fn open_route() -> Result<Self, ErrorValue> {
        Self::open(SockProtocol::NetlinkRoute, "NETLINK_ROUTE")
    }

    /// Opens `NETLINK_SOCK_DIAG`, the family that answers about sockets.
    pub(crate) fn open_diag() -> Result<Self, ErrorValue> {
        Self::open(
            SockProtocol::NetlinkSockDiag,
            "NETLINK_SOCK_DIAG (sock_diag)",
        )
    }

    fn open(protocol: SockProtocol, family: &'static str) -> Result<Self, ErrorValue> {
        let fd = socket(
            AddressFamily::Netlink,
            SockType::Raw,
            SockFlag::SOCK_CLOEXEC,
            protocol,
        )
        .map_err(|errno| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("{family} could not be opened: {errno}"),
            )
            .with_help(
                "the kernel offers no such netlink family here — it may be compiled out, or a \
                 sandbox may forbid it. This is not the same as there being nothing to report.",
            )
        })?;

        // Port id zero asks the kernel to allocate one, which is what lets several sockets in one
        // process talk netlink at the same time.
        bind(fd.as_raw_fd(), &NetlinkAddr::new(0, 0)).map_err(|errno| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("a {family} socket could not be bound: {errno}"),
            )
        })?;
        setsockopt(
            &fd,
            sockopt::ReceiveTimeout,
            &TimeVal::seconds(RECEIVE_TIMEOUT_SECONDS),
        )
        .map_err(|errno| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("a {family} socket would not take a receive timeout: {errno}"),
            )
        })?;

        Ok(Self { fd, family })
    }

    /// Sends one dump request and returns every byte the kernel answered with.
    ///
    /// The reply is returned undecoded, terminating `NLMSG_DONE` included, so that each dump can
    /// be decoded on its own. An acknowledgement carrying an errno ends the dump as an error,
    /// because that is the kernel refusing the request rather than describing an empty table.
    pub(crate) fn dump(&self, kind: u16, payload: &[u8]) -> Result<Vec<u8>, ErrorValue> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        self.send_request(kind, sequence, payload)?;

        let mut collected: Vec<u8> = Vec::new();
        let mut buffer = vec![0u8; RECEIVE_BUFFER];
        loop {
            let read =
                recv(self.fd.as_raw_fd(), &mut buffer, MsgFlags::empty()).map_err(|errno| {
                    match errno {
                        nix::errno::Errno::EAGAIN => ErrorValue::new(
                            ErrorCode::ProviderUnavailable,
                            format!(
                                "{} did not answer within {RECEIVE_TIMEOUT_SECONDS}s",
                                self.family
                            ),
                        )
                        .with_retryable(true),
                        other => ErrorValue::new(
                            ErrorCode::ProviderUnavailable,
                            format!("{} could not be read: {other}", self.family),
                        ),
                    }
                })?;
            if read == 0 {
                break;
            }
            let chunk = buffer.get(..read).unwrap_or(&[]);
            let finished = self.scan(chunk)?;

            if collected.len().saturating_add(chunk.len()) > MAXIMUM_DUMP {
                return Err(ErrorValue::new(
                    ErrorCode::ProviderUnavailable,
                    format!(
                        "the {} reply passed the {MAXIMUM_DUMP}-byte limit this provider accepts",
                        self.family
                    ),
                )
                .with_help("ask for less: a selector lets the kernel answer with fewer objects"));
            }
            collected.extend_from_slice(chunk);
            if finished {
                break;
            }
        }
        Ok(collected)
    }

    fn send_request(&self, kind: u16, sequence: u32, payload: &[u8]) -> Result<(), ErrorValue> {
        let length = sys::NLMSG_HEADER + payload.len();
        let mut request = Vec::with_capacity(length.div_ceil(sys::ALIGN) * sys::ALIGN);
        let Ok(declared) = u32::try_from(length) else {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                "a netlink request longer than four gigabytes cannot be expressed",
            ));
        };
        request.extend_from_slice(&declared.to_ne_bytes());
        request.extend_from_slice(&kind.to_ne_bytes());
        request.extend_from_slice(&(sys::NLM_F_REQUEST | sys::NLM_F_DUMP).to_ne_bytes());
        request.extend_from_slice(&sequence.to_ne_bytes());
        request.extend_from_slice(&0u32.to_ne_bytes());
        request.extend_from_slice(payload);
        request.resize(length.div_ceil(sys::ALIGN) * sys::ALIGN, 0);

        send(self.fd.as_raw_fd(), &request, MsgFlags::empty()).map_err(|errno| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("a {} request could not be sent: {errno}", self.family),
            )
        })?;
        Ok(())
    }

    /// Whether this batch ends the dump, and the kernel's refusal when it carries one.
    fn scan(&self, chunk: &[u8]) -> Result<bool, ErrorValue> {
        for frame in wire::frames(chunk) {
            match frame {
                // A batch that ends inside a message is as far as this dump goes; the decoder
                // reports the truncation, so the caller still sees what did arrive.
                Frame::Malformed(_) => return Ok(true),
                Frame::Message(message) if message.kind == sys::NLMSG_DONE => return Ok(true),
                Frame::Message(message) if message.kind == sys::NLMSG_ERROR => {
                    let errno = wire::i32_at(message.payload, 0).unwrap_or(0);
                    if errno == 0 {
                        // An acknowledgement with no error: the request was accepted and there is
                        // nothing more to read.
                        return Ok(true);
                    }
                    return Err(wire::errno_error(-errno).with_help(format!(
                        "{} refused the request; nothing is being hidden by an empty answer",
                        self.family
                    )));
                }
                Frame::Message(_) => {}
            }
        }
        Ok(false)
    }
}

/// A zeroed `struct ifinfomsg`, which asks for every link.
pub(crate) fn link_request() -> Vec<u8> {
    vec![0u8; sys::IFINFOMSG]
}

/// A zeroed `struct ifaddrmsg`, which asks for every address of every family.
pub(crate) fn address_request() -> Vec<u8> {
    vec![0u8; sys::IFADDRMSG]
}

/// A `struct rtmsg` asking for one address family's routes.
pub(crate) fn route_request(family: u8) -> Vec<u8> {
    let mut request = vec![0u8; sys::RTMSG];
    if let Some(slot) = request.first_mut() {
        *slot = family;
    }
    request
}

/// A `struct ndmsg` asking for every neighbour of every family.
pub(crate) fn neighbour_request() -> Vec<u8> {
    vec![0u8; sys::NDMSG]
}

/// An `inet_diag_req_v2` asking for every socket of one family and protocol.
pub(crate) fn inet_diag_request(family: u8, protocol: u8) -> Vec<u8> {
    let mut request = Vec::with_capacity(56);
    request.push(family);
    request.push(protocol);
    request.push(0); // idiag_ext: no optional extensions
    request.push(0); // padding
    // Every state, including the ones a socket passes through while closing: leaving them out
    // would silently drop sockets a user asked to see.
    request.extend_from_slice(&u32::MAX.to_ne_bytes());
    request.resize(56, 0); // a zeroed inet_diag_sockid matches every socket
    request
}

/// A `unix_diag_req` asking for every Unix socket, with its name and its peer.
pub(crate) fn unix_diag_request() -> Vec<u8> {
    let mut request = Vec::with_capacity(24);
    request.push(sys::AF_UNIX);
    request.push(0); // sdiag_protocol
    request.extend_from_slice(&0u16.to_ne_bytes()); // padding
    request.extend_from_slice(&u32::MAX.to_ne_bytes()); // udiag_states
    request.extend_from_slice(&0u32.to_ne_bytes()); // udiag_ino: every socket
    request.extend_from_slice(&sys::UDIAG_SHOW.to_ne_bytes());
    request.extend_from_slice(&[0u8; 8]); // udiag_cookie
    request
}
