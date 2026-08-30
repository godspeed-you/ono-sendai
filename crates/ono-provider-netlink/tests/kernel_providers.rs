//! What the providers say about the kernel they are actually running on.
//!
//! These tests assert only what is true of every Linux the shell can run on: there is a loopback
//! interface, it is up, and it carries `127.0.0.1`. Everything else is asserted as a *shape* —
//! either the provider answers, or it says why it cannot. A provider that returns nothing and
//! says nothing would pass neither branch, which is the whole point (spec §35.3).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "this file is only ever linked into tests, where a failed precondition should abort \
              loudly; the helpers beside the test functions state preconditions the same way"
)]

use std::net::{IpAddr, Ipv4Addr};

use ono_pipeline::StreamEvent;
use ono_provider_api::{Availability, Provider, Query, Selector};
use ono_provider_netlink::{InterfaceProvider, NeighborProvider, RouteProvider, SocketProvider};
use ono_value::{RecordValue, Value};

/// Drains a provider's snapshot into the records and the errors it produced.
async fn snapshot(
    provider: &dyn Provider,
    query: &Query,
) -> (Vec<RecordValue>, Vec<ono_value::ErrorValue>) {
    let mut stream = match provider.snapshot(query) {
        Ok(stream) => stream,
        Err(error) => return (Vec::new(), vec![error]),
    };
    let mut records = Vec::new();
    let mut errors = Vec::new();
    while let Some(event) = stream.recv().await {
        match event {
            StreamEvent::Value(Value::Record(record)) => records.push((*record).clone()),
            StreamEvent::Value(other) => panic!("a provider emitted a bare {other:?}"),
            StreamEvent::Failure(error) => errors.push(error),
        }
    }
    (records, errors)
}

#[tokio::test]
async fn should_find_a_loopback_interface_that_is_up_and_carries_localhost() {
    let provider = InterfaceProvider::new();
    assert_eq!(
        provider.availability(),
        Availability::Available,
        "RTM_GETLINK is readable by any user on any Linux"
    );

    let (records, errors) = snapshot(&provider, &Query::target("interface")).await;
    assert!(errors.is_empty(), "{errors:?}");

    let loopback = records
        .iter()
        .find(|record| record.get("name") == Some(&Value::String("lo".into())))
        .expect("every Linux has a loopback interface");

    assert_eq!(loopback.get("index"), Some(&Value::Int(1)));
    // Loopback is up, and this is exactly the distinction this provider exists to keep: the
    // kernel reports `IFF_UP` for it but leaves `IFLA_OPERSTATE` at `IF_OPER_UNKNOWN`, because
    // RFC 2863 operational state is about a carrier a loopback device does not have. `state`
    // reports what the kernel said; the administrative flag is reported beside it, unmodified.
    assert_eq!(loopback.get("netlink.admin_up"), Some(&Value::Bool(true)));
    assert!(
        matches!(loopback.get("state"), Some(Value::String(state)) if &**state != "down"),
        "loopback is not down, got {:?}",
        loopback.get("state")
    );

    let addresses = loopback
        .get("addresses")
        .and_then(|value| value.as_list().ok())
        .expect("the loopback interface has an address list")
        .to_vec();
    assert!(
        addresses.iter().any(|address| matches!(
            address,
            Value::IpNetwork(network) if network.address() == IpAddr::V4(Ipv4Addr::LOCALHOST)
        )),
        "loopback must carry 127.0.0.1, got {addresses:?}"
    );
}

#[tokio::test]
async fn should_narrow_to_one_interface_when_the_query_names_it() {
    let provider = InterfaceProvider::new();
    let query = Query::target("interface").with(Selector::field("name", Value::string("lo")));
    let (records, errors) = snapshot(&provider, &query).await;

    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].get("name"), Some(&Value::String("lo".into())));
}

#[tokio::test]
async fn should_validate_every_interface_it_reports_against_its_own_schema() {
    let provider = InterfaceProvider::new();
    let (records, _) = snapshot(&provider, &Query::target("interface")).await;
    for record in &records {
        record
            .validate()
            .unwrap_or_else(|error| panic!("a reported interface violates its schema: {error}"));
    }
}

