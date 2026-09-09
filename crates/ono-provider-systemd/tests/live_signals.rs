//! The systemd subscription against a real service manager (spec v0.5 §22.2, §47.4).
//!
//! Everything else in this crate is proven against a recorded manager, which is what makes the
//! positive path testable on machines with no systemd. That leaves one thing a recording cannot
//! prove: that `Manager.Subscribe`, the match rules and the signal decoding actually agree with
//! the service manager on the other end of the bus. This file proves it, and skips with a named
//! reason where the host supplies no manager to prove it against (v0.4.1 §38).
//!
//! The transitions are observed on the **per-user** manager, because an unprivileged account may
//! queue jobs there. §22.8 is why that matters: core v0.5 must be useful without root, so the
//! evidence that live unit transitions work has to be obtainable without it too.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml admits these inside `#[test]` functions, and the helpers here state a \
              test's preconditions the same way"
)]

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use ono_provider_api::{EventStream, ObjectEvent, Provider, Query, Selector};
use ono_provider_systemd::{SystemBus, SystemdBus, SystemdProvider};
use ono_testkit::{SkipReason, require};
use ono_value::Value;

/// No test may hang; a real manager answers in well under this.
const BUDGET: Duration = Duration::from_secs(20);

/// A transient unit name nothing else on the machine can be using.
fn probe_unit() -> String {
    format!("ono-live-{}", std::process::id())
}

/// Whether `systemd-run` is on `PATH`, so the test has a way to make the world change.
fn has_systemd_run() -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join("systemd-run").is_file())
    })
}

/// The provider over the per-user manager, or `None` where the host has none.
async fn user_provider() -> Option<SystemdProvider> {
    let bus = tokio::time::timeout(BUDGET, SystemBus::user())
        .await
        .expect("opening the session bus must not hang")
        .ok()?;
    let provider = SystemdProvider::over(Arc::new(bus) as Arc<dyn SystemdBus>).await;
    provider.availability().is_available().then_some(provider)
}

/// Everything the subscription reports within the budget, until it goes quiet for `settle`.
async fn drain(events: &mut EventStream, settle: Duration) -> Vec<ObjectEvent> {
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + BUDGET;
    loop {
        let next = tokio::time::timeout_at(deadline.min(tokio::time::Instant::now() + settle), {
            events.recv()
        })
        .await;
        match next {
            Ok(Some(event)) => seen.push(event),
            Ok(None) | Err(_) => return seen,
        }
    }
}

#[tokio::test]
async fn should_report_a_real_unit_transition_with_the_job_the_manager_gave_it() {
    // §22.2: "The systemd D-Bus provider SHOULD contribute live unit state transitions and job
    // identity where available." Both halves against a real manager: the transient unit below
    // is started by the manager, and the events that arrive carry the manager's own job path.
    let provider = user_provider().await;
    if require(
        provider.is_some(),
        SkipReason::ExternalToolUnavailable,
        "no per-user systemd manager answers on this host's session bus, so a live unit \
         transition cannot be observed",
    )
    .unmet()
    {
        return;
    }
    let Some(provider) = provider else { return };
    if require(
        has_systemd_run(),
        SkipReason::ExternalToolUnavailable,
        "`systemd-run` is not on PATH, so the test has no way to make a unit transition happen",
    )
    .unmet()
    {
        return;
    }

    let unit = probe_unit();
    let name = format!("{unit}.service");
    let query =
        Query::target("service").with(Selector::field("name", Value::String(name.as_str().into())));
    let mut events = provider
        .subscribe(&query)
        .expect("a running service manager offers a subscription");
    // The unit does not exist yet, so the opening state is empty and everything that follows is
    // the transition this test is about.
    assert!(
        drain(&mut events, Duration::from_millis(400))
            .await
            .is_empty(),
        "the transient unit `{name}` must not already exist on this host"
    );

    let started = tokio::process::Command::new("systemd-run")
        .args([
            "--user",
            "--collect",
            &format!("--unit={unit}"),
            "/bin/sleep",
            "1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .expect("systemd-run runs");
    assert!(started.success(), "the transient unit could not be started");

    let observed = drain(&mut events, Duration::from_millis(1_500)).await;
    assert!(
        !observed.is_empty(),
        "a unit the manager started must reach a subscriber, and nothing arrived"
    );
    for event in &observed {
        let reported = event
            .value()
            .and_then(|record| record.get("name").cloned())
            .and_then(|value| value.as_str().map(str::to_owned).ok());
        assert_eq!(
            reported.as_deref(),
            Some(name.as_str()),
            "a narrowed subscription reports only the unit it was narrowed to"
        );
    }
    let attributed = observed
        .iter()
        .filter_map(ObjectEvent::cause)
        .find(|token| token.starts_with("systemd:/org/freedesktop/systemd1/job/"));
    assert!(
        attributed.is_some(),
        "§15.2 needs the job identity on the transition, and the events carried none: {:?}",
        observed.iter().map(ObjectEvent::cause).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn should_open_a_subscription_against_the_real_system_manager() {
    // The system manager is the one `get service` answers from, and an unprivileged account
    // cannot make it transition — but it can subscribe to it, and a subscription that cannot be
    // opened is the failure that would make every claim above vacuous on a real machine.
    let provider = tokio::time::timeout(BUDGET, SystemdProvider::connect())
        .await
        .expect("probing the system bus must not hang");
    if require(
        provider.availability().is_available(),
        SkipReason::ExternalToolUnavailable,
        "no systemd service manager answers on this host's system bus",
    )
    .unmet()
    {
        return;
    }

    let mut events = provider
        .subscribe(&Query::target("service"))
        .expect("a running system manager offers a subscription");
    let opening = drain(&mut events, Duration::from_millis(2_000)).await;
    assert!(
        !opening.is_empty(),
        "a subscription opens with the current state, and this manager has units"
    );
    assert!(
        opening.iter().all(
            |event| event.kind() == ono_provider_api::EventKind::Snapshot
                || event.value().is_some()
        ),
        "every event a subscription reports carries the unit it is about"
    );
}
