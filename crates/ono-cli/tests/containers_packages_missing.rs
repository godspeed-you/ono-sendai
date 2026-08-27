//! Outcome tests for the two families the registry marks `phase: planned` in full: containers
//! (`get/start/stop/restart/enter/trace/watch/remove/set container`, `get image`) and packages
//! (`get/find/add/remove/set package`).
//!
//! Contracts: `docs/spec/commands/container.yaml`, `docs/spec/commands/package.yaml`, schemas
//! `ono.action-result/1`, `ono.context/1`, `ono.graph/1`. Narrative: spec §8.3 (infrastructure
//! targets), §9.1 (the Package and Container tables), §14.3 (object context), §16.5 (partial
//! failure is never collapsed), §17.1 (risk visibility), §18.2 (native live streams begin with a
//! snapshot, ADR-0024), §22.3 (traces), §23 (no parsing of human output where an API exists),
//! §31.57 (a provider speaks the daemon's API or a declared machine format), §35.3 (unknown is
//! null, never fabricated), §43 (error taxonomy), AGENTS.md §6.
//!
//! Every test runs unprivileged, offline and deterministically: no daemon and no package manager
//! on this machine is ever touched. The outside world is faked at the provider boundary, which
//! is exactly the boundary the contract fixes:
//!
//! - **Container runtime.** A Unix socket the test binds answers a minimal Docker Engine API
//!   (`GET /containers/json`, `GET /images/json`, `GET /containers/{id}/json`,
//!   `POST /containers/{id}/{start,stop,restart,update}`, `DELETE /containers/{id}`). Any
//!   `/v1.xx` version prefix and any query string are ignored. The shell is pointed at it with
//!   `DOCKER_HOST=unix://<path>` and `CONTAINER_HOST=unix://<path>` — the knobs Docker and
//!   Podman themselves honour. Neither the contract nor spec §23 fixes a knob, so this is the
//!   assumption these tests pin: the container provider reads those two variables, tries the
//!   sockets they name, and speaks the Docker-compatible engine API to whichever answers (Podman
//!   serves the same API on its socket). "No runtime" is made deterministic by pointing both at
//!   a socket path nothing listens on.
//! - **Canonical records.** No `ono.container/1` or `ono.image/1` schema exists yet, so the field
//!   names are taken from the contract's selectors and options and spec §9.1: a container has
//!   `id`, `name` (the engine's `Names[0]` without the leading slash), `image`, `state`; an image
//!   has `id`, `reference` (the first `RepoTag`, matching the `reference` selector of
//!   `get image`) and `size`. A package has `name`, `version` and `installed`.
//! - **Package managers.** A scratch `bin/` holding executable fake `dpkg-query`, `dpkg`,
//!   `apt-cache` and `apt-get` scripts is the whole `PATH`. The assumption pinned here: the
//!   package provider discovers managers by looking them up on `PATH` (never by absolute path),
//!   lists with `dpkg-query -W -f` in a tab-separated `Package Version Status` machine format
//!   (spec §31.58: an explicit machine-readable mode, never human output), searches with
//!   `apt-cache search`, and mutates with `apt-get`. A fake that prints something else is a
//!   provider defect (E0403), not a source of invented fields.
//! - **Graph shape.** `trace container` yields an `ono.graph/1` with at least the container node
//!   (`kind: ono.container/1`) and its image node (`kind: ono.image/1`) joined by a directed
//!   `image` edge. Namespaces, cgroups, mounts and processes need a real kernel view and are not
//!   asserted.
//!
//! Every test asserts what the user sees — stdout through `| to json`, the exit status, the
//! structured error code, the requests that reached the fake runtime — never how a stage is
//! wired (AGENTS.md §11).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ono_testkit::{Scratch, Shell, scratch};
use serde_yaml_ng::Value;

// --- shared helpers ---------------------------------------------------------------------------

