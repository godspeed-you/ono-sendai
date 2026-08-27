//! What the logind session provider promises, asserted through its public contract only.
//!
//! Every test drives [`SessionProvider`] through the [`Provider`] trait over a recorded login
//! manager — a fake of the outside world in the sense AGENTS.md §11 permits — and asserts on
//! what comes out: record fields, the user reference, availability, and the `--user` filter.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml admits these inside `#[test]` functions; the helpers below state a \
              test's preconditions the same way and belong to the same test binary"
)]

use std::sync::Arc;
use std::time::Duration;

use ono_core::ErrorCode;
use ono_provider_api::{Provider, Query};
use ono_provider_systemd::{
    LoginBus, SESSION_PROVIDER_ID, SessionListing, SessionProperties, SessionProvider,
};
use ono_value::{RecordValue, Value};
use zbus::zvariant::OwnedObjectPath;

const BUDGET: Duration = Duration::from_secs(5);

/// A login manager with two sessions: root at a tty, and a remote SSH login of uid 1000.
#[derive(Debug)]
struct RecordedLogind {
    reachable: bool,
}

fn path(id: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(format!("/org/freedesktop/login1/session/_3{id}"))
        .expect("a valid object path")
}

#[async_trait::async_trait]
impl LoginBus for RecordedLogind {
    async fn manager_reachable(&self) -> Result<(), ono_provider_systemd::BusError> {
        if self.reachable {
            Ok(())
        } else {
            Err(ono_provider_systemd::BusError::Unavailable(
                "org.freedesktop.login1 has no owner on this bus".to_owned(),
            ))
        }
    }

    async fn list_sessions(&self) -> Result<Vec<SessionListing>, ono_provider_systemd::BusError> {
        Ok(vec![
            SessionListing {
                id: "1".to_owned(),
                uid: 0,
                user_name: "root".to_owned(),
                seat: Some("seat0".to_owned()),
                path: path("1"),
            },
            SessionListing {
                id: "7".to_owned(),
                uid: 1000,
                user_name: "deploy".to_owned(),
                seat: None,
                path: path("7"),
            },
        ])
    }

    async fn session_properties(
        &self,
        path: &OwnedObjectPath,
    ) -> Result<Option<SessionProperties>, ono_provider_systemd::BusError> {
        Ok(match path.as_str() {
            "/org/freedesktop/login1/session/_31" => Some(SessionProperties {
                id: "1".to_owned(),
                tty: Some("tty1".to_owned()),
                kind: Some("tty".to_owned()),
                class: Some("user".to_owned()),
                state: Some("active".to_owned()),
                remote: Some(false),
                service: Some("login".to_owned()),
                leader: Some(812),
                scope: Some("session-1.scope".to_owned()),
                timestamp_usec: Some(1_756_300_000_000_000),
                ..SessionProperties::default()
            }),
            "/org/freedesktop/login1/session/_37" => Some(SessionProperties {
                id: "7".to_owned(),
                kind: Some("tty".to_owned()),
                class: Some("user".to_owned()),
                state: Some("online".to_owned()),
                remote: Some(true),
                remote_host: Some("10.0.0.5".to_owned()),
                service: Some("sshd".to_owned()),
                leader: Some(4419),
                scope: Some("session-7.scope".to_owned()),
                timestamp_usec: Some(1_756_300_500_000_000),
                ..SessionProperties::default()
            }),
            _ => None,
        })
    }
}

async fn provider(reachable: bool) -> SessionProvider {
    tokio::time::timeout(
        BUDGET,
        SessionProvider::over(Arc::new(RecordedLogind { reachable })),
    )
    .await
    .expect("probing a recorded login manager must not hang")
}

async fn sessions(provider: &SessionProvider, query: &Query) -> Vec<Arc<RecordValue>> {
    let stream = provider
        .snapshot(query)
        .expect("the recorded login manager answers");
    tokio::time::timeout(BUDGET, stream.collect())
        .await
        .expect("a snapshot of recorded sessions must not hang")
        .into_values()
        .into_iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some(record),
            _ => None,
        })
        .collect()
}

fn text(record: &RecordValue, field: &str) -> Option<String> {
    match record.get(field) {
        Some(Value::String(text)) => Some(text.to_string()),
        _ => None,
    }
}

