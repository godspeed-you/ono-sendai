//! What a handshake refusal carries to the peer it refuses (issue #182, ADR-1056; v0.4.1 §53.3,
//! §54.1, §54.2, §59.1).
//!
//! `docs/contracts/hardening/refusals.yaml` declares, per refusal, the metadata keys that explain
//! it (`explains`) and the subset of them a handshake refusal may carry over the link
//! (`over_the_wire`). These are the properties that contract promises at the client:
//!
//! - the declared keys arrive, so an unauthorized client can tell "you are not on the list" from
//!   "there is no list" (E1202's `store_present`);
//! - nothing else does — not a key the row does not name, not a place on the agent's filesystem;
//! - a refusal stays a refusal whatever its details look like: an older agent that sends none, a
//!   hostile one that sends garbage, and an older *client* that knows nothing of them.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::BTreeSet;
use std::sync::Arc;

use bytes::BytesMut;
use ono_core::ErrorCode;
use ono_protocol::{
    AuthorizedClient, AuthorizedClients, Frame, FrameKind, Limits, ServerAuthorization,
    UnauthenticatedTransport,
};
use ono_value::{ErrorValue, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

mod common;
use common::{
    client_config, client_key, other_client_key, server_config, try_connect_proving, within,
};

/// A row of the refusal census.
#[derive(Debug)]
struct Row {
    error: String,
    explains: Vec<String>,
    over_the_wire: Vec<String>,
}

/// Every error row of `docs/contracts/hardening/refusals.yaml`.
fn census() -> Vec<Row> {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/contracts/hardening/refusals.yaml"),
    )
    .expect("docs/contracts/hardening/refusals.yaml is the refusal census (§54.1)");
    let document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&text).expect("the census is YAML");
    let keys = |row: &serde_yaml_ng::Value, field: &str| -> Vec<String> {
        row[field]
            .as_sequence()
            .map(|keys| {
                keys.iter()
                    .filter_map(|key| key.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    document["refusals"]
        .as_sequence()
        .expect("the census lists refusals")
        .iter()
        .filter_map(|row| {
            Some(Row {
                error: row["error"].as_str()?.to_owned(),
                explains: keys(row, "explains"),
                over_the_wire: keys(row, "over_the_wire"),
            })
        })
        .collect()
}

/// The keys `error` carries, as a set.
fn keys_of(error: &ErrorValue) -> BTreeSet<String> {
    error
        .metadata()
        .iter()
        .map(|(key, _)| key.to_string())
        .collect()
}

/// A store that lists only the fixture client, which is not the client that connects.
fn somebody_else() -> Arc<AuthorizedClients> {
    Arc::new(AuthorizedClients::of([AuthorizedClient::observing(
        client_key().fingerprint(),
    )]))
}

/// What a client concludes when the far end answers its `Hello` with exactly `payload` as a
/// `Reject` — the bytes an agent of another version, or a hostile one, could put on the wire.
async fn refused_with(payload: &[u8]) -> ErrorValue {
    let (near, mut far) = tokio::io::duplex(4 << 20);
    let limits = Limits::default();
    let mut wire = BytesMut::new();
    ono_protocol::encode(
        &Frame::new(FrameKind::Reject, 0, payload.to_vec()),
        &limits,
        &mut wire,
    )
    .expect("the test's frame fits the envelope");
    let agent = tokio::spawn(async move {
        far.write_all(&wire).await.expect("the refusal is written");
        // Held open until the client has read it and hung up, as an agent's drain does.
        let mut rest = Vec::new();
        let _ = far.read_to_end(&mut rest).await;
    });
    let outcome = within(ono_protocol::Link::connect(
        UnauthenticatedTransport::new(near),
        client_config("testhost"),
    ))
    .await;
    let refused = outcome.expect_err("a Reject never becomes a link");
    agent.abort();
    refused
}

#[tokio::test]
async fn should_tell_an_unauthorized_client_that_the_agent_authorizes_nobody_yet() {
    let refused = try_connect_proving(
        client_config("testhost"),
        server_config().with_authorization(ServerAuthorization::Store(Arc::new(
            AuthorizedClients::empty(),
        ))),
        Some(other_client_key()),
    )
    .await
    .err()
    .expect("§59.1: an unlisted client is refused");

    assert_eq!(refused.code(), ErrorCode::RemoteUnauthorized);
    assert_eq!(
        refused.metadata().get("store_present"),
        Some(&Value::Bool(false)),
        "refusals.yaml: E1202 carries `store_present` over the wire, so the client can tell \
         \"there is no list\" apart, got {refused:?}"
    );
    assert_eq!(
        refused.metadata().get("peer_fingerprint"),
        Some(&Value::string(
            &other_client_key().fingerprint().to_string()
        )),
        "the fingerprint the agent refused is the client's own, which is what the operator types \
         into `add client-key`, got {refused:?}"
    );
}

#[tokio::test]
async fn should_tell_an_unauthorized_client_that_the_agent_lists_somebody_else() {
    let refused = try_connect_proving(
        client_config("testhost"),
        server_config().with_authorization(ServerAuthorization::Store(somebody_else())),
        Some(other_client_key()),
    )
    .await
    .err()
    .expect("§59.1: an unlisted client is refused");

    assert_eq!(refused.code(), ErrorCode::RemoteUnauthorized);
    assert_eq!(
        refused.metadata().get("store_present"),
        Some(&Value::Bool(true)),
        "issue #182: a store that lists somebody else is told apart from no store, got {refused:?}"
    );
    assert!(
        !refused.metadata().iter().any(
            |(_, value)| format!("{value:?}").contains(&client_key().fingerprint().to_string())
        ),
        "§59.1: the refused client learns nothing about who *is* authorized, got {refused:?}"
    );
}

#[tokio::test]
async fn should_carry_exactly_the_keys_the_contract_lets_cross_the_wire() {
    // Every refusal of the census, sent by the agent's pre-negotiation refusal path with every
    // key it explains *and* one it does not, arrives with exactly its `over_the_wire` keys. A
    // `store_path` set on the error stays on the agent, because no row lets it cross.
    for row in census() {
        let Some(code) = ErrorCode::from_name(&row.error) else {
            panic!("the census names `{}`, which no code declares", row.error);
        };
        let mut error = ErrorValue::new(code, "refused for the test");
        for key in &row.explains {
            // Each key with the shape the deciding crate gives it.
            let value = match key.as_str() {
                "store_present" => Value::Bool(true),
                "limit" | "timeout_ms" | "consumed" => Value::Int(7),
                _ => Value::string("x"),
            };
            error = error.with_metadata(key.as_str(), value);
        }
        error = error
            .with_metadata(
                "store_path",
                Value::string("/home/operator/.config/ono/authorized_clients"),
            )
            .with_metadata("private_key", Value::string("-----BEGIN PRIVATE KEY-----"));

        let (near, far) = tokio::io::duplex(64 * 1024);
        let agent = tokio::spawn(async move {
            ono_protocol::refuse(
                UnauthenticatedTransport::new(far),
                &error,
                &Limits::default(),
            )
            .await
        });
        let refused = within(ono_protocol::Link::connect(
            UnauthenticatedTransport::new(near),
            client_config("testhost"),
        ))
        .await
        .expect_err("a refusal is never a link");
        let _ = within(agent).await;

        assert_eq!(refused.code(), code, "the refusal keeps its code");
        let expected: BTreeSet<String> = row.over_the_wire.iter().cloned().collect();
        assert_eq!(
            keys_of(&refused),
            expected,
            "refusals.yaml `{}`: exactly the `over_the_wire` keys reach the refused peer",
            row.error
        );
    }
}

#[tokio::test]
async fn should_read_a_refusal_from_an_agent_that_sends_no_details() {
    // An agent from before ADR-1056: a code and a sentence, nothing else.
    let refused = refused_with(
        br#"{"code":"remote.unauthorized","message":"the client sha256:ab is authenticated and is not authorized on this host"}"#,
    )
    .await;
    assert_eq!(refused.code(), ErrorCode::RemoteUnauthorized);
    assert!(
        refused.metadata().iter().next().is_none(),
        "no detail is invented where the agent sent none (unknown is absent, never guessed)"
    );
    assert!(
        refused.help().is_some(),
        "the remediation is this side's to supply, details or not"
    );
}

#[tokio::test]
async fn should_keep_a_refusal_a_refusal_whatever_its_details_look_like() {
    let long = "f".repeat(64 * 1024);
    let nested = "[".repeat(40) + &"]".repeat(40);
    let many: String = (0..5_000)
        .map(|index| format!("\"k{index}\":{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let hostile = [
        r#""details":[1,2,3]"#.to_owned(),
        r#""details":"store_present""#.to_owned(),
        r#""details":null"#.to_owned(),
        r#""details":{"store_present":"yes","peer_fingerprint":17}"#.to_owned(),
        format!(r#""details":{{"peer_fingerprint":"{long}"}}"#),
        format!(r#""details":{{"store_present":{nested}}}"#),
        r#""details":{"store_present":{"value":true}}"#.to_owned(),
        r#""details":{"store_path":"/etc/ono/authorized_clients","private_key":"k"}"#.to_owned(),
        format!(r#""details":{{{many}}}"#),
    ];
    for details in hostile {
        let payload = format!(r#"{{"code":"remote.unauthorized","message":"refused",{details}}}"#);
        let refused = refused_with(payload.as_bytes()).await;
        assert_eq!(
            refused.code(),
            ErrorCode::RemoteUnauthorized,
            "a malformed details block leaves the refusal what it was: {}",
            &payload[..payload.len().min(120)]
        );
        assert!(
            keys_of(&refused).is_subset(
                &["peer_fingerprint", "store_present"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            ),
            "only E1202's declared keys can arrive, got {:?}",
            keys_of(&refused)
        );
        for (key, value) in refused.metadata().iter() {
            match (key.to_string().as_str(), value) {
                ("store_present", Value::Bool(_)) => {}
                ("peer_fingerprint", Value::String(text)) => assert!(
                    text.len() <= 256,
                    "a detail is bounded, whatever length the agent sent"
                ),
                (key, value) => {
                    panic!("`{key}` arrived as {value:?}, which is not its declared shape")
                }
            }
        }
    }

    // A well-formed detail beside the garbage still arrives.
    let refused = refused_with(
        br#"{"code":"remote.unauthorized","message":"refused","details":{"store_present":false,"store_path":"/x","k":[1]}}"#,
    )
    .await;
    assert_eq!(
        refused.metadata().get("store_present"),
        Some(&Value::Bool(false))
    );
    assert!(refused.metadata().get("store_path").is_none());
}

#[tokio::test]
async fn should_carry_nothing_for_a_code_the_client_does_not_know() {
    let refused = refused_with(
        br#"{"code":"remote.invented_by_a_newer_agent","message":"no","details":{"store_present":true}}"#,
    )
    .await;
    assert_eq!(
        refused.code(),
        ErrorCode::RemoteProtocolMismatch,
        "a code this build cannot name is reported as the mismatch it is, never as success"
    );
    assert!(
        refused.metadata().iter().next().is_none(),
        "no contract row governs an unknown code, so nothing of it is believed"
    );
}

#[tokio::test]
async fn should_send_a_refusal_an_older_client_still_reads() {
    // An older client decodes a `Reject` as `{code, message}` and ignores every other field —
    // serde's default, and the one rule ADR-1056 extends the protocol under. What the agent puts
    // on the wire therefore has to keep both, unchanged, at the top level.
    let (near, far) = tokio::io::duplex(64 * 1024);
    let refusal = ErrorValue::new(ErrorCode::RemoteUnauthorized, "refused for the test")
        .with_metadata("store_present", Value::Bool(true));
    let agent = tokio::spawn(async move {
        ono_protocol::refuse(
            UnauthenticatedTransport::new(far),
            &refusal,
            &Limits::default(),
        )
        .await
    });

    let (mut reader, mut writer) = tokio::io::split(near);
    let limits = Limits::default();
    let mut hello = BytesMut::new();
    ono_protocol::encode(
        &Frame::new(FrameKind::Hello, 0, b"{}".to_vec()),
        &limits,
        &mut hello,
    )
    .expect("a frame");
    writer
        .write_all(&hello)
        .await
        .expect("the hello is written");
    let mut buffer = BytesMut::new();
    let frame = loop {
        if let Some(frame) = ono_protocol::decode(&mut buffer, &limits).expect("frames") {
            break frame;
        }
        let read = within(reader.read_buf(&mut buffer))
            .await
            .expect("the agent answers");
        assert!(read > 0, "the agent answered before closing");
    };
    // Hanging up is what an older client does after reading a refusal; the agent waits for it.
    writer.shutdown().await.expect("the client hangs up");
    let _ = within(agent).await;

    assert_eq!(frame.kind(), FrameKind::Reject);
    #[derive(serde::Deserialize)]
    struct OlderReject {
        code: String,
        message: String,
    }
    let older: OlderReject =
        serde_json::from_slice(frame.payload()).expect("an older client parses the refusal");
    assert_eq!(older.code, "remote.unauthorized");
    assert_eq!(older.message, "refused for the test");
}
