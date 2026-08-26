//! What `RTM_GETLINK` and `RTM_GETADDR` bytes turn into (spec §23.2, §28.5, `ono.interface/1`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "this file is only ever linked into tests, where a failed precondition should abort \
              loudly; the helpers beside the test functions state preconditions the same way"
)]

mod support;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ono_provider_netlink::decode_interfaces;
use ono_value::{FieldAccess, IpNetwork, Value};
use support::{
    attr, concat, ifaddrmsg, ifinfomsg, link_stats64, message, nested, sockid, unix_diag_msg,
};

/// `lo`: up, no hardware address, index 1.
fn loopback_link() -> Vec<u8> {
    message(
        16,
        &concat(&[
            ifinfomsg(1, 772, 0x1 | 0x40 | 0x1_0000),
            attr(3, b"lo\0"),
            attr(4, &65536u32.to_ne_bytes()),
            attr(16, &[6]),
            attr(1, &[0, 0, 0, 0, 0, 0]),
            attr(23, &link_stats64([12, 34, 5_000, 7_000, 0, 0, 1, 2])),
        ]),
    )
}

/// `eth0`: up, a MAC, a driver kind, index 2.
fn ethernet_link() -> Vec<u8> {
    message(
        16,
        &concat(&[
            ifinfomsg(2, 1, 0x1 | 0x2 | 0x40 | 0x1000 | 0x1_0000),
            attr(3, b"eth0\0"),
            attr(4, &1500u32.to_ne_bytes()),
            attr(16, &[6]),
            attr(1, &[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]),
            nested(18, &[attr(1, b"veth\0")]),
        ]),
    )
}

/// A link that is administratively up but whose carrier is down.
fn dormant_link() -> Vec<u8> {
    message(
        16,
        &concat(&[
            ifinfomsg(3, 1, 0x1),
            attr(3, b"wlan0\0"),
            attr(4, &1500u32.to_ne_bytes()),
            attr(16, &[5]),
        ]),
    )
}

fn loopback_addresses() -> Vec<u8> {
    concat(&[
        message(
            20,
            &concat(&[
                ifaddrmsg(2, 8, 1),
                attr(2, &[127, 0, 0, 1]),
                attr(1, &[127, 0, 0, 1]),
            ]),
        ),
        message(
            20,
            &concat(&[
                ifaddrmsg(10, 128, 1),
                attr(1, &Ipv6Addr::LOCALHOST.octets()),
            ]),
        ),
    ])
}

fn field(records: &[ono_value::RecordValue], name: &str, field: &str) -> Value {
    let record = records
        .iter()
        .find(|record| record.get("name") == Some(&Value::String(name.into())))
        .unwrap_or_else(|| panic!("no interface named {name} was decoded"));
    record.get(field).cloned().unwrap_or(Value::Null)
}

#[test]
fn should_report_every_declared_field_when_the_kernel_supplies_it() {
    let decoded = decode_interfaces(&ethernet_link(), &[]);
    assert!(decoded.errors().is_empty(), "{:?}", decoded.errors());
    let records = decoded.records();
    assert_eq!(records.len(), 1);

    assert_eq!(field(records, "eth0", "index"), Value::Int(2));
    assert_eq!(
        field(records, "eth0", "mac"),
        Value::String("52:54:00:12:34:56".into())
    );
    assert_eq!(field(records, "eth0", "state"), Value::String("up".into()));
    assert_eq!(field(records, "eth0", "mtu"), Value::Int(1500));
    assert_eq!(field(records, "eth0", "addresses"), Value::list([]));
}

#[test]
fn should_report_a_hardware_address_of_all_zeroes_as_null() {
    let decoded = decode_interfaces(&loopback_link(), &[]);
    assert_eq!(field(decoded.records(), "lo", "mac"), Value::Null);
}

#[test]
fn should_attach_addresses_to_the_interface_that_owns_them() {
    let decoded = decode_interfaces(
        &concat(&[loopback_link(), ethernet_link()]),
        &loopback_addresses(),
    );
    assert!(decoded.errors().is_empty(), "{:?}", decoded.errors());

    let addresses = field(decoded.records(), "lo", "addresses");
    assert_eq!(
        addresses,
        Value::list([
            Value::IpNetwork(IpNetwork::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8).unwrap()),
            Value::IpNetwork(IpNetwork::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 128).unwrap()),
        ])
    );
    assert_eq!(
        field(decoded.records(), "eth0", "addresses"),
        Value::list([]),
        "an interface with no addresses has an empty list, not an unknown one"
    );
}

