//! What a bounded orientation reads, and what it may say about what it did not read
//! (v0.4.1 §34.4, §33.2, §2.17; ADR-0576).
//!
//! §34.4 is one sentence and one obligation:
//!
//! > A local neighborhood query SHOULD NOT require construction of the complete system graph when
//! > provider APIs can answer the neighborhood incrementally.
//!
//! Reading every object of every target before saying where a user is standing is that complete
//! construction, and it is what kept `enter compute; look` outside §33.2's 150 ms: six hundred
//! systemd units at three D-Bus round trips each, paid on every orientation, to draw a hundred
//! places and a count.
//!
//! So an orientation reads `limits.orientation_objects` of a target and stops. The whole of this
//! suite is the other half of that sentence — §2.17's, which says the shell may not then be vague
//! or wrong about the size of what it described:
//!
//! * a target that can be counted is counted, and the figure is the provider's, not the sample's;
//! * a target that cannot be counted reports no count at all, and says why;
//! * the objects themselves are unchanged — `get service` is not an orientation and reads
//!   everything, so nothing a user asks for directly is bounded by this.
//!
//! The service tests read a service manager this suite owns rather than the host's. The host's
//! unit list is not a fixture: a container starting beside the suite registers systemd scopes
//! while it runs, and a count compared across two readings of it measured the machine
//! (issue #155, ADR-0880). `FixtureServiceManager` answers `org.freedesktop.systemd1` on a
//! private bus with exactly the units it was given, and the shell under test reaches it through
//! `DBUS_SYSTEM_BUS_ADDRESS` — the same production provider, the same D-Bus calls, and a
//! population that is a number the test chose.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "spatial")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "support/service_manager.rs"]
mod service_manager;
mod support;

use ono_testkit::{Shell, SkipReason, skipped};
use serde_yaml_ng::Value;

use service_manager::FixtureServiceManager;

use support::{field, items, json, search};

/// A bound small enough that any host in the world exceeds it for the targets under test.
const BOUND: &str = "3";

fn look(space: &str, bound: &str) -> ono_testkit::Run {
    bounded(bound)
        .args(["-c", &format!("enter {space}; look --json")])
        .run()
}

/// The shell with the orientation bound set to `bound`.
fn bounded(bound: &str) -> Shell {
    Shell::new()
        .env("ONO_LIMITS_ORIENTATION_OBJECTS", bound.to_owned())
        .env("ONO_LIMITS_ORIENTATION_CEILING", bound.to_owned())
}

/// The `count` and `detail` of one exit of a `look --json` answer.
fn group(document: &Value, name: &str) -> (Option<i64>, Option<String>) {
    let groups = field(document, "groups");
    for group in items(&groups) {
        if search(group, "name").and_then(|found| found.as_str().map(str::to_owned))
            == Some(name.to_owned())
        {
            let count = search(group, "count").and_then(|count| count.as_i64());
            let detail =
                search(group, "detail").and_then(|detail| detail.as_str().map(str::to_owned));
            return (count, detail);
        }
    }
    panic!("`look` in this place has no `{name}` exit: {document:?}");
}

/// The whole truth, asked for the way a user asks for it: `get <target> | count`.
fn population(target: &str) -> Option<i64> {
    let run = Shell::new()
        .args(["-c", &format!("get {target} | count | to json")])
        .run();
    if !run.status().is_success() {
        return None;
    }
    // `count | to json` emits the stream as an array, so the one value is the only element.
    items(&json(run.stdout().trim()))
        .first()
        .and_then(serde_yaml_ng::Value::as_i64)
}

#[test]
fn should_count_a_bounded_target_by_what_the_provider_says_is_there() {
    let Some(manager) = FixtureServiceManager::try_start(FIXTURE_UNITS) else {
        skipped(
            SkipReason::ExternalToolUnavailable,
            "counting a bounded service enumeration needs `dbus-daemon`, so the fixture can serve \
             a service manager with more units than the bound",
        );
        return;
    };
    let services = FIXTURE_UNITS;

    let answered = look_with(&manager, "compute", BOUND);

    answered.assert_success();
    let (count, detail) = group(&json(answered.stdout().trim()), "services");
    assert_eq!(
        count,
        Some(services),
        "v0.4.1 §2.17: an orientation that read three units of {services} reports {services}, \
         because the count is a fact about the place and not about the reading. §34.4 permits \
         the bounded read; nothing permits a bounded count. Detail was {detail:?}"
    );
}

#[test]
fn should_report_no_count_for_a_bounded_target_whose_count_it_cannot_keep_true() {
    // A provider states one population per query, and `socket` answers both `network.listeners`
    // and `network.connections`; splitting that figure between them would be inventing it. So a
    // bounded read of that target has no count for either exit — `null`, with the reason — which
    // is §42.4's rule that a missing figure is never reported as zero.
    let Some(sockets) = population("socket").filter(|count| *count > 3) else {
        skipped(
            SkipReason::MissingPrivilege,
            "the socket table must hold more sockets than the bound for the bound to show",
        );
        return;
    };

    let answered = look("network", BOUND);

    answered.assert_success();
    let (count, detail) = group(&json(answered.stdout().trim()), "listeners");
    assert_eq!(
        count, None,
        "{sockets} sockets serve two kinds of place, so a bounded read of them counts neither. \
         A count here would be the number of sockets that happened to be read"
    );
    let detail = detail.unwrap_or_default();
    assert!(
        detail.contains("orientation bound"),
        "§42.4: the group says why it has no count instead of leaving a reader to guess, got \
         {detail:?}"
    );
}

#[test]
fn should_leave_what_a_user_asks_for_directly_unbounded() {
    let Some(manager) = FixtureServiceManager::try_start(FIXTURE_UNITS) else {
        skipped(
            SkipReason::ExternalToolUnavailable,
            "this needs `dbus-daemon`, so the fixture can serve a service manager with more \
             units than the bound",
        );
        return;
    };
    let services = FIXTURE_UNITS;

    let asked = bounded(BOUND)
        .env("DBUS_SYSTEM_BUS_ADDRESS", manager.address())
        .args(["-c", "get service | count | to json"])
        .run();

    asked.assert_success();
    assert_eq!(
        items(&json(asked.stdout().trim()))
            .first()
            .and_then(serde_yaml_ng::Value::as_i64),
        Some(services),
        "`limits.orientation_objects` bounds an orientation and nothing else. `get service` is a \
         question about services, and answering three of them because a *view* is budgeted would \
         be the wrong answer to the question that was asked (§34.4, §2.17)"
    );
}

/// How many units the fixture's service manager holds: well above the bound, and a number no
/// host happens to have by accident.
const FIXTURE_UNITS: i64 = 17;

/// `look` in `space` against the fixture's service manager.
fn look_with(manager: &FixtureServiceManager, space: &str, bound: &str) -> ono_testkit::Run {
    bounded(bound)
        .env("DBUS_SYSTEM_BUS_ADDRESS", manager.address())
        .args(["-c", &format!("enter {space}; look --json")])
        .run()
}
