//! Where the cryptography plugs in.
//!
//! Spec §21.5 requires that "remote agent mode MUST use authenticated encryption, explicit host
//! trust and least privilege". This crate implements the second and constrains the third; the
//! first is deliberately *not* implemented here.
//!
//! # What a deployment must supply
//!
//! A [`Transport`] is a byte stream that has already authenticated and encrypted itself, plus one
//! question it can answer: **which key did the peer prove it holds?** That is the whole interface
//! the trust store needs. A real deployment supplies an implementation over TLS 1.3, a Noise
//! session, or an SSH channel, and returns the peer's certified public key from
//! [`Transport::peer_key`]. Nothing else in this crate changes.
//!
//! The split is deliberate. Key exchange and record encryption are the parts of a secure channel
//! that must not be written twice, and the parts a shell has no business inventing; pinning,
//! negotiation and stream discipline are the parts that are specific to Ono. Keeping the boundary
//! at "a byte stream that knows who is on the other end" means the pinning of ADR-0015 T5 and T6
//! is testable over an in-memory duplex, with no key material and no clock.
//!
//! A transport that answers `None` has authenticated nobody. That is refused by default and
//! accepted only under [`TrustPolicy::Unauthenticated`](crate::TrustPolicy::Unauthenticated),
//! which a caller has to ask for by name.

use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::HostKey;

/// A byte stream to a peer, and what the transport authenticated about it.
pub trait Transport: AsyncRead + AsyncWrite + Send + Unpin + 'static {
    /// The key the peer proved it holds, or `None` when the transport authenticated nobody.
    ///
    /// This must be what the transport *verified*, never what the peer merely claimed: a key
    /// asserted inside the protocol would prove nothing, because an impersonator can assert one
    /// too. That is why the question is asked of the transport rather than of the handshake.
    fn peer_key(&self) -> Option<&HostKey>;
}

/// A transport over a byte stream that authenticates nobody, and says so in its name.
///
/// It is the adapter for a stream whose protection comes from somewhere else — a channel inside
/// an already-authenticated SSH connection, a unix socket in the shell's own runtime directory —
/// and it is what the test suites run over. [`with_peer_key`](Self::with_peer_key) sets what such
/// an outer layer authenticated, so the trust store sees exactly what it would see in production;
/// without it, [`peer_key`](Transport::peer_key) is `None` and only
/// [`TrustPolicy::Unauthenticated`](crate::TrustPolicy::Unauthenticated) will carry a link over
/// it.
///
/// The name is required rather than chosen. v0.4.1 §7.4: "if an unauthenticated transport remains
/// necessary for tests or in-process duplexes, it MUST be inaccessible from ordinary network CLI
/// configuration and clearly named `Unauthenticated` in internal APIs." §65.1 says why — calling a
/// session authenticated because it is encrypted is the mistake, and a type called `Plain` is
/// where somebody reaches for the wrong one (ADR-0440).
///
/// ```
/// use ono_protocol::{UnauthenticatedTransport, Transport};
/// let (near, _far) = tokio::io::duplex(64);
/// assert!(UnauthenticatedTransport::new(near).peer_key().is_none());
/// ```
#[derive(Debug)]
pub struct UnauthenticatedTransport<S> {
    stream: S,
    peer_key: Option<HostKey>,
}

impl<S> UnauthenticatedTransport<S> {
    /// A transport over `stream` that authenticates nobody.
    #[must_use]
    pub const fn new(stream: S) -> Self {
        Self {
            stream,
            peer_key: None,
        }
    }

    /// Declares the key an outer layer authenticated for this peer.
    #[must_use]
    pub fn with_peer_key(mut self, key: HostKey) -> Self {
        self.peer_key = Some(key);
        self
    }
}

impl<S: AsyncRead + AsyncWrite + Send + Unpin + 'static> Transport for UnauthenticatedTransport<S> {
    fn peer_key(&self) -> Option<&HostKey> {
        self.peer_key.as_ref()
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for UnauthenticatedTransport<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for UnauthenticatedTransport<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
