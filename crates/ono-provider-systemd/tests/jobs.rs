//! The job identity an Ono action becomes (v0.5 §15.2, §17.3, §17.6).
//!
//! §17.3: "Every mutation receives an `ActionId` before execution. […] If an external authority
//! returns its own transaction/job ID, the event ledger MUST record the mapping." systemd is that
//! authority for a service, and the identity it returns is a D-Bus object path:
//!
//! ```text
//! ActionId ono:a91f -> systemd job /org/freedesktop/systemd1/job/4821
//! ```
//!
//! This provider owns the half of that mapping the ledger cannot invent. What matters here is not
//! that a job path exists — `service.rs` already holds the provider to answering with one — but
//! that it is a **join key**: the identity a mutation hands up is the same identity the transition
//! that follows names, so a causal edge between them rests on something systemd published rather
//! than on the two events happening close together. §15.2: "Temporal proximity is insufficient."
//!
//! §17.6 is the other edge of the same claim, and the last test is about it: when the job the
//! authority reported has ended, the next transition is attributed to nothing. Ono claims what a
//! provider reported and no more.
//!
//! systemd does not run on the machines this suite runs on, so — like every other positive-path
//! test in this crate — it drives the provider over `fixture::RecordedSystemd` rather than over a
//! service manager that happens to be present.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml admits these inside `#[test]` functions; the helpers below state a \
              test's preconditions the same way and belong to the same test binary"
)]

mod fixture;

use std::sync::Arc;
use std::time::Duration;

use fixture::{FIRST_JOB_PATH, NGINX_STATE_CHANGE_USEC, PENDING_JOB_PATH, RecordedSystemd};
use ono_provider_api::{
    Action, ActionOutcome, EventKind, EventStream, ObjectEvent, ObjectId, Provider, Query,
};
use ono_provider_systemd::{JobRef, PROVIDER_ID, SystemdBus, SystemdProvider, UnitSignal};
use ono_value::{ActionStatus, SchemaId, Value};

/// No test may hang; every await in this file runs under this budget.
const BUDGET: Duration = Duration::from_secs(5);

/// The units [`RecordedSystemd::running`] holds, and therefore the length of a subscription's
/// opening state.
const RECORDED_UNITS: usize = 4;

/// The unit every job in this file is queued for.
const NGINX: &str = "nginx.service";

async fn provider_over(bus: Arc<RecordedSystemd>) -> SystemdProvider {
    tokio::time::timeout(BUDGET, SystemdProvider::over(bus as Arc<dyn SystemdBus>))
        .await
        .expect("probing a recorded systemd must not hang")
}

/// Runs `operation` against the unit `name` and answers with the outcome it produced.
///
/// The object reference is built inline rather than in a helper of its own: every test in this
/// file names a unit and wants an outcome, and none of them needs an [`ObjectId`] for anything
/// else (v0.4.1 §39.1).
async fn act(provider: &SystemdProvider, operation: &str, name: &str) -> ActionOutcome {
    let action = Action::new(
        "service",
        operation,
        ObjectId::new(
            SchemaId::new("ono.service", 1),
            [
                Value::String(PROVIDER_ID.into()),
                Value::String(name.into()),
            ],
        ),
    );
    tokio::time::timeout(BUDGET, provider.act(&action))
        .await
        .expect("an action against a recorded service manager must not hang")
        .expect("the provider can attempt this operation")
}

/// The job path a successful mutation handed up, or a panic naming the outcome that handed up
/// none — an action whose transaction identity was dropped can never be joined to anything.
fn job_path(outcome: &ActionOutcome) -> String {
    match outcome.metadata_value("systemd.job") {
        Some(Value::String(path)) => path.to_string(),
        other => panic!(
            "§17.3: a queued job's identity is what the ledger records the mapping to, and \
             `{}` handed up {other:?}",
            outcome.operation()
        ),
    }
}