#[test]
fn should_report_the_operational_state_rather_than_the_administrative_flag() {
    let decoded = decode_interfaces(&dormant_link(), &[]);
    assert_eq!(
        field(decoded.records(), "wlan0", "state"),
        Value::String("dormant".into())
    );
    let record = &decoded.records()[0];
    assert_eq!(
        record.get("netlink.admin_up"),
        Some(&Value::Bool(true)),
        "the administrative flag stays available as a provider extension"
    );
}

#[test]
fn should_report_counters_as_byte_sizes_and_null_when_the_kernel_sends_none() {
    let decoded = decode_interfaces(&concat(&[loopback_link(), ethernet_link()]), &[]);

    assert_eq!(
        field(decoded.records(), "lo", "rx_bytes"),
        Value::ByteSize(ono_value::ByteSize::from_bytes(5_000))
    );
    assert_eq!(
        field(decoded.records(), "lo", "tx_bytes"),
        Value::ByteSize(ono_value::ByteSize::from_bytes(7_000))
    );
    assert_eq!(
        field(decoded.records(), "eth0", "rx_bytes"),
        Value::Null,
        "an interface the kernel sends no counters for reports null, never zero"
    );
}

#[test]
fn should_report_the_driver_kind_as_null_when_the_interface_has_none() {
    let decoded = decode_interfaces(&concat(&[loopback_link(), ethernet_link()]), &[]);
    let ethernet = decoded
        .records()
        .iter()
        .find(|record| record.get("name") == Some(&Value::String("eth0".into())))
        .expect("eth0 was decoded");
    assert_eq!(
        ethernet.get("netlink.kind"),
        Some(&Value::String("veth".into()))
    );

    let loopback = decoded
        .records()
        .iter()
        .find(|record| record.get("name") == Some(&Value::String("lo".into())))
        .expect("lo was decoded");
    assert_eq!(loopback.get("netlink.kind"), Some(&Value::Null));
}

#[test]
fn should_name_the_flags_the_kernel_set() {
    let decoded = decode_interfaces(&loopback_link(), &[]);
    let flags = decoded.records()[0]
        .get("netlink.flags")
        .cloned()
        .expect("the flags extension is always present");
    let names: Vec<String> = flags
        .as_list()
        .expect("flags are a list")
        .iter()
        .map(|value| value.as_str().expect("a flag name").to_owned())
        .collect();
    assert_eq!(names, ["up", "running", "lower-up"]);
}

#[test]
fn should_carry_provenance_naming_the_provider_and_the_netlink_family() {
    let decoded = decode_interfaces(&loopback_link(), &loopback_addresses());
    let provenance = decoded.records()[0].provenance();
    assert_eq!(provenance.provider(), "linux.netlink");
    let source = provenance.source().expect("a source is always recorded");
    assert!(
        source.contains("NETLINK_ROUTE") && source.contains("RTM_GETLINK"),
        "provenance must name the netlink family it came from, got `{source}`"
    );
}

#[test]
fn should_report_a_missing_name_as_a_failed_field_rather_than_dropping_the_interface() {
    let nameless = message(
        16,
        &concat(&[
            ifinfomsg(9, 1, 0x1),
            attr(4, &1500u32.to_ne_bytes()),
            attr(16, &[6]),
        ]),
    );
    let decoded = decode_interfaces(&nameless, &[]);
    assert_eq!(decoded.records().len(), 1);
    assert!(
        decoded.records()[0].access("name").is_failed(),
        "an attribute the kernel did not send is unreadable, not absent and never invented"
    );
}

#[test]
fn should_ignore_messages_that_belong_to_another_family() {
    let decoded = decode_interfaces(&message(20, &unix_diag_msg(1, 10, 5, 0)), &[]);
    assert!(decoded.records().is_empty());
    assert!(
        !decoded.errors().is_empty(),
        "a message that is not a link must be reported, never silently dropped"
    );
    let _ = sockid((&[127, 0, 0, 1], 22), (&[0, 0, 0, 0], 0), 0);
}

#[test]
fn should_leave_every_unset_field_unknown_rather_than_absent() {
    let decoded = decode_interfaces(&dormant_link(), &[]);
    let record = &decoded.records()[0];
    assert_eq!(record.access("rx_bytes"), FieldAccess::Unknown);
    assert_eq!(record.access("nowhere"), FieldAccess::Absent);
}
