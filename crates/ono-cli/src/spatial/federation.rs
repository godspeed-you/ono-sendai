//! The second hop of `map links --depth 2`: what each linked host says about its own links
//! (v0.4 §19.3, §19.4, issue #310, ADR-1097).
//!
//! §19.3's federated map draws `prod/web01 ----- prod/db01`. This session cannot observe that
//! link; it can ask `prod/web01`'s agent for its link table (`ono.link.links/1`), which the agent
//! serves only to a connection that may observe it. What comes back is drawn as what it is — a
//! one-sided report relayed by one peer — and §19.4 decides how: one-sided observations "MAY be
//! displayed but MUST carry the correct confidence". So a second-hop edge is `inferred` from this
//! session's point of view, carries the confidence the far side itself claims for it, and is
//! attributed to the peer that reported it, from where, and when.
//!
//! Every hop-1 link says what became of the question — `listed`, `partial` (the far side left
//! links out), `not_permitted` (the far side refused: never shown as "no links"), `not_offered`
//! (an older agent, or no agent at all), `unavailable` (the link is down, the request failed or
//! timed out) or `not_asked` (the link is not connected and followed, or beyond the fan-out
//! bound). Nothing is relayed further: an agent answers about the links it holds and asks nobody
//! on the caller's behalf, so the depth bound is two by construction.

use jiff::Timestamp;
use ono_spatial_core::{RelationshipEdge, SpatialId, space};
use ono_spatial_query::HorizonPlace;
use ono_value::{ErrorValue, MapValue, Provenance, SchemaId, Value};

use crate::spatial::LinkFacts;

/// How long one hop-1 peer may take to send its table. All peers are asked side by side, so this
/// bounds the whole second hop.
pub const LINKS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The most hop-1 peers one map asks. Past it a link is `not_asked`, and the map says so.
pub const MAX_PEERS_ASKED: usize = 32;

/// The deepest `map links` draws: this host, its links, and theirs (ADR-1097).
pub const MAX_DEPTH: usize = 2;

/// The provider id a second-hop edge's provenance names: the request it was learned through.
const REPORTER: &str = "ono.link.links";

/// What the second hop adds to the map.
#[derive(Debug, Default)]
pub struct Federation {
    /// The far hosts, at depth 2.
    pub places: Vec<HorizonPlace>,
    /// The far links, from the hop-1 root that reported each.
    pub edges: Vec<RelationshipEdge>,
    /// One `{link, status, …}` per hop-1 link: what became of the question (`federation`).
    pub report: Vec<Value>,
}

