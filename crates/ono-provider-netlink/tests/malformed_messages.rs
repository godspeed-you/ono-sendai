//! Every decoder in this crate reads bytes the kernel handed it across a socket, which spec
//! §35.6 and ADR-0015 T7 treat as untrusted input: bounded lengths, no unchecked indexing, no
//! panic. These tests hold that line for each decoder in turn.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "this file is only ever linked into tests, where a failed precondition should abort \
              loudly; the helpers beside the test functions state preconditions the same way"
)]

mod support;

use ono_provider_netlink::{
    Decoded, InterfaceNames, SocketProtocol, decode_inet_sockets, decode_interfaces,
    decode_neighbors, decode_routes, decode_unix_sockets,
};
use ono_testkit::Rng;
use support::{
    attr, attr_claiming, concat, ifaddrmsg, ifinfomsg, inet_diag_msg, message, message_claiming,
    ndmsg, rtmsg, sockid, unix_diag_msg,
};

/// Every decoder, behind one signature, so a hostile input can be pushed through all of them.
fn decoders() -> Vec<(&'static str, fn(&[u8]) -> Decoded)> {
    vec![
        ("interface", |bytes| decode_interfaces(bytes, bytes)),
        ("route", |bytes| {
            decode_routes(bytes, &InterfaceNames::default())
        }),
        ("neighbor", |bytes| {
            decode_neighbors(bytes, &InterfaceNames::default())
        }),
        ("tcp", |bytes| {
            decode_inet_sockets(bytes, SocketProtocol::Tcp, None)
        }),
        ("unix", |bytes| decode_unix_sockets(bytes, None)),
    ]
}

/// One well-formed message of every kind, to be truncated and corrupted.
fn valid_messages() -> Vec<Vec<u8>> {
    vec![
        message(
            16,
            &concat(&[
                ifinfomsg(2, 1, 0x1 | 0x40),
                attr(3, b"eth0\0"),
                attr(4, &1500u32.to_ne_bytes()),
                attr(16, &[6]),
                attr(1, &[1, 2, 3, 4, 5, 6]),
            ]),
        ),
        message(20, &concat(&[ifaddrmsg(2, 24, 2), attr(2, &[10, 0, 0, 1])])),
        message(
            24,
            &concat(&[
                rtmsg(2, 24, 254, 2, 253, 1, 0),
                attr(1, &[10, 0, 0, 0]),
                attr(4, &2u32.to_ne_bytes()),
            ]),
        ),
        message(
            28,
            &concat(&[
                ndmsg(2, 2, 0x02, 0, 1),
                attr(1, &[10, 0, 0, 1]),
                attr(2, &[1, 2, 3, 4, 5, 6]),
            ]),
        ),
        message(
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
        ),
        message(
            20,
            &concat(&[unix_diag_msg(1, 10, 5_555, 3), attr(0, b"/run/ono\0")]),
        ),
    ]
}

#[test]
fn should_survive_every_truncation_of_a_valid_message() {
    for valid in valid_messages() {
        for length in 0..valid.len() {
            let truncated = &valid[..length];
            for (name, decode) in decoders() {
                let decoded = decode(truncated);
                assert!(
                    decoded.records().len() + decoded.errors().len() < 1_000,
                    "{name} invented output for a {length}-byte prefix"
                );
            }
        }
    }
}

#[test]
fn should_report_a_message_whose_header_claims_more_than_it_carries() {
    let overlong = message_claiming(16, 4_096, &ifinfomsg(2, 1, 0x1));
    for (name, decode) in decoders() {
        let decoded = decode(&overlong);
        assert!(
            decoded.records().is_empty(),
            "{name} decoded a record out of a message that ended early"
        );
        assert!(
            !decoded.errors().is_empty(),
            "{name} swallowed a message whose length field lies"
        );
    }
}

#[test]
fn should_report_a_message_whose_header_claims_less_than_a_header() {
    let stunted = message_claiming(16, 8, &ifinfomsg(2, 1, 0x1));
    for (name, decode) in decoders() {
        let decoded = decode(&stunted);
        assert!(
            !decoded.errors().is_empty(),
            "{name} accepted a message shorter than its own header"
        );
    }
}

#[test]
fn should_ignore_an_attribute_that_claims_more_than_the_message_carries() {
    let overlong_attribute = message(
        16,
        &concat(&[
            ifinfomsg(2, 1, 0x1),
            attr(3, b"eth0\0"),
            attr_claiming(4, 4_096, &1500u32.to_ne_bytes()),
        ]),
    );
    let decoded = decode_interfaces(&overlong_attribute, &[]);
    assert_eq!(decoded.records().len(), 1);
    assert!(
        decoded.records()[0].access("mtu").is_failed(),
        "an attribute that cannot be read is a failed field, not a fabricated one"
    );
}

#[test]
fn should_not_loop_on_an_attribute_of_zero_length() {
    let zero_length = message(
        16,
        &concat(&[ifinfomsg(2, 1, 0x1), attr_claiming(3, 0, b"eth0\0\0\0\0")]),
    );
    let decoded = decode_interfaces(&zero_length, &[]);
    assert_eq!(decoded.records().len(), 1);
}

#[test]
fn should_not_loop_on_a_message_of_zero_length() {
    let zero_length = message_claiming(16, 0, &ifinfomsg(2, 1, 0x1));
    for (name, decode) in decoders() {
        let decoded = decode(&zero_length);
        assert!(
            decoded.records().len() < 1_000,
            "{name} did not stop on a zero-length message"
        );
    }
}

#[test]
fn should_survive_mutated_kernel_messages() {
    // A fixed seed: spec §35.6 asks for fuzzing, AGENTS.md §11 asks for determinism, and a
    // failure is reproduced by re-running the same seed.
    let mut rng = Rng::seeded(0x0050_5e4d);
    let corpus = valid_messages();

    for round in 0..2_000 {
        let mut bytes = corpus
            .get(rng.below(corpus.len()))
            .cloned()
            .unwrap_or_default();
        if rng.chance(3) {
            bytes.extend_from_slice(
                &corpus
                    .get(rng.below(corpus.len()))
                    .cloned()
                    .unwrap_or_default(),
            );
        }
        for _ in 0..1 + rng.below(8) {
            if bytes.is_empty() {
                break;
            }
            let index = rng.below(bytes.len());
            let byte = u8::try_from(rng.next_u64() & 0xff).unwrap_or(0);
            if let Some(slot) = bytes.get_mut(index) {
                *slot = byte;
            }
        }
        if rng.chance(4) && !bytes.is_empty() {
            bytes.truncate(rng.below(bytes.len()));
        }

        for (name, decode) in decoders() {
            let decoded = decode(&bytes);
            assert!(
                decoded.records().len() + decoded.errors().len() < 10_000,
                "{name} produced an implausible amount of output in round {round}"
            );
            for record in decoded.records() {
                // Reading every field is what would touch an index the decoder got wrong.
                for name in ["name", "index", "address", "inode", "local", "destination"] {
                    let _ = record.access(name);
                }
            }
        }
    }
}

#[test]
fn should_survive_arbitrary_bytes() {
    let mut rng = Rng::seeded(0xdead_beef);
    for _ in 0..2_000 {
        let length = rng.below(256);
        let bytes: Vec<u8> = (0..length)
            .map(|_| u8::try_from(rng.next_u64() & 0xff).unwrap_or(0))
            .collect();
        for (name, decode) in decoders() {
            let decoded = decode(&bytes);
            assert!(
                decoded.records().len() + decoded.errors().len() < 10_000,
                "{name} produced an implausible amount of output"
            );
        }
    }
}
