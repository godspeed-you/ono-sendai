//! The transport that certifies its peer to *this* process (spec §21.5, ADR-0274).
//!
//! Spec §21.5: "Remote agent mode MUST use authenticated encryption, explicit host trust and
//! least privilege." `ono-protocol` implements the second and constrains the third, and asks a
//! [`Transport`] one question for the first: **which key did the peer prove it holds?**
//! [`SubprocessTransport`](crate::SubprocessTransport) can only answer `None` — OpenSSH verified
//! the host inside its own `known_hosts` and offers the parent process no way to learn the key
//! (ADR-0037 §4) — so until now the trust store of ADR-0015 T5/T6 was never consulted on a
//! production path and `remote.host_key_changed` was unreachable outside a test.
//!
//! This module is the transport that closes that gap, and it is deliberately the one ADR-0274
//! named: TCP with TLS 1.3, where **the peer's certificate is the host key**.
//!
//! # Why this is an honest answer to "which key did the peer prove it holds"
//!
//! TLS 1.3's `CertificateVerify` is a signature, made during this handshake, over a transcript of
//! this handshake, with the private key belonging to the certificate the peer sent. Verifying it
//! is something *this* process does, with bytes it saw itself, so [`Transport::peer_key`] can
//! report the certificate as a fact rather than as somebody else's claim. That verification is
//! performed by rustls — key exchange and record protection are exactly the parts a shell has no
//! business writing, which is why `ono-protocol` left them outside the crate in the first place.
//!
//! # What is *not* verified, and why that is not a gap
//!
//! There is no certificate authority and no name checking. A public key infrastructure answers
//! "does somebody I trust vouch for this name?", and Ono's answer to host identity is the pinned
//! key of §21.5 — "explicit host trust" — which answers "is this the key this host had last
//! time?" Layering a name check on a self-signed certificate would check nothing, so the
//! certificate is used for what it demonstrably is: a public key the peer proved it holds. The
//! whole certificate is the [`HostKey`], so re-issuing one is a deliberate re-pin.
//!
//! # Both ends do it, and the listening side is not exempt
//!
//! Until v0.4.1 only the connecting side asked the question. The listener presented a
//! certificate, asked for none, and everything downstream — the Ono `Hello`, the provider
//! inventory, capabilities, actions — was reached by whoever dialled the port; the `Identity` the
//! peer sent was a string it chose about itself, which v0.4.1 §2.1 forbids from satisfying the
//! word *authenticated*. §7.1 makes the transport symmetric: "both endpoints MUST present a
//! certificate and prove possession of the corresponding private key during TLS 1.3
//! negotiation", and §13.1 fixes when — "mutual TLS MUST complete before an Ono `Hello` frame is
//! accepted". So [`TlsListener::bind`] requires a client certificate, [`TlsListener::accept`]
//! reports it as the peer key, and [`connect`] takes the identity it will present rather than
//! having a way not to present one.
//!
//! # What a deployment does
//!
//! Each side keeps a [`PeerIdentity`] in a file it owns and pins the other's fingerprint.
//! Nothing is trusted on the strength of being reachable.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use ono_core::ErrorCode;
use ono_protocol::{HostKey, Transport};
use ono_value::ErrorValue;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};

use crate::identity::{ALGORITHM, PeerIdentity};

/// The port an Ono agent listens on when nobody says otherwise.
///
/// Not registered with IANA and not claimed to be: it is a default, and every command that takes
/// an address takes a port with it.
pub const DEFAULT_PORT: u16 = 7734;

/// The application the two ends agree they are speaking, negotiated in the TLS handshake.
///
/// A peer that speaks something else is refused before a frame crosses, rather than confusing
/// the link protocol's own version negotiation with a different protocol entirely.
///
/// `ono/2` since v0.4.1, and the number is the whole point: §13.4 asks the transport to "advance
/// from the existing `ono/1` ALPN token to a token that unambiguously represents the
/// mutual-authentication contract". `ono/1` named a link where one end proved who it was. This
/// one names a link where both do, and the name is settled inside the TLS handshake, before the
/// link protocol's own version negotiation can be reached (§13.2). Only this token is offered and
/// only this token is accepted; a peer that speaks the older one meets §13.3 (ADR-0439).
const ALPN: &[u8] = b"ono/2";

