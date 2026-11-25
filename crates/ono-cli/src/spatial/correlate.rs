//! Which of this host's connections reach a host this session links to (v0.4 §14.5, §19.4,
//! issue #309, ADR-1098).
//!
//! §14.5: "If a connection's remote endpoint is confidently mapped to a host available through
//! an Ono remote link, the map MAY expose" an edge to it, and "MUST NOT infer remote identity from
//! IP coincidence alone when ambiguity exists". §19.4: "Cross-host edges MUST be based on explicit
//! remote evidence or strong multi-sided correlation."
//!
//! So the evidence is graded, and the grade is the edge's confidence:
//!
//! - **two-sided** — the far side's own socket table holds the mirror of this connection (its
//!   local endpoint is this connection's remote one, and its remote endpoint this connection's
//!   local one). Both ends were observed, each by its own host: `strong`. A mirror between
//!   addresses every host has (loopback, link-local, unspecified) is evidence only when the far
//!   side is proven to be this very machine — it runs this kernel's boot — because every host
//!   holds such a socket table of its own; then "this connection reaches that host" is simply
//!   true.
//! - **address** — no mirrored socket, but the remote address is one of the far side's interface
//!   addresses. An address alone is weak — NAT, private ranges every network reuses — so the edge
//!   is `inferred`, and says when the address is in a private range. A loopback, unspecified or
//!   link-local address names no host at all and is never evidence.
//! - **ambiguous** — the evidence of the strongest grade found points at more than one link.
//!   Nothing is picked: no edge is drawn, and the map says which links it could not tell apart.
//!
//! The far side's connections and addresses are read with ordinary queries (`get connection
//! --protocol tcp`, `get address`), so the far agent authorizes them as it authorizes any read; a
//! refusal is reported as one, and nothing is inferred from a table this session may not read.
//! A table cut at its bound, or failed after its first record, is **partial**: what it does not
//! show cannot be excluded, so a verdict another link's partial table could have contradicted is
//! not unique, and the map says `partial` rather than drawing it.

use std::collections::BTreeSet;
use std::net::IpAddr;

use jiff::Timestamp;
use ono_provider_api::{ProviderRegistry, Query};
use ono_spatial_core::{RelationshipEdge, SpatialId};
use ono_spatial_query::HorizonPlace;
use ono_value::{ErrorValue, MapValue, Provenance, RecordValue, SchemaId, Value};

use crate::spatial::LinkFacts;
use crate::spatial::session::SpatialSessionState;

/// How long one far side may take to send its sockets and addresses.
pub const FAR_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The most sockets read from one far side, and from this host.
pub const MAX_SOCKETS: usize = 4096;

/// The most interface addresses read from one far side.
pub const MAX_ADDRESSES: usize = 1024;

/// The provider id a correlation edge's provenance names.
const CORRELATOR: &str = "ono.link.correlate";

/// One end of a TCP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct End {
    /// The address.
    pub address: IpAddr,
    /// The port.
    pub port: u16,
}

impl End {
    /// The same end with an IPv4-mapped IPv6 address read as the IPv4 address it maps: a
    /// dual-stack listener reports `::ffff:a.b.c.d` for a peer the other side sees as
    /// `a.b.c.d`.
    #[must_use]
    pub fn canonical(self) -> Self {
        Self {
            address: self.address.to_canonical(),
            port: self.port,
        }
    }
}

impl std::fmt::Display for End {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.address {
            IpAddr::V4(address) => write!(f, "{address}:{}", self.port),
            IpAddr::V6(address) => write!(f, "[{address}]:{}", self.port),
        }
    }
}

/// What one far side showed: its established TCP connections and its interface addresses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FarSide {
    /// `(local, remote)` of each established TCP socket, as the far side sees it.
    pub sockets: BTreeSet<(End, End)>,
    /// Its interface addresses.
    pub addresses: BTreeSet<IpAddr>,
    /// Whether the far side is proven to be this machine: it runs this kernel's boot (ADR-0985's
    /// identity). Only then is a mirror between loopback addresses evidence.
    pub same_machine: bool,
    /// Whether a table was cut at its bound or failed after its first record, so what it does
    /// not show cannot be excluded.
    pub partial: bool,
}