/// Parses the JSON document `to json` wrote as the stream's values.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let text = run.stdout().trim().to_owned();
    let stderr = run.stderr();
    let document: Value = serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!("`to json` must emit a JSON document, got {text:?} ({error}); stderr: {stderr:?}")
    });
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!(
                "spec §33.5: `to json` emits the stream as an array, got {text:?}; stderr: {stderr:?}"
            )
        })
        .clone()
}

/// The one `ono.action-result/1` row a single-target mutation emits.
fn single_result(run: &ono_testkit::Run) -> Value {
    let mut rows = rows(run);
    assert_eq!(
        rows.len(),
        1,
        "spec §11.5: one ActionResult per target, got {:?}",
        run.stdout()
    );
    rows.remove(0)
}

fn text(row: &Value, field: &str) -> String {
    row[field]
        .as_str()
        .unwrap_or_else(|| panic!("field `{field}` must be a string, got {row:?}"))
        .to_owned()
}

fn assert_success_row(row: &Value, operation: &str) {
    assert_eq!(
        text(row, "operation"),
        operation,
        "spec §11.5: `operation` is the command id, got {row:?}"
    );
    assert_eq!(
        text(row, "status"),
        "success",
        "the mutation reports its outcome as `success`, got {row:?}"
    );
    assert_eq!(
        row["changed"].as_bool(),
        Some(true),
        "spec §11.5: `changed` says the system state moved, got {row:?}"
    );
    assert!(
        row["error"].is_null(),
        "`error` is null for a success (action-result.v1), got {row:?}"
    );
}

fn assert_failed_row(row: &Value, operation: &str, code: &str) {
    assert_eq!(
        text(row, "operation"),
        operation,
        "spec §11.5: `operation` is the command id, got {row:?}"
    );
    assert_eq!(
        text(row, "status"),
        "failed",
        "the mutation reports a failure as a `failed` row, not as text, got {row:?}"
    );
    assert_eq!(
        row["changed"].as_bool(),
        Some(false),
        "a failed mutation changed nothing, got {row:?}"
    );
    assert_eq!(
        row["error"]["code"].as_str(),
        Some(code),
        "spec §43: the failed row carries the structured error {code}, got {row:?}"
    );
}

/// The stderr of a run that must have been refused at the provider boundary, not by the
/// "declared but not delivered" placeholder this build answers with today.
fn provider_boundary_error(run: &ono_testkit::Run, code: &str) -> String {
    let stderr = run.stderr().to_owned();
    assert!(
        !run.status().is_success(),
        "a provider error is an error: the exit status is non-zero, got {:?}; stderr: {stderr:?}",
        run.status()
    );
    assert!(
        !stderr.contains("E0101") && !stderr.contains("E0102"),
        "the command is delivered, not a placeholder (spec §9.1), got {stderr:?}"
    );
    assert!(
        stderr.contains(code),
        "spec §43: the refusal carries {code}, got {stderr:?}"
    );
    stderr
}

// --- the fake container runtime ---------------------------------------------------------------

const CONTAINER_ID: &str = "abc123def4567890abc123def4567890abc123def4567890abc123def4567890";
const IMAGE_ID: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const IMAGE_SIZE: i64 = 187_000_000;

/// How the fake runtime answers a mutation on a container it knows.
#[derive(Clone, Copy)]
enum Runtime {
    /// The engine does what it is asked.
    Accepting,
    /// The engine refuses every mutation with `403 {"message":"permission denied"}`.
    Refusing,
}

/// A Unix socket speaking just enough of the Docker Engine API for one container and one image.
struct FakeRuntime {
    socket: PathBuf,
    requests: Arc<Mutex<Vec<String>>>,
}

