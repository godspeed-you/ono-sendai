//! What `get process` and `kill process` answer (spec §23.1, §28.1, ADR-0015 T13).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers shared between the cases below sit outside a `#[test]` function, where a \
              failed precondition should still abort loudly"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{
    FakeAccounts, ProcFixture, RecordingSignals, StatFields, TestClock, USER_HZ, drain,
    expected_start, find, records,
};
use ono_core::ErrorCode;
use ono_provider_api::{Action, ObjectId, Provider, Query, Selector};
use ono_provider_linux::{ProcessProvider, schemas};
use ono_value::{ByteSize, FieldAccess, Value};

fn accounts() -> Arc<FakeAccounts> {
    Arc::new(
        FakeAccounts::new()
            .with_user(1000, 100, "ada")
            .with_group(100, "users", &["ada"]),
    )
}

fn provider(fixture: &ProcFixture) -> ProcessProvider {
    ProcessProvider::rooted(fixture.root()).with_accounts(accounts())
}

#[tokio::test]
async fn should_report_every_declared_field_when_the_process_is_fully_readable() {
    let fixture = ProcFixture::new();
    fixture
        .process(4419)
        .stat(
            "nginx",
            StatFields {
                state: 'R',
                ppid: 812,
                utime: 30,
                stime: 12,
                threads: 4,
                starttime: 12_345 * USER_HZ,
                vsize: 1_048_576,
                rss_pages: 100,
            },
        )
        .status(1000, 100)
        .cmdline(&["/usr/sbin/nginx", "-g", "daemon off;"])
        .exe("/usr/sbin/nginx")
        .cwd("/var/www")
        .cgroup("0::/system.slice/nginx.service\n");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("process"))
            .expect("the fixture proc tree can be enumerated"),
    )
    .await;
    let records = records(&collected);
    let process = records.first().expect("the fixture holds one process");

    assert_eq!(process.get("pid"), Some(&Value::Int(4419)));
    assert_eq!(process.get("ppid"), Some(&Value::Int(812)));
    assert_eq!(process.get("name"), Some(&Value::string("nginx")));
    assert_eq!(
        process.get("command"),
        Some(&Value::list([
            Value::string("/usr/sbin/nginx"),
            Value::string("-g"),
            Value::string("daemon off;"),
        ]))
    );
    assert_eq!(
        process.get("executable"),
        Some(&Value::Path(Arc::from(std::path::Path::new(
            "/usr/sbin/nginx"
        ))))
    );
    assert_eq!(
        process.get("cwd"),
        Some(&Value::Path(Arc::from(std::path::Path::new("/var/www"))))
    );
    assert_eq!(process.get("state"), Some(&Value::string("running")));
    assert_eq!(process.get("threads"), Some(&Value::Int(4)));
    assert_eq!(
        process.get("virtual_mem"),
        Some(&Value::ByteSize(ByteSize::from_bytes(1_048_576)))
    );
    assert_eq!(
        process.get("service"),
        Some(&Value::string("nginx.service")),
        "the cgroup names the unit, so the process reports which service claims it"
    );
    assert_eq!(
        process.get("started"),
        Some(&Value::Timestamp(expected_start(12_345 * USER_HZ)))
    );
    assert!(
        matches!(process.get("memory"), Some(Value::ByteSize(size)) if size.bytes() > 0),
        "the resident set is the page count times the page size, not the page count"
    );
    assert_eq!(
        process.access("cpu"),
        FieldAccess::Unknown,
        "one procfs read cannot answer a rate, and null is the only honest answer"
    );
    assert_eq!(
        process.access("container"),
        FieldAccess::Unknown,
        "no container provider claims this process, and unknown is null rather than a guess"
    );

    let user = process
        .get("user")
        .and_then(|value| value.as_record().ok())
        .expect("the user reference is a record");
    assert_eq!(user.get("uid"), Some(&Value::Int(1000)));
    assert_eq!(user.get("name"), Some(&Value::string("ada")));
    let group = process
        .get("group")
        .and_then(|value| value.as_record().ok())
        .expect("the group reference is a record");
    assert_eq!(group.get("gid"), Some(&Value::Int(100)));
    assert_eq!(group.get("name"), Some(&Value::string("users")));

    let source = process
        .provenance()
        .source()
        .expect("every record says what it was read from");
    assert!(
        source.contains("/4419/stat") && source.contains("/4419/status"),
        "provenance names the exact files, not just `procfs`: {source}"
    );
    assert_eq!(process.provenance().provider(), "linux.procfs");
}

