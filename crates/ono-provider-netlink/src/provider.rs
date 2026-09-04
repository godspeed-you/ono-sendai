//! The four providers: interfaces, routes, neighbours and sockets.
//!
//! Each one does the same three things in the same order — obtain a buffer from the kernel on a
//! blocking thread, decode it with a pure function, and stream the result — so that the only
//! part that has to be tested against a live kernel is the part that cannot be tested any other
//! way.

use std::sync::Arc;

use tokio::sync::mpsc;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, Diagnostics, PipelineConfig, StreamSink, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, EventStream, ObjectEvent, ObjectRef, Provider,
    Query, Risk, Selector,
};
use ono_value::{ErrorValue, RecordValue, Schema, Value};

use crate::decoded::{Decoded, Item};
use crate::interface::{InterfaceNames, decode_interfaces};
use crate::neighbor::decode_neighbors;
use crate::owners::SocketOwners;
use crate::route::decode_routes;
use crate::schema::{
    endpoint_schema, interface_schema, neighbor_schema, route_schema, socket_schema,
};
use crate::socket::{
    SocketProtocol, decode_inet_sockets, decode_unix_sockets, inet_sockets, unix_sockets,
};
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
        vec![
            Capability::new("interface.list", Risk::Read),
            Capability::new("interface.set", Risk::Mutate).needing_elevation(),
        ]
    }

    fn availability(&self) -> Availability {
        route_availability()
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        stream(query.clone(), |_| read_interfaces())
    }

    /// `watch interface`, through the rtnetlink multicast groups (spec §18.2, ADR-0235).
    ///
    /// Links and both address families, because an interface record carries its addresses: an
    /// address that appears is the interface changing.
    fn subscribe(&self, query: &Query) -> Result<EventStream, ErrorValue> {
        subscribe_table(
            sys::RTMGRP_LINK | sys::RTMGRP_IPV4_IFADDR | sys::RTMGRP_IPV6_IFADDR,
            query,
            read_interfaces,
        )
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        references(read_on_a_blocking_thread(read_interfaces).await?, selector)
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let action = action.clone();
        act_on_a_blocking_thread(move || crate::act::interface(&action)).await
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
        vec![
            Capability::new("route.list", Risk::Read),
            Capability::new("route.set", Risk::Mutate).needing_elevation(),
        ]
    }

    fn availability(&self) -> Availability {
        route_availability()
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        stream(query.clone(), |query| {
            read_routes(option_text(query, "family").as_deref())
        })
    }

    /// `watch route`, through the rtnetlink multicast groups (spec §18.2, ADR-0235).
    fn subscribe(&self, query: &Query) -> Result<EventStream, ErrorValue> {
        let family = option_text(query, "family");
        subscribe_table(
            sys::RTMGRP_IPV4_ROUTE | sys::RTMGRP_IPV6_ROUTE,
            query,
            move || read_routes(family.as_deref()),
        )
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        references(
            read_on_a_blocking_thread(|| read_routes(None)).await?,
            selector,
        )
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let action = action.clone();
        act_on_a_blocking_thread(move || crate::act::route(&action)).await
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
            Capability::new("socket.close", Risk::Destructive).needing_elevation(),
        ]
    }

    fn availability(&self) -> Availability {
        match NetlinkSocket::open_diag() {
            Ok(_) => Availability::Available,
            Err(error) => Availability::unavailable(error.message()),
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let query = query.clone();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let (sender, mut batches) = mpsc::channel(1);
                let diagnostics = sink.diagnostics().clone();
                let reader = tokio::task::spawn_blocking(move || {
                    stream_sockets(&query, &sender, &diagnostics)
                });
                'reading: while let Some(batch) = batches.recv().await {
                    for item in batch {
                        let sent = match item {
                            Item::Record(record) => {
                                sink.send(Value::Record(Arc::new(record))).await
                            }
                            Item::Failure(error) => sink.fail(error).await,
                        };
                        if sent.is_err() {
                            break 'reading;
                        }
                    }
                }
                // Dropping the receiver is how the reader is told to stop: its next handover
                // fails, and the dumps it has not issued yet are never asked for.
                drop(batches);
                let _ = reader.await;
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        references(
            read_on_a_blocking_thread(|| read_sockets(None, false)).await?,
            selector,
        )
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let action = action.clone();
        act_on_a_blocking_thread(move || crate::act::socket(&action)).await
    }
}

