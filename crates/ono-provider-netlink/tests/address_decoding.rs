//! What an `RTM_GETADDR` dump turns into when each address is an object of its own
//! (v0.3 §1.33, v0.4 §7.3, `ono.interface-address/1`, ADR-0971).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "this file is only ever linked into tests, where a failed precondition should abort \
              loudly; the helpers beside the test functions state preconditions the same way"
)]

mod support;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ono_provider_netlink::decode_addresses;
use ono_value::{IpNetwork, RecordValue, Value};
use support::{attr, concat, error, ifaddrmsg_scoped, ifinfomsg, message};

const PERMANENT: u8 = 0x80;
const SCOPE_GLOBAL: u8 = 0;
const SCOPE_LINK: u8 = 253;
const SCOPE_HOST: u8 = 254;

fn links() -> Vec<u8> {
    concat(&[
        message(16, &concat(&[ifinfomsg(1, 772, 0x1), attr(3, b"lo\0")])),
        message(16, &concat(&[ifinfomsg(2, 1, 0x1), attr(3, b"eth0\0")])),
    ])
}

fn network(address: IpAddr, prefix: u8) -> Value {
    Value::IpNetwork(IpNetwork::new(address, prefix).unwrap())
}

fn record_of<'a>(records: &'a [RecordValue], address: &Value) -> &'a RecordValue {
    records
        .iter()
        .find(|record| record.get("address") == Some(address))
        .unwrap_or_else(|| panic!("no record for {address:?} among {records:?}"))
}

/// What a host with a loopback and one DHCP-configured interface answers `RTM_GETADDR` with.
fn addresses() -> Vec<u8> {
    concat(&[
        message(
            20,
            &concat(&[
                ifaddrmsg_scoped(2, 8, PERMANENT, SCOPE_HOST, 1),
                attr(1, &[127, 0, 0, 1]),
                attr(2, &[127, 0, 0, 1]),
                attr(3, b"lo\0"),
            ]),
        ),
        message(
            20,
            &concat(&[
                ifaddrmsg_scoped(2, 24, 0, SCOPE_GLOBAL, 2),
                attr(1, &[192, 0, 2, 17]),
                attr(2, &[192, 0, 2, 17]),
                attr(3, b"eth0\0"),
                // `IFA_FLAGS`: the 32-bit flags, without `IFA_F_PERMANENT`.
                attr(8, &0u32.to_ne_bytes()),
            ]),
        ),
        message(
            20,
            &concat(&[
                ifaddrmsg_scoped(10, 64, PERMANENT, SCOPE_LINK, 2),
                attr(
                    1,
                    &Ipv6Addr::new(0xfe80, 0, 0, 0, 0x5054, 0xff, 0xfe12, 0x3456).octets(),
                ),
                attr(8, &u32::from(PERMANENT).to_ne_bytes()),
            ]),
        ),
    ])
}

#[test]
fn should_report_one_record_per_address_with_the_interface_that_carries_it() {
    let decoded = decode_addresses(&links(), &addresses());
    assert!(decoded.errors().is_empty(), "{:?}", decoded.errors());
    let records = decoded.records();
    assert_eq!(records.len(), 3, "one record per RTM_NEWADDR message");

    let leased = record_of(
        records,
        &network(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 17)), 24),
    );
    assert_eq!(leased.get("interface"), Some(&Value::String("eth0".into())));
    assert_eq!(leased.get("index"), Some(&Value::Int(2)));
    assert_eq!(leased.get("family"), Some(&Value::String("inet".into())));
    assert_eq!(leased.get("scope"), Some(&Value::String("global".into())));
    assert_eq!(leased.get("label"), Some(&Value::String("eth0".into())));
}

#[test]
fn should_name_the_scope_the_kernel_gave_each_address() {
    let decoded = decode_addresses(&links(), &addresses());
    let records = decoded.records();

    let loopback = record_of(records, &network(IpAddr::V4(Ipv4Addr::LOCALHOST), 8));
    assert_eq!(loopback.get("scope"), Some(&Value::String("host".into())));

    let link_local = record_of(
        records,
        &network(
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0x5054, 0xff, 0xfe12, 0x3456)),
            64,
        ),
    );
    assert_eq!(link_local.get("scope"), Some(&Value::String("link".into())));
    assert_eq!(
        link_local.get("family"),
        Some(&Value::String("inet6".into()))
    );
    assert_eq!(
        link_local.get("label"),
        Some(&Value::Null),
        "an IPv6 address carries no label, and none is made up from the interface name"
    );
}

#[test]
fn should_call_an_address_dynamic_when_the_kernel_does_not_mark_it_permanent() {
    let decoded = decode_addresses(&links(), &addresses());
    let records = decoded.records();

    let leased = record_of(
        records,
        &network(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 17)), 24),
    );
    assert_eq!(leased.get("dynamic"), Some(&Value::Bool(true)));

    let loopback = record_of(records, &network(IpAddr::V4(Ipv4Addr::LOCALHOST), 8));
    assert_eq!(
        loopback.get("dynamic"),
        Some(&Value::Bool(false)),
        "the header's flag byte answers where the kernel sent no IFA_FLAGS"
    );
}

#[test]
fn should_report_the_local_address_of_a_point_to_point_link_rather_than_its_peer() {
    let tunnel = message(
        20,
        &concat(&[
            ifaddrmsg_scoped(2, 32, PERMANENT, SCOPE_GLOBAL, 2),
            attr(1, &[10, 0, 0, 2]),
            attr(2, &[10, 0, 0, 1]),
        ]),
    );
    let decoded = decode_addresses(&links(), &tunnel);
    assert_eq!(
        decoded.records()[0].get("address"),
        Some(&network(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 32))
    );
}

#[test]
fn should_refer_to_an_interface_by_index_when_the_link_dump_does_not_name_it() {
    let orphan = message(
        20,
        &concat(&[
            ifaddrmsg_scoped(2, 24, PERMANENT, SCOPE_GLOBAL, 7),
            attr(2, &[198, 51, 100, 4]),
        ]),
    );
    let decoded = decode_addresses(&links(), &orphan);
    let record = &decoded.records()[0];
    assert_eq!(record.get("interface"), Some(&Value::Int(7)));
    assert_eq!(record.get("index"), Some(&Value::Int(7)));
}

#[test]
fn should_report_a_refused_dump_rather_than_an_empty_list() {
    let decoded = decode_addresses(&links(), &error(1));
    assert!(decoded.records().is_empty());
    assert_eq!(
        decoded.errors().len(),
        1,
        "the kernel's refusal travels beside the records, got {:?}",
        decoded.errors()
    );
}

#[test]
fn should_emit_only_records_that_validate_against_the_address_contract() {
    let decoded = decode_addresses(&links(), &addresses());
    for record in decoded.records() {
        record
            .validate()
            .unwrap_or_else(|error| panic!("{record:?} violates its schema: {error:?}"));
        assert_eq!(record.schema().id().to_string(), "ono.interface-address/1");
    }
}