#[tokio::test]
async fn should_report_a_rate_on_the_second_observation_after_null_on_the_first() {
    let fixture = ProcFixture::new();
    let clock = TestClock::new();
    let provider = ProcessProvider::rooted(fixture.root())
        .with_accounts(accounts())
        .with_clock(Arc::clone(&clock) as Arc<dyn ono_provider_linux::Clock>);

    fixture.process(7).stat(
        "busy",
        StatFields {
            utime: 0,
            stime: 0,
            ..StatFields::default()
        },
    );
    let first = drain(
        provider
            .snapshot(&Query::target("process"))
            .expect("a snapshot"),
    )
    .await;
    assert_eq!(
        records(&first)[0].access("cpu"),
        FieldAccess::Unknown,
        "the first observation has nothing to divide by"
    );

    // Half a second of wall clock, during which the process used a quarter of a second of CPU.
    clock.advance(Duration::from_millis(500));
    fixture.process(7).stat(
        "busy",
        StatFields {
            utime: 15,
            stime: 10,
            ..StatFields::default()
        },
    );
    let second = drain(
        provider
            .snapshot(&Query::target("process"))
            .expect("a snapshot"),
    )
    .await;
    let cpu = records(&second)[0]
        .get("cpu")
        .and_then(|value| value.as_float().ok())
        .expect("the second observation is a rate");
    assert!(
        (cpu - 50.0).abs() < 1e-9,
        "25 ticks of 100 per second over half a second is half a CPU: got {cpu}"
    );
}

#[tokio::test]
async fn should_keep_the_identity_stable_across_two_observations() {
    let fixture = ProcFixture::new();
    fixture.process(99).stat("stable", StatFields::default());
    let provider = provider(&fixture);

    let first = drain(provider.snapshot(&Query::target("process")).expect("first")).await;
    let second = drain(
        provider
            .snapshot(&Query::target("process"))
            .expect("second"),
    )
    .await;

    let left = ObjectId::of(&records(&first)[0]).expect("a process has an identity");
    let right = ObjectId::of(&records(&second)[0]).expect("a process has an identity");
    assert_eq!(
        left, right,
        "the identity is (pid, started) and neither moved between the two reads"
    );
}

#[tokio::test]
async fn should_skip_a_process_that_exits_between_enumeration_and_detail_read() {
    let fixture = ProcFixture::new();
    fixture.process(11).stat("survivor", StatFields::default());
    // A directory with no `stat` is exactly what the kernel leaves behind for the instant
    // between a listing and a read of a process that has just exited.
    fixture.process(12);

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("process"))
            .expect("a snapshot"),
    )
    .await;

    assert_eq!(
        records(&collected).len(),
        1,
        "the process that vanished is skipped, not reported half-read"
    );
    assert_eq!(
        collected.errors(),
        &[],
        "a process that no longer exists is not part of the answer and is not a failure to \
         report; `get process` on a busy machine would otherwise be unusable for the noise \
         (ADR-0029). Got {:?}",
        collected.errors()
    );
}

