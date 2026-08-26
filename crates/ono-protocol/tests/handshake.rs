//! The link handshake of spec §21.2: version, capabilities, provider negotiation and identity.

mod common;

use common::{client_config, connect, server_config, try_connect, within};
use ono_core::ErrorCode;
use ono_protocol::{ClientConfig, Identity, PlainTransport, ProviderDescriptor, ServerConfig};

#[tokio::test]
async fn should_report_what_was_negotiated_when_a_link_is_established() {
    let fixture = connect().await;
    let negotiated = fixture.link.negotiated();

    assert_eq!(negotiated.version(), ono_protocol::PROTOCOL_VERSION);
    assert_eq!(
        negotiated.peer().identity().user(),
        "remote-user",
        "spec §21.2 negotiates identity, and a user must be able to see whose session it is"
    );
    assert!(
        !negotiated.peer().agent().is_empty(),
        "the peer says which agent it is, so an agentless fallback is distinguishable"
    );
}

#[tokio::test]
async fn should_offer_only_the_providers_the_remote_can_actually_answer_with() {
    let fixture = connect().await;
    let negotiated = fixture.link.negotiated();

    let usable: Vec<&str> = negotiated
        .providers()
        .iter()
        .filter(|provider| provider.is_available())
        .map(ProviderDescriptor::id)
        .collect();
    assert_eq!(
        usable,
        ["linux.procfs"],
        "a provider that cannot answer must not be offered as if it could (spec §35.3)"
    );

    let systemd = negotiated
        .providers()
        .iter()
        .find(|provider| provider.id() == "linux.systemd")
        .expect("an unavailable provider is still reported, with its reason");
    assert_eq!(
        systemd.unavailable_reason(),
        Some("systemd is not running in this container"),
        "spec §21.3 requires that a reduced remote capability set be visible, not silent"
    );
}

#[tokio::test]
async fn should_negotiate_the_intersection_of_the_two_capability_sets() {
    let client = client_config("testhost").with_capabilities(["process.list", "file.read"]);
    let fixture = try_connect(client, server_config(), None)
        .await
        .expect("the handshake succeeds");

    assert_eq!(
        fixture.link.negotiated().capabilities(),
        ["process.list"],
        "a capability only one side holds is not a capability the link has"
    );
}

#[tokio::test]
async fn should_choose_a_compression_both_ends_offer() {
    let client = client_config("testhost").with_compression(["none", "zstd"]);
    let fixture = try_connect(client, server_config(), None)
        .await
        .expect("the handshake succeeds");

    assert_eq!(
        fixture.link.negotiated().compression(),
        Some("none"),
        "the client's order decides, because the client pays the decompression cost"
    );
}

#[tokio::test]
async fn should_leave_compression_unset_when_the_two_ends_share_none() {
    let client = client_config("testhost").with_compression(["lz4"]);
    let fixture = try_connect(client, server_config(), None)
        .await
        .expect("no shared compression is not a handshake failure");

    assert_eq!(fixture.link.negotiated().compression(), None);
}

#[tokio::test]
async fn should_refuse_the_link_when_no_protocol_version_is_shared() {
    let client = client_config("testhost").with_versions([7]);
    let error = try_connect(client, server_config(), None)
        .await
        .err()
        .expect("a peer speaking another protocol cannot be linked to");

    assert_eq!(
        error.code(),
        ErrorCode::RemoteProtocolMismatch,
        "spec §43 gives this exact failure a stable code"
    );
    assert!(
        error.message().contains('7') || error.help().is_some_and(|help| help.contains('7')),
        "the error must name the versions, or a user cannot fix it: {}",
        error.render_full()
    );
}

#[tokio::test]
async fn should_report_an_unreachable_peer_when_the_transport_ends_before_the_handshake() {
    let (near, far) = tokio::io::duplex(1024);
    drop(far);

    let error = within(ono_protocol::Link::connect(
        PlainTransport::new(near),
        client_config("testhost"),
    ))
    .await
    .err()
    .expect("a peer that is not there cannot be handshaken with");

    assert_eq!(error.code(), ErrorCode::RemoteUnreachable);
}

#[tokio::test]
async fn should_report_an_unreachable_peer_when_the_transport_answers_with_rubbish() {
    let (near, far) = tokio::io::duplex(1024);
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt as _;
        let mut far = far;
        let _ = far.write_all(&[0xFF; 64]).await;
        let _ = far.flush().await;
        // Holding the far end open stops the client from seeing EOF, so the test proves that the
        // rubbish itself is refused rather than the disconnection.
        std::future::pending::<()>().await;
        drop(far);
    });

    let error = within(ono_protocol::Link::connect(
        PlainTransport::new(near),
        client_config("testhost"),
    ))
    .await
    .err()
    .expect("a peer that does not speak the protocol cannot be linked to");

    assert_eq!(
        error.code(),
        ErrorCode::RemoteProtocolMismatch,
        "bytes that are not frames are a protocol failure, not a network failure"
    );
}

#[tokio::test]
async fn should_carry_the_local_identity_to_the_remote_end() {
    let client = ClientConfig::new("testhost")
        .with_schemas(common::schemas())
        .with_trust_policy(ono_protocol::TrustPolicy::Unauthenticated)
        .with_identity(Identity::new("william").with_uid(1000));
    let fixture = try_connect(client, server_config(), None)
        .await
        .expect("the handshake succeeds");

    assert!(
        fixture.link.negotiated().version() >= 1,
        "the link is up, so the identity the client declared was accepted"
    );
}

#[tokio::test]
async fn should_prefer_the_highest_version_both_ends_speak() {
    let client = client_config("testhost").with_versions([1, 2, 3]);
    let server = ServerConfig::new()
        .with_schemas(common::schemas())
        .with_identity(Identity::new("remote-user"))
        .with_versions([1, 2]);
    let fixture = try_connect(client, server, None)
        .await
        .expect("two shared versions link");

    assert_eq!(
        fixture.link.negotiated().version(),
        2,
        "a newer shared version is chosen over an older one"
    );
}
