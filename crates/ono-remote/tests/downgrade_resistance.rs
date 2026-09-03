//! What a v0.4.1 client does when the far side offers something weaker (v0.4.1 spec §4.2, §13.2,
//! §13.3, §13.4; issue #38).
//!
//! The direct transport used to name itself `ono/1` and to authenticate one end. v0.4.1 names the
//! mutual-authentication contract `ono/2` and refuses to fall back out of it:
//!
//! > If a v0.4.1 direct client encounters a server that does not support mutual client
//! > authentication, it MUST fail. It MUST NOT retry with no client certificate. (§13.3)
//!
//! The suites below stand a v0.4.0-shaped server on the loopback interface — `ono/1`, no client
//! certificate requested — and watch what the current client does with it. The stand-in is built
//! from rustls directly rather than from an old binary, because what is being tested is the
//! client's refusal and not the archaeology of the server.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::within;
use ono_core::ErrorCode;
use ono_protocol::Transport as _;
use ono_remote::{PeerIdentity, tls_connect};

/// Whether a stand-in server asks its peers to prove who they are.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ClientAuth {
    /// v0.4.0: the listening side authenticates nobody.
    Off,
    /// v0.4.1: a client that proves nothing is refused by the handshake.
    Required,
}

/// A server that is not Ono, so a test can choose what it offers.
struct StandIn {
    address: String,
    /// How many TCP connections it has accepted. §13.3 forbids a second attempt.
    connections: Arc<AtomicUsize>,
}

/// Anything that connects is who it says it is, which is the *server's* half and not the subject.
#[derive(Debug)]
struct AcceptsAnyClient {
    provider: Arc<rustls::crypto::CryptoProvider>,
    hints: Vec<rustls::DistinguishedName>,
}

impl rustls::server::danger::ClientCertVerifier for AcceptsAnyClient {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &self.hints
    }

    fn verify_client_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
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
        cert: &rustls::pki_types::CertificateDer<'_>,
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

/// Stands a TLS server on the loopback interface offering exactly `alpn`, asking for a client
/// certificate only when `auth` says so, and counting the connections it is given.
async fn stand_in(alpn: &[&[u8]], auth: ClientAuth) -> StandIn {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    // The stand-in needs its own key, and `PeerIdentity` deliberately never hands one out, so it
    // makes a pair of its own rather than taking one apart.
    let generated = rcgen::generate_simple_self_signed(vec!["ono.invalid".to_owned()])
        .expect("a key pair is generated");
    let certificate = rustls::pki_types::CertificateDer::from(generated.cert.der().to_vec());
    let key = rustls::pki_types::PrivateKeyDer::from(rustls::pki_types::PrivatePkcs8KeyDer::from(
        generated.key_pair.serialize_der(),
    ));
    let builder = rustls::ServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3 is available");
    let builder = match auth {
        ClientAuth::Off => builder.with_no_client_auth(),
        ClientAuth::Required => builder.with_client_cert_verifier(Arc::new(AcceptsAnyClient {
            provider,
            hints: Vec::new(),
        })),
    };
    let mut config = builder
        .with_single_cert(vec![certificate], key)
        .expect("the stand-in can present its own certificate");
    config.alpn_protocols = alpn.iter().map(|token| token.to_vec()).collect();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback listener binds");
    let address = listener
        .local_addr()
        .expect("the system reports the port it chose")
        .to_string();
    let connections = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&connections);
    tokio::spawn(async move {
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        while let Ok((stream, _)) = listener.accept().await {
            counted.fetch_add(1, Ordering::SeqCst);
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let _ = acceptor.accept(stream).await;
            });
        }
    });
    StandIn {
        address,
        connections,
    }
}

fn identity() -> PeerIdentity {
    PeerIdentity::generate().expect("a client identity is generated")
}

/// v0.4.1 §13.4: the token names the contract, and the contract changed. A client that still
/// offered `ono/1` would be indistinguishable, to a server, from one that authenticates nothing.
#[tokio::test]
async fn should_speak_the_mutual_authentication_token_and_no_older_one() {
    let current = stand_in(&[b"ono/2"], ClientAuth::Required).await;
    let previous = stand_in(&[b"ono/1"], ClientAuth::Off).await;

    let spoken = within(tls_connect(&current.address, &identity())).await;
    let refused = within(tls_connect(&previous.address, &identity())).await;

    assert!(
        spoken.is_ok(),
        "`ono/2` is the token this client offers, and a server offering it is spoken to: {:?}",
        spoken.err()
    );
    assert!(
        refused.is_err(),
        "a server that only offers `ono/1` is a server from before mutual authentication existed, \
         and §13.3 says a v0.4.1 client meeting one MUST fail"
    );
}

