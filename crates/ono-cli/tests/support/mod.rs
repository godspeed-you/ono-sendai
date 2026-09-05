//! Helpers shared by the `ono-cli` integration suites.
//!
//! Every helper here was declared identically in three or more suites before it moved. A helper
//! copied into each file is a helper that drifts: `text` already existed in seven variants and
//! `rows` in thirteen, and a suite that reads a field slightly differently from its neighbour
//! makes two tests of the same contract disagree about what the contract is.
//!
//! Only helpers that were *byte-for-byte identical* everywhere they appeared live here. Where a
//! suite genuinely needs its own reading — `files.rs` names an ActionResult field in its panic,
//! `storage.rs` reports stderr differently — it keeps its own, because moving it would change
//! what a failing test says (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    dead_code,
    unused_imports,
    reason = "a test states its preconditions directly, and not every helper — nor every \
              re-export — is used by every test binary (AGENTS.md section 16)"
)]

use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use ono_process::{Command, Executor, PtySession, WindowSize};
use ono_testkit::{Scratch, Shell};
use serde_yaml_ng::Value;

/// The string field `field` of a record, or a panic naming the record that lacked it.
pub fn text(row: &Value, field: &str) -> String {
    row[field]
        .as_str()
        .unwrap_or_else(|| panic!("field `{field}` must be a string, got {row:?}"))
        .to_owned()
}

/// The rows of the one JSON array a `to json` stage printed (spec §33.5).
///
/// Both failure messages carry stderr, because a command that answered with a diagnostic instead
/// of rows fails here, and the diagnostic is the thing worth reading.
pub fn rows(run: &ono_testkit::Run) -> Vec<Value> {
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

/// The last line of stdout that carries anything, ignoring trailing blanks.
pub fn last_line(run: &ono_testkit::Run) -> String {
    run.stdout()
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// A TCP listener the test owns on the loopback interface, with the port the kernel chose.
pub fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback listener");
    let port = listener.local_addr().expect("the bound address").port();
    (listener, port)
}

/// A shell that reads and writes nothing outside `dir`, so a test can never see — or leave —
/// state belonging to the person running it.
pub fn isolated(dir: &Scratch) -> Shell {
    Shell::new()
        .env("HOME", dir.path().display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            dir.path().join("xdg").display().to_string(),
        )
        .env(
            "XDG_STATE_HOME",
            dir.path().join("state").display().to_string(),
        )
        .env(
            "ONO_CONFIG_DIR",
            dir.path().join("ono").display().to_string(),
        )
        .env_remove("ONO_CONFIG")
        .timeout(Duration::from_secs(30))
}

/// Everything a pty session emitted up to `needle`, or everything it emitted within `budget`.
///
/// Returning what was seen rather than panicking is deliberate: the caller asserts on the text,
/// so a test that times out reports the screen it was actually looking at.
pub fn read_until(session: &mut PtySession, needle: &str, budget: Duration) -> String {
    let deadline = Instant::now() + budget;
    let mut seen = String::new();
    let mut buffer = [0u8; 4096];
    while Instant::now() < deadline {
        match session.read_timeout(&mut buffer, Duration::from_millis(200)) {
            Ok(Some(0)) | Err(_) => break,
            Ok(Some(count)) => {
                seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
                if seen.contains(needle) {
                    return seen;
                }
            }
            Ok(None) => {}
        }
    }
    seen
}

/// The bounded runner now lives in the testkit, so every suite that needs one uses the same
/// one (v0.4.1 §39.1, §39.2; ADR-0427). Re-exported here because the suites already say
/// `support::run_bounded`.
pub use ono_testkit::{Bounded, run_bounded};

pub fn ono_at_home(home: &Scratch, script: &str) -> ono_testkit::Run {
    Shell::new()
        .env("HOME", home.path().to_string_lossy().into_owned())
        .env(
            "XDG_CONFIG_HOME",
            home.path().to_string_lossy().into_owned(),
        )
        .args(["-c", script])
        .run()
}

/// The manifest of the SDK's example provider package, as a plugin home holds it on disk.
///
/// Two suites lay this package out — one for the read path of ADR-0582, one for the provider
/// registration of ADR-0583 — and both need the same bytes: the fixture on disk and the
/// `Manifest` a direct load parses must agree, or the two suites are testing two packages.
pub fn echo_package_manifest(id: &str) -> String {
    format!(
        r#"
format: kuang-package/1
package:
  id: {id}
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
    - clock.read
network:
  outbound: none
contributions:
  targets: [contributions/targets.yaml]
"#
    )
}

pub fn ono_with_plugins(home: &ono_testkit::Scratch, script: &str) -> ono_testkit::Run {
    let root = home.path();
    Shell::new()
        .args(["-c", script])
        .env(
            "ONO_PLUGIN_PATH",
            root.join("plugins").display().to_string(),
        )
        .env("HOME", root.join("home").display().to_string())
        .env("XDG_STATE_HOME", root.join("state").display().to_string())
        .env("XDG_CONFIG_HOME", root.join("config").display().to_string())
        .env(
            "ONO_CONFIG_DIR",
            root.join("config/ono").display().to_string(),
        )
        .timeout(Duration::from_secs(30))
        .run()
}

pub fn binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("the test binary knows where it is");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("ono")
}

