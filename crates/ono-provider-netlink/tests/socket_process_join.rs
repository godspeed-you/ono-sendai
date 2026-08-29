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

mod support;

use std::net::TcpListener;
use std::os::unix::fs::{PermissionsExt, symlink};

use ono_pipeline::StreamEvent;
use ono_provider_api::{Provider, Query};
use ono_provider_netlink::{SocketOwners, SocketProtocol, SocketProvider, decode_inet_sockets};
use ono_value::{RecordValue, Value};
use support::{inet_diag_msg, message, sockid};

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

/// A listening TCP socket whose inode is `4242`, so a scan that attributes nothing leaves it
/// without an owner.
fn unowned_listener() -> Vec<u8> {
    message(
        20,
        &inet_diag_msg(
            2,
            10,
            &sockid((&[0, 0, 0, 0], 22), (&[0, 0, 0, 0], 0), 0x1234_5678_9abc),
            0,
            0,
            0,
            4_242,
        ),
    )
}

#[test]
fn should_say_the_owner_is_denied_when_the_scan_was_refused_a_process() {
    // v0.4 §35.2 and AGENTS.md section 6. The kernel gave this socket an inode, so a process
    // holds it; the only join is `/proc/<pid>/fd`, and this reader was refused one of those
    // directories. The owner is therefore not absent — it is withheld, and a null that means
    // "there is none" would be the false-empty answer §42.4 forbids. The provider knows which
    // of the two happened, so the provider is what says so.
    let root = tempfile::tempdir().expect("a scratch directory");
    let readable = root.path().join("4242").join("fd");
    std::fs::create_dir_all(&readable).expect("the fixture tree is creatable");
    symlink("socket:[99001]", readable.join("3")).expect("a symlink is creatable");

    let closed = root.path().join("1234").join("fd");
    std::fs::create_dir_all(&closed).expect("the fixture tree is creatable");
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000))
        .expect("a directory this test owns can be closed to itself");

    let owners = SocketOwners::from_proc_root(root.path()).expect("the scan reads what it can");
    let decoded = decode_inet_sockets(&unowned_listener(), SocketProtocol::Tcp, Some(&owners));
    let socket = &decoded.records()[0];

    let error = match socket.get("process") {
        Some(Value::Error(error)) => error.clone(),
        other => panic!(
            "v0.4 section 35.2: an owner this reader was refused is denied, not absent; got \
             {other:?}"
        ),
    };
    assert_eq!(
        error.code().name(),
        "io.permission_denied",
        "the refusal keeps the taxonomy's own name so a spatial group can map it (v0.2 section 43)"
    );

    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o700))
        .expect("the fixture is removable again");
}

#[test]
fn should_leave_the_owner_null_when_a_complete_scan_found_nobody_holding_it() {
    // The other half of the same distinction: nothing was hidden from this scan, so the socket
    // simply has no holder among the processes that were readable. That is an ordinary answer,
    // and dressing it as a refusal would be as dishonest as the reverse.
    let root = tempfile::tempdir().expect("a scratch directory");
    let fd = root.path().join("4242").join("fd");
    std::fs::create_dir_all(&fd).expect("the fixture tree is creatable");
    symlink("socket:[99001]", fd.join("3")).expect("a symlink is creatable");

    let owners = SocketOwners::from_proc_root(root.path()).expect("the scan reads what it can");
    let decoded = decode_inet_sockets(&unowned_listener(), SocketProtocol::Tcp, Some(&owners));
    assert_eq!(
        decoded.records()[0].get("process"),
        Some(&Value::Null),
        "a complete scan that found no holder says so with null, not with a refusal"
    );
}
