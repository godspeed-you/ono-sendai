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
//! # What a deployment does
//!
//! The listening side keeps a [`HostIdentity`] in a file it owns; the connecting side pins its
//! fingerprint in the trust store. Nothing is trusted on the strength of being reachable.

use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use ono_core::ErrorCode;
use ono_protocol::{Fingerprint, HostKey, Transport};
use ono_value::ErrorValue;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};

/// The port an Ono agent listens on when nobody says otherwise.
///
/// Not registered with IANA and not claimed to be: it is a default, and every command that takes
/// an address takes a port with it.
pub const DEFAULT_PORT: u16 = 7734;

/// The application the two ends agree they are speaking, negotiated in the TLS handshake.
///
/// A peer that speaks something else is refused before a frame crosses, rather than confusing
/// the link protocol's own version negotiation with a different protocol entirely.
const ALPN: &[u8] = b"ono/1";

/// The algorithm name the trust store records for a key proved through TLS.
///
/// The material is the peer's end-entity certificate, so the name says so: what was pinned is a
/// certificate, and a re-issued certificate is a new key to Ono even when the key inside it is
/// the same one. That is the strict reading, and the one a person can check by hand.
const ALGORITHM: &str = "tls-x509";

/// A listening agent's own identity: the certificate it presents and the key it proves it holds.
#[derive(Debug)]
pub struct HostIdentity {
    certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
}

impl HostIdentity {
    /// The identity recorded in `path`, generating and writing one when the file is not there.
    ///
    /// The file holds both PEM blocks and is written with owner-only permissions, because it
    /// contains the private key that *is* this host's identity to everyone who pinned it.
    ///
    /// # Errors
    ///
    /// `io.permission_denied` when the file cannot be read or written, and `parse.syntax` when it
    /// is not the two PEM blocks this format defines.
    pub fn open_or_create(path: &Path) -> Result<Self, ErrorValue> {
        match std::fs::read_to_string(path) {
            Ok(pem) => Self::from_pem(&pem, path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let pem = generate_pem()?;
                write_private(path, &pem)?;
                Self::from_pem(&pem, path)
            }
            Err(error) => Err(io_error(path, &error)),
        }
    }

    /// An identity that lives only as long as this process.
    ///
    /// # Errors
    ///
    /// `io.permission_denied` when a key cannot be generated.
    pub fn generate() -> Result<Self, ErrorValue> {
        let pem = generate_pem()?;
        Self::from_pem(&pem, Path::new("<generated>"))
    }

    fn from_pem(pem: &str, path: &Path) -> Result<Self, ErrorValue> {
        let malformed = |detail: &str| {
            ErrorValue::new(
                ErrorCode::ParseSyntax,
                format!("{}: {detail}", path.display()),
            )
            .with_help(
                "a host identity is one CERTIFICATE block and one PRIVATE KEY block; remove the \
                 file to have a new identity generated, which every peer that pinned this host \
                 will then refuse until it is re-pinned",
            )
        };
        let mut reader = io::BufReader::new(pem.as_bytes());
        let certificate = rustls_pemfile::certs(&mut reader)
            .next()
            .transpose()
            .map_err(|error| malformed(&format!("the certificate is unreadable: {error}")))?
            .ok_or_else(|| malformed("no CERTIFICATE block"))?;
        let mut reader = io::BufReader::new(pem.as_bytes());
        let key = rustls_pemfile::private_key(&mut reader)
            .map_err(|error| malformed(&format!("the private key is unreadable: {error}")))?
            .ok_or_else(|| malformed("no PRIVATE KEY block"))?;
        Ok(Self { certificate, key })
    }

    /// The key a peer will see this host prove it holds.
    #[must_use]
    pub fn host_key(&self) -> HostKey {
        HostKey::new(ALGORITHM, self.certificate.as_ref().to_vec())
    }

    /// The fingerprint a person pins this host by.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        self.host_key().fingerprint()
    }
}

/// A self-signed certificate and its key, as two PEM blocks.
fn generate_pem() -> Result<String, ErrorValue> {
    // The name in the certificate is not what identifies the host — the pinned key is (see the
    // module documentation) — so it is a fixed, obviously non-resolvable name rather than
    // something that looks like a claim about DNS.
    let generated =
        rcgen::generate_simple_self_signed(vec!["ono.invalid".to_owned()]).map_err(|error| {
            ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                format!("this host's identity could not be generated: {error}"),
            )
        })?;
    Ok(format!(
        "{}{}",
        generated.cert.pem(),
        generated.key_pair.serialize_pem()
    ))
}

/// Writes `contents` to `path` so that only its owner can read it.
fn write_private(path: &Path, contents: &str) -> Result<(), ErrorValue> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| io_error(path, &error))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| io_error(path, &error))?;
    file.sync_all().map_err(|error| io_error(path, &error))
}

fn io_error(path: &Path, error: &io::Error) -> ErrorValue {
    let code = match error.kind() {
        io::ErrorKind::NotFound => ErrorCode::IoNotFound,
        _ => ErrorCode::IoPermissionDenied,
    };
    ErrorValue::new(
        code,
        format!(
            "the host identity at {} is not usable: {error}",
            path.display()
        ),
    )
}

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

/// Connects to `address` and completes the TLS handshake.
///
/// The returned transport reports the peer's certificate as its host key, which is what the trust
/// store then decides about (spec §21.5). `address` is `host` or `host:port`; without a port,
/// [`DEFAULT_PORT`].
///
/// # Errors
///
/// `remote.unreachable` (E0601) when the address cannot be resolved, the connection refused, or
/// the TLS handshake not completed.
pub async fn connect(
    address: &str,
) -> Result<TlsTransport<tokio_rustls::client::TlsStream<TcpStream>>, ErrorValue> {
    let (host, port) = split_address(address);
    let mut config = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| unreachable_error(address, &format!("TLS 1.3 is unavailable: {error}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinIsTheAnchor {
            provider: provider(),
        }))
        .with_no_client_auth();
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
        .map_err(|error| {
            unreachable_error(address, &format!("the TLS handshake failed: {error}"))
        })?;

    let peer_key = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(<[CertificateDer<'_>]>::first)
        .map(|certificate| HostKey::new(ALGORITHM, certificate.as_ref().to_vec()));
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
    pub async fn bind(address: &str, identity: &HostIdentity) -> Result<Self, ErrorValue> {
        let (host, port) = split_address(address);
        let mut config = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|error| {
                unreachable_error(address, &format!("TLS 1.3 is unavailable: {error}"))
            })?
            .with_no_client_auth()
            .with_single_cert(vec![identity.certificate.clone()], identity.key.clone_key())
            .map_err(|error| {
                ErrorValue::new(
                    ErrorCode::IoPermissionDenied,
                    format!("this host's identity cannot be presented: {error}"),
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
        // The listening side authenticates nobody: a client presents no certificate, and who it
        // is comes from the link protocol's own identity (spec §21.2), not from the transport.
        Ok(TlsTransport {
            stream,
            peer_key: None,
        })
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

fn unreachable_error(address: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RemoteUnreachable,
        format!("{address} could not be reached over the ono transport: {detail}"),
    )
}