/// What one hop-1 link answered.
enum Told {
    Listed(ono_protocol::LinkTable),
    Refused(ErrorValue),
    /// The far side's handshake did not offer the request: decided here, from what this side
    /// negotiated, never from text the far side wrote (review SEC-12).
    NotOffered(ErrorValue),
    NotAsked(&'static str),
}

/// Asks every connected, followed agent link for its table, side by side, and draws the answers.
///
/// `hop1` pairs each link with the root place `map links` drew for it. `own` is this shell's own
/// link fingerprint, where it has one: a far link back to it is a cycle to this host, not a new
/// place.
pub async fn second_hop(
    center: &SpatialId,
    hop1: &[(LinkFacts, SpatialId)],
    own: Option<&str>,
    now: Timestamp,
) -> Result<Federation, ErrorValue> {
    let mut asked = 0usize;
    let questions = hop1.iter().map(|(link, _)| {
        let prober = link.prober.clone().filter(|_| link.reachable());
        let within_bound = prober.is_some() && {
            asked += 1;
            asked <= MAX_PEERS_ASKED
        };
        async move {
            let Some(prober) = prober else {
                return Told::NotAsked(if link.reachable() {
                    "no agent answers this link (agentless)"
                } else {
                    "the link is not connected and followed"
                });
            };
            if !within_bound {
                return Told::NotAsked("beyond the bound on peers one map asks");
            }
            ask(&prober).await
        }
    });
    let answers = futures::future::join_all(questions).await;

    let mut federation = Federation::default();
    // A far host is one place however many peers report it, when they identify it by the key it
    // proved; an unidentified one stays one place per reporter (§2.17: the false distinction).
    let mut drawn: std::collections::BTreeMap<String, SpatialId> =
        std::collections::BTreeMap::new();
    for ((link, root), told) in hop1.iter().zip(answers) {
        let table = match told {
            Told::Listed(table) => table,
            Told::Refused(error) => {
                federation
                    .report
                    .push(report(link, refusal_status(&error), None, Some(&error)));
                continue;
            }
            Told::NotOffered(error) => {
                federation
                    .report
                    .push(report(link, "not_offered", None, Some(&error)));
                continue;
            }
            Told::NotAsked(reason) => {
                federation
                    .report
                    .push(report_reason(link, "not_asked", reason));
                continue;
            }
        };
        let status = if table.truncated { "partial" } else { "listed" };
        federation
            .report
            .push(report(link, status, Some(table.links.len()), None));
        let reporter_scope = ono_spatial_core::SpatialScope::remote_host(
            &link.name,
            ono_spatial_core::BootIdentity::unknown_boot(&link.name),
        );
        let observed = table.observed_at.unwrap_or(now);
        for entry in &table.links {
            let (target, joins) = place_of(entry, link, hop1, center, own, &mut drawn);
            if let Some(place) = joins {
                federation.places.push(place);
            }
            federation.edges.push(edge(
                root,
                &target,
                entry,
                link,
                &reporter_scope,
                hop1,
                own,
                observed,
                now,
            ));
        }
    }
    Ok(federation)
}

#[cfg(feature = "remote")]
async fn ask(prober: &crate::spatial::links::Prober) -> Told {
    if !prober.offers_links() {
        return Told::NotOffered(ErrorValue::new(
            ono_core::ErrorCode::RemoteProtocolMismatch,
            format!(
                "the far side does not offer `{}`",
                ono_protocol::LINKS_CAPABILITY
            ),
        ));
    }
    match prober.links(LINKS_TIMEOUT).await {
        Ok(table) => Told::Listed(table),
        Err(error) => Told::Refused(error),
    }
}

#[cfg(not(feature = "remote"))]
async fn ask(prober: &crate::spatial::links::Prober) -> Told {
    match *prober {}
}

/// The status a refusal stands for: "not permitted to see" is never "no links".
fn refusal_status(error: &ErrorValue) -> &'static str {
    use ono_core::ErrorCode;
    match error.code() {
        ErrorCode::RemoteCapabilityDenied
        | ErrorCode::RemoteUnauthorized
        | ErrorCode::RemoteUnauthenticated => "not_permitted",
        _ => "unavailable",
    }
}

fn report(
    link: &LinkFacts,
    status: &str,
    count: Option<usize>,
    error: Option<&ErrorValue>,
) -> Value {
    let mut fields = MapValue::new();
    fields.insert("link".into(), Value::string(&link.name));
    fields.insert("status".into(), Value::string(status));
    fields.insert(
        "count".into(),
        count.map_or(Value::Null, |count| {
            Value::Int(i128::try_from(count).unwrap_or(i128::MAX))
        }),
    );
    fields.insert(
        "code".into(),
        error.map_or(Value::Null, |error| Value::string(error.code().name())),
    );
    fields.insert(
        "reason".into(),
        error.map_or(Value::Null, |error| Value::string(error.message())),
    );
    Value::Map(std::sync::Arc::new(fields))
}

fn report_reason(link: &LinkFacts, status: &str, reason: &str) -> Value {
    let mut fields = MapValue::new();
    fields.insert("link".into(), Value::string(&link.name));
    fields.insert("status".into(), Value::string(status));
    fields.insert("count".into(), Value::Null);
    fields.insert("code".into(), Value::Null);
    fields.insert("reason".into(), Value::string(reason));
    Value::Map(std::sync::Arc::new(fields))
}

