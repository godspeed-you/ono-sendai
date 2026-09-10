//! `map --plan` draws the objects a plan touches under the identities they have now
//! (spec v0.6 §21.2, §21.4, §55.11 case 47).
//!
//! §21.4 forbids a map of a proposed change from inventing identities the change would create: a
//! plan that kills a process names the process that exists, and a restarted service is marked as
//! expecting a replacement worker rather than given a PID nobody has seen. These tests plan a
//! change against a process this test started and read the overlay the real binary attaches.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

#[path = "change_support/mod.rs"]
mod change_support;

use change_support::{Sleeper, home, ono_at, plan};

/// The overlay `map --plan <reference> | to json` attached, as the JSON text of the extension.
fn overlay_of(home: &std::path::Path, reference: &str) -> String {
    let run = ono_at(home, &format!("map --plan {reference} | to json"));
    run.assert_success();
    let output = run.stdout().replace("\\\"", "\"");
    let start = output
        .find("\"ono.change/plan-overlay\"")
        .unwrap_or_else(|| {
            panic!("§21.2: `map --plan` carries the overlay as `ono.change/plan-overlay`. Got {output:?}")
        });
    output[start..].to_owned()
}

/// Every process identity the overlay names, as the numbers it spells.
fn process_identities(overlay: &str) -> Vec<String> {
    let mut found = Vec::new();
    for marker in ["process:", "\"pid\":"] {
        for (index, _) in overlay.match_indices(marker) {
            let digits: String = overlay[index + marker.len()..]
                .chars()
                .skip_while(|character| *character == ' ')
                .take_while(char::is_ascii_digit)
                .collect();
            if !digits.is_empty() {
                found.push(digits);
            }
        }
    }
    found
}

#[test]
fn should_name_the_process_a_kill_would_signal_under_its_current_identity() {
    let home = home();
    let victim = Sleeper::start();
    let reference = plan(
        home.path(),
        &format!("kill process {} --signal SIGKILL", victim.pid()),
    );

    let overlay = overlay_of(home.path(), &reference);

    assert!(
        overlay.contains(&victim.pid().to_string()),
        "§21.2: the overlay of a kill carries the process it would signal. Got {overlay:?}"
    );
}

#[test]
fn should_name_no_process_identity_the_plan_would_create() {
    let home = home();
    let victim = Sleeper::start();
    let reference = plan(
        home.path(),
        &format!("kill process {} --signal SIGKILL", victim.pid()),
    );

    let overlay = overlay_of(home.path(), &reference);

    let invented: Vec<String> = process_identities(&overlay)
        .into_iter()
        .filter(|pid| *pid != victim.pid().to_string())
        .collect();
    assert!(
        invented.is_empty(),
        "§21.4: a map of a proposed change names only processes that exist, never one the change \
         would create. Found {invented:?} in {overlay:?}"
    );
}

#[test]
fn should_leave_the_process_running_when_the_plan_is_only_drawn() {
    let home = home();
    let mut victim = Sleeper::start();
    let reference = plan(
        home.path(),
        &format!("kill process {} --signal SIGKILL", victim.pid()),
    );

    let _ = overlay_of(home.path(), &reference);

    assert!(
        victim.is_alive(),
        "§2.1: drawing a plan on the map signals nothing"
    );
}
