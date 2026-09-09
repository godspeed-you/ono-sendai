//! `start recorder`, `stop recorder` and `get recorder` (v0.5 §10.3, §10.8, §10.9, §44.3).

mod common;

use common::{instant, options, settings};
use ono_recorder::{Recorder, RecorderHealth, RecorderSettings, service};
use ono_value::ByteSize;

#[test]
fn should_report_a_stopped_recorder_when_nothing_has_started_it() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));

    let status = recorder.status(instant("2026-08-31T12:00:00Z"));

    assert!(!status.running, "§10.2: recording is disabled by default");
    assert_eq!(status.health, RecorderHealth::Stopped);
    assert_eq!(
        status.store, None,
        "a stopped recorder retains nothing on disk"
    );
    assert!(
        !directory.path().join("ledger.sqlite3").exists(),
        "§32.1: the disabled path opens no file"
    );
}

#[test]
fn should_create_a_private_store_and_report_running_when_the_recorder_starts() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    let now = instant("2026-08-31T12:00:00Z");

    let outcome = recorder
        .start(&settings(), now)
        .expect("a first start succeeds");

    assert!(outcome.status.running);
    assert_eq!(outcome.status.since, Some(now));
    assert_eq!(outcome.status.health, RecorderHealth::Healthy);
    assert_eq!(
        outcome.status.store.as_deref(),
        Some(directory.path().join("ledger.sqlite3").as_path())
    );
    assert!(directory.path().join("ledger.sqlite3").exists());
    assert!(outcome.diagnostic.is_none());
}

#[test]
fn should_answer_the_running_recorder_when_a_start_repeats_with_the_same_settings() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    let first = instant("2026-08-31T12:00:00Z");
    recorder.start(&settings(), first).expect("a first start");

    let again = recorder
        .start(&settings(), instant("2026-08-31T12:05:00Z"))
        .expect("§10.8: `start recorder` MUST be idempotent");

    assert!(again.status.running);
    assert_eq!(
        again.status.since,
        Some(first),
        "an idempotent start reports the recorder that is already running"
    );
}

#[test]
fn should_refuse_with_recorder_already_running_when_a_start_carries_other_settings() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("a first start");

    let requested = RecorderSettings {
        max_size: ByteSize::from_bytes(64 * 1024 * 1024),
        ..settings()
    };
    let refused = recorder
        .start(&requested, instant("2026-08-31T12:05:00Z"))
        .expect_err("settings the running recorder is not using cannot be absorbed");

    assert_eq!(
        refused.code(),
        ono_core::ErrorCode::TemporalRecorderAlreadyRunning
    );
    let rendered = format!("{refused:?}");
    assert!(
        rendered.contains("temporal.retention.max_size"),
        "the refusal names the setting that could not be applied: {rendered}"
    );
}

#[test]
fn should_refuse_with_recorder_not_running_when_a_stop_finds_nothing_running() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));

    let refused = recorder
        .stop(instant("2026-08-31T12:00:00Z"))
        .expect_err("stopping a stopped recorder is a refusal");

    assert_eq!(
        refused.code(),
        ono_core::ErrorCode::TemporalRecorderNotRunning
    );
}

#[test]
fn should_flush_and_return_to_the_session_ledger_when_the_recorder_stops() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("a start");

    let stopped = recorder
        .stop(instant("2026-08-31T12:30:00Z"))
        .expect("§10.8: `stop recorder` MUST flush the ledger and stop cleanly");

    assert!(!stopped.running);
    assert_eq!(stopped.health, RecorderHealth::Stopped);
    assert!(
        !recorder.ledger().is_persistent(),
        "a stopped recorder holds §10.7's in-memory ledger again"
    );
    assert!(
        directory.path().join("ledger.sqlite3").exists(),
        "stopping flushes rather than discards"
    );
}

#[test]
fn should_produce_the_recorder_status_record_when_the_status_is_asked() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    let now = instant("2026-08-31T12:00:00Z");
    recorder.start(&settings(), now).expect("a start");

    let record = recorder
        .status(now)
        .to_record()
        .expect("§10.3: `get recorder` returns `ono.recorder-status/1`");

    assert_eq!(record.schema_id().to_string(), "ono.recorder-status/1");
    record
        .validate()
        .expect("the status record satisfies its own contract");
    assert_eq!(record.get("running"), Some(&ono_value::Value::Bool(true)));
    assert_eq!(
        record.get("health"),
        Some(&ono_value::Value::string("healthy"))
    );
}