/// The cryptography this transport uses, chosen once and by name.
///
/// `ring` rather than the default provider, so the build needs no assembler or C toolchain
/// beyond what a Rust build already has — an agent has to be installable on the machine it is
/// meant to run on.
fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    static PROVIDER: std::sync::OnceLock<Arc<rustls::crypto::CryptoProvider>> =
        std::sync::OnceLock::new();
    Arc::clone(PROVIDER.get_or_init(|| Arc::new(rustls::crypto::ring::default_provider())))
}

/// The verifier that makes the pin the trust anchor.
///
/// It performs the one check that proves possession — the handshake signature — and performs no
/// path building and no name check, because there is no authority to build a path to and the
/// name is not what identifies the host. What it deliberately does *not* do is decide whether the
/// peer is the right peer: that is [`TrustStore`](ono_protocol::TrustStore)'s decision, made
/// above, on the key this verifier has established the peer really holds.
#[derive(Debug)]
struct PinIsTheAnchor {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for PinIsTheAnchor {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// The verifier that makes the *client's* proof the whole of the check (v0.4.1 §7.1, §2.1).
///
/// The mirror image of [`PinIsTheAnchor`], and mirror-image reasoning: there is no authority to
/// build a path to and no name to check, so what a listening agent demands of a peer is the one
/// thing that cannot be faked — a `CertificateVerify` signature over this handshake's transcript,
/// made with the key inside the certificate the peer just sent. rustls performs it and refuses
/// the handshake when it does not hold, which is why nothing here has to.
///
/// What this verifier deliberately does *not* do is decide whether the client is an *allowed*
/// client. §9.1 is explicit that "a valid client certificate proves only that the connecting
/// process holds a private key", and the `authorized_clients` store that decides the rest is
/// phase H2's. §56.1 puts the boundary in the same place: `ono-remote` carries authenticated
/// identity and "no authorization policy semantics".
#[derive(Debug)]
struct ProofIsTheWholeCheck {
    provider: Arc<rustls::crypto::CryptoProvider>,
    /// No hints: there are no certificate authorities, so there is nothing to hint at, and RFC
    /// 8446 reads an empty list as "send whatever certificate you have".
    hints: Vec<rustls::DistinguishedName>,
}

impl rustls::server::danger::ClientCertVerifier for ProofIsTheWholeCheck {
    fn offer_client_auth(&self) -> bool {
        true
    }

    /// v0.4.1 §7.4: the canonical agent has no mode in which this is false.
    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &self.hints
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        // rustls hands these bytes over unparsed and asks the implementer to handle invalid data.
        // Bytes that are not a certificate carry no public key, so no proof of possession can
        // exist for them and the handshake is over here rather than at the signature.
        rustls::server::ParsedCertificate::try_from(end_entity)?;
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// The client's certificate, and a record of whether the server ever asked for it.
///
/// rustls calls [`resolve`](rustls::client::ResolvesClientCert::resolve) exactly when the server
/// sends a `CertificateRequest`, so this flag is the one observation available to a client about
/// whether the far side intends to authenticate it at all. v0.4.1 §13.3 requires the client to
/// fail when it meets a server that does not support mutual client authentication, and without
/// this the client would complete a handshake, send an Ono `Hello` and be answered by an agent
/// that never learned who it was talking to (ADR-0439).
#[derive(Debug)]
struct PresentsThisIdentity {
    certified: Arc<rustls::sign::CertifiedKey>,
    asked: Arc<AtomicBool>,
}

impl rustls::client::ResolvesClientCert for PresentsThisIdentity {
    fn resolve(
        &self,
        _root_hint_subjects: &[&[u8]],
        _schemes: &[rustls::SignatureScheme],
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        self.asked.store(true, Ordering::SeqCst);
        Some(Arc::clone(&self.certified))
    }