impl FarSide {
    /// Records that the far side holds the socket `local → remote`, its addresses read as the
    /// IPv4 addresses they map where they are IPv4-mapped IPv6.
    pub fn hold(&mut self, local: End, remote: End) {
        self.sockets.insert((local.canonical(), remote.canonical()));
    }

    /// Records one of the far side's interface addresses, read the same way.
    pub fn address(&mut self, address: IpAddr) {
        self.addresses.insert(address.to_canonical());
    }
}

/// How a connection was matched to a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Basis {
    /// The far side holds the mirrored socket.
    TwoSided,
    /// The remote address is one of the far side's addresses, and nothing more.
    Address,
}

impl Basis {
    /// The word the map spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TwoSided => "two_sided",
            Self::Address => "address",
        }
    }
}

/// What the evidence says about one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// One link, on this basis.
    Reaches(usize, Basis),
    /// Several links on the strongest basis found: nothing is picked.
    Ambiguous(Vec<usize>, Basis),
    /// No evidence for any link.
    Nowhere,
}

/// Whether an address names nothing about any one host: it exists on every host, or on none.
///
/// An IPv4 address a dual-stack socket reports as IPv4-mapped IPv6 (`::ffff:127.0.0.1`) is that
/// IPv4 address.
#[must_use]
pub fn names_no_host(address: IpAddr) -> bool {
    match address.to_canonical() {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_unspecified() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Whether an address is in a range many networks reuse, where it says even less.
#[must_use]
pub fn is_private(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => v4.is_private() || v4.octets()[0] == 100 && v4.octets()[1] & 0xc0 == 64,
        IpAddr::V6(v6) => (v6.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/// Grades the evidence that the connection `local → remote` reaches each of `far` (§14.5).
///
/// The strongest basis found decides; several links on it are ambiguous and none is picked. One
/// link is not unique while another link's table is partial — that table may hold the very
/// evidence it does not show — so the verdict is demoted to ambiguous, naming the partial links
/// among the candidates (ADR-1098).
#[must_use]
pub fn classify(local: End, remote: End, far: &[Option<&FarSide>]) -> Verdict {
    let (local, remote) = (local.canonical(), remote.canonical());
    // A mirror between addresses every host has is a socket table every host holds, unless the
    // far side is this machine.
    let anonymous = names_no_host(remote.address) || names_no_host(local.address);
    let mirrored: Vec<usize> = far
        .iter()
        .enumerate()
        .filter(|(_, side)| {
            side.is_some_and(|side| {
                (side.same_machine || !anonymous) && side.sockets.contains(&(remote, local))
            })
        })
        .map(|(index, _)| index)
        .collect();
    let partial: Vec<usize> = far
        .iter()
        .enumerate()
        .filter(|(_, side)| side.is_some_and(|side| side.partial))
        .map(|(index, _)| index)
        .collect();
    let decide = |found: Vec<usize>, basis| match found.as_slice() {
        [] => None,
        [one] => {
            let mut contested: Vec<usize> = partial.iter().copied().filter(|p| p != one).collect();
            if contested.is_empty() {
                Some(Verdict::Reaches(*one, basis))
            } else {
                contested.push(*one);
                contested.sort_unstable();
                Some(Verdict::Ambiguous(contested, basis))
            }
        }
        _ => Some(Verdict::Ambiguous(found, basis)),
    };
    if let Some(verdict) = decide(mirrored, Basis::TwoSided) {
        return verdict;
    }
    if names_no_host(remote.address) {
        return Verdict::Nowhere;
    }
    let addressed: Vec<usize> = far
        .iter()
        .enumerate()
        .filter(|(_, side)| {
            side.is_some_and(|side| {
                side.addresses
                    .iter()
                    .any(|address| address.to_canonical() == remote.address)
            })
        })
        .map(|(index, _)| index)
        .collect();
    decide(addressed, Basis::Address).unwrap_or(Verdict::Nowhere)
}

/// What the correlation adds to `map links --connections`.
#[derive(Debug, Default)]
pub struct Correlation {
    /// This host's connections that reach a linked host, at depth 1.
    pub places: Vec<HorizonPlace>,
    /// From each such connection to the linked host's root.
    pub edges: Vec<RelationshipEdge>,
    /// What each link let this session read, and every connection that was correlated or could
    /// not be told apart (`correlations`).
    pub report: Vec<Value>,
}

/// Everything a correlation reads, read before anything is placed: the far sides' tables and
/// this host's own connections (ADR-1098). Gathering spends the network and the providers;
/// [`draw`] only places what was gathered, so a caller holds the spatial session for that alone.
#[derive(Debug)]
pub struct Evidence {
    far: Vec<Result<FarSide, Option<ErrorValue>>>,
    local: Result<Table, ErrorValue>,
}

/// One table as read: its records, and whether it is partial.
#[derive(Debug, Default)]
struct Table {
    records: Vec<RecordValue>,
    partial: bool,
}

/// Reads each linked host's connections and addresses, side by side, and this host's own
/// established TCP connections (ADR-1098).
///
/// At most [`MAX_PEERS_ASKED`](crate::spatial::federation::MAX_PEERS_ASKED) links are asked, as
/// the second hop asks no more; each far side answers within [`FAR_TIMEOUT`], so a gather costs
/// at most that however many links there are.
pub async fn gather(providers: &ProviderRegistry, hop1: &[(LinkFacts, SpatialId)]) -> Evidence {
    let mut asked = 0usize;
    let local_boot = local_boot(providers).await;
    let reads = hop1.iter().map(|(link, _)| {
        let within_bound = link.prober.is_some() && link.reachable() && {
            asked += 1;
            asked <= crate::spatial::federation::MAX_PEERS_ASKED
        };
        let local_boot = local_boot.as_deref();
        async move {
            if within_bound {
                read_far(link, local_boot).await
            } else if link.prober.is_some() && link.reachable() {
                Err(Some(ErrorValue::new(
                    ono_core::ErrorCode::ResourceItemLimit,
                    format!(
                        "beyond the bound on peers one map asks ({})",
                        crate::spatial::federation::MAX_PEERS_ASKED
                    ),
                )))
            } else {
                Err(None)
            }
        }
    });
    let far: Vec<Result<FarSide, Option<ErrorValue>>> = futures::future::join_all(reads).await;
    let local = if far.iter().any(Result::is_ok) {
        read_local(providers).await
    } else {
        Ok(Table::default())
    };
    Evidence { far, local }
}

/// Places the connections `evidence` maps to a linked host (ADR-1098).
///
/// # Errors
///
/// A record that cannot be placed; a provider failure on this host's own table is a report
/// entry, not an error.
pub fn draw(
    evidence: &Evidence,
    session: &mut SpatialSessionState,
    hop1: &[(LinkFacts, SpatialId)],
    now: Timestamp,
) -> Result<Correlation, ErrorValue> {
    let mut correlation = Correlation::default();
    for ((link, _), read) in hop1.iter().zip(&evidence.far) {
        correlation.report.push(link_entry(link, read));
    }
    let sides: Vec<Option<&FarSide>> = evidence.far.iter().map(|read| read.as_ref().ok()).collect();
    if sides.iter().all(Option::is_none) {
        return Ok(correlation);
    }
    let local = match &evidence.local {
        Ok(local) => local,
        Err(error) => {
            correlation.report.push(entry([
                ("subject", Value::string("this host")),
                ("status", Value::string("unavailable")),
                ("reason", Value::string(error.message())),
            ]));
            return Ok(correlation);
        }
    };
    if local.partial {
        // Connections this host's table did not show are not drawn; those it showed are judged
        // on their own evidence.
        correlation.report.push(entry([
            ("subject", Value::string("this host")),
            ("status", Value::string("partial")),
            (
                "reason",
                Value::string(&format!(
                    "this host's connection table was cut at {MAX_SOCKETS} or failed part way; \
                     a connection it did not show is not correlated"
                )),
            ),
        ]));
    }
    for record in &local.records {
        let Some((here, there)) = established(record) else {
            continue;
        };
        let verdict = classify(here, there, &sides);
        let connection = format!("tcp {here} → {there}");
        match verdict {
            Verdict::Nowhere => {}
            Verdict::Ambiguous(candidates, basis) => {
                let name = |index: &usize| Value::string(&hop1[*index].0.name);
                let partial: Vec<Value> = candidates
                    .iter()
                    .filter(|index| sides[**index].is_some_and(|side| side.partial))
                    .map(name)
                    .collect();
                correlation.report.push(entry([
                    ("subject", Value::string("connection")),
                    ("connection", Value::string(&connection)),
                    ("status", Value::string("ambiguous")),
                    ("basis", Value::string(basis.as_str())),
                    ("links", Value::list(candidates.iter().map(name))),
                    ("partial", Value::list(partial)),
                ]));
            }
            Verdict::Reaches(index, basis) => {
                let (link, root) = &hop1[index];
                session.absorb(std::slice::from_ref(record), now);
                let Some(place) = session.place_of(record) else {
                    continue;
                };
                correlation.places.push(
                    HorizonPlace::new(place.clone(), 1, None)
                        .in_state(Some("established".to_owned())),
                );
                correlation
                    .edges
                    .push(edge(&place, root, link, here, there, basis, now));
                correlation.report.push(entry([
                    ("subject", Value::string("connection")),
                    ("connection", Value::string(&connection)),
                    ("status", Value::string("correlated")),
                    ("basis", Value::string(basis.as_str())),
                    ("links", Value::list(vec![Value::string(&link.name)])),
                    ("partial", Value::list(Vec::new())),
                ]));
            }
        }
    }
    Ok(correlation)
}

/// The query the join needs: TCP sockets that have a peer — `get connection --protocol tcp` —
/// so the bound is not spent on listeners, UDP and unix sockets.
fn connections_query() -> Query {
    Query::target("connection").option("protocol", Value::string("tcp"))
}

/// This host's own established TCP connections, to the bound; cut there, or failed part way,
/// the table is partial.
async fn read_local(providers: &ProviderRegistry) -> Result<Table, ErrorValue> {
    // One more than the bound, as for a far table: exactly the bound is not a cut table.
    let collected = providers
        .snapshot(&connections_query().limit(MAX_SOCKETS + 1))?
        .collect()
        .await;
    let mut records: Vec<RecordValue> = collected
        .values()
        .iter()
        .filter_map(|value| value.as_record().ok().cloned())
        .collect();
    if records.is_empty()
        && let Some(error) = collected.errors().first()
    {
        return Err(error.clone());
    }
    let cut = records.len() > MAX_SOCKETS;
    records.truncate(MAX_SOCKETS);
    Ok(Table {
        partial: cut || !collected.errors().is_empty(),
        records,
    })
}

/// This host's boot, as its own `kernel` target answers — the identity a far side is compared
/// by (ADR-0985).
async fn local_boot(providers: &ProviderRegistry) -> Option<String> {
    #[cfg(feature = "remote")]
    {
        crate::context::far_boot(providers).await
    }
    #[cfg(not(feature = "remote"))]
    {
        let _ = providers;
        None
    }
}

/// The two ends of an established TCP connection, where the record is one.
fn established(record: &RecordValue) -> Option<(End, End)> {
    let text = |field: &str| match record.get(field) {
        Some(Value::String(text)) => Some(text.to_string()),
        _ => None,
    };
    if text("protocol").as_deref() != Some("tcp") || text("state").as_deref() != Some("established")
    {
        return None;
    }
    Some((
        end_of(record.get("local")?)?,
        end_of(record.get("remote")?)?,
    ))
}

fn end_of(value: &Value) -> Option<End> {
    let Value::Record(endpoint) = value else {
        return None;
    };
    let address = match endpoint.get("address")? {
        Value::Ip(address) => *address,
        Value::String(text) => text.parse().ok()?,
        _ => return None,
    };
    let port = match endpoint.get("port")? {
        Value::Port(port) => *port,
        Value::Int(port) => u16::try_from(*port).ok()?,
        _ => return None,
    };
    Some(End {
        address: address.to_canonical(),
        port,
    })
}

/// Reads one far side's connections and addresses, or why it could not be read.
///
/// `Err(None)` is a link that is not asked — not connected and followed, or no agent.
async fn read_far(
    link: &LinkFacts,
    local_boot: Option<&str>,
) -> Result<FarSide, Option<ErrorValue>> {
    #[cfg(feature = "remote")]
    {
        let Some(prober) = link.prober.clone().filter(|_| link.reachable()) else {
            return Err(None);
        };
        let sockets = read(
            &prober,
            ono_protocol::RemoteQuery::from_query(&connections_query()),
            MAX_SOCKETS,
        );
        let addresses = read(
            &prober,
            ono_protocol::RemoteQuery::target("address"),
            MAX_ADDRESSES,
        );
        let (sockets, addresses) = futures::future::join(sockets, addresses).await;
        let (sockets, addresses) = (sockets.map_err(Some)?, addresses.map_err(Some)?);
        let mut side = FarSide {
            same_machine: local_boot.is_some() && link.boot_id.as_deref() == local_boot,
            partial: sockets.partial || addresses.partial,
            ..FarSide::default()
        };
        for record in &sockets.records {
            if let Some((local, remote)) = established(record) {
                side.hold(local, remote);
            }
        }
        for record in &addresses.records {
            if let Some(Value::IpNetwork(network)) = record.get("address") {
                side.address(network.address());
            }
        }
        Ok(side)
    }
    #[cfg(not(feature = "remote"))]
    {
        let _ = (link, local_boot);
        Err(None)
    }
}

/// One far table, read to its end or its bound, within [`FAR_TIMEOUT`].
///
/// A failure before the first record — a refusal, for a table this session may not read — is
/// the answer. A table that reaches its bound, or fails after its first record, is returned as
/// read and marked partial: what it did not show cannot be excluded.
#[cfg(feature = "remote")]
async fn read(
    prober: &crate::spatial::links::Prober,
    query: ono_protocol::RemoteQuery,
    limit: usize,
) -> Result<Table, ErrorValue> {
    let target = query.target_name().to_owned();
    // One more than the bound is asked for, so a table that holds exactly the bound is told
    // apart from one that holds more.
    let mut stream = prober.query(&query.limit(limit + 1))?;
    let mut table = Table::default();
    let collect = async {
        while let Some(message) = stream.recv().await {
            match message {
                ono_protocol::RemoteMessage::Value(Value::Record(record)) => {
                    if table.records.len() == limit {
                        table.partial = true;
                        break;
                    }
                    table.records.push(RecordValue::clone(&record));
                }
                ono_protocol::RemoteMessage::Failure(error) if table.records.is_empty() => {
                    return Err(error);
                }
                ono_protocol::RemoteMessage::Failure(_) => table.partial = true,
                _ => {}
            }
        }
        Ok(())
    };
    match tokio::time::timeout(FAR_TIMEOUT, collect).await {
        Ok(Ok(())) => Ok(table),
        Ok(Err(refusal)) => Err(refusal),
        // Out of time part way: what arrived is evidence, and what did not cannot be excluded.
        Err(_) if !table.records.is_empty() => {
            table.partial = true;
            Ok(table)
        }
        Err(_) => Err(ErrorValue::new(
            ono_core::ErrorCode::RemoteUnreachable,
            format!(
                "the far side did not send its `{target}` table within {} ms",
                FAR_TIMEOUT.as_millis()
            ),
        )),
    }
}

/// What one link let this session read.
fn link_entry(link: &LinkFacts, read: &Result<FarSide, Option<ErrorValue>>) -> Value {
    let (status, reason) = match read {
        Ok(side) if side.partial => (
            "partial",
            Value::string(&format!(
                "a table was cut at its bound ({MAX_SOCKETS} connections, {MAX_ADDRESSES} \
                 addresses) or failed part way; what it did not show cannot be excluded"
            )),
        ),
        Ok(_) => ("read", Value::Null),
        Err(None) => (
            "not_asked",
            Value::string(if link.reachable() {
                "no agent answers this link (agentless)"
            } else {
                "the link is not connected and followed"
            }),
        ),
        Err(Some(error)) => (
            match error.code() {
                ono_core::ErrorCode::ResourceItemLimit => "not_asked",
                ono_core::ErrorCode::RemoteCapabilityDenied
                | ono_core::ErrorCode::RemoteUnauthorized
                | ono_core::ErrorCode::RemoteUnauthenticated => "not_permitted",
                _ => "unavailable",
            },
            Value::string(error.message()),
        ),
    };
    entry([
        ("subject", Value::string("link")),
        ("link", Value::string(&link.name)),
        ("status", Value::string(status)),
        ("reason", reason),
    ])
}

fn entry<const N: usize>(fields: [(&str, Value); N]) -> Value {
    let mut map = MapValue::new();
    for (name, value) in fields {
        map.insert(name.into(), value);
    }
    Value::Map(std::sync::Arc::new(map))
}

/// The edge from a connection to the linked host it reaches, with its evidence.
fn edge(
    connection: &SpatialId,
    root: &SpatialId,
    link: &LinkFacts,
    here: End,
    there: End,
    basis: Basis,
    now: Timestamp,
) -> RelationshipEdge {
    let relation = ono_spatial_core::relation::spec("connection.reaches_host")
        .map(ono_spatial_core::RelationSpec::relation_type)
        .unwrap_or_else(|| unreachable!("`connection.reaches_host` is declared in relations.yaml"));
    let confidence = match basis {
        Basis::TwoSided => ono_spatial_core::Confidence::Strong,
        Basis::Address => ono_spatial_core::Confidence::Inferred,
    };
    let scope = ono_spatial_core::SpatialScope::remote_host(
        &link.name,
        ono_spatial_core::BootIdentity::unknown_boot(&link.name),
    );
    RelationshipEdge::new(
        connection.clone(),
        root.clone(),
        relation,
        confidence,
        Provenance::remote(
            CORRELATOR,
            scope.host_scope().id(),
            SchemaId::new("ono.spatial-relation", 1),
        )
        .observed_at(now),
        now,
    )
    .with_attribute("basis", Value::string(basis.as_str()))
    .with_attribute("link", Value::string(&link.name))
    .with_attribute("local", Value::string(&here.to_string()))
    .with_attribute("remote", Value::string(&there.to_string()))
    .with_attribute(
        "far_socket",
        match basis {
            Basis::TwoSided => Value::string(&format!("{there} → {here}")),
            Basis::Address => Value::Null,
        },
    )
    .with_attribute("private_address", Value::Bool(is_private(there.address)))
    .with_attribute("crosses_link", Value::string(&link.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end(text: &str) -> End {
        let (address, port) = text.rsplit_once(':').expect("address:port");
        End {
            address: address
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse()
                .expect("an address"),
            port: port.parse().expect("a port"),
        }
    }

    fn side(sockets: &[(&str, &str)], addresses: &[&str]) -> FarSide {
        let mut side = FarSide::default();
        for (local, remote) in sockets {
            side.hold(end(local), end(remote));
        }
        for address in addresses {
            side.address(address.parse().expect("an address"));
        }
        side
    }

    fn this_machine(mut side: FarSide) -> FarSide {
        side.same_machine = true;
        side
    }

    fn partial(mut side: FarSide) -> FarSide {
        side.partial = true;
        side
    }

    #[test]
    fn should_correlate_on_the_mirrored_socket_when_the_far_side_holds_it() {
        let far = side(&[("203.0.113.7:5432", "198.51.100.2:41000")], &[]);
        assert_eq!(
            classify(
                end("198.51.100.2:41000"),
                end("203.0.113.7:5432"),
                &[Some(&far)]
            ),
            Verdict::Reaches(0, Basis::TwoSided)
        );
    }

    #[test]
    fn should_grade_an_address_alone_as_the_weaker_basis() {
        let far = side(&[], &["203.0.113.7"]);
        assert_eq!(
            classify(
                end("198.51.100.2:41000"),
                end("203.0.113.7:5432"),
                &[Some(&far)]
            ),
            Verdict::Reaches(0, Basis::Address)
        );
        assert!(is_private("10.1.2.3".parse().expect("an address")));
        assert!(!is_private("203.0.113.7".parse().expect("an address")));
    }

    #[test]
    fn should_never_take_an_address_every_host_has_as_evidence() {
        let far = side(&[], &["127.0.0.1", "fe80::1"]);
        for remote in ["127.0.0.1:8080", "[fe80::1]:8080", "0.0.0.0:8080"] {
            assert_eq!(
                classify(end("127.0.0.1:41000"), end(remote), &[Some(&far)]),
                Verdict::Nowhere,
                "{remote} names no host"
            );
        }
    }

    #[test]
    fn should_report_two_links_with_the_same_evidence_as_ambiguous_and_pick_neither() {
        let one = side(&[], &["10.0.0.5"]);
        let two = side(&[], &["10.0.0.5"]);
        assert_eq!(
            classify(
                end("10.0.0.9:41000"),
                end("10.0.0.5:22"),
                &[Some(&one), Some(&two)]
            ),
            Verdict::Ambiguous(vec![0, 1], Basis::Address)
        );
    }

    #[test]
    fn should_let_the_two_sided_evidence_decide_over_a_shared_address() {
        // Both far sides carry the address (a private range two networks reuse); only one holds
        // the mirrored socket, and that is the one the connection reaches.
        let holder = side(&[("10.0.0.5:22", "10.0.0.9:41000")], &["10.0.0.5"]);
        let other = side(&[], &["10.0.0.5"]);
        assert_eq!(
            classify(
                end("10.0.0.9:41000"),
                end("10.0.0.5:22"),
                &[Some(&other), Some(&holder)]
            ),
            Verdict::Reaches(1, Basis::TwoSided)
        );
    }

    #[test]
    fn should_say_nowhere_when_a_far_side_could_not_be_read() {
        assert_eq!(
            classify(end("10.0.0.9:41000"), end("10.0.0.5:22"), &[None]),
            Verdict::Nowhere
        );
    }

    #[test]
    fn should_not_take_a_loopback_mirror_as_evidence_unless_the_far_side_is_this_machine() {
        // Every host holds a socket table with 127.0.0.1 in it: a far side that is another
        // machine and shows the mirror of a loopback connection shows its own loopback socket,
        // not this one (review TST-1). Proven to be this machine — this kernel's boot — the
        // connection does reach it.
        let mirror = [("127.0.0.31:5432", "127.0.0.1:41000")];
        let elsewhere = side(&mirror, &["127.0.0.1"]);
        assert_eq!(
            classify(
                end("127.0.0.1:41000"),
                end("127.0.0.31:5432"),
                &[Some(&elsewhere)]
            ),
            Verdict::Nowhere
        );
        let here = this_machine(side(&mirror, &[]));
        assert_eq!(
            classify(
                end("127.0.0.1:41000"),
                end("127.0.0.31:5432"),
                &[Some(&here)]
            ),
            Verdict::Reaches(0, Basis::TwoSided)
        );
        let link_local = side(&[("[fe80::2]:22", "[fe80::1]:41000")], &[]);
        assert_eq!(
            classify(
                end("[fe80::1]:41000"),
                end("[fe80::2]:22"),
                &[Some(&link_local)]
            ),
            Verdict::Nowhere,
            "a link-local mirror names no host either"
        );
    }

    #[test]
    fn should_not_take_the_same_service_with_another_client_as_a_mirror() {
        // The far side serves 203.0.113.7:5432 to somebody else: its local end is this
        // connection's remote end, its remote end is not this connection's local end.
        let far = side(&[("203.0.113.7:5432", "198.51.100.99:52000")], &[]);
        assert_eq!(
            classify(
                end("198.51.100.2:41000"),
                end("203.0.113.7:5432"),
                &[Some(&far)]
            ),
            Verdict::Nowhere
        );
    }

    #[test]
    fn should_match_an_ipv4_mapped_address_as_the_ipv4_address_it_maps() {
        // A far server bound to [::] reports its IPv4 peers as `::ffff:a.b.c.d` (review COR-3).
        let far = side(
            &[("[::ffff:203.0.113.7]:5432", "[::ffff:198.51.100.2]:41000")],
            &[],
        );
        assert_eq!(
            classify(
                end("198.51.100.2:41000"),
                end("203.0.113.7:5432"),
                &[Some(&far)]
            ),
            Verdict::Reaches(0, Basis::TwoSided)
        );
        assert!(names_no_host(
            "::ffff:127.0.0.1".parse().expect("an address")
        ));
    }

    #[test]
    fn should_not_call_a_verdict_unique_while_another_links_table_is_partial() {
        // Another link's table was cut or failed part way: it may hold the mirror it did not
        // show, so the one mirror found cannot be called the only one (review ARC-5, TST-13).
        let holder = side(&[("10.0.0.5:22", "10.0.0.9:41000")], &[]);
        let cut = partial(side(&[], &[]));
        assert_eq!(
            classify(
                end("10.0.0.9:41000"),
                end("10.0.0.5:22"),
                &[Some(&cut), Some(&holder)]
            ),
            Verdict::Ambiguous(vec![0, 1], Basis::TwoSided)
        );
        // A partial table that is the holder's own does not contest it.
        let partial_holder = partial(holder.clone());
        assert_eq!(
            classify(
                end("10.0.0.9:41000"),
                end("10.0.0.5:22"),
                &[Some(&partial_holder)]
            ),
            Verdict::Reaches(0, Basis::TwoSided)
        );
    }

    fn facts(name: &str) -> LinkFacts {
        LinkFacts {
            name: name.to_owned(),
            host: name.to_owned(),
            transport: "tcp".to_owned(),
            connected: true,
            following: true,
            round_trip: None,
            last_seen: None,
            boot_id: None,
            prober: None,
            fingerprint: None,
        }
    }

    #[test]
    fn should_state_the_confidence_and_evidence_each_basis_carries_on_its_edge() {
        // Review TST-12: the grade is the confidence — two-sided is `strong` and names the far
        // socket; an address alone is `inferred`, names none, and says when it is private.
        let connection = SpatialId::of_space("test-connection");
        let root = SpatialId::of_space("test-root");
        let now = Timestamp::UNIX_EPOCH;
        let link = facts("db01");
        let (here, there) = (end("10.0.0.9:41000"), end("10.0.0.5:22"));

        let two_sided = edge(&connection, &root, &link, here, there, Basis::TwoSided, now);
        assert_eq!(two_sided.confidence(), ono_spatial_core::Confidence::Strong);
        assert_eq!(
            two_sided.attributes().get("far_socket"),
            Some(&Value::string("10.0.0.5:22 → 10.0.0.9:41000"))
        );
        assert_eq!(
            two_sided.attributes().get("basis"),
            Some(&Value::string("two_sided"))
        );

        let address = edge(&connection, &root, &link, here, there, Basis::Address, now);
        assert_eq!(address.confidence(), ono_spatial_core::Confidence::Inferred);
        assert_eq!(address.attributes().get("far_socket"), Some(&Value::Null));
        assert_eq!(
            address.attributes().get("private_address"),
            Some(&Value::Bool(true))
        );
        let public = edge(
            &connection,
            &root,
            &link,
            here,
            end("203.0.113.7:22"),
            Basis::Address,
            now,
        );
        assert_eq!(
            public.attributes().get("private_address"),
            Some(&Value::Bool(false))
        );
    }
}
