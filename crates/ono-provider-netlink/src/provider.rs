//! The four providers: interfaces, routes, neighbours and sockets.
//!
//! Each one does the same three things in the same order — obtain a buffer from the kernel on a
//! blocking thread, decode it with a pure function, and stream the result — so that the only
//! part that has to be tested against a live kernel is the part that cannot be tested any other
//! way.

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamSink, ValueStream};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
use ono_value::{ErrorValue, RecordValue, Schema, Value};

use crate::decoded::Decoded;
use crate::interface::{InterfaceNames, decode_interfaces};
use crate::neighbor::decode_neighbors;
use crate::owners::SocketOwners;
use crate::route::decode_routes;
use crate::schema::{
    endpoint_schema, interface_schema, neighbor_schema, route_schema, socket_schema,
};
use crate::socket::{SocketProtocol, decode_inet_sockets, decode_unix_sockets};
use crate::sys;
use crate::transport::{
    NetlinkSocket, address_request, inet_diag_request, link_request, neighbour_request,
    route_request, unix_diag_request,
};

/// Interfaces and their addresses, from `RTM_GETLINK` and `RTM_GETADDR`.
#[derive(Debug, Clone, Copy, Default)]
pub struct InterfaceProvider;

impl InterfaceProvider {
    /// A provider reading this machine's interfaces.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// Routing table entries, from `RTM_GETROUTE`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RouteProvider;

impl RouteProvider {
    /// A provider reading this machine's routing tables.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// ARP and NDP neighbours, from `RTM_GETNEIGH`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeighborProvider;

impl NeighborProvider {
    /// A provider reading this machine's neighbour tables.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// Sockets, from `sock_diag` over `NETLINK_SOCK_DIAG`.
///
/// It answers two targets. `socket` is every socket the kernel will describe; `connection` is the
/// subset that has a peer, which is the view spec §9.1 names separately.
#[derive(Debug, Clone, Copy, Default)]
pub struct SocketProvider;

impl SocketProvider {
    /// A provider reading this machine's sockets.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Provider for InterfaceProvider {
    fn id(&self) -> &str {
        crate::NETLINK_PROVIDER
    }

