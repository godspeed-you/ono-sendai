//! What the systemd service provider promises, asserted through its public contract only.
//!
//! Every test here drives [`SystemdProvider`] through the [`Provider`] trait and asserts on what
//! comes out: record fields, schema validity, availability, and per-target [`ActionOutcome`]s.
//! None of them knows how the provider reaches systemd, so restructuring the crate cannot break
//! them (AGENTS.md §11).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "clippy.toml admits these inside `#[test]` functions; the helpers below state a \
              test's preconditions the same way and belong to the same test binary"
)]

mod fixture;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use fixture::{
    NGINX_MEMORY_BYTES, NGINX_STATE_CHANGE_USEC, POSTGRES_STATE_CHANGE_USEC, RecordedSystemd,
};
use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_pipeline::Collected;
use ono_provider_api::{Action, ActionOutcome, ObjectId, Provider, Query, Selector};
use ono_provider_systemd::{PROVIDER_ID, SystemdProvider, service_schema};
use ono_value::{ActionStatus, ByteSize, FieldAccess, RecordValue, SchemaId, Value};

/// No test may hang; every await in this file runs under this budget.
const BUDGET: Duration = Duration::from_secs(5);

async fn provider_over(bus: RecordedSystemd) -> SystemdProvider {
    tokio::time::timeout(BUDGET, SystemdProvider::over(Arc::new(bus)))
        .await
        .expect("probing a recorded systemd must not hang")
}

async fn collected(provider: &SystemdProvider, query: &Query) -> Collected {
    let stream = provider
        .snapshot(query)
        .expect("the recorded service manager answers");
    tokio::time::timeout(BUDGET, stream.collect())
        .await
        .expect("a snapshot of recorded units must not hang")
}

async fn units(provider: &SystemdProvider, query: &Query) -> Vec<Arc<RecordValue>> {
    collected(provider, query)
        .await
        .into_values()
        .into_iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some(record),
            _ => None,
        })
        .collect()
}

async fn unit(provider: &SystemdProvider, name: &str) -> Arc<RecordValue> {
    let query = Query::target("service").with(Selector::field("name", Value::String(name.into())));
    units(provider, &query)
        .await
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("the recorded service manager knows `{name}`"))
}

fn unit_id(name: &str) -> ObjectId {
    ObjectId::new(
        SchemaId::new("ono.service", 1),
        [
            Value::String(PROVIDER_ID.into()),
            Value::String(name.into()),
        ],
    )
}

async fn act(provider: &SystemdProvider, operation: &str, name: &str) -> ActionOutcome {
    let action = Action::new("service", operation, unit_id(name));
    tokio::time::timeout(BUDGET, provider.act(&action))
        .await
        .expect("an action against a recorded service manager must not hang")
        .expect("the provider can attempt this operation")
}

fn text(record: &RecordValue, field: &str) -> String {
    match record.get(field) {
        Some(Value::String(value)) => value.to_string(),
        other => panic!("`{field}` should be text, but is {other:?}"),
    }
}

// --- what a unit looks like ------------------------------------------------------------------

#[tokio::test]
async fn should_report_every_schema_field_when_a_unit_is_running() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let nginx = unit(&provider, "nginx.service").await;

    assert_eq!(text(&nginx, "name"), "nginx.service");
    assert_eq!(
        text(&nginx, "description"),
        "A high performance web server and a reverse proxy server"
    );
    assert_eq!(text(&nginx, "state"), "active");
    assert_eq!(text(&nginx, "substate"), "running");
    assert_eq!(text(&nginx, "provider"), PROVIDER_ID);
    assert_eq!(nginx.get("pid"), Some(&Value::Int(812)));
    assert_eq!(nginx.get("enabled"), Some(&Value::Bool(true)));
    assert_eq!(
        nginx.get("since"),
        Some(&Value::Timestamp(
            Timestamp::from_microsecond(
                i64::try_from(NGINX_STATE_CHANGE_USEC).expect("the fixture timestamp fits")
            )
            .expect("the fixture timestamp is a real instant")
        ))
    );
    assert_eq!(
        nginx.get("unit_file"),
        Some(&Value::Path(
            PathBuf::from("/lib/systemd/system/nginx.service").into()
        ))
    );
}

