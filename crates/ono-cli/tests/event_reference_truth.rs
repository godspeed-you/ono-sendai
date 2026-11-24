//! An event reference that does not resolve says why (v0.5 §11.6, §12.2, §34; issue #142).
//!
//! `at event @efa3` once answered "no event with that reference is retained" in CI one shell
//! after `inspect event @efa3` had resolved the same reference. Three paths gave that answer
//! without it being true: a configuration skipped when the temporal state was busy, a recorder
//! start whose refusal was dropped, and a ledger whose refusal — an ambiguous prefix, a store that
//! could not be read — was turned into "no such event" with `.ok()`. What the reader gets now is
//! one of four answers, each with its own code from §34's family:
//!
//! - no retained event matches the reference — `temporal.invalid_time`;
//! - the session showed the event and the ledger no longer holds it — `temporal.out_of_retention`;
//! - the store that would hold it could not be read — `temporal.store_unavailable`;
//! - the reference names several events — `temporal.ambiguous_event`.
//!
//! Every test drives the real binary over a home of its own (AGENTS.md §11).

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "temporal")]
#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::time::Duration;

use ono_testkit::{Scratch, Shell, scratch};

use support::recording_shell;

/// The reference digit two retained events share, so a one-digit reference names both.
///
/// Every recording shell adds its own coverage events to the store, so the store grows until
/// pigeonhole guarantees a shared digit: sixteen hex digits, seventeen events.
fn shared_digit(home: &Scratch) -> char {
    for _ in 0..8 {
        let found = recording_shell(home, "find event | to json");
        found.assert_success();
        let mut seen = std::collections::BTreeMap::<char, usize>::new();
        for row in support::last_json_rows(&found) {
            let id = support::text(&row, "event_id");
            if let Some(digit) = id.chars().nth(1) {
                *seen.entry(digit).or_default() += 1;
            }
        }
        if let Some((digit, _)) = seen.into_iter().find(|(_, count)| *count >= 2) {
            return digit;
        }
    }
    panic!("eight recording shells left fewer than seventeen events in the store")
}

/// A recording shell whose store cannot be created, because a file sits where its directory goes.
fn shell_with_an_unopenable_store(home: &Scratch, script: &str) -> ono_testkit::Run {
    let root = home.path().to_string_lossy().into_owned();
    std::fs::create_dir_all(home.path().join("data/ono")).expect("the data directory");
    std::fs::write(home.path().join("data/ono/temporal"), b"not a directory")
        .expect("a file where the store's directory belongs");
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

#[test]
fn should_refuse_at_event_as_ambiguous_when_the_reference_names_two_retained_events() {
    // §11.6 and §34 E1306: a shortened reference that names several events is refused with the
    // candidates, by `at` exactly as by `inspect event`. Before issue #142 `at` dropped the
    // ledger's refusal and said no event was retained — while both were.
    let home = support::home_with_a_recorded_action();
    let digit = shared_digit(&home);
    for command in ["at event", "inspect event"] {
        let run = recording_shell(&home, &format!("{command} @e{digit}"));
        assert!(
            !run.status().is_success() && run.output().contains("temporal.ambiguous_event"),
            "v0.5 §11.6, §34: `{command} @e{digit}` names more than one retained event and is \
             refused as ambiguous, not as unretained. Got {:?}",
            run.output()
        );
    }
}

#[test]
fn should_report_the_unopenable_store_when_a_reference_is_resolved_without_it() {
    // §44.3 lets the shell keep working on its session ledger when the store cannot be opened.
    // A reference the session ledger does not hold is then a question about the store, and the
    // answer is that the store could not be read — not that the event is not retained.
    let home = scratch();
    for command in ["at event", "inspect event", "why event"] {
        let run = shell_with_an_unopenable_store(&home, &format!("{command} @efa3"));
        assert!(
            !run.status().is_success()
                && run.output().contains("temporal.store_unavailable")
                && !run.output().contains("temporal.invalid_time"),
            "issue #142: `{command}` over a store that could not be opened says so. Got {:?}",
            run.output()
        );
    }
}

#[test]
fn should_report_a_failed_health_and_its_reason_when_the_configured_store_cannot_be_opened() {
    // §43.4 and §44.3: recording was asked for and is not happening, and `get recorder` is where
    // the reader learns that and why.
    let home = scratch();
    let run = shell_with_an_unopenable_store(&home, "get recorder | to json");
    run.assert_success();
    let status = support::single_result(&run);
    assert_eq!(
        support::text(&status, "health"),
        "failed",
        "v0.5 §43.4: a recorder the configuration asked for and could not start is `failed`. \
         Got {status:?}"
    );
    assert!(
        support::text(&status, "diagnostic").contains("could not be created"),
        "v0.5 §44.3: the diagnostic names the reason. Got {status:?}"
    );
}

#[test]
fn should_refuse_a_shown_reference_as_out_of_retention_when_the_ledger_dropped_its_event() {
    // §10.7 bounds the session ledger. A reference this session printed names an event that was
    // retained when the reader saw it; once the bound has dropped it, the answer is that it is no
    // longer retained — §34 E1303 — rather than "no event with that reference", which reads as a
    // typo.
    let home = scratch();
    let root = home.path().to_string_lossy().into_owned();
    let run = Shell::new()
        .env("NO_COLOR", "1")
        .env("HOME", root.clone())
        .env("XDG_CONFIG_HOME", format!("{root}/config"))
        .env("XDG_DATA_HOME", format!("{root}/data"))
        .env("XDG_STATE_HOME", format!("{root}/state"))
        .env("ONO_TEMPORAL_SESSION_MAX_EVENTS", "4")
        .args([
            "-c",
            "stop process 4294967294\n\
             let shown = (find event | take 1)\n\
             let reference = ($shown.reference)\n\
             stop process 4294967293\n\
             stop process 4294967292\n\
             inspect event $reference",
        ])
        .timeout(Duration::from_secs(60))
        .run();
    assert!(
        !run.status().is_success() && run.output().contains("temporal.out_of_retention"),
        "issue #142: a reference the session showed, whose event the bounded ledger dropped, is \
         refused as out of retention. Got {:?}",
        run.output()
    );
}
