//! Helpers shared by the v0.6 change suites.
//!
//! Every change command reads or writes the plan store, and the store is under `XDG_DATA_HOME`.
//! A suite that ran without setting it would read and write the developer's own plans, so every
//! helper here starts from a scratch home and points every XDG root inside it — the arrangement
//! `support::recording_shell` already uses for the temporal recorder.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    dead_code,
    reason = "a test states its preconditions directly, and not every helper is used by every \
              test binary (AGENTS.md section 16)"
)]

use std::path::Path;
use std::time::Duration;

use ono_testkit::{Run, Scratch, Shell, scratch};
use serde_yaml_ng::Value;

/// A scratch home for one test, so the plan store this test writes is nobody else's.
pub fn home() -> Scratch {
    scratch()
}

/// A home on the disk the build uses, removed when the test ends.
///
/// A protected change needs an asset, and §11.2 refuses to protect a volatile filesystem. [`home`]
/// is under the system temporary directory, which is tmpfs on many hosts, so a suite that needs a
/// real asset makes its home under Cargo's per-target temporary directory instead.
pub struct BuildDiskHome(std::path::PathBuf);

impl BuildDiskHome {
    /// The home's directory.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for BuildDiskHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A [`BuildDiskHome`] for one test of `suite`, unique within the run.
pub fn build_disk_home(suite: &str) -> BuildDiskHome {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("{suite}-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&path).expect("a home on the build disk");
    BuildDiskHome(path)
}

/// A process a test owns — `sleep 600` — killed and reaped when the test ends, whatever the plan
/// did to it.
///
/// A plan that kills a process needs a real one to freeze (§4.3, §21.4), and the process must
/// outlive the plan while never outliving the test.
pub struct Sleeper(std::process::Child);

impl Sleeper {
    /// Starts the process.
    pub fn start() -> Self {
        Self(
            std::process::Command::new("sleep")
                .arg("600")
                .spawn()
                .expect("a sleeping process can be started"),
        )
    }

    /// Its process id.
    pub fn pid(&self) -> u32 {
        self.0.id()
    }

    /// Whether it is still running.
    pub fn is_alive(&mut self) -> bool {
        self.0
            .try_wait()
            .expect("the child can be polled")
            .is_none()
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Runs `script` in a shell whose plan store, configuration and state live under `home`.
pub fn ono_at(home: &Path, script: &str) -> Run {
    let root = home.to_string_lossy().into_owned();
    Shell::new()
        .env("NO_COLOR", "1")
        .env("HOME", root.clone())
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .args(["-c", script])
        .timeout(Duration::from_secs(120))
        .run()
}

/// The same shell with one `ONO_*` setting written, as ADR-0010 spells a setting in the
/// environment.
pub fn ono_with(home: &Path, key: &str, value: &str, script: &str) -> Run {
    let root = home.to_string_lossy().into_owned();
    Shell::new()
        .env("NO_COLOR", "1")
        .env("HOME", root.clone())
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .env(key, value)
        .args(["-c", script])
        .timeout(Duration::from_secs(120))
        .run()
}

/// The same shell with v0.5 recording switched on, for a test about the ledger.
pub fn recording_at(home: &Path, script: &str) -> Run {
    ono_with(home, "ONO_TEMPORAL_RECORDING_ENABLED", "true", script)
}

/// The rows of the one JSON array a `to json` stage printed (spec §33.5).
pub fn rows(run: &Run) -> Vec<Value> {
    let text = run.stdout().trim().to_owned();
    let stderr = run.stderr();
    let document: Value = serde_yaml_ng::from_str(&text).unwrap_or_else(|error| {
        panic!("`to json` must emit a JSON document, got {text:?} ({error}); stderr: {stderr:?}")
    });
    document
        .as_sequence()
        .unwrap_or_else(|| {
            panic!("spec §33.5: `to json` emits the stream as an array, got {text:?}")
        })
        .clone()
}

/// The one record a `to json` stage printed.
pub fn one(run: &Run) -> Value {
    let rows = rows(run);
    assert_eq!(
        rows.len(),
        1,
        "the command answers with one record, got {rows:?}"
    );
    rows[0].clone()
}

// §39.1: one definition per job. The plan suites read the same string fields out of the same
// `to json` rows that every other suite does, so `text` is the one in `tests/support/mod.rs`
// rather than a second copy that could start disagreeing with it.
#[path = "../support/mod.rs"]
mod shared;
pub use shared::text;

/// Plans one mutation in `home` and answers with the short reference of the plan it produced.
pub fn plan(home: &Path, mutation: &str) -> String {
    let run = ono_at(home, &format!("plan {mutation} | to json"));
    run.assert_success();
    let record = one(&run);
    text(&record, "id")[..4].to_owned()
}

/// The name of a service this host actually serves, or `None` where none does.
///
/// The suites that need a real object ask for one rather than naming `nginx`: §55.1's case is
/// about a service that exists, and a container without a service manager has none.
pub fn any_service(home: &Path) -> Option<String> {
    let run = ono_at(home, "get service | take 1 | to json");
    if !run.status().is_success() {
        return None;
    }
    let text = run.stdout().trim().to_owned();
    let document: Value = serde_yaml_ng::from_str(&text).ok()?;
    let rows = document.as_sequence()?;
    rows.first()?["name"].as_str().map(str::to_owned)
}

/// A service this host serves, or `None` after asserting what a host without one owes.
///
/// A test with no object to plan against is not a test that asserts nothing: §4.3 freezes the
/// objects that match *now*, so a plan naming a unit no service manager serves must refuse rather
/// than invent one, and that refusal is worth holding the shell to on exactly the hosts that
/// cannot run the rest of the case. This is deliberately not a skip — nothing here is announced
/// as unrun, because on both branches a contract was checked (v0.4.1 §38.2).
pub fn requires_a_service(home: &Path) -> Option<String> {
    if let Some(service) = any_service(home) {
        return Some(service);
    }
    let run = ono_at(home, "plan restart service a-unit-no-host-serves");
    assert!(
        !run.status().is_success(),
        "v0.6 §4.3: a selector that matched nothing produced no plan, so `plan restart service`          over a unit this host does not serve must refuse. Got {:?}",
        run.output()
    );
    assert!(
        run.output().contains("change.target_unresolved")
            || run.output().contains("provider.unavailable"),
        "v0.6 §4.3: the refusal names the selector that resolved to nothing, or the provider          that could not answer at all. Got {:?}",
        run.output()
    );
    None
}

/// The `state`, `substate` and `since` of one service, which is the generation §7.2 freezes.
///
/// The three together are what changes when a unit is restarted, so a test that wants to prove
/// nothing happened compares them and not a single field a restart might leave alone.
pub fn service_generation(home: &Path, name: &str) -> String {
    let run = ono_at(home, &format!("get service {name} | to json"));
    run.assert_success();
    let record = one(&run);
    format!(
        "{}|{}|{}",
        record["state"].as_str().unwrap_or("unknown"),
        record["substate"].as_str().unwrap_or("unknown"),
        record["since"].as_str().unwrap_or("unknown"),
    )
}

/// A shell standing in the past, and the script that put it there.
///
/// §12.1 refuses a coordinate no source covers, so the recorder is started, a real mutation is
/// made to give it something to cover, and two seconds of wall clock put `at -1s` inside the
/// window. Without all three the session never enters historical context and every assertion
/// about §6.4 would pass against a shell that had never implemented it.
pub fn in_the_past(home: &Path, tail: &str) -> Run {
    let victim = home.join("recorded.txt");
    std::fs::write(&victim, b"an object whose removal is an event\n")
        .expect("the scratch file is written");
    recording_at(
        home,
        &format!(
            "start recorder\nremove file {} --confirm\nsleep 2\nat -1s\n{tail}",
            victim.display()
        ),
    )
}