#[tokio::test]
async fn should_report_the_resource_use_systemd_exposes_when_a_unit_is_running() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let nginx = unit(&provider, "nginx.service").await;

    assert_eq!(
        nginx.get("systemd.memory"),
        Some(&Value::ByteSize(ByteSize::from_bytes(u128::from(
            NGINX_MEMORY_BYTES
        ))))
    );
    assert_eq!(nginx.get("systemd.tasks"), Some(&Value::Int(5)));
    assert_eq!(text(&nginx, "systemd.load_state"), "loaded");
    assert_eq!(text(&nginx, "systemd.unit_file_state"), "enabled");
}

#[tokio::test]
async fn should_report_why_it_failed_and_with_which_status_when_a_unit_has_failed() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let postgres = unit(&provider, "postgresql.service").await;

    assert_eq!(text(&postgres, "state"), "failed");
    assert_eq!(text(&postgres, "substate"), "failed");
    assert_eq!(text(&postgres, "systemd.result"), "exit-code");
    assert_eq!(postgres.get("systemd.exit_code"), Some(&Value::Int(1)));
    assert_eq!(
        postgres.get("since"),
        Some(&Value::Timestamp(
            Timestamp::from_microsecond(
                i64::try_from(POSTGRES_STATE_CHANGE_USEC).expect("the fixture timestamp fits")
            )
            .expect("the fixture timestamp is a real instant")
        ))
    );
}

#[tokio::test]
async fn should_report_unknown_resource_use_as_null_rather_than_zero() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let postgres = unit(&provider, "postgresql.service").await;

    assert_eq!(
        postgres.access("systemd.memory"),
        FieldAccess::Unknown,
        "systemd's u64::MAX means it does not know, and an unknown byte count is null, not zero"
    );
    assert_eq!(postgres.access("systemd.tasks"), FieldAccess::Unknown);
}

#[tokio::test]
async fn should_report_no_main_pid_as_null_when_the_unit_runs_no_process() {
    let provider = provider_over(RecordedSystemd::running()).await;

    let postgres = unit(&provider, "postgresql.service").await;
    assert_eq!(
        postgres.access("pid"),
        FieldAccess::Unknown,
        "systemd reports MainPID 0 for a unit with no main process; 0 is not a pid"
    );

    let timer = unit(&provider, "logrotate.timer").await;
    assert_eq!(timer.access("pid"), FieldAccess::Unknown);
    assert_eq!(
        timer.access("unit_file"),
        FieldAccess::Unknown,
        "an empty FragmentPath is no unit file, not a path called \"\""
    );
    assert_eq!(
        timer.access("since"),
        FieldAccess::Unknown,
        "a zero StateChangeTimestamp is systemd saying it never moved, not the Unix epoch"
    );
    assert_eq!(
        timer.access("enabled"),
        FieldAccess::Unknown,
        "a static unit can be neither enabled nor disabled, so `enabled` is unknown"
    );
}

#[tokio::test]
async fn should_report_a_masked_unit_as_not_enabled() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let blocked = unit(&provider, "ono-blocked.service").await;

    assert_eq!(blocked.get("enabled"), Some(&Value::Bool(false)));
    assert_eq!(text(&blocked, "systemd.load_state"), "masked");
    assert_eq!(text(&blocked, "systemd.unit_file_state"), "masked");
    assert_eq!(text(&blocked, "state"), "inactive");
}

#[tokio::test]
async fn should_validate_every_record_against_the_schema_it_advertises() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let all = units(&provider, &Query::target("service")).await;

    assert_eq!(all.len(), 4, "every recorded unit is answered for");
    for record in &all {
        assert_eq!(record.schema_id(), service_schema().id());
        record.validate().unwrap_or_else(|error| {
            panic!("{} violates its own schema: {error}", record.schema_id())
        });
    }
}

#[tokio::test]
async fn should_answer_only_the_named_unit_when_a_query_selects_one() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let query = Query::target("service").with(Selector::field(
        "name",
        Value::String("nginx.service".into()),
    ));

    let names: Vec<String> = units(&provider, &query)
        .await
        .iter()
        .map(|record| text(record, "name"))
        .collect();
    assert_eq!(names, ["nginx.service"]);
}

#[tokio::test]
async fn should_find_a_unit_when_the_name_is_given_without_its_suffix() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let query =
        Query::target("service").with(Selector::field("name", Value::String("nginx".into())));

    let names: Vec<String> = units(&provider, &query)
        .await
        .iter()
        .map(|record| text(record, "name"))
        .collect();
    assert_eq!(
        names,
        ["nginx.service"],
        "`get service nginx` means nginx.service"
    );
}