async fn subscribed(bus: Arc<RecordedSystemd>) -> (SystemdProvider, EventStream) {
    let provider = provider_over(bus).await;
    let events = provider
        .subscribe(&Query::target("service"))
        .expect("a recorded service manager offers a subscription");
    (provider, events)
}

async fn next_event(events: &mut EventStream) -> ObjectEvent {
    tokio::time::timeout(BUDGET, events.recv())
        .await
        .expect("an announced signal must reach the subscriber")
        .expect("the subscription is still open")
}

/// Consumes the subscription's opening snapshot, so what follows is a change and not the baseline.
async fn opening_state(events: &mut EventStream) {
    for _ in 0..RECORDED_UNITS {
        assert_eq!(
            next_event(events).await.kind(),
            EventKind::Snapshot,
            "a subscription opens with the current state"
        );
    }
}

/// Announces `signal` the way the manager broadcasts it to a subscribed client.
async fn announce(announcer: &tokio::sync::mpsc::Sender<UnitSignal>, signal: UnitSignal) {
    announcer
        .send(signal)
        .await
        .expect("the subscriber is listening");
}

#[tokio::test]
async fn should_join_an_action_to_the_transition_that_followed_it_through_one_job_identity() {
    let bus = Arc::new(RecordedSystemd::running());
    let announcer = bus.announcer();
    let (provider, mut events) = subscribed(Arc::clone(&bus)).await;
    opening_state(&mut events).await;

    let outcome = act(&provider, "restart", NGINX).await;
    assert_eq!(outcome.status(), ActionStatus::Success);
    let job = job_path(&outcome);

    // What the service manager broadcasts about the job it just created for this action.
    announce(
        &announcer,
        UnitSignal::JobNew {
            job: JobRef::new(job.clone()),
            unit: NGINX.to_owned(),
        },
    )
    .await;
    bus.move_unit(
        NGINX,
        "activating",
        "start",
        NGINX_STATE_CHANGE_USEC + 5_000_000,
    );
    announce(
        &announcer,
        UnitSignal::UnitChanged {
            unit: NGINX.to_owned(),
            path: None,
        },
    )
    .await;

    let transition = next_event(&mut events).await;
    assert_eq!(transition.kind(), EventKind::Changed);
    assert_eq!(
        transition.cause(),
        Some(JobRef::new(job.clone()).token().as_str()),
        "§17.3: the identity the action handed up and the identity the transition names are one \
         job, so the ledger can record `ActionId -> {job}` and §15.2 has a published join rather \
         than two events that happened near each other"
    );
    assert_eq!(
        job, FIRST_JOB_PATH,
        "the recorded manager answers the first job with a fixed path, so the mapping under test \
         is a value and not a shape"
    );
}

#[tokio::test]
async fn should_name_the_authority_that_issued_the_transaction_when_an_action_becomes_a_job() {
    let provider = provider_over(Arc::new(RecordedSystemd::running())).await;

    let token = JobRef::new(job_path(&act(&provider, "restart", NGINX).await)).token();

    assert_eq!(
        token,
        format!("systemd:{FIRST_JOB_PATH}"),
        "§17.4's `external_transaction` says which authority issued the identity, so a ledger \
         holding jobs from several sources never has to guess whose path it is looking at"
    );
}

#[tokio::test]
async fn should_hand_up_a_job_identity_for_every_mutation_the_service_manager_accepts() {
    // §17.3 says "every mutation", not "every restart": an action whose job identity was dropped
    // is an action nothing downstream can ever be attributed to.
    let provider = provider_over(Arc::new(RecordedSystemd::running())).await;

    let mut jobs = Vec::new();
    for (operation, unit) in [
        ("restart", NGINX),
        ("reload", NGINX),
        ("stop", NGINX),
        ("start", "postgresql.service"),
    ] {
        let outcome = act(&provider, operation, unit).await;
        assert_eq!(
            outcome.status(),
            ActionStatus::Success,
            "the recorded manager accepts `{operation}` on `{unit}`"
        );
        let path = job_path(&outcome);
        let id = JobRef::new(path.clone())
            .id
            .unwrap_or_else(|| panic!("`{operation}` produced a job path with no id: {path}"));
        assert_eq!(
            outcome.metadata_value("systemd.job_id"),
            Some(&Value::Int(i128::from(id))),
            "§17.3: the number and the path name one job, so a consumer correlating a \
             `JobRemoved` by id does not have to re-parse a path — `{operation}` disagreed"
        );
        jobs.push(path);
    }

    let distinct: std::collections::BTreeSet<&String> = jobs.iter().collect();
    assert_eq!(
        distinct.len(),
        jobs.len(),
        "four mutations are four transactions; a shared identity would merge four causal chains \
         into one, got {jobs:?}"
    );
}