impl FakeRuntime {
    fn start(directory: &Scratch, behaviour: Runtime) -> Self {
        let socket = directory.path().join("docker.sock");
        let listener = UnixListener::bind(&socket).expect("bind the fake runtime socket");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        std::thread::spawn(move || {
            // The thread outlives the test; the harness process ends it. Each connection carries
            // exactly one request, answered with `Connection: close` like a real daemon may.
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let log = Arc::clone(&log);
                std::thread::spawn(move || serve(stream, behaviour, &log));
            }
        });
        Self { socket, requests }
    }

    fn url(&self) -> String {
        format!("unix://{}", self.socket.display())
    }

    /// The request lines the runtime received, `METHOD /path` with version prefix and query
    /// stripped.
    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("the request log").clone()
    }

    fn saw(&self, method: &str, path_suffix: &str) -> bool {
        self.requests().iter().any(|line| {
            line.split_once(' ')
                .is_some_and(|(m, path)| m == method && path.ends_with(path_suffix))
        })
    }
}

fn serve(mut stream: UnixStream, behaviour: Runtime, log: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(position) = find(&buffer, b"\r\n\r\n") {
            break position + 4;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_owned();
    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len() < header_end + content_length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
        }
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default();
    let path = strip_api_version(target.split('?').next().unwrap_or_default());
    log.lock()
        .expect("the request log")
        .push(format!("{method} {path}"));

    let (status, body) = route(&method, path, behaviour);
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// `/v1.45/containers/json` and `/containers/json` are the same request.
fn strip_api_version(path: &str) -> &str {
    let Some(rest) = path.strip_prefix("/v") else {
        return path;
    };
    let Some(slash) = rest.find('/') else {
        return path;
    };
    let version = &rest[..slash];
    if !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        &rest[slash..]
    } else {
        path
    }
}