#[test]
fn should_keep_working_without_persistence_when_the_store_cannot_be_opened() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let blocked = directory.path().join("not-a-directory");
    std::fs::write(&blocked, b"a file where a directory must be").expect("a blocking file");
    let options = ono_recorder::RecorderOptions::new(common::scope(), common::domain())
        .with_store(blocked.join("ledger.sqlite3"));
    let recorder = Recorder::new(options);

    let outcome = recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("§44.3: the shell keeps working when the store cannot be opened");

    assert!(!outcome.status.running);
    assert_eq!(outcome.status.health, RecorderHealth::Failed);
    assert!(
        outcome.diagnostic.is_some(),
        "§44.3 requires an explicit diagnostic"
    );
    assert!(
        !recorder.ledger().is_persistent(),
        "the session ledger of §10.7 is what remains"
    );
}

#[test]
fn should_name_the_user_service_when_the_unit_is_written() {
    let unit = service::unit_file(&service::UnitOptions::new(std::path::Path::new(
        "/usr/bin/ono",
    )));

    assert_eq!(service::SERVICE_UNIT, "ono-recorder.service");
    assert!(unit.contains("ExecStart=/usr/bin/ono"));
    assert!(
        unit.contains("NoNewPrivileges=yes"),
        "§10.5: the recorder never gains privilege, and the unit says so"
    );
    assert!(
        !unit.contains("User=root") && !unit.contains("[Install]\nWantedBy=multi-user.target"),
        "§10.9: it is a user service"
    );
}

#[test]
fn should_pass_user_to_every_invocation_when_the_service_is_managed() {
    let runner = service::RecordingRunner::default();
    let control = service::ServiceControl::new(&runner);

    control.start().expect("a recorded start");
    control.stop().expect("a recorded stop");

    let calls = runner.calls();
    assert_eq!(calls.len(), 2);
    for call in &calls {
        assert_eq!(
            call.first().map(String::as_str),
            Some("--user"),
            "§10.5, §10.9: the recorder is managed as the user's own service: {call:?}"
        );
        assert!(call.contains(&service::SERVICE_UNIT.to_owned()));
    }
}

#[test]
fn should_run_in_process_when_the_host_has_no_systemd() {
    let without = service::systemd_available(|_| None);
    assert!(
        !without,
        "a host with no user manager reports none rather than pretending"
    );

    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    let outcome = recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("§10.9: the recorder is a user-level collector, not a daemon requirement");
    assert!(outcome.status.running);
}

#[test]
fn should_report_degraded_when_nothing_has_driven_the_recorder_for_five_flush_intervals() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    let start = instant("2026-08-31T12:00:00Z");
    recorder.start(&settings(), start).expect("a start");

    assert_eq!(
        recorder.status(instant("2026-08-31T12:00:09Z")).health,
        RecorderHealth::Healthy,
        "a recorder driven within its flush interval is not behind"
    );
    assert_eq!(
        recorder.status(instant("2026-08-31T12:00:11Z")).health,
        RecorderHealth::Degraded,
        "§43.4: a persistent recorder that is falling behind is visible"
    );

    recorder
        .maintenance(instant("2026-08-31T12:00:12Z"))
        .expect("a maintenance turn");
    assert_eq!(
        recorder.status(instant("2026-08-31T12:00:13Z")).health,
        RecorderHealth::Healthy,
        "a turn of the handle brings it back"
    );
}

#[test]
fn should_say_why_history_stopped_when_get_recorder_is_asked_afterwards() {
    // §44.3 requires the diagnostic to be explicit, and §31.7 requires a store that cannot be
    // read to name itself. A health word with no reason beside it keeps half of each: the user
    // learns that history stopped and not what to do about it. `get recorder` is where they ask.
    let directory = tempfile::tempdir().expect("a scratch directory");
    let blocked = directory.path().join("not-a-directory");
    std::fs::write(&blocked, b"a file where a directory must be").expect("a blocking file");
    let options = ono_recorder::RecorderOptions::new(common::scope(), common::domain())
        .with_store(blocked.join("ledger.sqlite3"));
    let recorder = Recorder::new(options);

    let outcome = recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("§44.3: the shell keeps working when the store cannot be opened");
    assert!(outcome.diagnostic.is_some(), "the start says why");

    let later = recorder.status(instant("2026-08-31T12:05:00Z"));
    assert_eq!(later.health, RecorderHealth::Failed);
    let reason = later
        .diagnostic
        .as_deref()
        .expect("§44.3: the status says why too, five minutes later");
    assert!(
        !reason.trim().is_empty(),
        "a diagnostic nobody can read is a health word with extra steps"
    );

    let record = later.to_record().expect("the status renders");
    record
        .validate()
        .expect("`ono.recorder-status/1` accepts the diagnostic it declares");
    assert!(
        matches!(record.get("diagnostic"), Some(ono_value::Value::String(_))),
        "the reason reaches the record `get recorder` returns"
    );
}
