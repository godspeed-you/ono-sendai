//! What this crate's decoders *do*, held against the contracts they claim.
//!
//! The shape of each contract — its fields, types, nullability, units, identity and default view
//! — and the set of schemas each provider advertises are stated by the generated suite of spec
//! §35.3 (`crates/ono-cli/tests/provider_conformance.rs`, from `docs/contracts/providers/*.yaml` and
//! `docs/contracts/schemas/*.v1.yaml`). What is left here is what a declaration cannot express: what a
//! field means, and that every record decoded from a fixed netlink byte stream satisfies the
//! contract it claims (ADR-0012).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "this file is only ever linked into tests, where a failed precondition should abort \
              loudly; the helpers beside the test functions state preconditions the same way"
)]

mod support;

use std::net::Ipv6Addr;

use ono_provider_netlink::{
    InterfaceNames, SocketProtocol, decode_inet_sockets, decode_interfaces, decode_neighbors,
    decode_routes, decode_unix_sockets, socket_schema,
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
