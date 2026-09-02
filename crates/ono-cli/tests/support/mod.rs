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
    reason = "a test states its preconditions directly, and not every helper is used by every \
              test binary (AGENTS.md section 16)"
)]

use std::net::TcpListener;
use std::time::{Duration, Instant};

use ono_process::PtySession;
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

/// What a run produced within a budget it was not allowed to exceed.
///
/// [`ono_testkit::Shell`] panics when a run overruns and leaves the child behind, which is the
/// right shape for a suite whose subject is expected to answer. A test whose *subject is the
/// hang* needs the opposite: the overrun is the observation, and the child must be gone before
/// the assertion is written, so an ignored proof cannot leak a shell onto the machine that ran
/// it (issue #22 found two such strays holding a pipeline open for seven hours).
#[derive(Debug)]
pub struct Bounded {
    /// The script that was run, so a failure message can quote it.
    pub script: String,
    /// The budget it was given.
    pub budget: Duration,
    /// Whether it finished on its own rather than being killed at the deadline.
    pub finished: bool,
    /// Its exit code, when it exited with one.
    pub code: Option<i32>,
    /// Everything it wrote to standard output before it finished or was killed.
    pub stdout: String,
    /// Everything it wrote to standard error before it finished or was killed.
    pub stderr: String,
}

impl Bounded {
    /// Whether the run said nothing at all on either stream — v0.4.1 §33.3's "neither output nor
    /// progress".
    pub fn silent(&self) -> bool {
        self.stdout.trim().is_empty() && self.stderr.trim().is_empty()
    }

    /// The run, its budget and both streams, as a failure message reads them.
    pub fn report(&self) -> String {
        format!(
            "`ono -c {:?}` {} within {:?}\n--- stdout ({} bytes) ---\n{}\n--- stderr ({} bytes) ---\n{}",
            self.script,
            if self.finished {
                format!("exited {:?}", self.code)
            } else {
                "was still running and was killed".to_owned()
            },
            self.budget,
            self.stdout.len(),
            self.stdout,
            self.stderr.len(),
            self.stderr
        )
    }
}

/// Runs `script` through the real binary with `dir` as its whole configuration home, and kills it
/// at `budget` rather than waiting for it.
///
/// Both streams are drained on their own threads, so what comes back is what a user would have
/// seen by the deadline even when the shell never reaches the end of its pipeline.
pub fn run_bounded(dir: &Scratch, script: &str, budget: Duration) -> Bounded {
    let mut child = std::process::Command::new(ono_testkit::ono_binary())
        .args(["-c", script])
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path().join("xdg"))
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env("ONO_CONFIG_DIR", dir.path().join("ono"))
        .env("NO_COLOR", "1")
        .env_remove("ONO_CONFIG")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the ono binary must be built before an integration test runs it");

    let out = drain(child.stdout.take().expect("stdout was piped"));
    let err = drain(child.stderr.take().expect("stderr was piped"));

    let deadline = Instant::now() + budget;
    let mut finished = false;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => {
                finished = true;
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }
    if !finished {
        let _ = child.kill();
    }
    // Reaped on every path, including the one that killed it, so an overrunning proof leaves no
    // zombie behind the tests that run after it.
    let status = child.wait().ok();

    Bounded {
        script: script.to_owned(),
        budget,
        finished,
        code: status.and_then(|exited| exited.code()),
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    }
}

/// Reads a pipe to its end on a worker, so neither stream can deadlock the other.
fn drain(mut pipe: impl std::io::Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = pipe.read_to_end(&mut buffer);
        String::from_utf8_lossy(&buffer).into_owned()
    })
}