    fn has_certs(&self) -> bool {
        true
    }
}

/// The certificate the completed handshake proved its peer holds, as a [`HostKey`].
///
/// `None` only where a handshake completed with no peer certificate at all, which neither end of
/// this transport permits any more; the trust decision above refuses that case by policy rather
/// than trusting this function to be unreachable.
fn proved_key(certificates: Option<&[CertificateDer<'static>]>) -> Option<HostKey> {
    certificates
        .and_then(<[CertificateDer<'_>]>::first)
        .map(|certificate| HostKey::new(ALGORITHM, certificate.as_ref().to_vec()))
}

/// A link transport over TLS 1.3, and the key the peer proved it holds.
#[derive(Debug)]
pub struct TlsTransport<S> {
    stream: S,
    peer_key: Option<HostKey>,
}

impl<S: AsyncRead + AsyncWrite + Send + Unpin + 'static + std::fmt::Debug> Transport
    for TlsTransport<S>
{
    fn peer_key(&self) -> Option<&HostKey> {
        self.peer_key.as_ref()
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for TlsTransport<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for TlsTransport<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

/// Connects to `address`, presenting `identity`, and completes the mutual TLS handshake.
///
/// The returned transport reports the peer's certificate as its host key, which is what the trust
/// store then decides about (spec §21.5). `address` is `host` or `host:port`; without a port,
/// [`DEFAULT_PORT`].
///
/// `identity` is not optional and there is no sibling that omits it. v0.4.1 §13.3 forbids
/// retrying without a client certificate, and the cheapest way to keep a rule like that is to
/// leave no expressible way to break it (ADR-0439).
///
/// # Errors
///
/// `remote.unreachable` (E0601) when the address cannot be resolved, the connection refused, or
/// the TLS handshake not completed.
pub async fn connect(
    address: &str,
    identity: &PeerIdentity,
) -> Result<TlsTransport<tokio_rustls::client::TlsStream<TcpStream>>, ErrorValue> {
    let (host, port) = split_address(address);
    let (certificate, key) = identity.material();
    let certified = rustls::sign::CertifiedKey::from_der(vec![certificate], key, &provider())
        .map_err(|error| {
            ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                format!("this peer identity cannot be presented: {error}"),
            )
        })?;
    let asked = Arc::new(AtomicBool::new(false));
    let mut config = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| unreachable_error(address, &format!("TLS 1.3 is unavailable: {error}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinIsTheAnchor {
            provider: provider(),
        }))
        .with_client_cert_resolver(Arc::new(PresentsThisIdentity {
            certified: Arc::new(certified),
            asked: Arc::clone(&asked),
        }));
    config.alpn_protocols = vec![ALPN.to_vec()];

    let stream = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|error| unreachable_error(address, &error.to_string()))?;
    // The name is not what identifies the peer here (see `PinIsTheAnchor`), so it is the fixed
    // name every Ono host identity carries rather than something that pretends to be checked.
    let name = ServerName::try_from("ono.invalid")
        .map_err(|error| unreachable_error(address, &format!("{error}")))?;
    let stream = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await
        .map_err(|error| handshake_error(address, &error))?;

    if !asked.load(Ordering::SeqCst) {
        // §13.3: the far side completed a handshake without ever asking who we are, so nothing
        // it does with this link can be authorized to this identity. There is no second attempt
        // to make — the only thing a retry could drop is the certificate — so this is the end.
        return Err(unauthenticated_peer(
            address,
            "it completed the handshake without asking for a client certificate",
        ));
    }
    let peer_key = proved_key(stream.get_ref().1.peer_certificates());
    Ok(TlsTransport { stream, peer_key })
}

/// An agent's listening socket: one TLS 1.3 endpoint presenting this host's identity.
pub struct TlsListener {
    listener: TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
}

impl std::fmt::Debug for TlsListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsListener")
            .field("address", &self.listener.local_addr().ok())
            .finish_non_exhaustive()
    }
}

impl TlsListener {
    /// Binds `address` and presents `identity` to everyone who connects.
    ///
    /// # Errors
    ///
    /// `remote.unreachable` (E0601) when the address cannot be bound, and
    /// `io.permission_denied` when the identity is not one TLS can present.
    pub async fn bind(address: &str, identity: &PeerIdentity) -> Result<Self, ErrorValue> {
        let (host, port) = split_address(address);
        let (certificate, key) = identity.material();
        let mut config = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|error| {
                unreachable_error(address, &format!("TLS 1.3 is unavailable: {error}"))
            })?
            .with_client_cert_verifier(Arc::new(ProofIsTheWholeCheck {
                provider: provider(),
                hints: Vec::new(),
            }))
            .with_single_cert(vec![certificate], key)
            .map_err(|error| {
                ErrorValue::new(
                    ErrorCode::IoPermissionDenied,
                    format!("this peer identity cannot be presented: {error}"),
                )
            })?;
        config.alpn_protocols = vec![ALPN.to_vec()];

