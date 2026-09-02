//! What `sock_diag` bytes turn into (spec §23.2, §28.4, `ono.socket/1`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "this file is only ever linked into tests, where a failed precondition should abort \
              loudly; the helpers beside the test functions state preconditions the same way"
)]

mod support;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use ono_provider_netlink::{SocketProtocol, decode_inet_sockets, decode_unix_sockets};
use ono_value::{RecordValue, Value};
use support::{attr, concat, inet_diag_msg, listening_tcp, message, sockid, unix_diag_msg};

/// An established TCP connection from `10.0.0.2:51000` to `10.0.0.1:443`.
fn established_tcp() -> Vec<u8> {
    message(
        20,
        &inet_diag_msg(
            2,
            1,
            &sockid((&[10, 0, 0, 2], 51_000), (&[10, 0, 0, 1], 443), 7),
            128,
            64,
            1_000,
            9_001,
        ),
    )
}

/// An unconnected IPv6 UDP socket.
fn ipv6_udp() -> Vec<u8> {
    message(
        20,
        &inet_diag_msg(
            10,
            7,
            &sockid(
                (&Ipv6Addr::UNSPECIFIED.octets(), 5_353),
                (&Ipv6Addr::UNSPECIFIED.octets(), 0),
                11,
            ),
            0,
            0,
            65_534,
            9_002,
        ),
    )
}

/// A listening Unix socket bound to a filesystem path, and one bound to an abstract name.
fn unix_sockets() -> Vec<u8> {
    concat(&[
        message(
            20,
            &concat(&[
                unix_diag_msg(1, 10, 5_555, 3),
                attr(0, b"/run/ono/control\0"),
            ]),
        ),
        message(
            20,
            &concat(&[
                unix_diag_msg(2, 1, 5_556, 4),
                attr(0, b"\0abstract-name"),
                attr(2, &7_777u32.to_ne_bytes()),
            ]),
        ),
    ])
}

fn endpoint(record: &RecordValue, side: &str) -> Option<RecordValue> {
    match record.get(side) {
        Some(Value::Record(endpoint)) => Some((**endpoint).clone()),
        _ => None,
    }
}

#[test]
fn should_report_a_listening_socket_with_no_peer() {
    let decoded = decode_inet_sockets(&listening_tcp(), SocketProtocol::Tcp, None);
    assert!(decoded.errors().is_empty(), "{:?}", decoded.errors());
    let socket = &decoded.records()[0];

    assert_eq!(socket.get("protocol"), Some(&Value::String("tcp".into())));
    assert_eq!(socket.get("family"), Some(&Value::String("inet".into())));
    assert_eq!(socket.get("state"), Some(&Value::String("listen".into())));
    assert_eq!(socket.get("inode"), Some(&Value::Int(4_242)));
    assert_eq!(socket.get("user"), Some(&Value::Int(0)));
    assert_eq!(
        socket.get("remote"),
        Some(&Value::Null),
        "a listening socket has no peer; that is an answer, not an unknown"
    );

    let local = endpoint(socket, "local").expect("a listening socket has a local endpoint");
    assert_eq!(
        local.get("address"),
        Some(&Value::Ip(IpAddr::V4(Ipv4Addr::UNSPECIFIED)))
    );
    assert_eq!(local.get("port"), Some(&Value::Port(22)));
    assert_eq!(local.get("path"), Some(&Value::Null));
    assert_eq!(
        local.get("host"),
        Some(&Value::Null),
        "a host name is derived data and is never substituted for an address"
    );
}

#[test]
fn should_report_both_endpoints_of_an_established_connection() {
    let decoded = decode_inet_sockets(&established_tcp(), SocketProtocol::Tcp, None);
    let socket = &decoded.records()[0];

    assert_eq!(
        socket.get("state"),
        Some(&Value::String("established".into()))
    );
    assert_eq!(socket.get("user"), Some(&Value::Int(1_000)));

    let local = endpoint(socket, "local").expect("a local endpoint");
    let remote = endpoint(socket, "remote").expect("a remote endpoint");
    assert_eq!(
        local.get("address"),
        Some(&Value::Ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))))
    );
    assert_eq!(local.get("port"), Some(&Value::Port(51_000)));
    assert_eq!(
        remote.get("address"),
        Some(&Value::Ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))))
    );
    assert_eq!(remote.get("port"), Some(&Value::Port(443)));

    assert_eq!(socket.get("netlink.rx_queue"), Some(&Value::Int(128)));
    assert_eq!(socket.get("netlink.tx_queue"), Some(&Value::Int(64)));
    assert_eq!(socket.get("netlink.cookie"), Some(&Value::Int(7)));
}

