//! The typed transport a remote Ono link speaks (spec §21).
//!
//! Spec §21.1 says what this is for: SSH stays available, and "a Ono-Sendai link adds persistent
//! metadata, provider negotiation and object-aware remote execution". The point of the link is
//! that *native operations execute over there* — a remote `Process` arrives as the same record a
//! local one is, with its schema, its units, its nulls and its provenance, rather than as text to
//! be parsed back into a shape.
//!
//! This crate is the transport that makes that possible, and nothing above it. Link management,
//! the agentless SSH fallback of spec §21.3 and the agent binary of spec §21.4 are built on it.
//!
//! # The four things it does
//!
//! **A framed wire** ([`Frame`], [`encode`], [`decode`]). Twelve bytes of header — version, kind,
//! flags, stream id, length — then a payload. Every length is checked against [`Limits`] before
//! anything is allocated, because a length field from another machine is a claim, not an
//! instruction (ADR-0015 T7).
//!
//! **A handshake** ([`Hello`], [`Accept`], [`Negotiated`]). One round trip settles version,
//! providers with their availability, capabilities, compression, identity and the credit window
//! (spec §21.2).
//!
//! **Multiplexed, backpressured streams** ([`Link`], [`RemoteStream`], [`StreamResponder`]).
//! Many concurrent queries share one connection, each independently cancellable, each bounded by
//! a credit window so that a remote producer cannot outrun a local consumer — spec §11.2's
//! requirement, applied across a machine boundary.
//!
//! **Pinned host identity** ([`TrustStore`], [`HostKey`], [`TrustPolicy`]). A first connection
//! records the peer's key; a peer that later presents a different one is refused with
//! `remote.host_key_changed`, and there is no way to say "continue anyway" (ADR-0015 T5, T6).
//!
//! # What a deployment must supply
//!
//! Not the cryptography. A [`Transport`] is a byte stream that has already authenticated and
//! encrypted itself and can name the key the peer proved it holds; TLS, Noise or an SSH channel
//! all fit behind it. See the [`transport`] module for what an implementation owes.
//!
//! # Driving a link
//!
//! ```no_run
//! use ono_protocol::{ClientConfig, Link, UnauthenticatedTransport, RemoteMessage, RemoteQuery};
//!
//! # async fn example(stream: tokio::io::DuplexStream) -> Result<(), ono_value::ErrorValue> {
//! let link = Link::connect(UnauthenticatedTransport::new(stream), ClientConfig::new("db.example.com")).await?;
//! let mut processes = link.query(&RemoteQuery::target("process").limit(10))?;
//! while let Some(message) = processes.recv().await {
//!     if let RemoteMessage::Value(value) = message {
//!         println!("{value:?}");
//!     }
//! }
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod connection;
mod error;
mod frame;
mod handshake;
mod limits;
mod link;
mod message;
mod service;
mod trust;

pub mod transport;

pub use error::ProtocolError;
pub use frame::{FRAME_HEADER_LEN, FRAME_VERSION, Frame, FrameKind, decode, encode};
pub use handshake::{
    Accept, CapabilityDescriptor, Hello, Identity, Negotiated, PROTOCOL_VERSION, PeerInfo,
    ProviderDescriptor, Reject,
};
pub use limits::{
    DEFAULT_CREDIT, Limits, MAX_CREDIT, MAX_FRAME_PAYLOAD, MAX_STREAMS, MAX_VALUE_DEPTH,
};
pub use link::{ClientConfig, Link, RemoteMessage, RemoteStream};
pub use message::{ActRequest, AdaptRequest, Message, RemoteQuery, decode_message, encode_message};
pub use service::{RemoteService, ServerConfig, StreamResponder, serve};
pub use transport::{Transport, UnauthenticatedTransport};
pub use trust::{Fingerprint, HostKey, TrustDecision, TrustEntry, TrustPolicy, TrustStore};