/// Where a far link leads: back to this host, to a host this session links to itself, to a far
/// host another peer already reported, or to a new place — and the place to draw, if new.
fn place_of(
    entry: &ono_protocol::LinkEntry,
    reporter: &LinkFacts,
    hop1: &[(LinkFacts, SpatialId)],
    center: &SpatialId,
    own: Option<&str>,
    drawn: &mut std::collections::BTreeMap<String, SpatialId>,
) -> (SpatialId, Option<HorizonPlace>) {
    if let Some(proved) = entry.fingerprint.as_deref() {
        if own == Some(proved) {
            return (center.clone(), None);
        }
        if let Some((_, root)) = hop1
            .iter()
            .find(|(link, _)| link.fingerprint.as_deref() == Some(proved))
        {
            return (root.clone(), None);
        }
    }
    let key = entry.fingerprint.clone().map_or_else(
        || format!("{}/{}", reporter.name, entry.name),
        |proved| format!("key:{proved}"),
    );
    if let Some(known) = drawn.get(&key) {
        return (known.clone(), None);
    }
    // A host this session holds no link to is named through the peer that reported it — the
    // only route the session knows — and learning the name stands on nothing (§35.4).
    let name = format!("{}/{}", reporter.name, entry.name);
    let scope = ono_spatial_core::SpatialScope::remote_host(
        &name,
        ono_spatial_core::BootIdentity::unknown_boot(&name),
    );
    ono_spatial_core::space::learn(&scope);
    let root = space::root().spatial_id_in(Some(&scope));
    drawn.insert(key, root.clone());
    let place =
        HorizonPlace::new(root.clone(), 2, None).in_state(Some(entry.state.as_str().to_owned()));
    (root, Some(place))
}

/// One far link as an edge: `inferred` here, the far side's own claim beside it, and who said so.
#[allow(clippy::too_many_arguments)]
fn edge(
    from: &SpatialId,
    to: &SpatialId,
    entry: &ono_protocol::LinkEntry,
    reporter: &LinkFacts,
    reporter_scope: &ono_spatial_core::SpatialScope,
    hop1: &[(LinkFacts, SpatialId)],
    own: Option<&str>,
    observed: Timestamp,
    now: Timestamp,
) -> RelationshipEdge {
    let relation = ono_spatial_core::relation::spec("host.linked_to")
        .map(ono_spatial_core::RelationSpec::relation_type)
        .unwrap_or_else(|| unreachable!("`host.linked_to` is declared in relations.yaml"));
    let claimed = if entry.state == ono_protocol::LinkState::Connected {
        "exact"
    } else {
        "user_declared"
    };
    let text = |value: Option<&String>| value.map_or(Value::Null, |text| Value::string(text));
    let nanos = |value: Option<u64>| {
        value.map_or(Value::Null, |nanos| {
            Value::Duration(ono_value::Duration::from_nanoseconds(i128::from(nanos)))
        })
    };
    let same_peer = entry.fingerprint.as_deref().and_then(|proved| {
        hop1.iter()
            .find(|(link, _)| link.fingerprint.as_deref() == Some(proved))
            .map(|(link, _)| Value::string(&link.name))
    });
    RelationshipEdge::new(
        from.clone(),
        to.clone(),
        relation,
        ono_spatial_core::Confidence::Inferred,
        Provenance::remote(
            REPORTER,
            reporter_scope.host_scope().id(),
            SchemaId::new("ono.spatial-relation", 1),
        )
        .observed_at(observed),
        now,
    )
    .with_attribute("hop", Value::Int(2))
    .with_attribute("reported_by", Value::string(&reporter.name))
    .with_attribute("reported_confidence", Value::string(claimed))
    .with_attribute("verified", Value::Bool(false))
    .with_attribute("far_name", Value::string(&entry.name))
    .with_attribute("far_host", Value::string(&entry.host))
    .with_attribute("transport", Value::string(entry.transport.as_str()))
    .with_attribute("state", Value::string(entry.state.as_str()))
    .with_attribute("fingerprint", text(entry.fingerprint.as_ref()))
    .with_attribute("boot_id", text(entry.boot_id.as_ref()))
    .with_attribute("round_trip", nanos(entry.round_trip_ns))
    .with_attribute("latency", nanos(entry.latency_ns))
    .with_attribute("same_peer_as", same_peer.unwrap_or(Value::Null))
    .with_attribute(
        "back_to_this_host",
        Value::Bool(entry.fingerprint.is_some() && entry.fingerprint.as_deref() == own),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_not_take_a_far_sides_words_for_a_status_this_side_decides() {
        // Review SEC-12: "not offered" is what this side's handshake negotiated. A far side that
        // refuses with a sentence that merely says so is a refusal, not a missing feature.
        let worded = ErrorValue::new(
            ono_core::ErrorCode::RemoteProtocolMismatch,
            "the far side does not offer `ono.link.links/1`",
        )
        .with_metadata("capability", Value::string("ono.link.links/1"));
        assert_eq!(refusal_status(&worded), "unavailable");
        let denied = ErrorValue::new(ono_core::ErrorCode::RemoteCapabilityDenied, "no");
        assert_eq!(refusal_status(&denied), "not_permitted");
    }
}