#[tokio::test]
async fn should_answer_nothing_rather_than_a_stub_when_the_named_unit_does_not_exist() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let query = Query::target("service").with(Selector::field(
        "name",
        Value::String("nothing-here".into()),
    ));

    let answer = collected(&provider, &query).await;
    assert!(
        answer.values().is_empty(),
        "systemd answers a name it does not know with a `not-found` stub; a stub is not a service"
    );
    assert!(
        answer.errors().is_empty(),
        "not existing is an answer, not a failure"
    );
}

#[tokio::test]
async fn should_find_a_unit_on_disk_when_systemd_has_not_loaded_it() {
    // systemd unloads inactive units from memory, so a disabled-but-present service is absent
    // from `ListUnits` while `systemctl status` happily reports it. A name pins the unit, and a
    // pinned unit is resolved through `LoadUnit` — not by filtering the enumeration.
    let provider =
        provider_over(RecordedSystemd::running().with_unit_file_on_disk(fixture::on_disk_only()))
            .await;

    let everything: Vec<String> = units(&provider, &Query::target("service"))
        .await
        .iter()
        .map(|record| text(record, "name"))
        .collect();
    assert!(
        !everything.contains(&"certbot.service".to_owned()),
        "the fixture's premise: an unloaded unit is not enumerated"
    );

    let certbot = unit(&provider, "certbot.service").await;
    assert_eq!(text(&certbot, "name"), "certbot.service");
    assert_eq!(
        text(&certbot, "state"),
        "inactive",
        "the state is what systemd says after loading the unit, not a guess"
    );
    assert_eq!(certbot.get("enabled"), Some(&Value::Bool(false)));
    assert_eq!(text(&certbot, "provider"), PROVIDER_ID);
    assert_eq!(
        certbot.schema_id(),
        service_schema().id(),
        "a unit loaded on demand answers in the same shape as an enumerated one"
    );
    certbot
        .validate()
        .unwrap_or_else(|error| panic!("{} violates its own schema: {error}", certbot.schema_id()));
}

#[tokio::test]
async fn should_report_no_service_when_a_listed_unit_is_only_a_dangling_reference() {
    // Recorded from a real service manager: a unit whose file is gone stays in `ListUnits` as a
    // `not-found` stub for as long as something references its name. The by-name path already
    // refuses such stubs; the listing must agree, or a unit found by `get service` vanishes the
    // moment it is asked for by name — the disagreement that made a CI round trip flaky.
    let provider =
        provider_over(RecordedSystemd::running().with_dangling_reference("hv_kvp_daemon.service"))
            .await;

    let names: Vec<String> = units(&provider, &Query::target("service"))
        .await
        .iter()
        .map(|record| text(record, "name"))
        .collect();
    assert!(
        !names.contains(&"hv_kvp_daemon.service".to_owned()),
        "a stub with no unit file is not a service this machine has"
    );
    assert_eq!(names.len(), 4, "the four real units are still answered for");

    let by_name = Query::target("service").with(Selector::field(
        "name",
        Value::String("hv_kvp_daemon.service".into()),
    ));
    let answer = collected(&provider, &by_name).await;
    assert!(
        answer.values().is_empty() && answer.errors().is_empty(),
        "by name and by enumeration must give the same answer: no such service"
    );
}

#[tokio::test]
async fn should_resolve_a_named_unit_to_one_object_reference() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let selector = Selector::field("name", Value::String("nginx.service".into()));

    let refs = tokio::time::timeout(BUDGET, provider.resolve(&selector))
        .await
        .expect("resolution must not hang")
        .expect("the recorded service manager answers");

    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].id(), &unit_id("nginx.service"));
}

// --- degrading honestly ------------------------------------------------------------------------

#[tokio::test]
async fn should_report_unavailable_with_an_actionable_reason_when_no_service_manager_answers() {
    let provider = provider_over(RecordedSystemd::absent(
        "the D-Bus system bus socket /run/dbus/system_bus_socket does not exist",
    ))
    .await;

    let availability = provider.availability();
    assert!(!availability.is_available());
    let reason = availability
        .reason()
        .expect("an unavailable provider says why");
    assert!(
        reason.contains("/run/dbus/system_bus_socket"),
        "the reason must name what is missing, not just that something is: {reason}"
    );
}