#[tokio::test]
async fn should_name_the_job_this_action_created_rather_than_the_one_already_in_flight() {
    // `postgresql.service` is recorded with a job already queued for it. Attributing this action
    // to that job would map an `ActionId` onto a transaction it did not cause (§17.3, §15.2).
    let provider = provider_over(Arc::new(RecordedSystemd::running())).await;

    let job = job_path(&act(&provider, "restart", "postgresql.service").await);

    assert_eq!(job, FIRST_JOB_PATH);
    assert_ne!(
        job, PENDING_JOB_PATH,
        "§17.3: the mapping is to the job this mutation created, not to whatever the unit \
         happened to have in flight when it was read"
    );
}

#[tokio::test]
async fn should_claim_no_job_for_a_transition_observed_after_that_job_ended() {
    // §17.6: "Ono MUST NOT claim arbitrary downstream effects […] unless an adapter, provider or
    // other evidence source reports them." A job's window is exactly what the manager announced:
    // it opens at `JobNew` and closes at `JobRemoved`, and a unit that moves afterwards moved for
    // a reason nobody reported.
    let bus = Arc::new(RecordedSystemd::running());
    let announcer = bus.announcer();
    let (_provider, mut events) = subscribed(Arc::clone(&bus)).await;
    opening_state(&mut events).await;

    let job = JobRef::new(FIRST_JOB_PATH);
    announce(
        &announcer,
        UnitSignal::JobNew {
            job: job.clone(),
            unit: NGINX.to_owned(),
        },
    )
    .await;
    bus.move_unit(
        NGINX,
        "activating",
        "start",
        NGINX_STATE_CHANGE_USEC + 5_000_000,
    );
    announce(
        &announcer,
        UnitSignal::UnitChanged {
            unit: NGINX.to_owned(),
            path: None,
        },
    )
    .await;
    assert_eq!(
        next_event(&mut events).await.cause(),
        Some(job.token().as_str()),
        "a transition inside the job's window names the job"
    );

    bus.move_unit(
        NGINX,
        "active",
        "running",
        NGINX_STATE_CHANGE_USEC + 7_000_000,
    );
    announce(
        &announcer,
        UnitSignal::JobRemoved {
            job: job.clone(),
            unit: NGINX.to_owned(),
            result: Some("done".to_owned()),
        },
    )
    .await;
    assert_eq!(
        next_event(&mut events).await.cause(),
        Some(job.token().as_str()),
        "`JobRemoved` arrives after the transition it completes, so the identity survives being \
         taken out of flight"
    );

    bus.move_unit(
        NGINX,
        "failed",
        "failed",
        NGINX_STATE_CHANGE_USEC + 9_000_000,
    );
    announce(
        &announcer,
        UnitSignal::UnitChanged {
            unit: NGINX.to_owned(),
            path: None,
        },
    )
    .await;
    let afterwards = next_event(&mut events).await;
    assert_eq!(afterwards.kind(), EventKind::Changed);
    assert_eq!(
        afterwards.cause(),
        None,
        "§17.6: the job the authority reported has ended, so this transition is attributed to \
         nothing — a stale job identity here would be Ono claiming a downstream effect systemd \
         never reported"
    );
}