#[tokio::test]
async fn should_report_a_named_process_that_does_not_exist_rather_than_answering_with_nothing() {
    let fixture = ProcFixture::new();
    fixture.process(11).stat("survivor", StatFields::default());

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("process").with(Selector::field("pid", Value::Int(12))))
            .expect("a snapshot"),
    )
    .await;

    assert!(
        records(&collected).is_empty(),
        "there is no process 12 to report"
    );
    let failure = collected.errors().first().expect(
        "a process the user named by pid is a target, and a target that is not there is an \
         answer the user needs",
    );
    assert!(
        failure.message().contains("/12/stat"),
        "the failure names which process was asked for: {}",
        failure.message()
    );
    assert_eq!(failure.code(), ErrorCode::IoNotFound);
}

#[tokio::test]
async fn should_report_a_process_it_is_not_allowed_to_read_while_enumerating() {
    let fixture = ProcFixture::new();
    fixture.process(11).stat("survivor", StatFields::default());
    // A directory where `stat` belongs fails with EISDIR for every user, root included, so this
    // says the same thing whoever runs the suite. It is not the vanishing case: the process is
    // there and could not be read, which the user must be told about.
    fixture.process(12);
    std::fs::create_dir_all(fixture.proc().join("12/stat")).expect("the blocked stat");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("process"))
            .expect("a snapshot"),
    )
    .await;

    assert_eq!(records(&collected).len(), 1, "the readable process arrives");
    let failure = collected
        .errors()
        .first()
        .expect("a process that exists and cannot be read is reported (spec §16.5)");
    assert!(
        failure.message().contains("/12/stat"),
        "the failure names the process: {}",
        failure.message()
    );
    assert_ne!(
        failure.code(),
        ErrorCode::IoNotFound,
        "the process is there; only reading it failed"
    );
}

#[tokio::test]
async fn should_report_an_unreadable_command_line_as_an_error_rather_than_as_null() {
    let fixture = ProcFixture::new();
    fixture.process(21).stat("hidden", StatFields::default());
    // A directory where a file belongs makes every reader fail with EISDIR, including root — so
    // the assertion means the same thing whoever runs the suite.
    std::fs::create_dir_all(fixture.proc().join("21/cmdline")).expect("the blocked cmdline");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("process"))
            .expect("a snapshot"),
    )
    .await;
    let process = &records(&collected)[0];

    assert!(
        process.access("command").is_failed(),
        "a field this reader may not see is an error value, never null and never an empty list"
    );
    assert!(
        !process.access("command").is_unknown(),
        "an unreadable command line must stay distinguishable from an unknown one"
    );
}

#[tokio::test]
async fn should_report_permission_denied_in_the_field_when_a_file_is_closed_to_this_user() {
    // Running as root defeats the mode bits this fixture relies on, and a root CI run would
    // otherwise silently assert nothing.
    if is_root() {
        return;
    }
    let fixture = ProcFixture::new();
    fixture
        .process(22)
        .stat("private", StatFields::default())
        .unreadable("cmdline");

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("process"))
            .expect("a snapshot"),
    )
    .await;
    let process = &records(&collected)[0];
    let FieldAccess::Failed(error) = process.access("command") else {
        panic!("a command line closed to this user is a failed access");
    };
    assert_eq!(error.code(), ErrorCode::IoPermissionDenied);
}

#[tokio::test]
async fn should_report_null_command_for_a_process_the_kernel_gives_no_argument_vector() {
    let fixture = ProcFixture::new();
    fixture
        .process(2)
        .stat("kthreadd", StatFields::default())
        .cmdline(&[]);

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("process"))
            .expect("a snapshot"),
    )
    .await;
    assert_eq!(
        records(&collected)[0].access("command"),
        FieldAccess::Unknown,
        "a kernel thread has no argument vector; that is an absence the kernel states"
    );
}

#[tokio::test]
async fn should_read_only_the_named_process_when_a_pid_selector_pins_the_query() {
    let fixture = ProcFixture::new();
    fixture.process(5).stat("five", StatFields::default());
    fixture.process(6).stat("six", StatFields::default());

    let query = Query::target("process").with(Selector::field("pid", Value::Int(6)));
    let collected = drain(provider(&fixture).snapshot(&query).expect("a snapshot")).await;
    let records = records(&collected);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].get("name"), Some(&Value::string("six")));
}