#[tokio::test]
async fn should_refuse_to_answer_rather_than_return_no_services_when_systemd_is_absent() {
    let provider = provider_over(RecordedSystemd::absent(
        "the D-Bus system bus socket /run/dbus/system_bus_socket does not exist",
    ))
    .await;

    let error = provider
        .snapshot(&Query::target("service"))
        .expect_err("an empty answer would be indistinguishable from a machine with no services");

    assert_eq!(error.code(), ErrorCode::ProviderUnavailable);
    assert!(error.message().contains("/run/dbus/system_bus_socket"));
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("not the same")),
        "a user must be told that no answer is not the answer `none`"
    );
}

#[tokio::test]
async fn should_refuse_to_act_when_systemd_is_absent() {
    let provider = provider_over(RecordedSystemd::absent("no service manager here")).await;
    let action = Action::new("service", "start", unit_id("nginx.service"));

    let error = tokio::time::timeout(BUDGET, provider.act(&action))
        .await
        .expect("refusing must not hang")
        .expect_err("an action cannot even be attempted without a service manager");
    assert_eq!(error.code(), ErrorCode::ProviderUnavailable);
}

#[tokio::test]
async fn should_name_the_reason_it_cannot_answer_on_this_machine() {
    // Runs everywhere: on a systemd host it is available; in the container and under WSL it is
    // not, and then the reason has to be something a person can act on.
    let provider = tokio::time::timeout(BUDGET, SystemdProvider::connect())
        .await
        .expect("probing the system bus must not hang");

    if let Some(reason) = provider.availability().reason() {
        assert!(
            reason.contains("D-Bus"),
            "the reason must name the mechanism that is missing: {reason}"
        );
        assert!(
            reason.len() > 30,
            "`{reason}` is not something a user can act on"
        );
    }
}

// --- operations --------------------------------------------------------------------------------

#[tokio::test]
async fn should_report_a_change_when_a_stopped_unit_is_started() {
    let provider = provider_over(RecordedSystemd::running()).await;

    let outcome = act(&provider, "start", "postgresql.service").await;
    assert_eq!(outcome.status(), ActionStatus::Success);
    assert!(outcome.changed());
    assert_eq!(outcome.operation(), "start");

    let postgres = unit(&provider, "postgresql.service").await;
    assert_eq!(text(&postgres, "state"), "active");
}

#[tokio::test]
async fn should_report_skipped_when_the_unit_is_already_in_the_requested_state() {
    let provider = provider_over(RecordedSystemd::running()).await;

    let outcome = act(&provider, "start", "nginx.service").await;
    assert_eq!(
        outcome.status(),
        ActionStatus::Skipped,
        "starting a running service is not a success that changed something"
    );
    assert!(!outcome.changed());

    let stopped = act(&provider, "stop", "logrotate.timer").await;
    assert_eq!(stopped.status(), ActionStatus::Skipped);
}

#[tokio::test]
async fn should_report_a_change_when_a_running_unit_is_restarted() {
    let provider = provider_over(RecordedSystemd::running()).await;

    let outcome = act(&provider, "restart", "nginx.service").await;
    assert_eq!(
        outcome.status(),
        ActionStatus::Success,
        "a restart always does something, however the unit was"
    );
    assert!(outcome.changed());
}

#[tokio::test]
async fn should_report_a_change_when_a_running_unit_is_stopped() {
    let provider = provider_over(RecordedSystemd::running()).await;

    let outcome = act(&provider, "stop", "nginx.service").await;
    assert_eq!(outcome.status(), ActionStatus::Success);
    assert!(outcome.changed());
    assert_eq!(
        text(&unit(&provider, "nginx.service").await.clone(), "state"),
        "inactive"
    );
}

#[tokio::test]
async fn should_report_a_change_when_a_running_unit_is_reloaded() {
    let provider = provider_over(RecordedSystemd::running()).await;

    let outcome = act(&provider, "reload", "nginx.service").await;
    assert_eq!(outcome.status(), ActionStatus::Success);
    assert!(outcome.changed());
}

#[tokio::test]
async fn should_report_a_change_when_a_disabled_unit_is_enabled() {
    let provider = provider_over(RecordedSystemd::running()).await;

    let disabled = act(&provider, "disable", "nginx.service").await;
    assert_eq!(disabled.status(), ActionStatus::Success);
    assert!(disabled.changed());
    assert_eq!(
        unit(&provider, "nginx.service").await.get("enabled"),
        Some(&Value::Bool(false))
    );

    let enabled = act(&provider, "enable", "nginx.service").await;
    assert_eq!(enabled.status(), ActionStatus::Success);
    assert!(enabled.changed());
}