    fn targets(&self) -> &[&str] {
        &["interface"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![interface_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("interface.list", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        route_availability()
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        stream(query.clone(), |_| read_interfaces())
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        references(read_on_a_blocking_thread(read_interfaces).await?, selector)
    }
}

#[async_trait::async_trait]
impl Provider for RouteProvider {
    fn id(&self) -> &str {
        crate::NETLINK_PROVIDER
    }

    fn targets(&self) -> &[&str] {
        &["route"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![route_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("route.list", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        route_availability()
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        stream(query.clone(), |query| {
            read_routes(option_text(query, "family").as_deref())
        })
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        references(
            read_on_a_blocking_thread(|| read_routes(None)).await?,
            selector,
        )
    }
}

#[async_trait::async_trait]
impl Provider for NeighborProvider {
    fn id(&self) -> &str {
        crate::NETLINK_PROVIDER
    }

    fn targets(&self) -> &[&str] {
        &["neighbor"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![neighbor_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("neighbor.list", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        route_availability()
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        stream(query.clone(), |_| read_neighbors())
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        references(read_on_a_blocking_thread(read_neighbors).await?, selector)
    }
}

#[async_trait::async_trait]
impl Provider for SocketProvider {
    fn id(&self) -> &str {
        crate::SOCK_DIAG_PROVIDER
    }

    fn targets(&self) -> &[&str] {
        &["socket", "connection"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![socket_schema(), endpoint_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("socket.list", Risk::Read),
            Capability::new("connection.list", Risk::Read),
        ]
    }

    fn availability(&self) -> Availability {
        match NetlinkSocket::open_diag() {
            Ok(_) => Availability::Available,
            Err(error) => Availability::unavailable(error.message()),
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        stream(query.clone(), |query| {
            read_sockets(
                option_text(query, "protocol").as_deref(),
                query.flag("process"),
            )
        })
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        references(
            read_on_a_blocking_thread(|| read_sockets(None, false)).await?,
            selector,
        )
    }
}

/// `NETLINK_ROUTE` is readable by any user on any Linux; saying so requires opening it, because
/// the alternative — assuming — is how a provider ends up returning an empty answer in a sandbox
/// that forbids the family (spec §35.3).
fn route_availability() -> Availability {
    match NetlinkSocket::open_route() {
        Ok(_) => Availability::Available,
        Err(error) => Availability::unavailable(error.message()),
    }
}

/// Runs `read` on a blocking thread and streams what it produced.
fn stream<F>(query: Query, read: F) -> Result<ValueStream, ErrorValue>
where
    F: FnOnce(&Query) -> Result<Decoded, ErrorValue> + Send + 'static,
{
    Ok(ValueStream::spawn(
        PipelineConfig::new(),
        Boundedness::Bounded,
        move |sink| async move {
            let for_reader = query.clone();
            let outcome = tokio::task::spawn_blocking(move || read(&for_reader)).await;
            let decoded = match outcome {
                Ok(Ok(decoded)) => decoded,
                Ok(Err(error)) => {
                    let _ = sink.fail(error).await;
                    return;
                }
                Err(joined) => {
                    let _ = sink
                        .fail(ErrorValue::new(
                            ErrorCode::ProviderUnavailable,
                            format!("the netlink reader stopped before answering: {joined}"),
                        ))
                        .await;
                    return;
                }
            };
            emit(decoded, &query, &sink).await;
        },
    ))
}

/// Sends the errors first and then the objects.
///
/// Errors go first deliberately: `get socket | take 5` must not be able to hide the fact that one
/// address family could not be read (spec §16.5).
async fn emit(decoded: Decoded, query: &Query, sink: &StreamSink) {
    let (records, errors) = decoded.into_parts();
    for error in errors {
        if sink.fail(error).await.is_err() {
            return;
        }
    }
    let mut sent = 0;
    for record in records {
        if !keep(&record, query) {
            continue;
        }
        if query.max().is_some_and(|max| sent >= max) {
            return;
        }
        sent += 1;
        if sink.send(Value::Record(Arc::new(record))).await.is_err() {
            return;
        }
    }
}

/// Whether a record survives the query's selectors and options.
fn keep(record: &RecordValue, query: &Query) -> bool {
    if query.target_name() == "connection"
        && !matches!(record.get("remote"), Some(Value::Record(_)))
    {
        return false;
    }
    if query.flag("listening") && record.get("state") != Some(&Value::String("listen".into())) {
        return false;
    }
    if let Some(table) = option_text(query, "table")
        && record.get("table") != Some(&Value::String(table.as_str().into()))
    {
        return false;
    }
    if let Some(interface) = option_text(query, "interface")
        && record.get("interface") != Some(&Value::String(interface.as_str().into()))
    {
        return false;
    }
    // `trace socket --port 443` (spec §22.3) spells the port as an option; it means the same
    // as the selector below — either end of the socket.
    if let Some(Value::Port(port)) = query.option_value("port")
        && endpoint_port(record, "local") != Some(*port)
        && endpoint_port(record, "remote") != Some(*port)
    {
        return false;
    }
    query.selectors().iter().all(|selector| match selector {
        // A port is not a field of `ono.socket/1`; it lives inside either endpoint, and a user
        // asking for port 443 means "either end of this socket", which is what `trace socket
        // --port 443` asks for in spec §22.3.
        Selector::Field {
            name,
            value: Value::Port(port),
        } if name == "port" => {
            endpoint_port(record, "local") == Some(*port)
                || endpoint_port(record, "remote") == Some(*port)
        }
        other => other.matches(record),
    })
}

/// The port of one end of a socket record.
fn endpoint_port(record: &RecordValue, side: &str) -> Option<u16> {
    match record.get(side) {
        Some(Value::Record(endpoint)) => match endpoint.get("port") {
            Some(Value::Port(port)) => Some(*port),
            _ => None,
        },
        _ => None,
    }
}

/// A string-valued query option.
fn option_text(query: &Query, name: &str) -> Option<String> {
    match query.option_value(name) {
        Some(Value::String(text)) => Some(text.to_string()),
        _ => None,
    }
}

/// Runs a blocking read on the runtime's blocking pool.
async fn read_on_a_blocking_thread<F>(read: F) -> Result<Decoded, ErrorValue>
where
    F: FnOnce() -> Result<Decoded, ErrorValue> + Send + 'static,
{
    match tokio::task::spawn_blocking(read).await {
        Ok(outcome) => outcome,
        Err(joined) => Err(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!("the netlink reader stopped before answering: {joined}"),
        )),
    }
}

/// The objects a selector names, as references.
fn references(decoded: Decoded, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
    Ok(decoded
        .records()
        .iter()
        .filter(|record| selector.matches(record))
        .filter_map(ObjectRef::of)
        .collect())
}

/// One `RTM_GETLINK` and one `RTM_GETADDR` dump, decoded together.
fn read_interfaces() -> Result<Decoded, ErrorValue> {
    let socket = NetlinkSocket::open_route()?;
    let links = socket.dump(sys::RTM_GETLINK, &link_request())?;
    let addresses = socket.dump(sys::RTM_GETADDR, &address_request())?;
    Ok(decode_interfaces(&links, &addresses))
}

/// One dump per address family, named through the link dump.
fn read_routes(family: Option<&str>) -> Result<Decoded, ErrorValue> {
    let socket = NetlinkSocket::open_route()?;
    let names = InterfaceNames::from_links(&socket.dump(sys::RTM_GETLINK, &link_request())?);

    let mut decoded = Decoded::new();
    for (name, number) in [("inet", sys::AF_INET), ("inet6", sys::AF_INET6)] {
        if family.is_some_and(|wanted| wanted != name) {
            continue;
        }
        // One family failing must not hide the other: the error joins the stream beside the
        // routes that were readable.
        match socket.dump(sys::RTM_GETROUTE, &route_request(number)) {
            Ok(bytes) => decoded.absorb(decode_routes(&bytes, &names)),
            Err(error) => decoded.fail(error),
        }
    }
    Ok(decoded)
}

/// One `RTM_GETNEIGH` dump, named through the link dump.
fn read_neighbors() -> Result<Decoded, ErrorValue> {
    let socket = NetlinkSocket::open_route()?;
    let names = InterfaceNames::from_links(&socket.dump(sys::RTM_GETLINK, &link_request())?);
    let bytes = socket.dump(sys::RTM_GETNEIGH, &neighbour_request())?;
    Ok(decode_neighbors(&bytes, &names))
}

/// One dump per protocol and family, plus the Unix table.
///
/// `owners` is scanned at most once for the whole answer, never once per socket.
fn read_sockets(protocol: Option<&str>, with_owners: bool) -> Result<Decoded, ErrorValue> {
    let socket = NetlinkSocket::open_diag()?;
    let mut decoded = Decoded::new();

    let owners = if with_owners {
        match SocketOwners::from_proc() {
            Ok(owners) => Some(owners),
            Err(error) => {
                decoded.fail(error);
                None
            }
        }
    } else {
        None
    };

    for transport in [SocketProtocol::Tcp, SocketProtocol::Udp] {
        if protocol.is_some_and(|wanted| wanted != transport.as_str()) {
            continue;
        }
        for family in [sys::AF_INET, sys::AF_INET6] {
            match socket.dump(
                sys::SOCK_DIAG_BY_FAMILY,
                &inet_diag_request(family, transport.number()),
            ) {
                Ok(bytes) => {
                    decoded.absorb(decode_inet_sockets(&bytes, transport, owners.as_ref()));
                }
                Err(error) => decoded.fail(error),
            }
        }
    }

    if protocol.is_none_or(|wanted| wanted == "unix") {
        match socket.dump(sys::SOCK_DIAG_BY_FAMILY, &unix_diag_request()) {
            Ok(bytes) => decoded.absorb(decode_unix_sockets(&bytes, owners.as_ref())),
            Err(error) => decoded.fail(error),
        }
    }
    Ok(decoded)
}
