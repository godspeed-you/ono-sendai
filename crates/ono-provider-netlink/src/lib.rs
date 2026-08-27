//! Network providers that read the Linux kernel through netlink (spec §23.2, §28.4, §28.5).
//!
//! Four providers live here — [`InterfaceProvider`], [`RouteProvider`], [`NeighborProvider`] and
//! [`SocketProvider`] — and they share one shape. The kernel is asked with a netlink dump; the
//! bytes it answers with are decoded by a pure function; the records that come out are streamed.
//! Nothing in this crate runs `ip`, `ss`, `netstat`, `arp` or `route`, or reads anything they
//! print. Spec §23.2 asks for netlink and spec §50 forbids parsing another program's output, and
//! the difference is not stylistic: `ip -j` did not exist for most of its life, `ss` renders
//! addresses differently between versions, and neither can distinguish a value that is unknown
//! from a value that is zero.
//!
//! # The decoders are the unit under test
//!
//! [`decode_interfaces`], [`decode_routes`], [`decode_neighbors`], [`decode_inet_sockets`] and
//! [`decode_unix_sockets`] take a byte slice and return a [`Decoded`]: the records they read and
//! the problems they met, side by side. They touch no socket, so every shape a kernel can send —
//! a truncated message, an attribute claiming more bytes than the message holds, a family this
//! crate does not describe — is a test rather than a hope. Everything the kernel hands over is
//! treated as untrusted input (spec §35.6, ADR-0015 T7): every length is checked against what
//! remains and no buffer is ever indexed unchecked.
//!
//! # What a null means here
//!
//! - A route with no destination is the **default route** — `destination` is null because that is
//!   the answer, not because the answer is missing.
//! - A neighbour with no `mac` is one the kernel has **not resolved**, not one without a
//!   hardware address.
//! - A socket with a null `process` is one whose **owner was not looked up**, or one this user
//!   may not see. Finding the owner means scanning every `/proc/<pid>/fd` on the machine, which
//!   spec §34's latency budget cannot absorb on every `get socket`, so it happens only when the
//!   query carries `--process`, and then exactly once for the whole answer.
//! - An attribute the kernel did not send at all becomes an [`ErrorValue`](ono_value::ErrorValue)
//!   in the field: unreadable, which spec §10.5 keeps apart from both absent and unknown.
//!
//! A provider that cannot open its netlink family reports
//! [`Availability::unavailable`](ono_provider_api::Availability::unavailable) with the kernel's
//! reason. It never answers with an empty stream, because an empty stream reads as "there are
//! none of those", and telling those two apart is what the value model exists for.
//!
//! # Example
//!
//! ```no_run
//! use ono_pipeline::StreamEvent;
//! use ono_provider_api::{Provider, Query};
//! use ono_provider_netlink::InterfaceProvider;
//!
//! # async fn run() -> Result<(), ono_value::ErrorValue> {
//! let provider = InterfaceProvider::new();
//! let mut stream = provider.snapshot(&Query::target("interface"))?;
//! while let Some(event) = stream.recv().await {
//!     if let StreamEvent::Value(value) = event {
//!         println!("{value:?}");
//!     }
//! }
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod act;
mod decoded;
mod interface;
mod neighbor;
mod owners;
mod provider;
mod route;
mod schema;
mod socket;
mod sys;
mod transport;
mod wire;

pub use decoded::Decoded;
pub use interface::{InterfaceNames, decode_interfaces};
pub use neighbor::decode_neighbors;
pub use owners::{ProcessOwner, SocketOwners};
pub use provider::{InterfaceProvider, NeighborProvider, RouteProvider, SocketProvider};
pub use route::decode_routes;
pub use schema::{
    endpoint_schema, interface_schema, neighbor_schema, route_schema, schemas, socket_schema,
};
pub use socket::{SocketProtocol, decode_inet_sockets, decode_unix_sockets};

/// The id every record read over `NETLINK_ROUTE` carries in its provenance.
pub const NETLINK_PROVIDER: &str = "linux.netlink";

/// The id every record read over `NETLINK_SOCK_DIAG` carries in its provenance.
///
/// Sockets get an id of their own because they come from a different netlink family with a
/// different availability: a kernel or a sandbox can offer one and refuse the other, and
/// `inspect` should say which one an answer came from.
pub const SOCK_DIAG_PROVIDER: &str = "linux.sock-diag";