/// How many objects one handover from the socket reader to its consumer carries.
///
/// The handover is the backpressure. The channel holds a single batch, so the reader parks as
/// soon as one is outstanding, and a pipeline that stops after the first object leaves at most
/// one batch decoded and one dump issued behind it. Batching rather than sending record by
/// record keeps the whole table cheap as well: `get socket | count` on a host with thousands
/// pays one handover per batch (ADR-0418).
const SOCKET_BATCH: usize = 64;

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
    // `trace connection --remote 10.4.2.11` and `get connection --remote …` (spec §22.3) name
    // the peer; a socket with another peer, or none, is not the one asked for.
    if let Some(Value::Ip(remote)) = query.option_value("remote")
        && endpoint_address(record, "remote") != Some(*remote)
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

/// The address of one end of a socket record.
fn endpoint_address(record: &RecordValue, side: &str) -> Option<std::net::IpAddr> {
    match record.get(side) {
        Some(Value::Record(endpoint)) => match endpoint.get("address") {
            Some(Value::Ip(address)) => Some(*address),
            _ => None,
        },
        _ => None,
    }
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

/// Runs one netlink write on the runtime's blocking pool.
async fn act_on_a_blocking_thread<F>(act: F) -> Result<ActionOutcome, ErrorValue>
where
    F: FnOnce() -> Result<ActionOutcome, ErrorValue> + Send + 'static,
{
    match tokio::task::spawn_blocking(act).await {
        Ok(outcome) => outcome,
        Err(joined) => Err(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!("the netlink writer stopped before answering: {joined}"),
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

/// How long a multicast reader waits for the kernel before coming up for air.
///
/// Nothing is re-read when it expires: it is only how often the reader notices that nobody is
/// listening, so a cancelled watch stops within it.
const REAP: u16 = 200;

/// Subscribes to `groups` and reports what `read` sees change (spec §18.2, ADR-0235).
///
/// The kernel says *that* something changed; the answer to *what* is a fresh dump through the
/// same decoders a `get` uses, diffed by object identity. That keeps one description of an
/// interface or a route in the crate rather than two that can drift, and it costs one dump per
/// change rather than one per tick — on an idle machine, none at all.
///
/// # Errors
///
/// Whatever opening or binding the multicast socket refused. The watch runtime falls back to
/// polling on any error, so a refusal costs latency and never the answer.
fn subscribe_table<F>(groups: u32, query: &Query, read: F) -> Result<EventStream, ErrorValue>
where
    F: Fn() -> Result<Decoded, ErrorValue> + Send + Sync + 'static,
{
    let socket = NetlinkSocket::open_route_multicast(groups)?;
    let read = Arc::new(read);
    // The watch narrows exactly as the listing does: `watch interface lo` is about `lo`, and a
    // change to another interface is not an answer to it.
    let query = query.clone();
    // A dump before the first wake, so the first change is a change against the real table and
    // not against emptiness.
    let mut known = table(&read()?, &query);

    Ok(EventStream::spawn(
        PipelineConfig::new(),
        move |sink| async move {
            let (sender, mut receiver) = tokio::sync::mpsc::channel::<()>(8);
            std::thread::spawn(move || wait_for_changes(&socket, &sender));

            while receiver.recv().await.is_some() {
                let reader = Arc::clone(&read);
                let Ok(Ok(fresh)) = tokio::task::spawn_blocking(move || reader()).await else {
                    // The table could not be read this time; the next change asks again.
                    continue;
                };
                let seen = table(&fresh, &query);
                for (id, record) in &seen {
                    let event = match known.get(id) {
                        None => ObjectEvent::added(record),
                        Some(previous) if previous == record => continue,
                        Some(previous) => ObjectEvent::changed(record, moved(previous, record)),
                    };
                    if sink.send(event).await.is_err() {
                        return;
                    }
                }
                for (id, record) in &known {
                    if !seen.contains_key(id)
                        && sink.send(ObjectEvent::removed(record)).await.is_err()
                    {
                        return;
                    }
                }
                known = seen;
            }
        },
    ))
}

/// The decoded records the query keeps, by identity — which is what a change is measured against.
fn table(decoded: &Decoded, query: &Query) -> std::collections::BTreeMap<String, RecordValue> {
    decoded
        .records()
        .iter()
        .filter(|record| keep(record, query))
        .filter_map(|record| {
            ono_provider_api::ObjectId::of(record).map(|id| (id.to_string(), record.clone()))
        })
        .collect()
}

/// The fields whose values moved between two observations of one object.
fn moved(previous: &RecordValue, current: &RecordValue) -> Vec<String> {
    current
        .schema()
        .fields()
        .iter()
        .filter(|field| previous.get(field.name()) != current.get(field.name()))
        .map(|field| field.name().to_owned())
        .collect()
}

/// Waits on the multicast socket and says so, until nobody is listening any more.
fn wait_for_changes(socket: &NetlinkSocket, sender: &tokio::sync::mpsc::Sender<()>) {
    use nix::poll::{PollFd, PollFlags, PollTimeout};

    loop {
        if sender.is_closed() {
            return;
        }
        let mut fds = [PollFd::new(socket.as_fd(), PollFlags::POLLIN)];
        match nix::poll::poll(&mut fds, PollTimeout::from(REAP)) {
            Ok(0) => continue,
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(_) => return,
        }
        if socket.drain() && sender.blocking_send(()).is_err() {
            return;
        }
    }
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

/// Reads the socket table for `query` and sends what it finds, one object at a time.
///
/// The dumps are issued in order and each one is decoded message by message, so the work stops
/// where the consumer does: `get socket | take 1` closes the channel after the first object, the
/// next send fails, and the dumps behind it are never asked for (ADR-0418). That is also why
/// failures travel in the order they were met rather than ahead of every object — a family this
/// function never reached has not failed, and one it did reach is reported before anything read
/// after it (spec §16.5).
fn stream_sockets(query: &Query, sender: &mpsc::Sender<Vec<Item>>, diagnostics: &Diagnostics) {
    let mut batch = Batch::new(sender);
    // §34.4's population, when this query is one whose answer is every socket the kernel dumps.
    // A query that filters — `get connection`, `--listening`, a selector — would need the records
    // to count what survives, which is the work the bound exists to avoid, so it states no
    // population and a caller that bounded it claims no count (§2.17, ADR-0576).
    let mut population = countable(query).then_some(0_u64);
    let socket = match NetlinkSocket::open_diag() {
        Ok(socket) => socket,
        Err(error) => {
            if batch.push(Item::Failure(error)) {
                batch.flush();
            }
            return;
        }
    };

    let mut owners = None;
    if query.flag("process") {
        match SocketOwners::from_proc() {
            Ok(scanned) => owners = Some(scanned),
            Err(error) => {
                if !batch.push(Item::Failure(error)) {
                    return;
                }
            }
        }
    }

    let protocol = option_text(query, "protocol");
    let protocol = protocol.as_deref();
    let mut sent = 0;
    let mut sending = true;
    for transport in [SocketProtocol::Tcp, SocketProtocol::Udp] {
        if protocol.is_some_and(|wanted| wanted != transport.as_str()) {
            continue;
        }
        for family in [sys::AF_INET, sys::AF_INET6] {
            // Once the answer is full and no population is being counted, there is nothing left
            // for another dump to contribute, and asking the kernel for one is the cost
            // `get socket | take 1` must not pay (ADR-0418).
            if !sending && population.is_none() {
                batch.flush();
                return;
            }
            let request = inet_diag_request(family, transport.number());
            match socket.dump(sys::SOCK_DIAG_BY_FAMILY, &request) {
                Ok(bytes) => {
                    if sending {
                        let items = inet_sockets(&bytes, transport, owners.as_ref());
                        match collect(items, query, &mut batch, &mut sent) {
                            Reading::More => {}
                            Reading::Bounded => sending = false,
                            Reading::Gone => return,
                        }
                    }
                    population = add_population(population, &bytes);
                }
                Err(error) => {
                    if !batch.push(Item::Failure(error)) {
                        return;
                    }
                    // A family that could not be dumped is a family nobody counted, so the
                    // population stops being a figure anybody may show (§2.17).
                    population = None;
                }
            }
            if !batch.flush() {
                return;
            }
        }
    }

    // A Unix socket has no remote endpoint — its peer is an inode, and `remote` is null on every
    // record this crate builds for one — so it can never answer the `connection` target, and a
    // dump nothing in the answer can come from is not worth asking the kernel for.
    if protocol.is_none_or(|wanted| wanted == "unix")
        && query.target_name() != "connection"
        && (sending || population.is_some())
    {
        match socket.dump(sys::SOCK_DIAG_BY_FAMILY, &unix_diag_request()) {
            Ok(bytes) => {
                if sending {
                    let items = unix_sockets(&bytes, owners.as_ref());
                    match collect(items, query, &mut batch, &mut sent) {
                        Reading::More | Reading::Bounded => {}
                        Reading::Gone => return,
                    }
                }
                population = add_population(population, &bytes);
            }
            Err(error) => {
                if !batch.push(Item::Failure(error)) {
                    return;
                }
                population = None;
            }
        }
    }
    if let Some(population) = population {
        diagnostics.record_population(population);
    }
    batch.flush();
}

/// Whether the population of this query is the number of sockets the kernel dumps.
///
/// [`keep`] is what decides whether a socket survives a query, and every condition it can apply
/// beyond the dump itself — the `connection` target's peer, `--listening`, a table, an interface,
/// a remote, a port, a selector — needs the decoded record. Counting those would be exactly the
/// work a bounded query asked not to do, so a query carrying one states no population at all
/// rather than a figure that would have to be guessed (§2.17, ADR-0576).
fn countable(query: &Query) -> bool {
    query.target_name() != "connection"
        && !query.flag("listening")
        && query.selectors().is_empty()
        && option_text(query, "table").is_none()
        && option_text(query, "interface").is_none()
        && query.option_value("remote").is_none()
        && query.option_value("port").is_none()
}

/// Adds one dump's socket count to a running population, or gives it up.
fn add_population(population: Option<u64>, bytes: &[u8]) -> Option<u64> {
    let running = population?;
    let counted = crate::socket::count_diag_sockets(bytes)?;
    Some(running.saturating_add(counted))
}

/// How far [`collect`] got through one dump.
enum Reading {
    /// The dump is exhausted and the answer has room for more.
    More,
    /// The answer is full: the query's bound is reached, and nothing further is sent.
    Bounded,
    /// The consumer has gone, and nothing is worth reading.
    Gone,
}

/// Puts everything one dump decodes to into `batch`, answering whether the caller should keep
/// going.
///
/// `sent` counts the objects that survived the query across every dump, because `--first` is a
/// bound on the answer rather than on one address family.
fn collect(
    items: impl Iterator<Item = Item>,
    query: &Query,
    batch: &mut Batch<'_>,
    sent: &mut usize,
) -> Reading {
    for item in items {
        if let Item::Record(record) = &item {
            if !keep(record, query) {
                continue;
            }
            if query.max().is_some_and(|max| *sent >= max) {
                batch.flush();
                return Reading::Bounded;
            }
            *sent += 1;
        }
        if !batch.push(item) {
            return Reading::Gone;
        }
    }
    Reading::More
}

/// What the socket reader hands to its consumer, and the point at which it hands it over.
struct Batch<'a> {
    sender: &'a mpsc::Sender<Vec<Item>>,
    items: Vec<Item>,
}

impl<'a> Batch<'a> {
    fn new(sender: &'a mpsc::Sender<Vec<Item>>) -> Self {
        Self {
            sender,
            items: Vec::new(),
        }
    }

    /// Adds one item, handing the batch over when it is full. `false` once the consumer has gone.
    fn push(&mut self, item: Item) -> bool {
        self.items.push(item);
        self.items.len() < SOCKET_BATCH || self.flush()
    }

    /// Hands over what has been collected, and waits until the consumer has taken it.
    ///
    /// An empty batch is handed over too, at the end of every dump: waiting there is what keeps
    /// the reader one dump ahead of its consumer at most, so `get socket | take 1` does not pay
    /// for three address families nobody read (ADR-0418). `false` once the consumer has gone.
    fn flush(&mut self) -> bool {
        self.sender
            .blocking_send(std::mem::take(&mut self.items))
            .is_ok()
    }
}

/// One dump per protocol and family, plus the Unix table.
///
/// `owners` is scanned at most once for the whole answer, never once per socket.
pub(crate) fn read_sockets(
    protocol: Option<&str>,
    with_owners: bool,
) -> Result<Decoded, ErrorValue> {
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
