//! The engine's own lifecycle event stream, live and historical (spec v0.5 §21.4, §22.7).
//!
//! The engine is the outside world, and it is absent from almost every machine this suite runs
//! on. It is faked the way `http.rs`'s own tests fake it — a real Unix socket serving recorded
//! Docker Engine API bytes — so the positive path is a tested contract rather than something that
//! only holds where a daemon happens to be installed. That is a fake of the system being read,
//! which AGENTS.md §11 permits, and not a mock of a layer this crate wrote.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml admits these inside `#[test]` functions, and the helpers here state a \
              test's preconditions the same way"
)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use ono_provider_api::{EventKind, Provider, Query, Selector, TimeWindow};
use ono_provider_container::ContainerProvider;
use ono_value::{RecordValue, Value};

/// No test may hang; a recorded engine answers in microseconds.
const BUDGET: Duration = Duration::from_secs(5);

/// The events one container's life produces, as Docker's Engine API sends them: one JSON
/// document per line, inside a chunked body.
///
/// Recorded shape, not invented: `Actor.ID` is the full container id, `Actor.Attributes` carries
/// the image and name, `Action` is the verb and `timeNano` the instant.
const RECORDED_EVENTS: [&str; 4] = [
    r#"{"status":"create","id":"9f1c2e5a7b3d4c6e8f0a1b2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f506","Type":"container","Action":"create","Actor":{"ID":"9f1c2e5a7b3d4c6e8f0a1b2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f506","Attributes":{"image":"nginx:1.27","name":"web"}},"scope":"local","time":1787000000,"timeNano":1787000000111222333}"#,
    r#"{"status":"start","id":"9f1c2e5a7b3d4c6e8f0a1b2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f506","Type":"container","Action":"start","Actor":{"ID":"9f1c2e5a7b3d4c6e8f0a1b2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f506","Attributes":{"image":"nginx:1.27","name":"web"}},"scope":"local","time":1787000001,"timeNano":1787000001444555666}"#,
    r#"{"Type":"image","Action":"pull","Actor":{"ID":"nginx:1.27","Attributes":{"name":"nginx"}},"scope":"local","time":1787000002,"timeNano":1787000002000000000}"#,
    r#"{"status":"die","id":"9f1c2e5a7b3d4c6e8f0a1b2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f506","Type":"container","Action":"die","Actor":{"ID":"9f1c2e5a7b3d4c6e8f0a1b2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f506","Attributes":{"exitCode":"0","image":"nginx:1.27","name":"web"}},"scope":"local","time":1787000003,"timeNano":1787000003777888999}"#,
];

/// The full container id the recorded events are about.
const CONTAINER_ID: &str = "9f1c2e5a7b3d4c6e8f0a1b2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f506";

/// A recorded engine: a Unix socket that answers `/_ping` and serves the events as a chunked
/// stream, one chunk per event, then closes.
///
/// The request line is kept so a test can assert what the engine was actually asked — which is
/// the only way to tell a windowed replay from a live tail from the outside.
struct RecordedEngine {
    socket: PathBuf,
    asked: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    _directory: tempfile::TempDir,
}

impl RecordedEngine {
    fn serving(events: &'static [&'static str]) -> Self {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let socket = directory.path().join("engine.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).expect("a socket to bind");
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = std::sync::Arc::clone(&asked);
        std::thread::spawn(move || {
            for connection in listener.incoming() {
                let Ok(mut stream) = connection else { return };
                let mut request = vec![0u8; 8192];
                let read = stream.read(&mut request).unwrap_or(0);
                let head = String::from_utf8_lossy(&request[..read]).into_owned();
                let line = head.lines().next().unwrap_or_default().to_owned();
                recorded
                    .lock()
                    .expect("the recorded requests are not poisoned")
                    .push(line.clone());

                let mut body = String::from(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: \
                     chunked\r\n\r\n",
                );
                if line.contains("/events") {
                    for event in events {
                        let piece = format!("{event}\n");
                        body.push_str(&format!("{:x}\r\n{piece}\r\n", piece.len()));
                    }
                }
                body.push_str("0\r\n\r\n");
                let _ = stream.write_all(body.as_bytes());
                let _ = stream.flush();
            }
        });
        Self {
            socket,
            asked,
            _directory: directory,
        }
    }

    fn provider(&self) -> ContainerProvider {
        ContainerProvider::from_environment([(
            "DOCKER_HOST",
            format!("unix://{}", self.socket.display()).as_str(),
        )])
    }

    fn requests(&self) -> Vec<String> {
        self.asked
            .lock()
            .expect("the recorded requests are not poisoned")
            .clone()
    }
}