#[tokio::test]
async fn should_refuse_to_signal_when_the_start_time_changed() {
    let fixture = ProcFixture::new();
    let signals = RecordingSignals::new();
    let provider = ProcessProvider::rooted(fixture.root())
        .with_accounts(accounts())
        .with_signals(Arc::clone(&signals) as Arc<dyn ono_provider_linux::Signals>);

    fixture.process(4242).stat(
        "chosen",
        StatFields {
            starttime: 1_000 * USER_HZ,
            ..StatFields::default()
        },
    );
    let selected = ObjectId::new(
        schemas::process_id(),
        [
            Value::Int(4242),
            Value::Timestamp(expected_start(1_000 * USER_HZ)),
        ],
    );

    // The pid is recycled: same number, different process, different start time.
    fixture.process(4242).stat(
        "impostor",
        StatFields {
            starttime: 9_000 * USER_HZ,
            ..StatFields::default()
        },
    );

    let outcome = provider
        .act(&Action::new("process", "kill", selected))
        .await
        .expect("a refusal is an outcome, not a provider failure");

    assert!(!outcome.is_success());
    assert_eq!(
        outcome.error().map(ono_value::ErrorValue::code),
        Some(ErrorCode::IoNotFound)
    );
    assert!(
        signals.sent().is_empty(),
        "nothing may be signalled once the identity has moved"
    );
}

#[tokio::test]
async fn should_deliver_the_signal_when_the_identity_still_matches() {
    let fixture = ProcFixture::new();
    let signals = RecordingSignals::new();
    let provider = ProcessProvider::rooted(fixture.root())
        .with_accounts(accounts())
        .with_signals(Arc::clone(&signals) as Arc<dyn ono_provider_linux::Signals>);

    fixture.process(4242).stat(
        "chosen",
        StatFields {
            starttime: 1_000 * USER_HZ,
            ..StatFields::default()
        },
    );
    let selected = ObjectId::new(
        schemas::process_id(),
        [
            Value::Int(4242),
            Value::Timestamp(expected_start(1_000 * USER_HZ)),
        ],
    );

    let outcome = provider
        .act(&Action::new("process", "kill", selected.clone()))
        .await
        .expect("an outcome");
    assert!(outcome.is_success());
    assert!(outcome.changed());
    assert_eq!(
        signals.sent(),
        [(4242, 9)],
        "`kill` defaults to SIGKILL, which is what distinguishes it from `stop`"
    );

    let outcome = provider
        .act(&Action::new("process", "stop", selected.clone()))
        .await
        .expect("an outcome");
    assert!(outcome.is_success());
    assert_eq!(signals.sent()[1], (4242, 15), "`stop` asks with SIGTERM");

    let named = Action::new("process", "kill", selected).with("signal", Value::string("SIGHUP"));
    provider.act(&named).await.expect("an outcome");
    assert_eq!(signals.sent()[2], (4242, 1));
}

#[tokio::test]
async fn should_report_what_it_would_do_without_signalling_when_asked_for_a_dry_run() {
    let fixture = ProcFixture::new();
    let signals = RecordingSignals::new();
    let provider = ProcessProvider::rooted(fixture.root())
        .with_signals(Arc::clone(&signals) as Arc<dyn ono_provider_linux::Signals>);
    fixture.process(31).stat("target", StatFields::default());

    let selected = ObjectId::new(
        schemas::process_id(),
        [
            Value::Int(31),
            Value::Timestamp(expected_start(StatFields::default().starttime)),
        ],
    );
    let outcome = provider
        .act(&Action::new("process", "kill", selected).as_dry_run())
        .await
        .expect("an outcome");

    assert!(!outcome.changed());
    assert!(signals.sent().is_empty());
}

