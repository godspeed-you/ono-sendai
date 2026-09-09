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

/// One `ono -c <script>` with colour off and a generous budget.
///
/// The four temporal suites each declared this identically before it moved here. It takes no
/// home, so it is for scripts that neither record nor read a store; a suite that needs a private
/// store uses [`ono_at_home`] instead.
pub fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .env("NO_COLOR", "1")
        .timeout(Duration::from_secs(60))
        .run()
}

/// A shell whose history is persisted under `home`, as `temporal.recording.enabled` makes it.
///
/// Every XDG root is inside the scratch directory, so the store the recorder opens is the test's
/// own and the developer's history is neither read nor written. `ONO_TEMPORAL_RECORDING_ENABLED`
/// is the environment spelling of the setting (`crates/ono-cli/src/settings.rs`), which is how a
/// test asks for v0.5 §10.2's opt-in without writing a configuration file.
pub fn recording_shell(home: &Scratch, script: &str) -> ono_testkit::Run {
    let root = home.path().to_string_lossy().into_owned();
    Shell::new()
        .env("NO_COLOR", "1")
        .env("HOME", root.clone())
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .env("ONO_TEMPORAL_RECORDING_ENABLED", "true")
        .args(["-c", script])
        .timeout(Duration::from_secs(60))
        .run()
}

/// A background `sleep` this process owns, for a test that needs a real process to act on.
///
/// Started here rather than through the shell, so stopping it is a mutation Ono makes against a
/// process it did not create — which is what §17.1's action lifecycle is about.
pub fn fixture_process() -> std::process::Child {
    std::process::Command::new("sleep")
        .arg("120")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("a fixture process starts")
}

/// A recording home whose ledger already holds one Ono action's four lifecycle events (§17.2).
///
/// The mutation runs in an invocation of its own, so a query asked afterwards is asked in a
/// *later* shell and can only answer from what was retained — which is what makes it a test of the
/// store rather than of the session that filled it.
///
/// # Panics
///
/// Panics if the mutation did not happen, because every assertion built on it would otherwise be
/// a statement about an empty ledger.
pub fn home_with_a_recorded_action() -> Scratch {
    let home = ono_testkit::scratch();
    let mut victim = fixture_process();
    let run = recording_shell(&home, &format!("stop process {}", victim.id()));
    let _ = victim.kill();
    let _ = victim.wait();
    assert!(
        run.status().is_success(),
        "the fixture needs one real Ono mutation to have happened; `stop process` said {:?}",
        run.output()
    );
    home
}

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
    echo_package_manifest_with(id, &[])
}