fn text(record: &RecordValue, field: &str) -> Option<String> {
    record
        .get(field)
        .and_then(|value| value.as_str().ok().map(str::to_owned))
}

#[tokio::test]
async fn should_answer_the_lifecycle_the_engine_recorded_when_asked_about_a_past_window() {
    // §21.4 and §22.7: the engine keeps its event log, so `GET /events?since=&until=` is a
    // historical query and not a re-reading of the present.
    let engine = RecordedEngine::serving(&RECORDED_EVENTS);
    let provider = engine.provider();
    let from = "2026-08-31T12:00:00Z".parse().expect("a fixed instant");
    let until = "2026-08-31T13:00:00Z".parse().expect("a fixed instant");

    let stream = provider
        .history(
            &Query::target("container"),
            &TimeWindow::between(from, until),
        )
        .expect("the engine answers about its own past");
    let collected = tokio::time::timeout(BUDGET, stream.collect())
        .await
        .expect("a recorded engine must not hang");

    let records: Vec<RecordValue> = collected
        .into_values()
        .into_iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some((*record).clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        records.len(),
        3,
        "three of the four recorded events are about a container; the image pull is not"
    );
    assert!(
        records
            .iter()
            .all(|record| text(record, "id").as_deref() == Some(CONTAINER_ID)),
        "§22.7: the event's container id is the identity `get container` answers with"
    );
    assert_eq!(
        records
            .iter()
            .filter_map(|record| text(record, "state"))
            .collect::<Vec<_>>(),
        ["created", "running", "exited"],
        "the state follows the engine's own action"
    );

    let asked = engine.requests();
    assert!(
        asked
            .iter()
            .any(|line| line.contains("since=") && line.contains("until=")),
        "a historical query states both ends, so the engine replays and closes: {asked:?}"
    );
}

#[tokio::test]
async fn should_date_a_historical_event_by_the_instant_the_engine_recorded_it() {
    // §3.3 and §24.2: the source's instant is the observation's, and the instant this shell got
    // round to asking is not. The engine sends `timeNano`, so there is no excuse for the latter.
    let engine = RecordedEngine::serving(&RECORDED_EVENTS);
    let stream = engine
        .provider()
        .history(&Query::target("container"), &TimeWindow::unbounded())
        .expect("the engine answers about its own past");
    let collected = tokio::time::timeout(BUDGET, stream.collect())
        .await
        .expect("a recorded engine must not hang");

    let first = collected
        .into_values()
        .into_iter()
        .find_map(|value| match value {
            Value::Record(record) => Some(record),
            _ => None,
        })
        .expect("the recorded engine answered with events");
    assert_eq!(
        first.provenance().observed(),
        Some(
            "2026-08-17T20:53:20.111222333Z"
                .parse()
                .expect("the recorded instant")
        ),
        "the record is dated by the engine's `timeNano`"
    );
}

