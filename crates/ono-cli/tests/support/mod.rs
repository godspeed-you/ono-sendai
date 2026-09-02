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

/// The bounded runner now lives in the testkit, so every suite that needs one uses the same
/// one (v0.4.1 §39.1, §39.2; ADR-0427). Re-exported here because the suites already say
/// `support::run_bounded`.
pub use ono_testkit::{Bounded, run_bounded};