#[tokio::test]
async fn should_emit_one_session_record_per_login_with_its_holder_referenced_by_uid() {
    let provider = provider(true).await;
    let found = sessions(&provider, &Query::target("session")).await;
    assert_eq!(found.len(), 2, "every session logind lists is a record");

    let root = found
        .iter()
        .find(|record| text(record, "id").as_deref() == Some("1"))
        .expect("session 1 is listed");
    assert_eq!(root.schema_id().to_string(), "ono.session/1");
    let Some(Value::Record(user)) = root.get("user") else {
        panic!(
            "`user` is a ref<ono.user/1> record, got {:?}",
            root.get("user")
        );
    };
    assert_eq!(
        user.get("uid"),
        Some(&Value::Int(0)),
        "the holder is referenced by uid"
    );
    assert_eq!(
        text(user, "name").as_deref(),
        Some("root"),
        "and by the name logind recorded"
    );
    assert_eq!(text(root, "seat").as_deref(), Some("seat0"));
    assert_eq!(text(root, "tty").as_deref(), Some("tty1"));
    assert_eq!(text(root, "type").as_deref(), Some("tty"));
    assert_eq!(text(root, "state").as_deref(), Some("active"));
    assert_eq!(root.get("remote"), Some(&Value::Bool(false)));
    assert_eq!(root.get("leader"), Some(&Value::Int(812)));
    assert!(
        matches!(root.get("since"), Some(Value::Timestamp(_))),
        "`since` is logind's Timestamp as a timestamp value, got {:?}",
        root.get("since")
    );
    assert_eq!(
        root.provenance().provider(),
        SESSION_PROVIDER_ID,
        "the record says which provider produced it"
    );
}

#[tokio::test]
async fn should_leave_what_logind_did_not_say_as_null() {
    let provider = provider(true).await;
    let found = sessions(&provider, &Query::target("session")).await;
    let remote = found
        .iter()
        .find(|record| text(record, "id").as_deref() == Some("7"))
        .expect("session 7 is listed");
    assert_eq!(
        remote.get("seat"),
        Some(&Value::Null),
        "an SSH login has no seat"
    );
    assert_eq!(
        remote.get("tty"),
        Some(&Value::Null),
        "and no tty was recorded"
    );
    assert_eq!(remote.get("display"), Some(&Value::Null));
    assert_eq!(text(remote, "remote_host").as_deref(), Some("10.0.0.5"));
    assert_eq!(text(remote, "service").as_deref(), Some("sshd"));
}

#[tokio::test]
async fn should_restrict_to_one_user_by_name_or_uid_when_the_user_option_is_given() {
    let provider = provider(true).await;
    let by_name = Query::target("session").option("user", Value::string("deploy"));
    let found = sessions(&provider, &by_name).await;
    assert_eq!(
        found.len(),
        1,
        "`--user deploy` keeps deploy's session only"
    );
    assert_eq!(text(&found[0], "id").as_deref(), Some("7"));

    let by_uid = Query::target("session").option("user", Value::Int(0));
    let found = sessions(&provider, &by_uid).await;
    assert_eq!(found.len(), 1, "`--user 0` keeps root's session only");
    assert_eq!(text(&found[0], "id").as_deref(), Some("1"));

    let nobody = Query::target("session").option("user", Value::string("nobody-such"));
    assert!(
        sessions(&provider, &nobody).await.is_empty(),
        "a user with no session yields an empty stream, not an error"
    );
}

#[tokio::test]
async fn should_refuse_with_provider_unavailable_when_no_login_manager_answers() {
    let provider = provider(false).await;
    let reason = provider
        .availability()
        .reason()
        .expect("an unreachable login manager is reported as unavailable, with a reason")
        .to_owned();
    assert!(
        reason.contains("login1"),
        "the reason names what was missing, got {reason:?}"
    );
    let error = match provider.snapshot(&Query::target("session")) {
        Err(error) => error,
        Ok(_) => panic!("no empty stream stands in for an absent login manager (spec §35.3)"),
    };
    assert_eq!(error.code(), ErrorCode::ProviderUnavailable);
}
