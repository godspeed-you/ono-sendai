//! `get namespace`: the kernel namespaces the processes of this host live in, one
//! `ono.namespace/1` record each, read from `/proc/<pid>/ns` (spec §8.1, v0.3 §1.35, v0.4 §7.3,
//! ADR-0971, #206).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers shared between the cases below sit outside a `#[test]` function, where a \
              failed precondition should still abort loudly"
)]

mod common;

use std::sync::Arc;

use common::{FakeAccounts, ProcFixture, StatFields, drain, is_root, records};
use ono_core::ErrorCode;
use ono_provider_api::{Provider, Query, Selector};
use ono_provider_linux::NamespaceProvider;
use ono_testkit::SkipReason;
use ono_value::{RecordValue, Value};

const HOST_PID: u64 = 4_026_531_836;
const HOST_NET: u64 = 4_026_531_840;
const CONTAINER_PID: u64 = 4_026_532_301;
const CONTAINER_NET: u64 = 4_026_532_304;

fn provider(fixture: &ProcFixture) -> NamespaceProvider {
    NamespaceProvider::rooted(fixture.root()).with_accounts(Arc::new(
        FakeAccounts::new()
            .with_user(0, 0, "root")
            .with_user(33, 33, "www-data"),
    ))
}

/// Three processes: init and a shell in the host's namespaces, nginx in a container's.
fn three_processes() -> ProcFixture {
    let fixture = ProcFixture::new();
    fixture
        .process(1)
        .stat(
            "systemd",
            StatFields {
                ppid: 0,
                ..StatFields::default()
            },
        )
        .status(0, 0)
        .cmdline(&["/sbin/init"])
        .namespace("pid", HOST_PID)
        .namespace("net", HOST_NET)
        // The namespace its children will enter is not one any process is in.
        .namespace("pid_for_children", HOST_PID);
    fixture
        .process(812)
        .stat("bash", StatFields::default())
        .status(0, 0)
        .cmdline(&["-bash"])
        .namespace("pid", HOST_PID)
        .namespace("net", HOST_NET);
    fixture
        .process(4242)
        .stat(
            "nginx",
            StatFields {
                ppid: 4200,
                ..StatFields::default()
            },
        )
        .status(33, 33)
        .cmdline(&["nginx", "-g", "daemon off;"])
        .namespace("pid", CONTAINER_PID)
        .namespace("net", CONTAINER_NET);
    fixture
}

fn namespace(records: &[Arc<RecordValue>], id: u64) -> Arc<RecordValue> {
    records
        .iter()
        .find(|record| record.get("id") == Some(&Value::Int(i128::from(id))))
        .cloned()
        .unwrap_or_else(|| panic!("no namespace {id} among {records:?}"))
}

#[tokio::test]
async fn should_report_each_namespace_once_with_the_processes_that_live_in_it() {
    let fixture = three_processes();

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("namespace"))
            .expect("a snapshot"),
    )
    .await;
    assert!(collected.errors().is_empty(), "{:?}", collected.errors());
    let records = records(&collected);
    let listed: Vec<(String, i128)> = records
        .iter()
        .map(|record| {
            let kind = match record.get("type") {
                Some(Value::String(kind)) => kind.to_string(),
                other => panic!("a namespace names its kind, got {other:?}"),
            };
            let Some(Value::Int(id)) = record.get("id") else {
                panic!("a namespace carries its inode");
            };
            (kind, *id)
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("net".to_owned(), i128::from(HOST_NET)),
            ("net".to_owned(), i128::from(CONTAINER_NET)),
            ("pid".to_owned(), i128::from(HOST_PID)),
            ("pid".to_owned(), i128::from(CONTAINER_PID)),
        ],
        "one record per namespace, by kind and inode, and none for a `*_for_children` link"
    );

    let host = namespace(&records, HOST_PID);
    assert_eq!(host.get("processes"), Some(&Value::Int(2)));
    assert_eq!(
        host.get("pid"),
        Some(&Value::Int(1)),
        "the lowest pid in it"
    );
    assert_eq!(host.get("parent_pid"), Some(&Value::Int(0)));
    assert_eq!(host.get("user"), Some(&Value::string("root")));
    assert_eq!(host.get("uid"), Some(&Value::Int(0)));
    assert_eq!(host.get("command"), Some(&Value::string("/sbin/init")));

    let container = namespace(&records, CONTAINER_NET);
    assert_eq!(container.get("processes"), Some(&Value::Int(1)));
    assert_eq!(container.get("pid"), Some(&Value::Int(4242)));
    assert_eq!(container.get("parent_pid"), Some(&Value::Int(4200)));
    assert_eq!(container.get("user"), Some(&Value::string("www-data")));
    assert_eq!(
        container.get("command"),
        Some(&Value::string("nginx -g daemon off;"))
    );
}