/// v0.4.1 §13.3 and §4.2: the refusal is stable, non-retryable and says what happened, because
/// the person who hits it is upgrading a fleet and needs to know which end is old.
#[tokio::test]
async fn should_refuse_a_server_that_does_not_ask_for_a_client_certificate() {
    let previous = stand_in(&[b"ono/2"], ClientAuth::Off).await;

    let refusal = within(tls_connect(&previous.address, &identity()))
        .await
        .expect_err("a server that never asked who we are cannot have authenticated us");

    assert_eq!(
        refusal.code(),
        ErrorCode::RemotePeerUnauthenticated,
        "§4.2 asks for `remote.protocol_mismatch` or `a new stable authentication-specific \
         error before any provider operation`, and this is the authentication-specific one"
    );
    assert!(
        refusal.retryable() != Some(true),
        "§13.3: it MUST NOT retry with no client certificate, so the error must not invite one"
    );
    let said = format!(
        "{} {}",
        refusal.message(),
        refusal.help().unwrap_or_default()
    );
    assert!(
        said.contains("certificate") || said.contains("authentic"),
        "the diagnostic has to name what was missing, got {said}"
    );
}

/// v0.4.1 §13.3, the sentence that has to be observable rather than asserted: "it MUST NOT retry
/// with no client certificate". A retry would show up as a second TCP connection.
#[tokio::test]
async fn should_not_try_again_after_a_server_refuses_mutual_authentication() {
    let previous = stand_in(&[b"ono/1"], ClientAuth::Off).await;

    let _ = within(tls_connect(&previous.address, &identity())).await;
    common::settle().await;

    assert_eq!(
        previous.connections.load(Ordering::SeqCst),
        1,
        "one attempt, one connection. A fallback path would show here as a second one, made \
         without the certificate the first one carried"
    );
}

/// The server's half of §13.2: a peer cannot ask for the older, one-sided protocol once this
/// listener is up, because the listener does not offer that name at all.
#[tokio::test]
async fn should_refuse_a_client_that_asks_for_the_older_protocol_token() {
    let agent = PeerIdentity::generate().expect("an agent identity is generated");
    let listener = ono_remote::TlsListener::bind("127.0.0.1:0", &agent)
        .await
        .expect("a loopback listener binds");
    let address = listener
        .local_addr()
        .expect("the system reports the port it chose")
        .to_string();
    tokio::spawn(async move { listener.accept().await });

    let refused = within(connect_offering(&address, &[b"ono/1"])).await;

    assert!(
        refused.is_err(),
        "§13.2: a peer MUST NOT be able to request a legacy unauthenticated protocol mode; the \
         name of that mode is not on offer — {refused:?}"
    );
}

/// Dials `address` as a well-formed Ono client except for the ALPN tokens it offers.
async fn connect_offering(address: &str, alpn: &[&[u8]]) -> Result<(), String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let generated = rcgen::generate_simple_self_signed(vec!["ono.invalid".to_owned()])
        .expect("a key pair is generated");
    let certificate = rustls::pki_types::CertificateDer::from(generated.cert.der().to_vec());
    let key = rustls::pki_types::PrivateKeyDer::from(rustls::pki_types::PrivatePkcs8KeyDer::from(
        generated.key_pair.serialize_der(),
    ));
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| format!("TLS 1.3 is unavailable: {error}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptsAnyServer { provider }))
        .with_client_auth_cert(vec![certificate], key)
        .map_err(|error| format!("the certificate was not usable: {error}"))?;
    config.alpn_protocols = alpn.iter().map(|token| token.to_vec()).collect();

    let stream = tokio::net::TcpStream::connect(address)
        .await
        .map_err(|error| format!("the port did not answer: {error}"))?;
    let name = rustls::pki_types::ServerName::try_from("ono.invalid")
        .map_err(|error| format!("{error}"))?;
    tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, stream)
        .await
        .map_err(|error| format!("the TLS handshake was refused: {error}"))?;
    Ok(())
}

/// The subject here is the ALPN token, so the client makes no demand of the server's certificate.
#[derive(Debug)]
struct AcceptsAnyServer {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for AcceptsAnyServer {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
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
        cert: &rustls::pki_types::CertificateDer<'_>,
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

/// The far side of a v0.4.1 link proves its key too, so a connection that succeeded knows one.
#[tokio::test]
async fn should_know_the_key_of_a_server_it_did_agree_to_speak_to() {
    let current = stand_in(&[b"ono/2"], ClientAuth::Required).await;

    let transport = within(tls_connect(&current.address, &identity()))
        .await
        .expect("a server offering the mutual-authentication token is spoken to");

    assert!(
        transport.peer_key().is_some(),
        "§7.1's symmetry is both ways: the client verified the server's certificate in the same \
         handshake the server verified the client's"
    );
}
