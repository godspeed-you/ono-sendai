//! The privacy floor: what the recorder must never persist (v0.5 §10.6, §17.5, §30.3, §30.4, §30.5).

mod common;

use common::{instant, options, process_record, settings};
use ono_recorder::{Recorder, Redaction, WITHHELD_FIELDS};
use ono_temporal_core::{REDACTED, Redactable, RedactedCommandSummary};
use ono_value::{Value, builtin_schemas};

#[test]
fn should_not_persist_raw_argv_when_the_setting_is_off() {
    let redaction = Redaction::new(false);
    let record = process_record(1842, "nginx", &["nginx", "--password=hunter2"]);

    let stored = redaction.record(&record);

    assert_eq!(
        stored.get("command"),
        Some(&Value::Null),
        "§30.4: the default recorder SHOULD NOT persist full raw argv"
    );
    assert!(!redaction.persists_argv());
}

#[test]
fn should_keep_the_executable_the_name_and_the_identity_when_argv_is_withheld() {
    let redaction = Redaction::new(false);
    let record = process_record(1842, "nginx", &["nginx", "-g", "daemon off;"]);

    let stored = redaction.record(&record);

    assert_eq!(stored.get("name"), Some(&Value::string("nginx")));
    assert_eq!(stored.get("pid"), Some(&Value::Int(1842)));
    assert!(
        matches!(stored.get("executable"), Some(Value::Path(_))),
        "§30.4: executable, name and identity fields survive"
    );
    assert!(
        matches!(stored.get("started"), Some(Value::Timestamp(_))),
        "the identity field a process id is keyed on survives"
    );
}

#[test]
fn should_persist_argv_when_the_setting_is_on() {
    let redaction = Redaction::new(true);
    let record = process_record(1842, "nginx", &["nginx", "-g", "daemon off;"]);

    let stored = redaction.record(&record);

    let Some(Value::List(words)) = stored.get("command") else {
        panic!("§30.4 permits argv persistence under `temporal.record.process_argv`");
    };
    assert_eq!(words.len(), 3);
    assert!(redaction.persists_argv());
}

#[test]
fn should_replace_a_secret_shaped_value_even_when_argv_is_enabled() {
    let redaction = Redaction::new(true);
    let record = process_record(1842, "psql", &["psql", "--password=hunter2"]);

    let stored = redaction.record(&record);

    let rendered = format!("{:?}", stored.get("command"));
    assert!(
        !rendered.contains("hunter2"),
        "§30.3: a secret is redacted before persistence, whatever else is enabled: {rendered}"
    );
    assert!(rendered.contains(REDACTED) || rendered.contains("<redacted>"));
}

#[test]
fn should_withhold_every_body_the_collection_policy_forbids_when_a_record_is_persisted() {
    let redaction = Redaction::new(false);

    for field in WITHHELD_FIELDS {
        assert!(
            redaction.withholds(field),
            "§10.6 forbids persisting `{field}` by default"
        );
    }
    assert!(!redaction.withholds("name"));
    assert!(!redaction.withholds("pid"));
}

#[test]
fn should_redact_the_command_summary_when_an_action_is_persisted() {
    let summary = RedactedCommandSummary::of(
        "restart",
        Some("service"),
        &[
            Redactable::plain("nginx"),
            Redactable::secret("--token", "sk-live-4711"),
        ],
    );

    assert!(!summary.as_str().contains("sk-live-4711"));
    assert!(
        summary.is_redacted(),
        "§17.5: the persisted summary says a hole is there"
    );
    assert_eq!(
        summary.as_str(),
        format!("restart service nginx --token={REDACTED}")
    );
}

#[test]
fn should_hold_no_secret_bytes_when_the_ledger_file_is_scanned() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    recorder
        .start(&settings(), instant("2026-08-31T12:00:00Z"))
        .expect("a start");

    recorder
        .record_observation(
            &process_record(1842, "psql", &["psql", "--password=hunter2"]),
            instant("2026-08-31T12:00:01Z"),
        )
        .expect("one observation");
    recorder.flush().expect("a flush");

    let bytes = std::fs::read(directory.path().join("ledger.sqlite3")).expect("the ledger file");
    assert!(
        !contains(&bytes, b"hunter2"),
        "§30.3: no secret value reaches the ledger's bytes"
    );
}

#[test]
fn should_leave_a_record_that_declares_no_forbidden_field_untouched() {
    let redaction = Redaction::new(false);
    let schema_id = ono_value::SchemaId::new("ono.recorder-status", 1);
    let Some(schema) = builtin_schemas().get(&schema_id) else {
        panic!("the recorder status contract ships with the binary");
    };
    let record = ono_value::RecordValue::builder(
        schema,
        ono_value::Provenance::local("ono.recorder", schema_id),
    )
    .build();

    let stored = redaction.record(&record);

    assert_eq!(stored.schema_id(), record.schema_id());
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn should_persist_an_action_with_its_secret_already_gone() {
    use ono_temporal_core::{
        ActionEvent, ActionId, AuthorizationDecision, AuthorizationSummary, LedgerRead as _,
        TimeRange,
    };

    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    let at = instant("2026-08-31T12:00:00Z");
    recorder.start(&settings(), at).expect("a start");

    let action = ActionEvent {
        action_id: ActionId::of("s1", at, "restart", None),
        command: RedactedCommandSummary::of(
            "restart",
            Some("service"),
            &[
                Redactable::plain("nginx"),
                Redactable::secret("--token", "sk-live-4711"),
            ],
        ),
        actor: "case".into(),
        session_id: "s1".into(),
        requested_at: at,
        target: None,
        operation: "restart".into(),
        authorization: AuthorizationSummary {
            decision: AuthorizationDecision::Allowed,
            risk: "destructive".into(),
            capability: None,
            reason: None,
        },
        result: None,
        external_transaction: None,
        provenance: ono_value::Provenance::local(
            "ono.session",
            ono_value::SchemaId::new("ono.action-event", 1),
        ),
    };
    recorder
        .record_action(&action)
        .expect("§10.6: action events are collected");
    recorder.flush().expect("a flush");

    let stored = recorder
        .ledger()
        .actions(TimeRange {
            from: None,
            until: None,
        })
        .expect("the store answers");
    assert_eq!(stored.len(), 1);
    assert!(stored[0].command.is_redacted());
    assert!(!stored[0].command.as_str().contains("sk-live-4711"));

    let bytes = std::fs::read(directory.path().join("ledger.sqlite3")).expect("the ledger file");
    assert!(
        !contains(&bytes, b"sk-live-4711"),
        "§17.5: the raw value never enters the persisted summary"
    );
}

#[test]
fn should_discard_the_whole_store_when_temporal_history_is_removed() {
    use ono_temporal_core::LedgerRead as _;

    let directory = tempfile::tempdir().expect("a scratch directory");
    let recorder = Recorder::new(options(directory.path()));
    let at = instant("2026-08-31T12:00:00Z");
    recorder.start(&settings(), at).expect("a start");
    recorder
        .record_observation(&process_record(1842, "nginx", &["nginx"]), at)
        .expect("one observation");
    assert!(recorder.ledger().retention().events > 0);

    recorder
        .remove_history()
        .expect("§30.8: the user MUST be able to clear local retained history explicitly");

    assert_eq!(recorder.ledger().retention().events, 0);
    assert_eq!(recorder.ledger().retention().earliest, None);
}
