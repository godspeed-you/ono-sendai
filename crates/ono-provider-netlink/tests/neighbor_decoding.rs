//! What `RTM_GETNEIGH` bytes turn into (`ono.neighbor/1`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "this file is only ever linked into tests, where a failed precondition should abort \
              loudly; the helpers beside the test functions state preconditions the same way"
)]

mod support;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ono_provider_netlink::{InterfaceNames, decode_neighbors};
use ono_value::Value;
use support::{attr, concat, message, ndmsg};

fn names() -> InterfaceNames {
    [(2u32, "eth0".to_owned())].into_iter().collect()
}

/// A reachable IPv4 neighbour with a resolved link-layer address.
fn reachable() -> Vec<u8> {
    message(
        28,
        &concat(&[
            ndmsg(2, 2, 0x02, 0, 1),
            attr(1, &[192, 168, 1, 1]),
            attr(2, &[0x52, 0x54, 0x00, 0xaa, 0xbb, 0xcc]),
        ]),
    )
}

/// An IPv4 neighbour the kernel could not resolve.
fn incomplete() -> Vec<u8> {
    message(
        28,
        &concat(&[ndmsg(2, 2, 0x01, 0, 1), attr(1, &[192, 168, 1, 99])]),
    )
}

/// An IPv6 neighbour advertising itself as a router.
fn ipv6_router() -> Vec<u8> {
    message(
        28,
        &concat(&[
            ndmsg(10, 2, 0x04, 0x80, 1),
            attr(1, &Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1).octets()),
            attr(2, &[0x52, 0x54, 0x00, 0x11, 0x22, 0x33]),
        ]),
    )
}

#[test]
fn should_report_a_resolved_neighbour_field_by_field() {
    let decoded = decode_neighbors(&reachable(), &names());
    assert!(decoded.errors().is_empty(), "{:?}", decoded.errors());
    let neighbor = &decoded.records()[0];

    assert_eq!(
        neighbor.get("address"),
        Some(&Value::Ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))))
    );
    assert_eq!(
        neighbor.get("mac"),
        Some(&Value::String("52:54:00:aa:bb:cc".into()))
    );
    assert_eq!(
        neighbor.get("interface"),
        Some(&Value::String("eth0".into()))
    );
    assert_eq!(neighbor.get("family"), Some(&Value::String("inet".into())));
    assert_eq!(
        neighbor.get("state"),
        Some(&Value::String("reachable".into()))
    );
    assert_eq!(
        neighbor.get("router"),
        Some(&Value::Null),
        "the router flag is an NDP concept; outside NDP it is unknown, not false"
    );
    assert_eq!(
        neighbor.get("updated"),
        Some(&Value::Null),
        "this provider keeps no confirmation timestamp and does not invent one"
    );
}

#[test]
fn should_report_an_unresolved_neighbour_with_a_null_hardware_address() {
    let decoded = decode_neighbors(&incomplete(), &names());
    let neighbor = &decoded.records()[0];
    assert_eq!(neighbor.get("mac"), Some(&Value::Null));
    assert_eq!(
        neighbor.get("state"),
        Some(&Value::String("incomplete".into()))
    );
}

#[test]
fn should_report_the_router_flag_for_ndp_entries() {
    let decoded = decode_neighbors(&ipv6_router(), &names());
    let neighbor = &decoded.records()[0];
    assert_eq!(neighbor.get("family"), Some(&Value::String("inet6".into())));
    assert_eq!(neighbor.get("router"), Some(&Value::Bool(true)));
    assert_eq!(neighbor.get("state"), Some(&Value::String("stale".into())));
}

#[test]
fn should_report_a_neighbour_without_an_address_as_a_failed_field() {
    let addressless = message(28, &concat(&[ndmsg(2, 2, 0x02, 0, 1)]));
    let decoded = decode_neighbors(&addressless, &names());
    assert_eq!(decoded.records().len(), 1);
    assert!(decoded.records()[0].access("address").is_failed());
}

#[test]
fn should_carry_provenance_naming_the_netlink_family() {
    let decoded = decode_neighbors(&reachable(), &names());
    let source = decoded.records()[0]
        .provenance()
        .source()
        .expect("a source is always recorded")
        .to_owned();
    assert!(source.contains("RTM_GETNEIGH"), "got `{source}`");
}
