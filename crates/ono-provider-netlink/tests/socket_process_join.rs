//! Joining a socket to the process that holds it.
//!
//! The kernel hands out an inode; the owner is only discoverable by scanning `/proc/<pid>/fd`.
//! That scan is opt-in (`--process`), because spec §34 budgets `get socket` in milliseconds and
//! the scan costs one `readlink` per open descriptor on the machine. These tests hold both
//! halves of that decision: the field is null when the scan was not asked for, and it names the
//! right process when it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "this file is only ever linked into tests, where a failed precondition should abort \
              loudly; the helpers beside the test functions state preconditions the same way"
)]

use std::net::TcpListener;
use std::os::unix::fs::symlink;

use ono_pipeline::StreamEvent;
use ono_provider_api::{Provider, Query};
use ono_provider_netlink::{SocketOwners, SocketProvider};
use ono_value::{RecordValue, Value};

async fn sockets(query: &Query) -> Vec<RecordValue> {
    let provider = SocketProvider::new();
    if !provider.availability().is_available() {
        return Vec::new();
    }
    let Ok(mut stream) = provider.snapshot(query) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    while let Some(event) = stream.recv().await {
        if let StreamEvent::Value(Value::Record(record)) = event {
            records.push((*record).clone());
        }
    }
    records
}

fn local_port(record: &RecordValue) -> Option<u16> {
    match record.get("local") {
        Some(Value::Record(endpoint)) => match endpoint.get("port") {
            Some(Value::Port(port)) => Some(*port),
            _ => None,
        },
        _ => None,
    }
}

#[tokio::test]
async fn should_name_the_process_that_holds_a_socket_this_test_created() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is always available");
    let port = listener
        .local_addr()
        .expect("a bound listener has an address")
        .port();

    let query = Query::target("socket").option("process", Value::Bool(true));
    let records = sockets(&query).await;
    if records.is_empty() {
        // sock_diag is not readable here; `should_either_read_sockets_or_say_why_not` covers it.
        return;
    }

    let ours = records
        .iter()
        .find(|record| local_port(record) == Some(port))
        .unwrap_or_else(|| panic!("the listener on port {port} was not reported"));

    let owner = match ours.get("process") {
        Some(Value::Map(owner)) => owner.clone(),
        other => panic!("the owner must be an identity map, got {other:?}"),
    };
    assert_eq!(
        owner.get("pid"),
        Some(&Value::Int(i128::from(std::process::id()))),
        "the socket belongs to the test process that bound it"
    );
    assert!(
        matches!(owner.get("name"), Some(Value::String(_))),
        "the owner carries a name so that `group process.name` works"
    );

    drop(listener);
}

#[tokio::test]
async fn should_leave_the_owner_null_when_the_scan_was_not_asked_for() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is always available");
    let port = listener
        .local_addr()
        .expect("a bound listener has an address")
        .port();

    let records = sockets(&Query::target("socket")).await;
    if records.is_empty() {
        return;
    }
    let ours = records
        .iter()
        .find(|record| local_port(record) == Some(port))
        .unwrap_or_else(|| panic!("the listener on port {port} was not reported"));

    assert_eq!(
        ours.get("process"),
        Some(&Value::Null),
        "an owner nobody looked up is unknown, and null is how that is said"
    );

    drop(listener);
}

#[tokio::test]
async fn should_narrow_the_dump_to_one_port_when_the_query_names_it() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is always available");
    let port = listener
        .local_addr()
        .expect("a bound listener has an address")
        .port();

    let query =
        Query::target("socket").with(ono_provider_api::Selector::field("port", Value::Port(port)));
    let records = sockets(&query).await;
    if records.is_empty() {
        return;
    }
    assert!(
        records.iter().all(|record| {
            local_port(record) == Some(port) || remote_port(record) == Some(port)
        }),
        "a port selector must not leave unrelated sockets in the answer"
    );

    drop(listener);
}

fn remote_port(record: &RecordValue) -> Option<u16> {
    match record.get("remote") {
        Some(Value::Record(endpoint)) => match endpoint.get("port") {
            Some(Value::Port(port)) => Some(*port),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn should_map_an_inode_to_its_owner_from_a_proc_tree() {
    let root = tempfile::tempdir().expect("a scratch directory");
    let fd = root.path().join("4242").join("fd");
    std::fs::create_dir_all(&fd).expect("the fixture tree is creatable");
    std::fs::write(root.path().join("4242").join("comm"), "ono-under-test\n")
        .expect("the fixture tree is writable");
    symlink("socket:[99001]", fd.join("3")).expect("a symlink is creatable");
    symlink("/etc/hosts", fd.join("4")).expect("a symlink is creatable");

    // A directory that is not a pid, and a process with no readable `fd` directory: both are
    // ordinary on a live machine and neither may abort the scan.
    std::fs::create_dir_all(root.path().join("self")).expect("the fixture tree is creatable");
    std::fs::create_dir_all(root.path().join("77")).expect("the fixture tree is creatable");

    let owners = SocketOwners::from_proc_root(root.path()).expect("the scan reads what it can");
    let owner = owners.owner(99_001).expect("the socket inode was found");
    assert_eq!(owner.pid(), 4_242);
    assert_eq!(owner.name(), Some("ono-under-test"));
    assert!(
        owners.owner(99_002).is_none(),
        "an inode nobody holds has no owner, and that is not an error"
    );
}