#[tokio::test]
async fn should_change_what_starts_at_boot_when_set_carries_the_enabled_property() {
    // `set service nginx --enabled false` reaches the provider as the operation `set` with the
    // property as an argument (service.yaml `ono.service.set`, ADR-0068).
    let provider = provider_over(RecordedSystemd::running()).await;
    let action =
        Action::new("service", "set", unit_id("nginx.service")).with("enabled", Value::Bool(false));

    let outcome = tokio::time::timeout(BUDGET, provider.act(&action))
        .await
        .expect("an action against a recorded service manager must not hang")
        .expect("`set` with a declared property is an operation the provider attempts");
    assert_eq!(outcome.status(), ActionStatus::Success);
    assert!(outcome.changed());
    assert_eq!(
        unit(&provider, "nginx.service").await.get("enabled"),
        Some(&Value::Bool(false))
    );
}

#[tokio::test]
async fn should_refuse_set_when_no_property_it_can_change_is_given() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let action = Action::new("service", "set", unit_id("nginx.service"));

    let error = tokio::time::timeout(BUDGET, provider.act(&action))
        .await
        .expect("refusing must not hang")
        .expect_err("`set` with nothing to set is not an action the provider can attempt");
    assert_eq!(error.code(), ErrorCode::ProviderUnsupported);
    assert!(
        error.message().contains("enabled"),
        "the refusal names the property it can change: {}",
        error.message()
    );
}

#[tokio::test]
async fn should_report_skipped_when_an_already_enabled_unit_is_enabled() {
    let provider = provider_over(RecordedSystemd::running()).await;

    let outcome = act(&provider, "enable", "nginx.service").await;
    assert_eq!(outcome.status(), ActionStatus::Skipped);
    assert!(!outcome.changed());
}

#[tokio::test]
async fn should_report_permission_denied_with_what_systemd_said_when_polkit_refuses() {
    let provider = provider_over(RecordedSystemd::refusing_authorisation()).await;

    let outcome = act(&provider, "restart", "nginx.service").await;
    assert_eq!(outcome.status(), ActionStatus::Failed);
    assert!(!outcome.changed());

    let error = outcome.error().expect("a refusal is never silent");
    assert_eq!(error.code(), ErrorCode::IoPermissionDenied);
    assert!(
        error
            .message()
            .contains("Interactive authentication required."),
        "systemd's own words must survive: {}",
        error.message()
    );
    assert!(error.help().is_some_and(|help| help.contains("polkit")));
}

#[tokio::test]
async fn should_report_failure_rather_than_success_when_the_unit_does_not_exist() {
    let provider = provider_over(RecordedSystemd::running()).await;

    let outcome = act(&provider, "start", "nothing-here.service").await;
    assert_eq!(outcome.status(), ActionStatus::Failed);
    let error = outcome.error().expect("a failure says why");
    assert_eq!(error.code(), ErrorCode::IoNotFound);
}

#[tokio::test]
async fn should_change_nothing_when_an_action_is_a_dry_run() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let action = Action::new("service", "stop", unit_id("nginx.service")).as_dry_run();

    let outcome = tokio::time::timeout(BUDGET, provider.act(&action))
        .await
        .expect("a dry run must not hang")
        .expect("the provider can report what it would do");

    // A dry run is a report, never a claimed change: `skipped`, saying what would happen —
    // the same contract `ono-provider-linux` keeps. Reporting `succeeded/changed` here would
    // put a completed mutation in the record for something that never ran.
    assert_eq!(outcome.status(), ActionStatus::Skipped);
    assert!(!outcome.changed(), "nothing ran, so nothing changed");
    assert_eq!(
        text(&unit(&provider, "nginx.service").await.clone(), "state"),
        "active",
        "a dry run reports; it does not act"
    );
}

#[tokio::test]
async fn should_refuse_an_operation_it_does_not_implement() {
    let provider = provider_over(RecordedSystemd::running()).await;
    let action = Action::new("service", "vaporise", unit_id("nginx.service"));

    let error = tokio::time::timeout(BUDGET, provider.act(&action))
        .await
        .expect("refusing must not hang")
        .expect_err("the provider has no such operation");
    assert_eq!(error.code(), ErrorCode::ProviderUnsupported);
}