#[tokio::test]
async fn should_close_an_open_upper_end_so_a_historical_query_is_not_a_live_tail() {
    // `GET /events` without `until` never ends. A question about the past that never returns is
    // not an answer, so the provider bounds it at the instant it asks.
    let engine = RecordedEngine::serving(&RECORDED_EVENTS);
    let stream = engine
        .provider()
        .history(&Query::target("container"), &TimeWindow::unbounded())
        .expect("the engine answers about its own past");
    let _ = tokio::time::timeout(BUDGET, stream.collect())
        .await
        .expect("a recorded engine must not hang");

    let asked = engine.requests();
    assert!(
        asked.iter().any(|line| line.contains("until=")),
        "an unbounded window still asks a bounded question: {asked:?}"
    );
}

#[tokio::test]
async fn should_report_a_container_appearing_and_going_away_by_the_engines_own_action() {
    // §22.7: "Container providers SHOULD expose runtime-native lifecycle events when the runtime
    // supports them." The engine says `create`, `start` and `die`; the provider relays what the
    // engine said rather than comparing two listings and guessing at what happened between them.
    let engine = RecordedEngine::serving(&RECORDED_EVENTS);
    let provider = engine.provider();
    let mut events = provider
        .subscribe(&Query::target("container"))
        .expect("a running engine offers its event stream");

    let mut seen = Vec::new();
    while let Ok(Some(event)) = tokio::time::timeout(BUDGET, events.recv()).await {
        seen.push(event);
    }
    assert_eq!(
        seen.iter().map(|event| event.kind()).collect::<Vec<_>>(),
        [EventKind::Added, EventKind::Changed, EventKind::Changed],
        "`create` is an appearance and the rest are the container changing"
    );
    assert_eq!(
        seen[0].at(),
        "2026-08-17T20:53:20.111222333Z"
            .parse()
            .expect("the recorded instant"),
        "a live event is dated by the engine, not by when this shell read it"
    );
}

#[tokio::test]
async fn should_narrow_the_event_stream_to_the_container_the_query_names() {
    let engine = RecordedEngine::serving(&RECORDED_EVENTS);
    let provider = engine.provider();
    let query =
        Query::target("container").with(Selector::field("name", Value::string("not-this-one")));
    let mut events = provider.subscribe(&query).expect("a subscription");

    let mut seen = 0usize;
    while let Ok(Some(_)) = tokio::time::timeout(BUDGET, events.recv()).await {
        seen += 1;
    }
    assert_eq!(
        seen, 0,
        "`watch container not-this-one` is about that container, and the recorded events are not"
    );
}

#[tokio::test]
async fn should_refuse_to_watch_images_rather_than_report_container_events_as_image_events() {
    let engine = RecordedEngine::serving(&RECORDED_EVENTS);
    let error = engine
        .provider()
        .subscribe(&Query::target("image"))
        .expect_err("the engine's lifecycle stream is about containers");
    assert_eq!(error.code(), ono_core::ErrorCode::ProviderUnsupported);
}

#[tokio::test]
async fn should_refuse_a_historical_query_where_no_runtime_answers() {
    // §10.5: no runtime is not the same as no containers, and it is not the same as no history
    // either. The refusal names the sockets it tried.
    let provider = ContainerProvider::from_environment([("DOCKER_HOST", "unix:///nowhere/x.sock")]);
    let error = provider
        .history(&Query::target("container"), &TimeWindow::unbounded())
        .expect_err("there is no engine to ask about the past");
    assert_eq!(error.code(), ono_core::ErrorCode::ProviderUnavailable);
}

#[test]
fn should_claim_the_event_stream_it_actually_opens() {
    let provider = ContainerProvider::from_environment([("DOCKER_HOST", "unix:///nowhere/x.sock")]);
    let claims = provider.temporal();
    assert!(
        claims.live_events,
        "the engine's `GET /events` is a subscription"
    );
    assert!(claims.historical_query, "and bounded, it replays a window");
    assert!(
        !claims.exhaustive_events,
        "§21.5: the engine's event log is bounded by its own retention and the daemon's lifetime"
    );
    assert!(
        !claims.causal_tokens,
        "the event names no request id, so there is no transaction to join on"
    );
}
