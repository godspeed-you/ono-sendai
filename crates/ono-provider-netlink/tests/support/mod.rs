//! Assembles netlink messages the way the kernel does, so a decoder can be tested without one.
//!
//! Every fixture in this crate's tests is built from these helpers rather than from a hex blob,
//! for two reasons. A blob says nothing about *why* a byte is where it is, and a decoder tested
//! only against blobs captured from one kernel silently stops covering the shapes another kernel
//! sends. Building the messages here states the wire format explicitly, and lets a test bend one
//! field — a length, an attribute — to see what the decoder does with it.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a fixture builder states its preconditions the way a test does, and not every \
              helper is used by every test binary"
)]

/// Rounds `value` up to the next multiple of four, the netlink alignment.
fn align4(value: usize) -> usize {
    value.div_ceil(4) * 4
}

/// One netlink message: a 16-byte header, `payload`, and the padding that follows it.
pub fn message(kind: u16, payload: &[u8]) -> Vec<u8> {
    message_with_flags(kind, 2, payload)
}

/// A netlink message with explicit header flags.
pub fn message_with_flags(kind: u16, flags: u16, payload: &[u8]) -> Vec<u8> {
    let length = 16 + payload.len();
    let mut out = Vec::with_capacity(align4(length));
    out.extend_from_slice(&u32::try_from(length).unwrap().to_ne_bytes());
    out.extend_from_slice(&kind.to_ne_bytes());
    out.extend_from_slice(&flags.to_ne_bytes());
    out.extend_from_slice(&1u32.to_ne_bytes());
    out.extend_from_slice(&0u32.to_ne_bytes());
    out.extend_from_slice(payload);
    out.resize(align4(length), 0);
    out
}

/// A message whose header claims `claimed_length` bytes whatever it really carries.
pub fn message_claiming(kind: u16, claimed_length: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&claimed_length.to_ne_bytes());
    out.extend_from_slice(&kind.to_ne_bytes());
    out.extend_from_slice(&2u16.to_ne_bytes());
    out.extend_from_slice(&1u32.to_ne_bytes());
    out.extend_from_slice(&0u32.to_ne_bytes());
    out.extend_from_slice(payload);
    out
}

/// The `NLMSG_DONE` that ends a dump.
pub fn done() -> Vec<u8> {
    message(3, &0i32.to_ne_bytes())
}

/// An `NLMSG_ERROR` carrying `-errno`.
pub fn error(errno: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(-errno).to_ne_bytes());
    payload.extend_from_slice(&[0u8; 16]);
    message(2, &payload)
}

/// One `rtattr`: a 4-byte header, `payload`, and its padding.
pub fn attr(kind: u16, payload: &[u8]) -> Vec<u8> {
    let length = 4 + payload.len();
    let mut out = Vec::with_capacity(align4(length));
    out.extend_from_slice(&u16::try_from(length).unwrap().to_ne_bytes());
    out.extend_from_slice(&kind.to_ne_bytes());
    out.extend_from_slice(payload);
    out.resize(align4(length), 0);
    out
}

/// An attribute whose header claims `claimed_length` bytes whatever it really carries.
pub fn attr_claiming(kind: u16, claimed_length: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&claimed_length.to_ne_bytes());
    out.extend_from_slice(&kind.to_ne_bytes());
    out.extend_from_slice(payload);
    out
}

/// An attribute containing other attributes.
pub fn nested(kind: u16, inner: &[Vec<u8>]) -> Vec<u8> {
    attr(kind, &concat(inner))
}

/// Concatenates message or attribute fragments.
pub fn concat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.iter().flatten().copied().collect()
}

/// A `struct ifinfomsg` for interface `index`.
pub fn ifinfomsg(index: i32, kind: u16, flags: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.push(0); // ifi_family
    out.push(0); // padding
    out.extend_from_slice(&kind.to_ne_bytes());
    out.extend_from_slice(&index.to_ne_bytes());
    out.extend_from_slice(&flags.to_ne_bytes());
    out.extend_from_slice(&0u32.to_ne_bytes());
    out
}

