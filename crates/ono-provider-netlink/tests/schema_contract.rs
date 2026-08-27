//! The schemas this crate advertises are the contract in `docs/spec/schemas/*.v1.yaml`, and
//! every record it emits satisfies the one it claims (spec §35.3, ADR-0012).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "this file is only ever linked into tests, where a failed precondition should abort \
              loudly; the helpers beside the test functions state preconditions the same way"
)]

mod support;

use std::net::Ipv6Addr;

use ono_provider_api::Provider;
use ono_provider_netlink::{
    InterfaceNames, InterfaceProvider, NeighborProvider, RouteProvider, SocketProtocol,
    SocketProvider, decode_inet_sockets, decode_interfaces, decode_neighbors, decode_routes,
    decode_unix_sockets, endpoint_schema, interface_schema, neighbor_schema, route_schema,
    socket_schema,
};
use ono_value::Value;
use support::{
    attr, concat, ifaddrmsg, ifinfomsg, inet_diag_msg, message, ndmsg, rtmsg, sockid, unix_diag_msg,
};

fn names() -> InterfaceNames {
    [(1u32, "lo".to_owned()), (2u32, "eth0".to_owned())]
        .into_iter()
        .collect()
}

#[test]
fn should_declare_the_identity_the_contract_names() {
    assert_eq!(
        interface_schema()
            .identity()
            .iter()
            .map(|f| &**f)
            .collect::<Vec<_>>(),
        ["index"],
        "an interface can be renamed while staying the same interface (spec §23.2)"
    );
    assert_eq!(
        route_schema()
            .identity()
            .iter()
            .map(|f| &**f)
            .collect::<Vec<_>>(),
        ["table", "family", "destination", "gateway", "interface"]
    );
    assert_eq!(
        neighbor_schema()
            .identity()
            .iter()
            .map(|f| &**f)
            .collect::<Vec<_>>(),
        ["address", "interface"]
    );
    assert_eq!(
        socket_schema()
            .identity()
            .iter()
            .map(|f| &**f)
            .collect::<Vec<_>>(),
        ["inode"]
    );
    assert!(
        endpoint_schema().identity().is_empty(),
        "an endpoint is a structural sub-record, not an addressable object"
    );
}

#[test]
fn should_declare_every_field_the_contract_names() {
    let interface_definition = interface_schema();
    let interface: Vec<&str> = interface_definition
        .fields()
        .iter()
        .map(ono_value::FieldDef::name)
        .collect();
    assert_eq!(
        interface,
        [
            "name",
            "index",
            "mac",
            "state",
            "mtu",
            "addresses",
            "rx_bytes",
            "tx_bytes"
        ]
    );

    let socket_definition = socket_schema();
    let socket: Vec<&str> = socket_definition
        .fields()
        .iter()
        .map(ono_value::FieldDef::name)
        .collect();
    assert_eq!(
        socket,
        [
            "protocol", "family", "local", "remote", "state", "process", "user", "inode"
        ]
    );

    let endpoint_definition = endpoint_schema();
    let endpoint: Vec<&str> = endpoint_definition
        .fields()
        .iter()
        .map(ono_value::FieldDef::name)
        .collect();
    assert_eq!(endpoint, ["address", "port", "path", "host"]);
}

#[test]
fn should_document_the_option_that_fills_the_owning_process() {
    let definition = socket_schema();
    let process = definition
        .field("process")
        .expect("the socket schema declares an owner");
    let doc = process.doc().expect("every declared field is documented");
    assert!(
        doc.contains("--process"),
        "a field that is null unless an option is given must say which option, got `{doc}`"
    );
}

#[test]
fn should_advertise_exactly_the_schemas_it_emits() {
    let advertised: Vec<String> = [
        InterfaceProvider::new().schemas(),
        RouteProvider::new().schemas(),
        NeighborProvider::new().schemas(),
        SocketProvider::new().schemas(),
    ]
    .concat()
    .iter()
    .map(|schema| schema.id().to_string())
    .collect();

    assert!(advertised.contains(&"ono.interface/1".to_owned()));
    assert!(advertised.contains(&"ono.route/1".to_owned()));
    assert!(advertised.contains(&"ono.neighbor/1".to_owned()));
    assert!(advertised.contains(&"ono.socket/1".to_owned()));
    assert!(
        advertised.contains(&"ono.endpoint/1".to_owned()),
        "a nested record type is part of the contract a consumer must be able to read"
    );
}

#[test]
fn should_emit_only_records_that_validate_against_the_schema_they_claim() {
    let links = concat(&[
        message(
            16,
            &concat(&[
                ifinfomsg(1, 772, 0x1 | 0x40),
                attr(3, b"lo\0"),
                attr(4, &65_536u32.to_ne_bytes()),
                attr(16, &[6]),
            ]),
        ),
        message(
            16,
            &concat(&[
                ifinfomsg(2, 1, 0x1 | 0x40),
                attr(3, b"eth0\0"),
                attr(4, &1_500u32.to_ne_bytes()),
                attr(16, &[2]),
                attr(1, &[1, 2, 3, 4, 5, 6]),
            ]),
        ),
    ]);
    let addresses = message(20, &concat(&[ifaddrmsg(2, 8, 1), attr(2, &[127, 0, 0, 1])]));
    let routes = concat(&[
        message(
            24,
            &concat(&[rtmsg(2, 0, 254, 16, 0, 1, 0), attr(5, &[10, 0, 0, 1])]),
        ),
        message(
            24,
            &concat(&[
                rtmsg(10, 128, 255, 2, 254, 2, 0),
                attr(1, &Ipv6Addr::LOCALHOST.octets()),
            ]),
        ),
    ]);
    let neighbors = message(
        28,
        &concat(&[
            ndmsg(2, 2, 0x02, 0, 1),
            attr(1, &[10, 0, 0, 1]),
            attr(2, &[1, 2, 3, 4, 5, 6]),
        ]),
    );
    let tcp = message(
        20,
        &inet_diag_msg(
            2,
            1,
            &sockid((&[10, 0, 0, 2], 51_000), (&[10, 0, 0, 1], 443), 7),
            0,
            0,
            1_000,
            9_001,
        ),
    );
    let unix = message(
        20,
        &concat(&[unix_diag_msg(1, 10, 5_555, 3), attr(0, b"/run/ono\0")]),
    );

    let batches = [
        decode_interfaces(&links, &addresses),
        decode_routes(&routes, &names()),
        decode_neighbors(&neighbors, &names()),
        decode_inet_sockets(&tcp, SocketProtocol::Tcp, None),
        decode_unix_sockets(&unix, None),
    ];

    let mut seen = 0;
    for decoded in &batches {
        assert!(decoded.errors().is_empty(), "{:?}", decoded.errors());
        for record in decoded.records() {
            record.validate().unwrap_or_else(|error| {
                panic!("{} violates its own schema: {error}", record.schema_id())
            });
            seen += 1;

            // A nested endpoint is a record in its own right and answers to its own schema.
            for side in ["local", "remote"] {
                if let Some(Value::Record(endpoint)) = record.get(side) {
                    endpoint.validate().unwrap_or_else(|error| {
                        panic!("an endpoint violates ono.endpoint/1: {error}")
                    });
                }
            }
        }
    }
    assert_eq!(seen, 7, "every fixture above must have produced a record");
}