#[test]
fn should_report_a_udp_socket_without_a_connection_state() {
    let decoded = decode_inet_sockets(&ipv6_udp(), SocketProtocol::Udp, None);
    let socket = &decoded.records()[0];

    assert_eq!(socket.get("protocol"), Some(&Value::String("udp".into())));
    assert_eq!(socket.get("family"), Some(&Value::String("inet6".into())));
    assert_eq!(
        socket.get("state"),
        Some(&Value::Null),
        "UDP has no connection state, and the kernel's reuse of the TCP constants is not one"
    );

    let local = endpoint(socket, "local").expect("a local endpoint");
    assert_eq!(
        local.get("address"),
        Some(&Value::Ip(IpAddr::V6(Ipv6Addr::UNSPECIFIED)))
    );
    assert_eq!(local.get("port"), Some(&Value::Port(5_353)));
}

#[test]
fn should_report_a_unix_socket_by_its_path() {
    let decoded = decode_unix_sockets(&unix_sockets(), None);
    assert!(decoded.errors().is_empty(), "{:?}", decoded.errors());
    let socket = &decoded.records()[0];

    assert_eq!(socket.get("protocol"), Some(&Value::String("unix".into())));
    assert_eq!(socket.get("family"), Some(&Value::String("unix".into())));
    assert_eq!(socket.get("state"), Some(&Value::String("listen".into())));
    assert_eq!(socket.get("inode"), Some(&Value::Int(5_555)));
    assert_eq!(
        socket.get("netlink.socket_type"),
        Some(&Value::String("stream".into()))
    );

    let local = endpoint(socket, "local").expect("a bound unix socket has a local endpoint");
    assert_eq!(
        local.get("path"),
        Some(&Value::Path(Path::new("/run/ono/control").into()))
    );
    assert_eq!(
        local.get("address"),
        Some(&Value::Null),
        "a unix socket has no IP address, and null is how that is said"
    );
    assert_eq!(local.get("port"), Some(&Value::Null));
}

#[test]
fn should_report_an_abstract_unix_socket_with_its_leading_marker() {
    let decoded = decode_unix_sockets(&unix_sockets(), None);
    let socket = &decoded.records()[1];
    let local = endpoint(socket, "local").expect("an abstract socket still has a name");

    assert_eq!(
        local.get("path"),
        Some(&Value::Path(Path::new("@abstract-name").into()))
    );
    assert_eq!(
        socket.get("netlink.socket_type"),
        Some(&Value::String("dgram".into()))
    );
    assert_eq!(socket.get("netlink.peer_inode"), Some(&Value::Int(7_777)));
}

#[test]
fn should_report_an_unbound_unix_socket_with_no_endpoint() {
    let unnamed = message(20, &unix_diag_msg(1, 7, 5_557, 9));
    let decoded = decode_unix_sockets(&unnamed, None);
    let socket = &decoded.records()[0];
    assert_eq!(socket.get("local"), Some(&Value::Null));
    assert_eq!(socket.get("remote"), Some(&Value::Null));
}

#[test]
fn should_leave_the_owning_process_null_when_no_owner_was_looked_up() {
    let decoded = decode_inet_sockets(&established_tcp(), SocketProtocol::Tcp, None);
    assert_eq!(
        decoded.records()[0].get("process"),
        Some(&Value::Null),
        "without the owner scan the field is null, never an invented value"
    );
}

#[test]
fn should_carry_provenance_naming_the_netlink_family() {
    let decoded = decode_inet_sockets(&listening_tcp(), SocketProtocol::Tcp, None);
    let provenance = decoded.records()[0].provenance();
    assert_eq!(provenance.provider(), "linux.sock-diag");
    let source = provenance.source().expect("a source is always recorded");
    assert!(source.contains("NETLINK_SOCK_DIAG"), "got `{source}`");

    let unix = decode_unix_sockets(&unix_sockets(), None);
    let unix_source = unix.records()[0]
        .provenance()
        .source()
        .expect("a source is always recorded")
        .to_owned();
    assert!(unix_source.contains("AF_UNIX"), "got `{unix_source}`");
}