#[tokio::test]
async fn should_count_only_the_processes_whose_links_the_caller_may_read() {
    let fixture = three_processes();
    // Another user's process: the `ns` directory is there and discloses nothing.
    let hidden = fixture.proc().join("999");
    std::fs::create_dir_all(hidden.join("ns")).expect("the ns directory");
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(hidden.join("ns"), std::fs::Permissions::from_mode(0o000))
            .expect("the mode");
    }

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("namespace"))
            .expect("a snapshot"),
    )
    .await;
    let records = records(&collected);
    // Restore the mode so the fixture can be removed.
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(hidden.join("ns"), std::fs::Permissions::from_mode(0o755))
            .expect("the mode");
    }
    assert_eq!(
        namespace(&records, HOST_PID).get("processes"),
        Some(&Value::Int(2)),
        "a process that hides its namespaces is not counted into one"
    );
    assert_eq!(records.len(), 4);
}

#[tokio::test]
async fn should_refuse_rather_than_answer_empty_when_every_link_is_withheld() {
    if is_root() {
        ono_testkit::skipped(
            SkipReason::MissingPrivilege,
            "root reads through a mode of 000, so the refusal cannot be staged",
        );
        return;
    }
    let fixture = ProcFixture::new();
    let hidden = fixture.proc().join("999");
    std::fs::create_dir_all(hidden.join("ns")).expect("the ns directory");
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(hidden.join("ns"), std::fs::Permissions::from_mode(0o000))
            .expect("the mode");
    }

    let collected = drain(
        provider(&fixture)
            .snapshot(&Query::target("namespace"))
            .expect("a snapshot"),
    )
    .await;
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(hidden.join("ns"), std::fs::Permissions::from_mode(0o755))
            .expect("the mode");
    }
    assert!(records(&collected).is_empty());
    assert_eq!(
        collected
            .errors()
            .iter()
            .map(ono_value::ErrorValue::code)
            .collect::<Vec<_>>(),
        [ErrorCode::IoPermissionDenied],
        "namespaces that exist and may not be read are a refusal, never an empty list (§42.4)"
    );
}

#[tokio::test]
async fn should_resolve_one_namespace_by_its_inode() {
    let fixture = three_processes();

    let references = provider(&fixture)
        .resolve(&Selector::field(
            "id",
            Value::Int(i128::from(CONTAINER_PID)),
        ))
        .await
        .expect("a resolution");
    assert_eq!(references.len(), 1, "{references:?}");
}

/// v0.4.1 §34.4 and §2.17: the orientation of `network/namespaces` reads a bounded view, and the
/// count it shows must stay the count that is there (ADR-0576).
#[tokio::test]
async fn should_state_how_many_namespaces_there_are_when_a_query_bounds_what_it_answers() {
    let fixture = three_processes();
    let provider = provider(&fixture);

    let whole = drain(
        provider
            .snapshot(&Query::target("namespace"))
            .expect("a snapshot"),
    )
    .await;
    let population = records(&whole).len();
    assert!(
        population > 1,
        "the fixture must hold more namespaces than the bound, or nothing is left out"
    );

    let bounded = drain(
        provider
            .snapshot(&Query::target("namespace").limit(1))
            .expect("a snapshot"),
    )
    .await;
    assert_eq!(records(&bounded).len(), 1, "the bound is honoured");
    assert_eq!(
        bounded.diagnostics().population(),
        Some(population as u64),
        "the bounded answer says how many namespaces it would have answered with"
    );
}