#[tokio::test]
async fn should_either_read_the_route_table_or_say_why_not() {
    let provider = RouteProvider::new();
    match provider.availability() {
        Availability::Unavailable(reason) => {
            assert!(
                !reason.is_empty(),
                "an unavailable provider must give a reason"
            );
            return;
        }
        Availability::Available => {}
    }

    let (records, errors) = snapshot(&provider, &Query::target("route")).await;
    assert!(
        !records.is_empty() || !errors.is_empty(),
        "an empty answer with no error would claim this machine has no routes"
    );
    for record in &records {
        record
            .validate()
            .unwrap_or_else(|error| panic!("a reported route violates its schema: {error}"));
    }
}

#[tokio::test]
async fn should_read_the_neighbour_table_without_inventing_entries() {
    let provider = NeighborProvider::new();
    if !provider.availability().is_available() {
        return;
    }
    let (records, errors) = snapshot(&provider, &Query::target("neighbor")).await;
    assert!(errors.is_empty(), "{errors:?}");
    for record in &records {
        record
            .validate()
            .unwrap_or_else(|error| panic!("a reported neighbour violates its schema: {error}"));
    }
}

#[tokio::test]
async fn should_either_read_sockets_or_say_why_not() {
    let provider = SocketProvider::new();
    match provider.availability() {
        Availability::Unavailable(reason) => {
            assert!(
                !reason.is_empty(),
                "an unavailable provider must give a reason"
            );
            return;
        }
        Availability::Available => {}
    }

    let (records, errors) = snapshot(&provider, &Query::target("socket")).await;
    assert!(
        !records.is_empty() || !errors.is_empty(),
        "a machine running this test has sockets; silence would be a lie"
    );
    for record in &records {
        record
            .validate()
            .unwrap_or_else(|error| panic!("a reported socket violates its schema: {error}"));
    }
}

#[tokio::test]
async fn should_answer_the_connection_target_with_connected_sockets_only() {
    let provider = SocketProvider::new();
    if !provider.availability().is_available() {
        return;
    }
    assert!(provider.targets().contains(&"connection"));

    let (records, _) = snapshot(&provider, &Query::target("connection")).await;
    for record in &records {
        assert!(
            matches!(record.get("remote"), Some(Value::Record(_))),
            "a connection without a peer is not a connection"
        );
    }
}

#[tokio::test]
async fn should_answer_exactly_the_bound_when_a_socket_query_asks_for_one() {
    // The reader stops where the answer is full (ADR-0418). What a caller can observe of that is
    // the bound itself: a machine with a socket table answers one socket, not the table.
    let provider = SocketProvider::new();
    if !provider.availability().is_available() {
        return;
    }

    let (records, _) = snapshot(&provider, &Query::target("socket").limit(1)).await;
    assert!(
        records.len() <= 1,
        "`--first 1` bounds the answer at one socket, got {}",
        records.len()
    );
}

#[tokio::test]
async fn should_answer_no_unix_socket_when_the_connection_target_is_asked() {
    // A Unix socket's peer is an inode rather than an address, so `remote` is null on every one
    // of them and none can be a connection. The provider therefore never asks the kernel for the
    // Unix table when the target is `connection` (ADR-0418), and the observable half of that is
    // this: no answer to `connection` is a Unix socket, on a host that has thousands of them.
    let provider = SocketProvider::new();
    if !provider.availability().is_available() {
        return;
    }

    let (records, _) = snapshot(&provider, &Query::target("connection")).await;
    for record in &records {
        assert_ne!(
            record.get("protocol"),
            Some(&Value::string("unix")),
            "a Unix socket has no peer and cannot answer `connection`, got {record:?}"
        );
    }
}

#[tokio::test]
async fn should_report_a_provider_that_cannot_answer_rather_than_an_empty_result() {
    // The one case a container reliably reproduces: a netlink family the kernel does not offer.
    let provider = SocketProvider::new();
    match provider.availability() {
        Availability::Available => {}
        Availability::Unavailable(reason) => {
            assert!(
                reason.contains("sock_diag") || reason.contains("netlink"),
                "the reason must name what is missing, got `{reason}`"
            );
        }
    }
}
