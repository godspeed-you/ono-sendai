//! Pins as a user sees them across sessions — spec v0.4 §20.4, §26.4, §46, §46.1.
//!
//! §20.4: "`pin` marks a place as a persistent user landmark. Pins MUST store a resilient
//! selector and identity metadata rather than only a rendered path. If the target cannot be
//! resolved later, the pin remains but reports unresolved state." §46.1 allows pins to persist
//! where the trail may not.
//!
//! `pin` and `unpin` themselves are the navigation phase's; what is asserted here is the storage
//! and the resolution underneath them — that a pin written by one session is read by the next,
//! that a pin survives the identity of the object it named changing, and that a pin nothing
//! answers stays in the store rather than disappearing.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use jiff::Timestamp;
use ono_cli::spatial::PinStore;
use ono_spatial_core::{SpatialId, SpatialType};
use ono_spatial_index::{Pin, PinRegistry};
use ono_testkit::scratch;
use serde_yaml_ng::Value;

mod support;
use support::isolated;

/// A pin on a place that is not there, so only its selector can answer for it.
fn pin(name: &str, selector: &str, object_type: SpatialType) -> Pin {
    Pin::new(
        name,
        SpatialId::of_space("compute.processes"),
        selector,
        object_type,
        "host:testbox",
        Timestamp::UNIX_EPOCH,
    )
}

#[test]
fn should_read_back_every_pin_a_previous_session_wrote() {
    // §46.1: "Pins MAY persist." They do, and unconditionally: a pin is something the user chose,
    // which is exactly what §46.1 keeps out of the trail's privacy default.
    let dir = scratch();
    let store = PinStore::at(dir.path().join("pins.json"));

    let mut written = PinRegistry::new();
    written.insert(pin("edge-proxy", "nginx.service", SpatialType::Service));
    written.insert(pin("data", "/data", SpatialType::Mount));
    store.save(&written).expect("the store is writable");

    let read = store.load().expect("what was written can be read");
    assert_eq!(read.len(), 2);
    let edge = read
        .get("edge-proxy")
        .expect("the pin survives the session");
    assert_eq!(
        edge.selector(),
        "nginx.service",
        "§20.4: a pin stores the resilient selector that found the place"
    );
    assert_eq!(
        edge.object_type(),
        SpatialType::Service,
        "§20.4: and the identity metadata beside it, so `nginx` the service is never re-bound \
         to `nginx` the process"
    );
    assert_eq!(edge.scope(), "host:testbox");
    assert_eq!(edge.spatial_id(), &SpatialId::of_space("compute.processes"));
}

#[test]
fn should_answer_an_empty_registry_when_the_user_has_never_pinned_anything() {
    // A user with no pins has no file, and that is the ordinary case rather than a failure.
    let dir = scratch();
    let store = PinStore::at(dir.path().join("pins.json"));
    assert!(store.load().expect("a missing store is empty").is_empty());
}

#[test]
fn should_report_a_pin_store_it_cannot_read_rather_than_replacing_it() {
    // Silently starting again would delete what the user chose. §2.17: unknown is visible.
    let dir = scratch();
    let path = dir.path().join("pins.json");
    std::fs::write(&path, "this is not a pin store\n").expect("the fixture is writable");
    let error = PinStore::at(&path)
        .load()
        .expect_err("a document that is not a pin store is not an empty one");
    assert!(
        error.message().contains("pin store"),
        "the refusal names what could not be read, got {:?}",
        error.message()
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file is still there"),
        "this is not a pin store\n",
        "a store that could not be read is left exactly as it was"
    );
}

#[test]
fn should_keep_a_pin_whose_target_no_longer_resolves_rather_than_dropping_it() {
    // §20.4: "If the target cannot be resolved later, the pin remains but reports unresolved
    // state." The store is not a cache of live objects.
    let dir = scratch();
    let store = PinStore::at(dir.path().join("pins.json"));
    let mut written = PinRegistry::new();
    written.insert(pin(
        "gone",
        "a-service-that-is-not-here",
        SpatialType::Service,
    ));
    store.save(&written).expect("the store is writable");

    let read = store.load().expect("the store reads back");
    assert!(
        read.get("gone").is_some(),
        "an unresolvable pin stays in the store"
    );
    assert_eq!(
        read.resolve("gone", |_| false, |_, _| None),
        Some(ono_spatial_index::PinResolution::Unresolved),
        "§20.4: it reports unresolved state rather than answering with a place"
    );
}

#[test]
fn should_re_bind_a_pin_by_its_selector_when_the_identity_it_stored_is_gone() {
    // The point of storing a selector beside the identity (§20.4): a process that restarted or a
    // service that moved keeps its pin, and the pin says it was re-bound rather than pretending
    // the identity never changed.
    let dir = scratch();
    let store = PinStore::at(dir.path().join("pins.json"));
    let mut written = PinRegistry::new();
    written.insert(pin("edge", "nginx.service", SpatialType::Service));
    store.save(&written).expect("the store is writable");

    let read = store.load().expect("the store reads back");
    let replacement = SpatialId::of_space("compute.services");
    assert_eq!(
        read.resolve(
            "edge",
            |_| false,
            |selector, object_type| {
                (selector == "nginx.service" && object_type == SpatialType::Service)
                    .then(|| replacement.clone())
            }
        ),
        Some(ono_spatial_index::PinResolution::Rebound(replacement)),
        "§20.4: the selector finds the place again when the identity cannot"
    );
}

// --- what a search does with a pin ---------------------------------------------------------------

/// A `sleep` child this test owns, so the assertions do not depend on the machine's processes.
struct SleepChild(Child);

impl SleepChild {
    fn spawn() -> Self {
        let child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("`sleep` is on PATH in every environment this suite runs in");
        std::thread::sleep(Duration::from_millis(120));
        Self(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for SleepChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn should_rank_a_pinned_place_first_and_say_so_when_a_search_answers() {
    // §26.4: "User pins are landmarks", and a pin outranks every heuristic. The pin here carries
    // an identity that is not a place, so only its resilient selector can answer for it — which
    // is §20.4's contract exercised end to end, through the shell a user runs.
    let child = SleepChild::spawn();
    let dir = scratch();
    let store = PinStore::at(
        dir.path()
            .join("state")
            .join(ono_core::SHORT_NAME)
            .join("pins.json"),
    );
    let mut pins = PinRegistry::new();
    pins.insert(pin("mine", &child.pid().to_string(), SpatialType::Process));
    store.save(&pins).expect("the store is writable");

    let run = isolated(&dir)
        .args(["-c", "find place sleep | to json"])
        .run();
    run.assert_success();
    let document: Value = serde_yaml_ng::from_str(run.stdout().trim()).unwrap_or_else(|error| {
        panic!(
            "a stream serializes as JSON, got {error}: {:?}",
            run.stdout()
        )
    });
    let rows = document
        .as_sequence()
        .expect("a stream is an array")
        .clone();
    let first = rows.first().expect("the test's own `sleep` child is found");

    assert_eq!(
        first["pinned"].as_bool(),
        Some(true),
        "§26.4: the pinned place is the first answer, got {rows:?}"
    );
    assert_eq!(
        first["spatial_type"].as_str(),
        Some("Process"),
        "the pin's identity metadata kept it a process, got {first:?}"
    );
    assert!(
        rows.iter()
            .filter(|row| row["pinned"].as_bool() == Some(true))
            .count()
            == 1,
        "exactly the pinned place is marked, got {rows:?}"
    );
}
