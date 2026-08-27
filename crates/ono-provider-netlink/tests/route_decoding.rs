//! What `RTM_GETROUTE` bytes turn into (`ono.route/1`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "this file is only ever linked into tests, where a failed precondition should abort \
              loudly; the helpers beside the test functions state preconditions the same way"
)]

mod support;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ono_provider_netlink::{InterfaceNames, decode_routes};
use ono_value::{IpNetwork, RecordValue, Value};
use support::{attr, concat, message, rtmsg};

fn names() -> InterfaceNames {
    [(1u32, "lo".to_owned()), (2u32, "eth0".to_owned())]
        .into_iter()
        .collect()
}

/// `default via 192.168.1.1 dev eth0 proto dhcp metric 100`.
fn default_route() -> Vec<u8> {
    message(
        24,
        &concat(&[
            rtmsg(2, 0, 254, 16, 0, 1, 0),
            attr(5, &[192, 168, 1, 1]),
            attr(4, &2u32.to_ne_bytes()),
            attr(6, &100u32.to_ne_bytes()),
        ]),
    )
}

/// `192.168.1.0/24 dev eth0 proto kernel scope link src 192.168.1.42`.
fn connected_route() -> Vec<u8> {
    message(
        24,
        &concat(&[
            rtmsg(2, 24, 254, 2, 253, 1, 0),
            attr(1, &[192, 168, 1, 0]),
            attr(4, &2u32.to_ne_bytes()),
            attr(7, &[192, 168, 1, 42]),
        ]),
    )
}

/// A local-table entry for `::1`, with the table carried in `RTA_TABLE` rather than `rtm_table`.
fn ipv6_local_route() -> Vec<u8> {
    message(
        24,
        &concat(&[
            rtmsg(10, 128, 0, 2, 254, 2, 0),
            attr(1, &Ipv6Addr::LOCALHOST.octets()),
            attr(4, &1u32.to_ne_bytes()),
            attr(15, &255u32.to_ne_bytes()),
        ]),
    )
}

fn only(records: &[RecordValue]) -> &RecordValue {
    assert_eq!(records.len(), 1, "expected exactly one route");
    &records[0]
}

#[test]
fn should_report_a_default_route_with_a_null_destination() {
    let decoded = decode_routes(&default_route(), &names());
    assert!(decoded.errors().is_empty(), "{:?}", decoded.errors());
    let route = only(decoded.records());

    assert_eq!(
        route.get("destination"),
        Some(&Value::Null),
        "the default route has no prefix; that is an answer, not an unknown"
    );
    assert_eq!(
        route.get("gateway"),
        Some(&Value::Ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))))
    );
    assert_eq!(route.get("interface"), Some(&Value::String("eth0".into())));
    assert_eq!(route.get("metric"), Some(&Value::Int(100)));
    assert_eq!(route.get("family"), Some(&Value::String("inet".into())));
    assert_eq!(route.get("table"), Some(&Value::String("main".into())));
    assert_eq!(route.get("protocol"), Some(&Value::String("dhcp".into())));
    assert_eq!(route.get("type"), Some(&Value::String("unicast".into())));
}

#[test]
fn should_report_a_connected_route_with_its_prefix_scope_and_source() {
    let decoded = decode_routes(&connected_route(), &names());
    let route = only(decoded.records());

    assert_eq!(
        route.get("destination"),
        Some(&Value::IpNetwork(
            IpNetwork::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 0)), 24).unwrap()
        ))
    );
    assert_eq!(
        route.get("gateway"),
        Some(&Value::Null),
        "a directly connected route has no next hop"
    );
    assert_eq!(route.get("scope"), Some(&Value::String("link".into())));
    assert_eq!(
        route.get("source"),
        Some(&Value::Ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42))))
    );
    assert_eq!(route.get("protocol"), Some(&Value::String("kernel".into())));
}

#[test]
fn should_prefer_the_table_attribute_over_the_header_field() {
    let decoded = decode_routes(&ipv6_local_route(), &names());
    let route = only(decoded.records());

    assert_eq!(route.get("table"), Some(&Value::String("local".into())));
    assert_eq!(route.get("family"), Some(&Value::String("inet6".into())));
    assert_eq!(route.get("type"), Some(&Value::String("local".into())));
    assert_eq!(route.get("interface"), Some(&Value::String("lo".into())));
}

#[test]
fn should_report_an_unnamed_interface_by_its_index() {
    let decoded = decode_routes(&default_route(), &InterfaceNames::default());
    let route = only(decoded.records());
    assert_eq!(
        route.get("interface"),
        Some(&Value::Int(2)),
        "a reference to an interface whose name is unknown still carries its identity"
    );
}

#[test]
fn should_report_an_unnamed_protocol_by_its_number() {
    let exotic = message(24, &concat(&[rtmsg(2, 0, 254, 231, 0, 1, 0)]));
    let decoded = decode_routes(&exotic, &names());
    assert_eq!(
        only(decoded.records()).get("protocol"),
        Some(&Value::String("231".into())),
        "the set of route origins is the system's registry, not Ono's"
    );
}

#[test]
fn should_report_a_route_of_an_unsupported_family_as_an_error() {
    let mpls = message(24, &concat(&[rtmsg(28, 0, 254, 2, 0, 1, 0)]));
    let decoded = decode_routes(&mpls, &names());
    assert!(decoded.records().is_empty());
    assert_eq!(
        decoded.errors().len(),
        1,
        "a route this provider cannot describe is reported, not dropped"
    );
}

#[test]
fn should_carry_provenance_naming_the_netlink_family() {
    let decoded = decode_routes(&default_route(), &names());
    let provenance = only(decoded.records()).provenance();
    assert_eq!(provenance.provider(), "linux.netlink");
    let source = provenance.source().expect("a source is always recorded");
    assert!(source.contains("RTM_GETROUTE"), "got `{source}`");
}