/// Starts `ono` interactively on a pseudo-terminal, in `directory`.
pub fn interactive_shell_in(directory: &Scratch) -> PtySession {
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", directory.path().display().to_string())
        .current_dir(directory.path());
    executor
        .run_pty(&command, WindowSize::new(24, 100))
        .expect("a pseudo-terminal must be available")
}

pub fn assert_failed_row(row: &Value, operation: &str, code: &str) {
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

pub fn executable(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write the fake manager");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("mark the fake manager executable");
}

/// Runs a one-liner with exactly `bin` as the `PATH`.
pub fn ono_with_path(bin: &Path, script: &str) -> ono_testkit::Run {
    Shell::new()
        .env("PATH", bin.display().to_string())
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

/// Parses the JSON document `to json` wrote as the stream's values.
/// The one `ono.action-result/1` row a single-target mutation emits.
pub fn single_result(run: &ono_testkit::Run) -> Value {
    let mut rows = rows(run);
    assert_eq!(
        rows.len(),
        1,
        "spec §11.5: one ActionResult per target, got {:?}",
        run.stdout()
    );
    rows.remove(0)
}

pub fn items(value: &Value) -> &[Value] {
    value
        .as_sequence()
        .unwrap_or_else(|| {
            panic!("`to json` emits an array of the stream's values (spec §33.5), got {value:?}")
        })
        .as_slice()
}

/// Parses one line of `to json` output. JSON is YAML, so the workspace's YAML parser reads it.
pub fn json(text: &str) -> Value {
    serde_yaml_ng::from_str(text).unwrap_or_else(|error| {
        panic!("`to json` emits a JSON document (spec §33.5): {error}\n{text}")
    })
}

/// The end of the JSON document that starts at `chars[start]`, or `None` when it never closes.
pub fn balanced_end(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, character) in chars.iter().enumerate().skip(start) {
        if in_string {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// The first value anywhere in `document` stored under `key`.
pub fn search(document: &Value, key: &str) -> Option<Value> {
    match document {
        Value::Mapping(mapping) => {
            for (name, value) in mapping {
                if name.as_str() == Some(key) {
                    return Some(value.clone());
                }
            }
            mapping.values().find_map(|value| search(value, key))
        }
        Value::Sequence(items) => items.iter().find_map(|item| search(item, key)),
        _ => None,
    }
}

/// This shell's own fingerprint — what the agent's store would have to name.
pub fn client_fingerprint(home: &Scratch) -> String {
    let printed = std::process::Command::new(binary())
        .arg("--print-peer-key")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .output()
        .expect("the peer key is printable");
    String::from_utf8_lossy(&printed.stdout).trim().to_owned()
}

/// The value at a dotted path, falling back to the first field anywhere in the document whose key
/// is the path's last segment. v0.4 fixes the facts a `PlaceView` (§6.1) and a `SpatialMap` (§22)
/// carry, not how a `PlaceView` nests them.
pub fn field(document: &Value, path: &str) -> Value {
    let mut cursor = document.clone();
    for segment in path.split('.') {
        match cursor.get(segment) {
            Some(next) => cursor = next.clone(),
            None => {
                let last = path.rsplit('.').next().unwrap_or(path);
                return search(document, last).unwrap_or(Value::Null);
            }
        }
    }
    cursor
}

/// A `sleep` child the test owns: its pid is a target nobody else will touch, and it is killed
/// **and reaped** when the test ends, whether or not the shell got to it first.
///
/// Three suites had written this fixture identically before it moved here (v0.4.1 §39.1). Two
/// others keep their own, because theirs are not copies: `spatial_pins.rs` waits for the child to
/// settle and `spatial_topology.rs` gives it a duration nothing else on the host shares, and
/// unifying either would change what its tests do (ADR-0427).
pub struct SleepChild(Child);

impl SleepChild {
    pub fn spawn() -> Self {
        let child = std::process::Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("`sleep` is available on every test host");
        Self(child)
    }

    pub fn pid(&self) -> u32 {
        self.0.id()
    }

    /// The signal the child died from, waiting up to `budget` for it to die; `None` if it is
    /// still alive when the budget runs out.
    pub fn signal_within(&mut self, budget: Duration) -> Option<i32> {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if let Some(status) = self.0.try_wait().expect("try_wait works on an owned child") {
                return status.signal();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    /// The niceness of the child as the kernel reports it in `/proc/<pid>/stat`, field 19 —
    /// the system state a `set process --priority` must leave behind.
    pub fn niceness(&self) -> i64 {
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", self.pid()))
            .expect("the child's stat is readable while it lives");
        let after_comm = stat
            .rsplit_once(')')
            .map(|(_, rest)| rest)
            .expect("stat has a comm in parentheses");
        after_comm
            .split_whitespace()
            .nth(16)
            .and_then(|field| field.parse().ok())
            .expect("stat carries the nice value as its 19th field")
    }
}

impl Drop for SleepChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn list_at(document: &Value, path: &str, what: &str) -> Vec<Value> {
    field(document, path)
        .as_sequence()
        .unwrap_or_else(|| panic!("{what} — `{path}` must be a list, got {document:?}"))
        .clone()
}