fn route(method: &str, path: &str, behaviour: Runtime) -> (&'static str, String) {
    let container_list = format!(
        r#"[{{"Id":"{CONTAINER_ID}","Names":["/web"],"Image":"nginx:1.27","ImageID":"{IMAGE_ID}","State":"running","Status":"Up 2 hours","Created":1700000000,"Labels":{{}},"Ports":[]}}]"#
    );
    let image_list = format!(
        r#"[{{"Id":"{IMAGE_ID}","RepoTags":["nginx:1.27"],"RepoDigests":[],"Size":{IMAGE_SIZE},"Created":1700000000,"Labels":{{}}}}]"#
    );
    let inspect = format!(
        r#"{{"Id":"{CONTAINER_ID}","Name":"/web","Image":"{IMAGE_ID}","Created":"2023-11-14T22:13:20Z","State":{{"Status":"running","Running":true,"Pid":0}},"Config":{{"Image":"nginx:1.27","Labels":{{}}}},"HostConfig":{{"Memory":0,"NanoCpus":0}},"Mounts":[]}}"#
    );

    match (method, path) {
        ("GET" | "HEAD", "/_ping") => ("200 OK", "OK".to_owned()),
        ("GET", "/version") => (
            "200 OK",
            r#"{"Version":"27.0.0","ApiVersion":"1.46","MinAPIVersion":"1.24","Os":"linux","Arch":"amd64"}"#
                .to_owned(),
        ),
        ("GET", "/info") => (
            "200 OK",
            r#"{"ID":"fake","Containers":1,"Images":1,"Name":"fake-runtime","OSType":"linux"}"#
                .to_owned(),
        ),
        ("GET", "/containers/json") => ("200 OK", container_list),
        ("GET", "/images/json") => ("200 OK", image_list),
        ("GET", "/events") => ("200 OK", String::new()),
        ("GET", other) if other.starts_with("/containers/") && other.ends_with("/json") => {
            let id = other
                .trim_start_matches("/containers/")
                .trim_end_matches("/json");
            if is_known(id) {
                ("200 OK", inspect)
            } else {
                ("404 Not Found", no_such_container(id))
            }
        }
        ("GET", other) if other.starts_with("/images/") && other.ends_with("/json") => (
            "200 OK",
            format!(
                r#"{{"Id":"{IMAGE_ID}","RepoTags":["nginx:1.27"],"Size":{IMAGE_SIZE},"Created":"2023-11-14T22:13:20Z"}}"#
            ),
        ),
        ("POST", other) if other.starts_with("/containers/") => {
            let mut segments = other.trim_start_matches("/containers/").split('/');
            let id = segments.next().unwrap_or_default();
            let action = segments.next().unwrap_or_default();
            mutate(id, action, behaviour)
        }
        ("DELETE", other) if other.starts_with("/containers/") => {
            let id = other
                .trim_start_matches("/containers/")
                .split('/')
                .next()
                .unwrap_or_default();
            mutate(id, "remove", behaviour)
        }
        _ => ("404 Not Found", r#"{"message":"page not found"}"#.to_owned()),
    }
}

fn is_known(id: &str) -> bool {
    id == "web" || CONTAINER_ID.starts_with(id)
}

fn no_such_container(id: &str) -> String {
    format!(r#"{{"message":"No such container: {id}"}}"#)
}

fn mutate(id: &str, action: &str, behaviour: Runtime) -> (&'static str, String) {
    if !is_known(id) {
        return ("404 Not Found", no_such_container(id));
    }
    match behaviour {
        Runtime::Refusing => (
            "403 Forbidden",
            r#"{"message":"permission denied"}"#.to_owned(),
        ),
        Runtime::Accepting => match action {
            "update" => ("200 OK", r#"{"Warnings":[]}"#.to_owned()),
            _ => ("204 No Content", String::new()),
        },
    }
}

/// Runs a one-liner against the fake runtime.
fn ono_with_runtime(runtime: &FakeRuntime, script: &str) -> ono_testkit::Run {
    Shell::new()
        .env("DOCKER_HOST", runtime.url())
        .env("CONTAINER_HOST", runtime.url())
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

/// Runs a one-liner with both runtime knobs pointing at a socket nothing listens on.
fn ono_without_runtime(directory: &Scratch, script: &str) -> ono_testkit::Run {
    let nowhere = format!("unix://{}", directory.path().join("nowhere.sock").display());
    Shell::new()
        .env("DOCKER_HOST", &nowhere)
        .env("CONTAINER_HOST", &nowhere)
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

// --- containers: enumeration --------------------------------------------------------------------

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_provider_unavailable_when_no_container_runtime_answers() {
    let directory = scratch();
    // Spec §43 / errors.yaml E0401: "no container runtime socket" is the provider's honest
    // answer — not E0101, and not an empty list pretending there are no containers.
    let run = ono_without_runtime(&directory, "get container | to json");
    let stderr = provider_boundary_error(&run, "Ono-Sendai-E0401");
    assert!(
        stderr.contains("nowhere.sock"),
        "the refusal names the runtime socket that was tried (errors.yaml E0401 help), got {stderr:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_provider_unavailable_when_no_runtime_answers_for_images() {
    let directory = scratch();
    let run = ono_without_runtime(&directory, "get image | to json");
    let stderr = provider_boundary_error(&run, "Ono-Sendai-E0401");
    assert!(
        stderr.contains("nowhere.sock"),
        "the refusal names the runtime socket that was tried (errors.yaml E0401 help), got {stderr:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_list_containers_from_the_engine_api_when_a_runtime_socket_answers() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    // Spec §9.1: `get container` enumerates through the installed provider; spec §23/§31.57 and
    // AGENTS.md §6: the provider speaks the daemon's API, so the record is built from the JSON
    // the engine returned — never from `docker ps` text.
    let run = ono_with_runtime(
        &runtime,
        "get container | select name state image | to json",
    );
    run.assert_success();
    assert_eq!(
        rows(&run),
        vec![
            serde_yaml_ng::from_str::<Value>(
                r#"{"name":"web","state":"running","image":"nginx:1.27"}"#
            )
            .unwrap()
        ],
        "the engine's list maps onto the canonical container record, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    assert!(
        runtime.saw("GET", "/containers/json"),
        "the provider asked the engine API for its containers, got {:?}",
        runtime.requests()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_list_images_from_the_engine_api_when_a_runtime_socket_answers() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    let run = ono_with_runtime(&runtime, "get image | select id reference size | to json");
    run.assert_success();
    let rows = rows(&run);
    assert_eq!(
        rows.len(),
        1,
        "one image in the engine is one image record, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    let image = &rows[0];
    assert_eq!(
        text(image, "reference"),
        "nginx:1.27",
        "the image's reference is its first repo tag, got {image:?}"
    );
    assert_eq!(
        text(image, "id"),
        IMAGE_ID,
        "the image's id is the engine's content digest, got {image:?}"
    );
    assert_eq!(
        image["size"].as_i64(),
        Some(IMAGE_SIZE),
        "spec §12.4: size is a number, not text, got {image:?}"
    );
    assert!(
        runtime.saw("GET", "/images/json"),
        "the provider asked the engine API for its images, got {:?}",
        runtime.requests()
    );
}

// --- containers: mutations -----------------------------------------------------------------------

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_start_a_container_through_the_engine_api_when_the_runtime_accepts() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    let run = ono_with_runtime(&runtime, "start container web | to json");
    run.assert_success();
    assert_success_row(&single_result(&run), "ono.container.start");
    assert!(
        runtime.saw("POST", "/start"),
        "the start reached the engine, got {:?}",
        runtime.requests()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_stop_a_container_through_the_engine_api_when_the_runtime_accepts() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    let run = ono_with_runtime(&runtime, "stop container web | to json");
    run.assert_success();
    assert_success_row(&single_result(&run), "ono.container.stop");
    assert!(
        runtime.saw("POST", "/stop"),
        "the stop reached the engine, got {:?}",
        runtime.requests()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_restart_a_container_through_the_engine_api_when_the_runtime_accepts() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    let run = ono_with_runtime(&runtime, "restart container web | to json");
    run.assert_success();
    assert_success_row(&single_result(&run), "ono.container.restart");
    assert!(
        runtime.saw("POST", "/restart"),
        "the restart reached the engine, got {:?}",
        runtime.requests()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_remove_a_container_through_the_engine_api_when_the_runtime_accepts() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    let run = ono_with_runtime(&runtime, "remove container web | to json");
    run.assert_success();
    assert_success_row(&single_result(&run), "ono.container.remove");
    assert!(
        runtime
            .requests()
            .iter()
            .any(|line| line.starts_with("DELETE /containers/")),
        "the removal reached the engine as a DELETE, got {:?}",
        runtime.requests()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_update_a_memory_limit_through_the_engine_api_when_setting_a_container() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    let run = ono_with_runtime(&runtime, "set container web --memory 512MiB | to json");
    run.assert_success();
    assert_success_row(&single_result(&run), "ono.container.set");
    assert!(
        runtime.saw("POST", "/update"),
        "the new limit reached the engine's update endpoint, got {:?}",
        runtime.requests()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_fail_with_not_found_when_stopping_a_container_the_runtime_does_not_know() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    // Spec §16.5 + ADR-0006: the failure is one `failed` row carrying io.not_found, and any
    // failed row makes the exit status 1.
    let run = ono_with_runtime(&runtime, "stop container nope | to json");
    assert_failed_row(
        &single_result(&run),
        "ono.container.stop",
        "Ono-Sendai-E0301",
    );
    run.assert_status(1);
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_fail_with_permission_denied_when_the_runtime_refuses_the_stop() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Refusing);

    // The engine's 403 is the system saying no; errors.yaml E0302 is its structured form.
    let run = ono_with_runtime(&runtime, "stop container web | to json");
    assert_failed_row(
        &single_result(&run),
        "ono.container.stop",
        "Ono-Sendai-E0302",
    );
    run.assert_status(1);
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_name_the_provider_and_the_risk_when_explaining_a_container_stop() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    // Spec §17.1: the risk of a native mutation is visible before it runs; spec §27: the plan
    // names the provider that will carry it out, as `explain stop service nginx` names systemd.
    let run = ono_with_runtime(&runtime, "explain stop container web");
    run.assert_success();
    let plan = run.stdout().to_owned();
    assert!(
        plan.contains("ono.container.stop"),
        "the plan names the command, got {plan:?}"
    );
    assert!(
        plan.lines()
            .any(|line| line.trim_start().starts_with("provider")),
        "the plan names the container provider that answers (spec §27), got {plan:?}"
    );
    assert!(
        plan.contains("risk") && plan.contains("mutate"),
        "spec §17.1: a stop is labelled as a mutation, got {plan:?}"
    );
}

// --- containers: context, watch, trace ---------------------------------------------------------

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_push_a_container_frame_when_entering_a_container() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    // Spec §14.3 and context.v1: `enter container web` pushes a frame of kind `container` whose
    // identity is the container.
    let run = ono_with_runtime(&runtime, "enter container web; get context | to json");
    run.assert_success();
    let frames = rows(&run);
    assert_eq!(
        frames.len(),
        2,
        "the ground frame plus the container frame, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    let top = &frames[1];
    assert_eq!(
        text(top, "kind"),
        "container",
        "context.v1: the frame's kind is `container`, got {top:?}"
    );
    assert_eq!(
        text(top, "target"),
        "container",
        "context.v1: the frame narrows to the `container` target, got {top:?}"
    );
    assert_eq!(
        text(top, "identity"),
        "web",
        "context.v1: the frame carries the container's identity, got {top:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_pop_the_container_frame_when_leaving_it() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    let run = ono_with_runtime(
        &runtime,
        "enter container web; leave; get context | select kind | to json",
    );
    run.assert_success();
    assert_eq!(
        rows(&run),
        vec![serde_yaml_ng::from_str::<Value>(r#"{"kind":"local"}"#).unwrap()],
        "spec §14.1: `leave` restores the ground frame, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    assert!(
        run.stderr().is_empty(),
        "entering a container that exists is not an error, got {:?}",
        run.stderr()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_begin_with_a_snapshot_when_watching_containers() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    // Spec §18.2 + ADR-0024: a live stream begins with a snapshot of what exists, so a piped
    // consumer that takes one value gets the current state rather than waiting for an event.
    let run = ono_with_runtime(&runtime, "watch container | take 1 | select kind | to json");
    run.assert_success();
    assert_eq!(
        rows(&run),
        vec![serde_yaml_ng::from_str::<Value>(r#"{"kind":"snapshot"}"#).unwrap()],
        "the first value of the stream is the snapshot, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_relate_a_container_to_its_image_when_tracing_it() {
    let directory = scratch();
    let runtime = FakeRuntime::start(&directory, Runtime::Accepting);

    // Spec §9.1: `trace container` shows "... and image relation"; graph.v1 carries it as a node
    // for the container, a node for the image and one directed edge between them.
    let run = ono_with_runtime(&runtime, "trace container web | to json");
    run.assert_success();
    let graphs = rows(&run);
    assert_eq!(
        graphs.len(),
        1,
        "one trace is one graph value, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    let graph = &graphs[0];
    let nodes = graph["nodes"]
        .as_sequence()
        .unwrap_or_else(|| panic!("graph.v1: `nodes` is a list, got {graph:?}"));
    let kinds: Vec<String> = nodes.iter().map(|node| text(node, "kind")).collect();
    assert!(
        kinds.iter().any(|kind| kind == "ono.container/1"),
        "the traced container is a node of the graph, got {kinds:?}"
    );
    assert!(
        kinds.iter().any(|kind| kind == "ono.image/1"),
        "the container's image is a node of the graph, got {kinds:?}"
    );
    let edges = graph["edges"]
        .as_sequence()
        .unwrap_or_else(|| panic!("graph.v1: `edges` is a list, got {graph:?}"));
    let image_edge = edges.iter().find(|edge| {
        edge["from"]["schema"].as_str() == Some("ono.container/1")
            && edge["to"]["schema"].as_str() == Some("ono.image/1")
    });
    let Some(image_edge) = image_edge else {
        panic!("an edge runs from the container to its image, got {edges:?}")
    };
    assert_eq!(
        text(image_edge, "relation"),
        "image",
        "the relationship is named `image`, got {image_edge:?}"
    );
    assert_eq!(
        text(image_edge, "confidence"),
        "exact",
        "spec §22.2: the engine reported the image, it was not inferred, got {image_edge:?}"
    );
}

// --- the fake package managers ------------------------------------------------------------------

/// What the fake `dpkg-query` prints when asked for the installed packages.
#[derive(Clone, Copy)]
enum Listing {
    /// Two installed packages in the `Package\tVersion\tStatus` machine format.
    TwoPackages,
    /// Bytes that are not a listing in any format.
    Garbage,
}

/// Writes fake package-manager executables into `<scratch>/bin` and returns that directory.
fn fake_managers(directory: &Scratch, listing: Listing) -> PathBuf {
    let bin = directory.path().join("bin");
    std::fs::create_dir_all(&bin).expect("create the fake PATH");

    let dpkg_query_body = match listing {
        Listing::TwoPackages => {
            "printf 'curl\\t8.5.0-2\\tinstall ok installed\\n'\nprintf 'nginx\\t1.24.0-1\\tinstall ok installed\\n'\n"
        }
        Listing::Garbage => "printf '\\377\\376 not a listing at all ~~~ %s\\n' \"$*\"\n",
    };
    executable(
        &bin.join("dpkg-query"),
        &format!("#!/bin/sh\n{dpkg_query_body}exit 0\n"),
    );
    executable(
        &bin.join("dpkg"),
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'Debian dpkg package management program version 1.22.6 (amd64).'; exit 0;;\nesac\necho 'dpkg: error: requested operation requires superuser privilege' >&2\nexit 2\n",
    );
    executable(
        &bin.join("apt-cache"),
        "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'apt 2.7.14 (amd64)'; exit 0;;\n  search) printf 'curl - command line tool for transferring data with URL syntax\\nlibcurl4 - easy-to-use client-side URL transfer library\\n'; exit 0;;\nesac\nexit 0\n",
    );
    let apt_get = "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'apt 2.7.14 (amd64)'; exit 0;;\nesac\necho 'E: Could not open lock file /var/lib/dpkg/lock-frontend - open (13: Permission denied)' >&2\necho 'E: Unable to acquire the dpkg frontend lock (/var/lib/dpkg/lock-frontend), are you root?' >&2\nexit 100\n";
    executable(&bin.join("apt-get"), apt_get);
    executable(&bin.join("apt"), apt_get);
    executable(&bin.join("apt-mark"), apt_get);
    bin
}

fn executable(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write the fake manager");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("mark the fake manager executable");
}

/// Runs a one-liner with exactly `bin` as the `PATH`.
fn ono_with_path(bin: &Path, script: &str) -> ono_testkit::Run {
    Shell::new()
        .env("PATH", bin.display().to_string())
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

// --- packages: enumeration and search ----------------------------------------------------------

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_provider_unavailable_when_no_package_manager_is_on_the_path() {
    let directory = scratch();
    let empty = directory.path().join("empty-bin");
    std::fs::create_dir_all(&empty).expect("create an empty PATH");

    // errors.yaml E0401: the backing system is absent — and the refusal says which managers
    // were looked for, so the user knows what would make the command answer.
    let run = ono_with_path(&empty, "get package | to json");
    let stderr = provider_boundary_error(&run, "Ono-Sendai-E0401");
    for manager in ["dpkg", "rpm"] {
        assert!(
            stderr.contains(manager),
            "the refusal names `{manager}` among the managers it looked for, got {stderr:?}"
        );
    }
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_list_installed_packages_when_dpkg_query_answers_in_its_machine_format() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages);

    // Spec §9.1 and §31.58: the provider asks the manager for its explicit machine-readable
    // format and maps each line onto the canonical package record.
    let run = ono_with_path(&bin, "get package | select name version | to json");
    run.assert_success();
    let mut packages: Vec<(String, String)> = rows(&run)
        .iter()
        .map(|row| (text(row, "name"), text(row, "version")))
        .collect();
    packages.sort();
    assert_eq!(
        packages,
        vec![
            ("curl".to_owned(), "8.5.0-2".to_owned()),
            ("nginx".to_owned(), "1.24.0-1".to_owned()),
        ],
        "both packages the manager listed are records, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_resolve_one_package_when_getting_it_by_name() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages);

    // package.yaml: the `name` selector resolves one package.
    let run = ono_with_path(&bin, "get package curl | select name installed | to json");
    run.assert_success();
    let rows = rows(&run);
    assert_eq!(
        rows.len(),
        1,
        "the selector narrows to the one named package, got {:?}; stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    assert_eq!(text(&rows[0], "name"), "curl", "got {:?}", rows[0]);
    assert_eq!(
        rows[0]["installed"].as_bool(),
        Some(true),
        "`install ok installed` is an installed package, got {:?}",
        rows[0]
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_search_the_repositories_when_finding_a_package() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages);

    // Spec §9.1: `find package <query>` searches the provider's repositories.
    let run = ono_with_path(&bin, "find package curl | select name | to json");
    run.assert_success();
    let names: Vec<String> = rows(&run).iter().map(|row| text(row, "name")).collect();
    assert!(
        names.iter().any(|name| name == "curl"),
        "the search result names the package that matches, got {names:?}; stderr {:?}",
        run.stderr()
    );
    assert!(
        names.iter().all(|name| name.contains("curl")),
        "every hit is a package name, not a description line, got {names:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_a_schema_violation_when_the_manager_prints_garbage() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::Garbage);

    // Spec §35.3: unknown is null, never fabricated. A listing that is not in the declared
    // machine format is a provider defect (errors.yaml E0403), not two packages named after
    // whatever the bytes happened to say.
    let run = ono_with_path(&bin, "get package | to json");
    let stderr = provider_boundary_error(&run, "Ono-Sendai-E0403");
    assert!(
        !run.stdout().contains("not a listing"),
        "no record is fabricated from the garbage, got stdout {:?}, stderr {stderr:?}",
        run.stdout()
    );
}

// --- packages: mutations --------------------------------------------------------------------------

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_fail_with_permission_denied_when_adding_a_package_unprivileged() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages);

    // package.yaml: `privilege: elevated`. Spec §16.5 + ADR-0006: the refusal is one `failed`
    // row with io.permission_denied and the exit status is 1 — never a placeholder.
    let run = ono_with_path(&bin, "add package foo | to json");
    assert_failed_row(&single_result(&run), "ono.package.add", "Ono-Sendai-E0302");
    run.assert_status(1);
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_fail_with_permission_denied_when_removing_a_package_unprivileged() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages);

    let run = ono_with_path(&bin, "remove package curl | to json");
    assert_failed_row(
        &single_result(&run),
        "ono.package.remove",
        "Ono-Sendai-E0302",
    );
    run.assert_status(1);
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_fail_with_permission_denied_when_setting_a_package_version_unprivileged() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages);

    let run = ono_with_path(&bin, "set package curl --version 2.0 | to json");
    assert_failed_row(&single_result(&run), "ono.package.set", "Ono-Sendai-E0302");
    run.assert_status(1);
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_name_the_provider_and_the_privilege_when_explaining_a_package_install() {
    let directory = scratch();
    let bin = fake_managers(&directory, Listing::TwoPackages);

    // Spec §17.1/§17.2: elevation is visible before it happens; spec §27: the plan names the
    // provider that will carry the install out.
    let run = ono_with_path(&bin, "explain add package foo");
    run.assert_success();
    let plan = run.stdout().to_owned();
    assert!(
        plan.contains("ono.package.add"),
        "the plan names the command, got {plan:?}"
    );
    assert!(
        plan.lines()
            .any(|line| line.trim_start().starts_with("provider")),
        "the plan names the package provider that answers (spec §27), got {plan:?}"
    );
    assert!(
        plan.contains("elevated"),
        "package.yaml: an install requires elevated privilege and the plan says so, got {plan:?}"
    );
    assert!(
        plan.contains("mutate"),
        "spec §17.1: an install is labelled as a mutation, got {plan:?}"
    );
}
