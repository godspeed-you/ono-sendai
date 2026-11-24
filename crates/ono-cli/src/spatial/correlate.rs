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
//!   local one). Both ends were observed, each by its own host: `strong`.
//! - **address** — no mirrored socket, but the remote address is one of the far side's interface
//!   addresses. An address alone is weak — NAT, private ranges every network reuses — so the edge
//!   is `inferred`, and says when the address is in a private range. A loopback, unspecified or
//!   link-local address names no host at all and is never evidence.
//! - **ambiguous** — the evidence of the strongest grade found points at more than one link.
//!   Nothing is picked: no edge is drawn, and the map says which links it could not tell apart.
//!
//! The far side's sockets and addresses are read with ordinary queries (`get socket`, `get
//! address`), so the far agent authorizes them as it authorizes any read; a refusal is reported
//! as one, and nothing is inferred from a table this session may not read.

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
#[must_use]
pub fn names_no_host(address: IpAddr) -> bool {
    match address {
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
/// The strongest basis found decides; several links on it are ambiguous and none is picked.
#[must_use]
pub fn classify(local: End, remote: End, far: &[Option<&FarSide>]) -> Verdict {
    let mirrored: Vec<usize> = far
        .iter()
        .enumerate()
        .filter(|(_, side)| side.is_some_and(|side| side.sockets.contains(&(remote, local))))
        .map(|(index, _)| index)
        .collect();
    let decide = |found: Vec<usize>, basis| match found.as_slice() {
        [] => None,
        [one] => Some(Verdict::Reaches(*one, basis)),
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
        .filter(|(_, side)| side.is_some_and(|side| side.addresses.contains(&remote.address)))
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

/// Reads this host's established TCP connections and each linked host's sockets and addresses,
/// and draws the connections the evidence maps to a linked host (ADR-1098).
///
/// # Errors
///
/// A record that cannot be placed; a provider failure on this host's own socket table is a
/// report entry, not an error.
pub async fn connections(
    providers: &ProviderRegistry,
    session: &mut SpatialSessionState,
    hop1: &[(LinkFacts, SpatialId)],
    now: Timestamp,
) -> Result<Correlation, ErrorValue> {
    let mut correlation = Correlation::default();
    let reads = hop1.iter().map(|(link, _)| read_far(link));
    let far: Vec<Result<FarSide, Option<ErrorValue>>> = futures::future::join_all(reads).await;
    for ((link, _), read) in hop1.iter().zip(&far) {
        correlation.report.push(link_entry(link, read));
    }
    let sides: Vec<Option<&FarSide>> = far.iter().map(|read| read.as_ref().ok()).collect();
    if sides.iter().all(Option::is_none) {
        return Ok(correlation);
    }

    let local = match providers.snapshot(&Query::target("socket").limit(MAX_SOCKETS)) {
        Ok(stream) => stream.collect().await,
        Err(error) => {
            correlation.report.push(entry([
                ("subject", Value::string("this host")),
                ("status", Value::string("unavailable")),
                ("reason", Value::string(error.message())),
            ]));
            return Ok(correlation);
        }
    };
    for value in local.values() {
        let Ok(record) = value.as_record() else {
            continue;
        };
        let Some((here, there)) = established(record) else {
            continue;
        };
        let verdict = classify(here, there, &sides);
        let connection = format!("tcp {here} → {there}");
        match verdict {
            Verdict::Nowhere => {}
            Verdict::Ambiguous(candidates, basis) => {
                let links: Vec<Value> = candidates
                    .iter()
                    .map(|index| Value::string(&hop1[*index].0.name))
                    .collect();
                correlation.report.push(entry([
                    ("subject", Value::string("connection")),
                    ("connection", Value::string(&connection)),
                    ("status", Value::string("ambiguous")),
                    ("basis", Value::string(basis.as_str())),
                    ("links", Value::list(links)),
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
                ]));
            }
        }
    }
    Ok(correlation)
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
    Some(End { address, port })
}

/// Reads one far side's sockets and addresses, or why it could not be read.
///
/// `Err(None)` is a link that is not asked — not connected and followed, or no agent.
async fn read_far(link: &LinkFacts) -> Result<FarSide, Option<ErrorValue>> {
    #[cfg(feature = "remote")]
    {
        let Some(prober) = link.prober.clone().filter(|_| link.reachable()) else {
            return Err(None);
        };
        let sockets = read(&prober, "socket", MAX_SOCKETS);
        let addresses = read(&prober, "address", MAX_ADDRESSES);
        let (sockets, addresses) = futures::future::join(sockets, addresses).await;
        let (sockets, addresses) = (sockets.map_err(Some)?, addresses.map_err(Some)?);
        let mut side = FarSide::default();
        for record in &sockets {
            if let Some(pair) = established(record) {
                side.sockets.insert(pair);
            }
        }
        for record in &addresses {
            if let Some(Value::IpNetwork(network)) = record.get("address") {
                side.addresses.insert(network.address());
            }
        }
        Ok(side)
    }
    #[cfg(not(feature = "remote"))]
    {
        let _ = link;
        Err(None)
    }
}

/// One far target, read to its end or its bound, within [`FAR_TIMEOUT`]; the first failure of
/// the stream — a refusal, for a table this session may not read — is the answer.
#[cfg(feature = "remote")]
async fn read(
    prober: &crate::spatial::links::Prober,
    target: &str,
    limit: usize,
) -> Result<Vec<RecordValue>, ErrorValue> {
    let mut stream = prober.query(&ono_protocol::RemoteQuery::target(target).limit(limit))?;
    let collect = async {
        let mut records = Vec::new();
        while let Some(message) = stream.recv().await {
            match message {
                ono_protocol::RemoteMessage::Value(Value::Record(record)) => {
                    records.push(RecordValue::clone(&record));
                }
                ono_protocol::RemoteMessage::Failure(error) if records.is_empty() => {
                    return Err(error);
                }
                _ => {}
            }
            if records.len() >= limit {
                break;
            }
        }
        Ok(records)
    };
    match tokio::time::timeout(FAR_TIMEOUT, collect).await {
        Ok(read) => read,
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
        FarSide {
            sockets: sockets.iter().map(|(l, r)| (end(l), end(r))).collect(),
            addresses: addresses
                .iter()
                .map(|a| a.parse().expect("an address"))
                .collect(),
        }
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
}