/// The same manifest, declaring the v0.2 §31.7 relation shapes `shapes` names.
///
/// A shape is what a package says about the graph before any of its code runs, so it belongs to
/// the manifest and not to the handshake — and a suite that is about relations declares its own
/// shapes for the same reason a suite that is about targets declares its own targets.
pub fn echo_package_manifest_with(id: &str, shapes: &[&str]) -> String {
    let relations = if shapes.is_empty() {
        String::new()
    } else {
        format!(
            "\n  relations: [{}]",
            shapes
                .iter()
                .map(|shape| format!("\"{shape}\""))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
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
    - relation.write
network:
  outbound: none
contributions:
  targets: [contributions/targets.yaml]{relations}
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

/// Starts `ono` interactively on a pseudo-terminal, with `plugin_path` as its plugin path.
///
/// Completion is the one surface that exists only at a terminal, so a contributed command's
/// declared option can be proven to reach it only from a pty. The plugin path is an argument
/// rather than a fixed subdirectory because the two package fixtures lay their packages out
/// differently, and neither layout is the point of the test.
pub fn interactive_shell_with_plugins(directory: &Scratch, plugin_path: &Path) -> PtySession {
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", directory.path().display().to_string())
        .env("ONO_PLUGIN_PATH", plugin_path.display().to_string())
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

/// Lays the SDK's example package out in a scratch plugin home, with the target document the
/// calling suite wants it to declare.
///
/// The manifest is [`echo_package_manifest`], so every suite that lays the package out lays out
/// the same package; `targets` is the one thing that differs between them, because a suite
/// declares the targets it is about (v0.4.1 §39.1).
pub fn lay_out_echo_package(root: &Path, id: &str, targets: &str) {
    lay_out_echo_package_with(root, id, targets, &[]);
}

/// The same package, declaring `shapes` as its `contributions.relations`.
pub fn lay_out_echo_package_with(root: &Path, id: &str, targets: &str, shapes: &[&str]) {
    let package = root.join(id);
    std::fs::create_dir_all(package.join("runtime")).expect("the runtime directory");
    std::fs::write(
        package.join("manifest.yaml"),
        echo_package_manifest_with(id, shapes),
    )
    .expect("the manifest");
    std::fs::create_dir_all(package.join("contributions")).expect("the contributions directory");
    std::fs::write(package.join("contributions/targets.yaml"), targets).expect("the document");
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    std::fs::copy(&binary, package.join("runtime/echo"))
        .expect("the example plugin binary is built");
}

/// A scratch plugin home holding the example package, declaring `targets`.
pub fn echo_plugin_home(id: &str, targets: &str) -> Scratch {
    echo_plugin_home_with(id, targets, &[])
}

/// A scratch plugin home holding the example package, declaring `targets` and the relation
/// shapes `shapes` names.
pub fn echo_plugin_home_with(id: &str, targets: &str, shapes: &[&str]) -> Scratch {
    let scratch = ono_testkit::scratch();
    lay_out_echo_package_with(&scratch.path().join("plugins"), id, targets, shapes);
    scratch
}

/// The rows of the last `to json` document on stdout (§33.5).
///
/// [`rows`] reads a stdout that holds nothing else; this one reads the stdout of a script whose
/// earlier statements printed prose — `load plugin …; get … | to json` — which is what every suite
/// that drives a KUANG/11 package has. Two of them had written it identically before it moved here
/// (v0.4.1 §39.1).
pub fn last_json_rows(run: &ono_testkit::Run) -> Vec<Value> {
    last_json_document(run)
        .as_sequence()
        .unwrap_or_else(|| panic!("a sequence of records, got {:?}", run.output()))
        .clone()
}

/// The last `to json` document on stdout, for a script whose earlier statements print prose.
pub fn last_json_document(run: &ono_testkit::Run) -> serde_yaml_ng::Value {
    let line = run
        .stdout()
        .lines()
        .rfind(|line| line.starts_with('['))
        .unwrap_or_else(|| panic!("a `to json` document on stdout, got {:?}", run.output()));
    json(line)
}

/// A deterministic signing key for a fixture, seeded so two suites agree on the publisher.
pub fn key(seed: u8) -> ono_kuang_protocol::SecretKey {
    ono_kuang_protocol::SecretKey::from_bytes(&[seed; 32])
}

/// Signs the package in `directory` with `key`, as a package author would (v0.4.1 §17.2).
///
/// Two suites had written it identically before it moved here (v0.4.1 §39.1).
pub fn sign(directory: &Path, key: &ono_kuang_protocol::SecretKey) {
    let text = std::fs::read_to_string(directory.join("manifest.yaml")).expect("the manifest");
    let manifest =
        ono_kuang_protocol::Manifest::parse(&text).expect("the fixture manifest is valid");
    let described = ono_kuang_protocol::SignedPackage::new(
        &manifest.package.id,
        &manifest.package.version,
        &manifest.package.publisher,
        ono_kuang_protocol::artifact_files(directory),
    )
    .expect("the fixture package is describable");
    std::fs::write(
        directory.join(ono_kuang_protocol::SIGNATURE_FILE),
        key.sign(&described).to_yaml(),
    )
    .expect("the signature is written");
}

// --- the KUANG/11 reference package, for the suites that install and decide (K11P, K11A) -----

/// The reference manifest of the suites that drive the permission layer: a `kuang-package/2`
/// package with the seven-permission shape K11P §26.1 describes, under the echo runtime.
pub fn declared_manifest(id: &str, name: &str, version: &str, kubeconfig: &str) -> String {
    format!(
        r#"
format: kuang-package/2
package:
  id: {id}
  name: {name}
  version: {version}
  description: Emits what it is asked to emit.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.2 <12"
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
    - network.connect
    - process.signal
    - filesystem.read: {{paths: ["{kubeconfig}"]}}
    - secret.use
    - clock.read
    - relation.write
    - process.exec
network:
  outbound: none
contributions:
  commands: [contributions/commands.yaml]
permissions:
  profiles:
    minimal:
      title: Minimal
      permissions: [spatial-relations]
    recommended:
      title: Recommended
      permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations]
    operate:
      title: Observe and change
      permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations, cluster-mutation]
  requests:
    - id: cluster-access
      kind: external-observe
      title: Connect to clusters
      phase: install
      recommended: true
      grants:
        - capability: network.connect
          scope: runtime-derived
    - id: kubeconfig-read
      kind: filesystem-read
      title: Read cluster configuration
      phase: install
      recommended: true
      grants:
        - capability: filesystem.read
          scope: {{paths: ["{kubeconfig}"]}}
    - id: credential-use
      kind: secret-use
      title: Use credentials without exposing their values
      phase: install
      recommended: true
      grants:
        - capability: secret.use
    - id: spatial-relations
      kind: local-contribution
      title: Add relationships to Ono
      phase: automatic
      recommended: true
      grants:
        - capability: relation.write
          scope: package-contributions
    - id: login-helper
      kind: execute-helper
      title: Run an external login helper when a context requires it
      purpose: authenticate to the selected context
      phase: jit
      recommended: false
      grants:
        - capability: process.exec
          scope: runtime-derived
    - id: cluster-mutation
      kind: host-change
      title: Change resources
      phase: explicit
      recommended: false
      grants:
        - capability: process.signal
"#
    )
}

/// The commands the reference manifest contributes.
pub const COMMANDS: &str = r#"commands:
  - id: dev.example.echo.command.emit
    verb: get
    target: echo-item
    summary: Emit a counted stream of integers.
    output: stream<int>
    argument_mode: expression
    capabilities: []
    examples:
      - get echo-item --count 3
  - id: dev.example.echo.command.exec
    verb: get
    target: echo-exec
    summary: Run a program through the host and report its output.
    output: stream<string>
    argument_mode: expression
    capabilities: [process.exec]
    examples:
      - get echo-exec --program /bin/true
  - id: dev.example.echo.command.signal
    verb: get
    target: echo-signal
    summary: Send a signal to a process through the host.
    output: stream<string>
    argument_mode: expression
    capabilities: [process.signal]
    examples:
      - get echo-signal --pid 1 --signal TERM
"#;

/// Lays a package out under `root/<id>`: manifest, contributions, the example runtime.
pub fn lay_out(root: &Path, id: &str, manifest: &str) -> PathBuf {
    let package = root.join(id);
    std::fs::create_dir_all(package.join("runtime")).expect("the runtime directory");
    std::fs::create_dir_all(package.join("contributions")).expect("the contributions directory");
    std::fs::write(package.join("manifest.yaml"), manifest).expect("the manifest");
    std::fs::write(
        package.join("contributions/commands.yaml"),
        COMMANDS.replace("dev.example.echo", id),
    )
    .expect("the contributions");
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    std::fs::copy(&binary, package.join("runtime/echo"))
        .expect("the example plugin binary is built");
    package
}

/// An operator catalog naming `entries` as `(id, name, version, artifact)`.
pub fn catalog(
    home: &ono_testkit::Scratch,
    file: &str,
    catalog_name: &str,
    entries: &[(&str, &str, &str, &str)],
) {
    let mut text = format!(
        "format: kuang-catalog/1\ncatalog:\n  name: {catalog_name}\n  description: A test catalog.\nentries:\n"
    );
    for (id, name, version, artifact) in entries {
        let publisher = id
            .rsplit_once('.')
            .map(|(publisher, _)| publisher)
            .unwrap_or(id);
        text.push_str(&format!(
            "  - id: {id}\n    name: {name}\n    description: A package from the catalog.\n    publisher: {publisher}\n    releases:\n      - version: {version}\n        platforms: [linux-amd64, linux-arm64]\n        kuang_api: \">=11.1 <12\"\n        artifact: \"{artifact}\"\n"
        ));
    }
    home.write(format!("config/ono/kuang/catalogs/{file}.yaml"), text);
}

/// An operator catalog whose releases carry a digest, and that may allow plain `http://`
/// artifacts (K11A §6): entries as `(id, name, version, artifact, digest)`.
pub fn catalog_with_digests(
    home: &ono_testkit::Scratch,
    file: &str,
    catalog_name: &str,
    insecure_http: bool,
    entries: &[(&str, &str, &str, &str, &str)],
) {
    let mut text = format!(
        "format: kuang-catalog/1\ncatalog:\n  name: {catalog_name}\n  description: A test catalog.\n  insecure_http: {insecure_http}\nentries:\n"
    );
    for (id, name, version, artifact, digest) in entries {
        let publisher = id
            .rsplit_once('.')
            .map(|(publisher, _)| publisher)
            .unwrap_or(id);
        text.push_str(&format!(
            "  - id: {id}\n    name: {name}\n    description: A package from the catalog.\n    publisher: {publisher}\n    releases:\n      - version: {version}\n        platforms: [linux-amd64, linux-arm64]\n        kuang_api: \">=11.1 <12\"\n        artifact: \"{artifact}\"\n        digest: \"{digest}\"\n"
        ));
    }
    home.write(format!("config/ono/kuang/catalogs/{file}.yaml"), text);
}

/// Where the reference manifest pins `kubeconfig-read`.
pub fn kubeconfig_of(home: &ono_testkit::Scratch) -> String {
    format!("{}/.kube/config", home.path().join("home").display())
}

/// Runs a script with the scratch root as the whole world: the plugin home, the local package
/// source, the system source root, the configuration and the cache all under it.
pub fn kuang_shell(home: &ono_testkit::Scratch, script: &str) -> ono_testkit::Run {
    let root = home.path();
    ono_testkit::Shell::new()
        .args(["-c", script])
        .env(
            "ONO_PLUGIN_PATH",
            root.join("plugins").display().to_string(),
        )
        .env(
            "ONO_PLUGIN_SOURCES",
            root.join("sources").display().to_string(),
        )
        .env(
            "ONO_SYSTEM_CONFIG_DIR",
            root.join("system").display().to_string(),
        )
        .env(
            "ONO_PLUGIN_SYSTEM_SOURCES",
            root.join("system-sources").display().to_string(),
        )
        .env("HOME", root.join("home").display().to_string())
        .env("XDG_STATE_HOME", root.join("state").display().to_string())
        .env("XDG_CONFIG_HOME", root.join("config").display().to_string())
        .env("XDG_CACHE_HOME", root.join("cache").display().to_string())
        .env(
            "ONO_CONFIG_DIR",
            root.join("config/ono").display().to_string(),
        )
        .timeout(Duration::from_secs(30))
        .run()
}