// --- what the provider declares ----------------------------------------------------------------

#[tokio::test]
async fn should_declare_the_target_schema_and_capabilities_the_service_commands_require() {
    let provider = provider_over(RecordedSystemd::running()).await;

    assert_eq!(provider.id(), PROVIDER_ID);
    assert_eq!(provider.targets(), ["service"]);
    assert_eq!(
        provider
            .schemas()
            .iter()
            .map(|schema| schema.id().to_string())
            .collect::<Vec<_>>(),
        ["ono.service/1"]
    );

    let capabilities: Vec<String> = provider
        .capabilities()
        .iter()
        .map(|capability| capability.id().to_owned())
        .collect();
    assert_eq!(capabilities, ["service.list", "service.manage"]);
    assert!(
        provider
            .capabilities()
            .iter()
            .find(|capability| capability.id() == "service.manage")
            .is_some_and(ono_provider_api::Capability::needs_elevation),
        "docs/spec/capabilities.yaml gives service.manage elevation `required`"
    );
}

#[tokio::test]
async fn should_say_it_cannot_watch_rather_than_poll_without_saying_so() {
    let provider = provider_over(RecordedSystemd::running()).await;

    let error = provider
        .subscribe(&Query::target("service"))
        .expect_err("this provider has no event source yet");
    assert_eq!(error.code(), ErrorCode::ProviderUnsupported);
}

#[tokio::test]
async fn should_agree_with_the_running_service_manager_when_one_is_present() {
    // Not `#[ignore]`d: an ignored test is untracked unfinished work. It decides at runtime,
    // because whether systemd runs here is a property of the machine, not of the code.
    let provider = tokio::time::timeout(BUDGET, SystemdProvider::connect())
        .await
        .expect("probing the system bus must not hang");
    if !provider.availability().is_available() {
        return;
    }

    let query = Query::target("service").limit(10);
    let stream = provider
        .snapshot(&query)
        .expect("an available provider answers");
    let collected = tokio::time::timeout(BUDGET, stream.collect())
        .await
        .expect("reading ten real units must not hang");

    let records: Vec<Arc<RecordValue>> = collected
        .into_values()
        .into_iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some(record),
            _ => None,
        })
        .collect();
    assert!(
        !records.is_empty(),
        "a running service manager has units; an empty answer here would be the very \
         absence-versus-ignorance conflation this provider exists to avoid"
    );
    for record in &records {
        record
            .validate()
            .unwrap_or_else(|error| panic!("a real unit violates ono.service/1: {error}"));
        assert!(!text(record, "name").is_empty());
        assert_eq!(text(record, "provider"), PROVIDER_ID);
    }

    // A name a user typed without its suffix must reach the unit, and a name that cannot exist
    // must reach nothing. A real service manager answers the first with `InvalidArgs` on the
    // bare spelling and the second with a `not-found` stub rather than an error, which is why
    // both are asserted against the live one rather than only against the recording.
    let nothing = Query::target("service").with(Selector::field(
        "name",
        Value::String("ono-definitely-not-a-unit".into()),
    ));
    let answer = tokio::time::timeout(
        BUDGET,
        provider.snapshot(&nothing).expect("available").collect(),
    )
    .await
    .expect("asking for a unit that cannot exist must not hang");
    assert!(answer.values().is_empty(), "no such unit is not a service");
    assert!(
        answer.errors().is_empty(),
        "no such unit is an answer, not a failure"
    );

    // Only an *active* service is a stable subject: systemd unloads inactive units from memory,
    // so a condition-failed daemon that shows up in one enumeration can be gone by the next
    // query — which is exactly what happened to `hv_kvp_daemon.service` on a CI runner.
    let Some(service) = records
        .iter()
        .filter(|record| text(record, "state") == "active")
        .map(|record| text(record, "name"))
        .find(|name| name.ends_with(".service"))
    else {
        return;
    };
    let bare = service.trim_end_matches(".service").to_owned();
    let by_bare_name =
        Query::target("service").with(Selector::field("name", Value::String(bare.as_str().into())));
    let found = tokio::time::timeout(
        BUDGET,
        provider
            .snapshot(&by_bare_name)
            .expect("available")
            .collect(),
    )
    .await
    .expect("resolving a bare unit name must not hang");
    let names: Vec<String> = found
        .values()
        .iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some(text(record, "name")),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        std::slice::from_ref(&service),
        "`get service {bare}` must find {service}"
    );
}