/// A `struct ifaddrmsg`.
pub fn ifaddrmsg(family: u8, prefix_len: u8, index: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.push(family);
    out.push(prefix_len);
    out.push(0); // ifa_flags
    out.push(0); // ifa_scope
    out.extend_from_slice(&index.to_ne_bytes());
    out
}

/// A `struct rtmsg`.
#[allow(
    clippy::too_many_arguments,
    reason = "the kernel struct has this many fields"
)]
pub fn rtmsg(
    family: u8,
    dst_len: u8,
    table: u8,
    protocol: u8,
    scope: u8,
    kind: u8,
    flags: u32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    out.push(family);
    out.push(dst_len);
    out.push(0); // rtm_src_len
    out.push(0); // rtm_tos
    out.push(table);
    out.push(protocol);
    out.push(scope);
    out.push(kind);
    out.extend_from_slice(&flags.to_ne_bytes());
    out
}

/// A `struct ndmsg`.
pub fn ndmsg(family: u8, index: i32, state: u16, flags: u8, kind: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    out.push(family);
    out.push(0);
    out.extend_from_slice(&0u16.to_ne_bytes());
    out.extend_from_slice(&index.to_ne_bytes());
    out.extend_from_slice(&state.to_ne_bytes());
    out.push(flags);
    out.push(kind);
    out
}

/// An `inet_diag_sockid`: ports in network order, addresses padded to sixteen bytes.
pub fn sockid(source: (&[u8], u16), destination: (&[u8], u16), cookie: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(48);
    out.extend_from_slice(&source.1.to_be_bytes());
    out.extend_from_slice(&destination.1.to_be_bytes());
    out.extend_from_slice(&padded_address(source.0));
    out.extend_from_slice(&padded_address(destination.0));
    out.extend_from_slice(&0u32.to_ne_bytes()); // idiag_if
    out.extend_from_slice(&u32::try_from(cookie & 0xffff_ffff).unwrap().to_ne_bytes());
    out.extend_from_slice(&u32::try_from(cookie >> 32).unwrap().to_ne_bytes());
    out
}

fn padded_address(address: &[u8]) -> [u8; 16] {
    let mut padded = [0u8; 16];
    padded[..address.len()].copy_from_slice(address);
    padded
}

/// An `inet_diag_msg`.
pub fn inet_diag_msg(
    family: u8,
    state: u8,
    id: &[u8],
    rqueue: u32,
    wqueue: u32,
    uid: u32,
    inode: u32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(72);
    out.push(family);
    out.push(state);
    out.push(0); // idiag_timer
    out.push(0); // idiag_retrans
    out.extend_from_slice(id);
    out.extend_from_slice(&0u32.to_ne_bytes()); // idiag_expires
    out.extend_from_slice(&rqueue.to_ne_bytes());
    out.extend_from_slice(&wqueue.to_ne_bytes());
    out.extend_from_slice(&uid.to_ne_bytes());
    out.extend_from_slice(&inode.to_ne_bytes());
    out
}

/// A `unix_diag_msg`.
pub fn unix_diag_msg(kind: u8, state: u8, inode: u32, cookie: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.push(1); // AF_UNIX
    out.push(kind);
    out.push(state);
    out.push(0);
    out.extend_from_slice(&inode.to_ne_bytes());
    out.extend_from_slice(&u32::try_from(cookie & 0xffff_ffff).unwrap().to_ne_bytes());
    out.extend_from_slice(&u32::try_from(cookie >> 32).unwrap().to_ne_bytes());
    out
}

/// A `rtnl_link_stats64`, of which only the first eight counters are ever read.
pub fn link_stats64(counters: [u64; 8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 * 24);
    for counter in counters {
        out.extend_from_slice(&counter.to_ne_bytes());
    }
    out.resize(8 * 24, 0);
    out
}