#[tokio::test]
async fn should_report_the_refusal_when_the_kernel_denies_the_signal() {
    let fixture = ProcFixture::new();
    let signals = RecordingSignals::refusing(ono_value::ErrorValue::new(
        ErrorCode::IoPermissionDenied,
        "not yours",
    ));
    let provider = ProcessProvider::rooted(fixture.root())
        .with_signals(Arc::clone(&signals) as Arc<dyn ono_provider_linux::Signals>);
    fixture
        .process(41)
        .stat("root-owned", StatFields::default());

    let selected = ObjectId::new(schemas::process_id(), [Value::Int(41), Value::Null]);
    let outcome = provider
        .act(&Action::new("process", "kill", selected))
        .await
        .expect("a refused signal is an outcome, not a provider failure");

    assert!(!outcome.is_success());
    assert_eq!(
        outcome.error().map(ono_value::ErrorValue::code),
        Some(ErrorCode::IoPermissionDenied)
    );
}

#[tokio::test]
async fn should_refuse_an_operation_it_does_not_implement() {
    let fixture = ProcFixture::new();
    let error = provider(&fixture)
        .act(&Action::new(
            "process",
            "renice",
            ObjectId::new(schemas::process_id(), [Value::Int(1), Value::Null]),
        ))
        .await
        .expect_err("an unimplemented operation is a provider error, not an outcome");
    assert_eq!(error.code(), ErrorCode::ProviderUnsupported);
}

#[tokio::test]
async fn should_report_the_provider_as_unavailable_when_there_is_no_proc_filesystem() {
    let empty = tempfile::tempdir().expect("a temporary directory");
    let provider = ProcessProvider::rooted(empty.path());
    assert!(!provider.availability().is_available());
    assert!(
        provider
            .availability()
            .reason()
            .is_some_and(|reason| reason.contains("proc")),
        "an unavailable provider says why, so it cannot be mistaken for an empty result"
    );
}

#[tokio::test]
async fn should_find_the_running_process_on_the_real_proc_filesystem() {
    let provider = ProcessProvider::new();
    let collected = drain(
        provider
            .snapshot(&Query::target("process"))
            .expect("this machine has a procfs"),
    )
    .await;
    let records = records(&collected);

    assert!(
        find(&records, "pid", "1").is_some(),
        "pid 1 exists on every running Linux system"
    );
    let me = find(&records, "pid", &std::process::id().to_string())
        .expect("the test process appears in its own process list");
    assert!(
        matches!(me.get("name"), Some(Value::String(name)) if !name.is_empty()),
        "a process always has a name"
    );
    assert!(
        matches!(me.access("executable"), FieldAccess::Known(Value::Path(_))),
        "a process can always read its own executable link"
    );
    assert!(
        me.get("started").is_some_and(|value| !value.is_null()),
        "the start time is half the identity and must be answerable for one's own process"
    );
}

#[tokio::test]
async fn should_report_the_files_it_read_in_every_record_from_the_real_proc_filesystem() {
    let collected = drain(
        ProcessProvider::new()
            .snapshot(&Query::target("process").limit(5))
            .expect("a snapshot"),
    )
    .await;
    for record in records(&collected) {
        let source = record
            .provenance()
            .source()
            .expect("provenance names a source");
        assert!(
            source.contains("/proc/"),
            "every record says which kernel files answered it: {source}"
        );
    }
}

#[test]
fn should_declare_the_capability_ids_the_registry_defines() {
    let ids: Vec<String> = ProcessProvider::new()
        .capabilities()
        .iter()
        .map(|capability| capability.id().to_owned())
        .collect();
    assert!(ids.contains(&"process.list".to_owned()));
    assert!(ids.contains(&"process.inspect".to_owned()));
    assert!(ids.contains(&"process.signal".to_owned()));
}

/// Whether the suite is running with the privileges that make mode bits meaningless.
fn is_root() -> bool {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata("/proc/self").is_ok_and(|meta| meta.uid() == 0)
}