        let listener = TcpListener::bind((host.as_str(), port))
            .await
            .map_err(|error| unreachable_error(address, &error.to_string()))?;
        Ok(Self {
            listener,
            acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(config)),
        })
    }

    /// Where the listener actually is, which is how a caller learns a port it asked the system
    /// to choose.
    ///
    /// # Errors
    ///
    /// `remote.unreachable` (E0601) when the socket has no address the system will report.
    pub fn local_addr(&self) -> Result<SocketAddr, ErrorValue> {
        self.listener
            .local_addr()
            .map_err(|error| unreachable_error("the listening socket", &error.to_string()))
    }

    /// Waits for one peer and completes its TLS handshake.
    ///
    /// # Errors
    ///
    /// `remote.unreachable` (E0601) when the connection cannot be accepted or the handshake
    /// fails — a peer speaking something other than this protocol, most often.
    pub async fn accept(
        &self,
    ) -> Result<TlsTransport<tokio_rustls::server::TlsStream<TcpStream>>, ErrorValue> {
        let (stream, from) = self
            .listener
            .accept()
            .await
            .map_err(|error| unreachable_error("a peer", &error.to_string()))?;
        let stream = self.acceptor.accept(stream).await.map_err(|error| {
            unreachable_error(
                &from.to_string(),
                &format!("the TLS handshake failed: {error}"),
            )
        })?;
        // The handshake completed, so the client presented a certificate and signed this
        // handshake's transcript with the key inside it (v0.4.1 §7.1). That is what the peer key
        // is: something this process verified, not something the peer said about itself.
        let peer_key = proved_key(stream.get_ref().1.peer_certificates());
        Ok(TlsTransport { stream, peer_key })
    }
}

/// `host` or `host:port`, with [`DEFAULT_PORT`] where no port is written.
///
/// An IPv6 literal is written in brackets, as everywhere else a host and a port share a string.
#[must_use]
pub fn split_address(address: &str) -> (String, u16) {
    if let Some(rest) = address.strip_prefix('[')
        && let Some((literal, tail)) = rest.split_once(']')
    {
        let port = tail
            .strip_prefix(':')
            .and_then(|port| port.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        return (literal.to_owned(), port);
    }
    match address.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !host.contains(':') => {
            (host.to_owned(), port.parse().unwrap_or(DEFAULT_PORT))
        }
        _ => (address.to_owned(), DEFAULT_PORT),
    }
}

/// The refusal for a peer that will not or cannot prove it holds a key (v0.4.1 §13.3, §4.2).
///
/// Never retryable, and deliberately carrying no way forward: the only thing a second attempt
/// could change is whether a certificate is offered, and dropping it is exactly what §13.3
/// forbids.
fn unauthenticated_peer(address: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RemotePeerUnauthenticated,
        format!("{address} did not authenticate this link: {detail}"),
    )
    .with_retryable(false)
    .with_help(
        "a direct link is mutually authenticated from v0.4.1 on, and never falls back to an \
         unauthenticated one (§7.1, §13.3). The far side is most likely an older agent: upgrade \
         it, or reach it over `--transport ssh`, where OpenSSH authenticates the host instead.",
    )
}

/// A failed TLS handshake, as the reason it failed rather than as a connection problem.
///
/// The one distinction that matters to a person is between "nothing is listening the way I
/// expected" and "the far side speaks an older protocol than this one" — §4.2 asks for the second
/// to arrive as a stable authentication-specific error, and a peer that answers with
/// `no_application_protocol` to an offer of `ono/2` is exactly that peer.
fn handshake_error(address: &str, error: &io::Error) -> ErrorValue {
    let alpn_refused = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        .is_some_and(|error| {
            matches!(
                error,
                rustls::Error::AlertReceived(rustls::AlertDescription::NoApplicationProtocol)
                    | rustls::Error::NoApplicationProtocol
            )
        });
    if alpn_refused {
        return unauthenticated_peer(
            address,
            "it does not speak `ono/2`, the protocol in which both ends prove who they are",
        );
    }
    unreachable_error(address, &format!("the TLS handshake failed: {error}"))
}

fn unreachable_error(address: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RemoteUnreachable,
        format!("{address} could not be reached over the ono transport: {detail}"),
    )
}
