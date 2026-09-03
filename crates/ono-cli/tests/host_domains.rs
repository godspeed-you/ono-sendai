//! The host API domains a package reaches through the shell (spec §31.12; ADR-0567): the
//! context the session publishes, and the schemas the shell registers — pulled as a stream.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::{Scratch, Shell, scratch};

/// The example package, installed under `<scratch>/plugins`, requesting the read capabilities.
fn plugin_home(home: &Scratch) {
    home.write(
        "plugins/dev.example.echo/manifest.yaml",
        r#"format: kuang-package/1
package:
  id: dev.example.echo
  name: echo
  version: 0.1.0
  description: Emits what it is asked to emit.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: native-process
  entry: runtime/echo
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
capabilities:
  optional:
    - context.read
    - schema.read
    - object.read
    - history.read
    - secret.use: {secrets: ["api-token"]}
    - process.exec: {programs: ["/bin/**", "/usr/bin/**"]}
    - network.connect: {hosts: ["127.0.0.1"], ports: ["*"]}
network:
  outbound: none
"#,
    );
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    let entry = home.path().join("plugins/dev.example.echo/runtime/echo");
    std::fs::create_dir_all(entry.parent().expect("a parent")).expect("the runtime directory");
    std::fs::copy(&binary, &entry).expect("the example plugin binary is built");
}

fn ono(home: &Scratch, script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .env("HOME", home.path().display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            home.path().join("config").display().to_string(),
        )
        .env(
            "XDG_STATE_HOME",
            home.path().join("state").display().to_string(),
        )
        .env(
            "ONO_PLUGIN_PATH",
            home.path().join("plugins").display().to_string(),
        )
        .cwd(home.path())
        .run()
}

#[test]
fn should_tell_a_granted_package_where_the_session_stands_and_nothing_beyond_it() {
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        "grant capability context.read --plugin dev.example.echo | count; load plugin dev.example.echo; echo:context | to json",
    );
    run.assert_success();
    let shown = run.stdout();
    assert!(
        shown.contains(&format!("\\\"cwd\\\":\\\"{}\\\"", home.path().display())),
        "the working directory the session stands in (spec §31.12); stdout {shown:?}"
    );
    assert!(
        shown.contains("\\\"interactive\\\":false") && shown.contains("\\\"link\\\":null"),
        "a `-c` run is not interactive and is on no link; stdout {shown:?}"
    );
    assert!(
        !shown.contains("HOME=") && !shown.contains("\\\"environment\\\""),
        "no environment reaches a package through the context; stdout {shown:?}"
    );
}

#[test]
fn should_stream_the_shells_schemas_to_a_granted_package() {
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        "grant capability schema.read --plugin dev.example.echo | count; load plugin dev.example.echo; echo:schemas --prefix ono.proc | to json",
    );
    run.assert_success();
    let shown = run.stdout();
    assert!(
        shown.contains("\"ono.process/1\"") && shown.contains("\"ono.process-detail/1\""),
        "the core schemas under the prefix, pulled through streams.next; stdout {shown:?}"
    );
}

#[test]
fn should_refuse_the_context_to_a_package_without_the_grant() {
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        "load plugin dev.example.echo; echo:context | to json",
    );
    assert_ne!(run.status().code(), 0, "stdout {:?}", run.stdout());
    assert!(
        run.stderr().contains("Ono-Sendai-K11301"),
        "deny by default (spec §31.19); stderr {:?}",
        run.stderr()
    );
}

#[test]
fn should_stream_the_sessions_objects_to_a_granted_package_with_its_limit() {
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        "grant capability object.read --plugin dev.example.echo | count; load plugin dev.example.echo; echo:objects --target env --limit 2 | to json",
    );
    run.assert_success();
    let shown = run.stdout();
    let listed: Vec<&str> = shown
        .lines()
        .last()
        .unwrap_or_default()
        .split("\",\"")
        .collect();
    assert_eq!(
        listed.len(),
        2,
        "two env objects, the limit honoured through the stream; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
}

#[test]
fn should_hand_a_granted_package_the_redacted_history_the_session_keeps() {
    let home = scratch();
    plugin_home(&home);
    home.write(
        "state/ono/history.jsonl",
        "{\"id\":\"h1\",\"at\":\"2026-09-03T10:00:00Z\",\"command\":\"get process\",\"cwd\":\"/\",\"status\":0,\"session\":\"s1\"}\n{\"id\":\"h2\",\"at\":\"2026-09-03T10:01:00Z\",\"command\":\"curl -H \\\"Authorization: Bearer sk-live-123\\\" https://x\",\"cwd\":\"/\",\"status\":0,\"session\":\"s1\"}\n",
    );
    let run = ono(
        &home,
        "grant capability history.read --plugin dev.example.echo | count; load plugin dev.example.echo; echo:history | to json",
    );
    run.assert_success();
    let shown = run.stdout();
    assert!(
        shown.contains("get process"),
        "the history reaches the package; stdout {shown:?}"
    );
    assert!(
        !shown.contains("sk-live-123"),
        "a secret-bearing value reaches a package redacted (ADR-0015 T8); stdout {shown:?}"
    );
}

#[test]
fn should_say_the_shell_has_no_secret_store_and_record_the_attempt() {
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        r#"grant capability secret.use --plugin dev.example.echo --scope "secrets=api-token" | count; load plugin dev.example.echo; echo:secret --name api-token | to json; get audit --plugin dev.example.echo | where action == "secrets.request" | select result | to json"#,
    );
    let shown = run.stdout();
    assert!(
        !shown.contains("released"),
        "no handle is issued by a host without a secret store; stdout {shown:?}"
    );
    assert!(
        shown.contains(r#"{"result":"failed"}"#),
        "the refused request is in the trail with its outcome; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
}

#[test]
fn should_run_a_program_for_a_granted_package_under_the_hosts_confinement() {
    let home = scratch();
    plugin_home(&home);
    // The check is against the resolved program (ADR-0015 T11), so the scope names where
    // `/bin/echo` really lives on this host.
    let echo = std::fs::canonicalize("/bin/echo").expect("/bin/echo exists");
    let bundle = echo.parent().expect("a directory").display().to_string();
    let run = ono(
        &home,
        &format!(
            r#"grant capability process.exec --plugin dev.example.echo --scope "programs={bundle}/**" | count; load plugin dev.example.echo; echo:exec --program /bin/echo --arguments "[\"through\", \"the\", \"broker\"]" | to json"#
        ),
    );
    run.assert_success();
    let shown = run.stdout();
    assert!(
        shown.contains("\"stdout: through the broker\"") && shown.contains("\"exited: 0\""),
        "the program's lines and its exit status come back as a stream; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
}

#[test]
fn should_broker_a_tcp_connection_for_a_granted_package() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        if let Ok((mut socket, _)) = listener.accept() {
            let mut buffer = [0u8; 64];
            if let Ok(count) = socket.read(&mut buffer) {
                let _ = socket.write_all(b"pong: ");
                let _ = socket.write_all(&buffer[..count]);
            }
        }
    });
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        &format!(
            r#"grant capability network.connect --plugin dev.example.echo --scope "hosts=127.0.0.1" | count; load plugin dev.example.echo; echo:connect --host 127.0.0.1 --port {port} --send "ping" | to json"#
        ),
    );
    run.assert_success();
    let shown = run.stdout();
    assert!(
        shown.contains("\"pong: ping\""),
        "the bytes went out and came back through the broker; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
}
