//! `map` — the bounded, ranked projection of the graph around the current place
//! (spec v0.4 §6.9, §22, §8, §23, §29.4, §45.6).
//!
//! The command is deliberately thin, exactly as §45.6 requires: it reads the arguments, asks the
//! providers for the places around the current one — which nothing else may do (§2.16) — hands
//! that horizon to `ono-spatial-query` to be ranked, zoomed, bounded and clustered (§45.3), and
//! turns the answer into the `ono.spatial-map/1` record of §22. It decides no layout: the text
//! map is `ono-spatial-render`'s, and `map --json` has no layout at all, because §22 forbids
//! screen coordinates in the semantic contract outright.
//!
//! Two arguments look like navigation and are not (§23.4, §8.3, §53):
//!
//! - `--focus <node>` centres the *view* on a node. The current place does not move, and the map
//!   says so by carrying `center` and `focus` as two separate fields.
//! - `--expand <cluster>` draws the objects a cluster stood for. It is a view action; `enter` is
//!   navigation, and this is not `enter`.

use std::sync::Arc;

use jiff::Timestamp;
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture};
use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_spatial_core::{CanonicalSpace, HierarchyKind, SpatialId, space};
use ono_spatial_index::SpatialIndex;
use ono_spatial_query::{
    HorizonPlace, MapCluster, MapEdge, MapHorizon, MapNode, MapRequest, SpatialMap,
};
use ono_value::{
    ErrorValue, MapValue, Provenance, RecordValue, SchemaId, Uuid, Value, builtin_schemas,
};
use sha2::{Digest, Sha256};

use crate::spatial::session::{SpatialSessionState, spatial_session};
use crate::spatial::view;

/// The provider id the spatial layer signs its own composition with (ADR-0140).
const COMPOSER: &str = "ono.spatial";

/// `map` (spec v0.4 §6.9, §22, §23).
#[derive(Debug)]
pub struct Map {
    pins: Option<crate::spatial::PinStore>,
}

impl Map {
    /// The implementation registered against `ono.place.map`.
    #[must_use]
    pub fn new(pins: Option<crate::spatial::PinStore>) -> Self {
        Self { pins }
    }
}

impl CommandImpl for Map {
    fn id(&self) -> &str {
        "ono.place.map"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("map"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments();
            let json = arguments.flag("json");
            let mut request = MapRequest::new().all(arguments.flag("all"));
            if let Some(level) = integer(arguments.option("zoom")) {
                request = request.zoom(
                    u8::try_from(level.clamp(0, i128::from(u8::MAX)))
                        .unwrap_or(ono_spatial_query::map::MAX_ZOOM),
                );
            }
            if let Some(depth) = integer(arguments.option("depth")) {
                request = request.depth(usize::try_from(depth.max(0)).unwrap_or(usize::MAX));
            }
            if let Some(relations) = words(arguments.option("relations")) {
                request = request.relations(relations);
            }
            if let Some(types) = words(arguments.option("type")) {
                let mut wanted = Vec::with_capacity(types.len());
                for name in &types {
                    wanted.push(crate::spatial::spatial_type(&Value::string(name))?);
                }
                request = request.types(wanted);
            }
            if let Some(clusters) = words(arguments.option("expand")) {
                request = request.expand(clusters);
            }
            if let Some(node) = arguments.option("focus").and_then(text_of) {
                request = request.focus(node);
            }
            let selector = arguments.selector("selector").and_then(text_of);
            let now = Timestamp::now();

            let mut session = spatial_session().await;
            crate::spatial::commands::with_pins(&mut session, self.pins.as_ref(), now).await?;

            // §6.9's `map <selector>` maps another place. It is a view, not a movement: the
            // session's current place is untouched, exactly as `--focus` is (§23.4).
            let center = match selector {
                Some(selector) => {
                    let here = session.current_place().clone();
                    crate::spatial::commands::resolved_place(
                        ctx,
                        &mut session,
                        &here,
                        &selector,
                        now,
                    )
                    .await?
                }
                None => session.current_place().clone(),
            };

            // §23.3, §52.1: at an interactive terminal `map` may open a full-screen view. It
            // is the same projection with a viewport and a cursor around it, never a second
            // selection (§45.4) — and never where the values are about to be consumed by
            // another stage, redirected or read by a script (§29.1).
            let live = arguments.flag("live") || crate::spatial::interactive::live_by_default();
            if !json && crate::spatial::interactive::may_open(ctx) {
                crate::spatial::interactive::run_map_view(
                    ctx,
                    &mut session,
                    self.pins.as_ref(),
                    center,
                    request,
                    live,
                    now,
                )
                .await?;
                return Ok(Outcome::Values(ValueStream::from_values(Vec::new())));
            }
            if live {
                return Err(ErrorValue::new(
                    ErrorCode::SpatialUnsupported,
                    "`map --live` needs an interactive terminal to draw into",
                )
                .with_help(
                    "`map --json` answers the same graph once, which is what a script asks for \
                     (spec v0.4 §25.1, §29.1)",
                ));
            }

            let record = projection(ctx, &mut session, &center, &request, now).await?;

            if json {
                let document = ono_value::to_json_data(&Value::Record(Arc::new(record)));
                let text = serde_json::to_string(&document).map_err(|error| {
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("the map could not be written as JSON: {error}"),
                    )
                })?;
                return Ok(Outcome::Values(ValueStream::from_values(vec![
                    Value::string(&text),
                ])));
            }
            Ok(Outcome::Values(ValueStream::from_values(vec![
                Value::Record(Arc::new(record)),
            ])))
        })
    }
}

/// One `ono.spatial-map/1`: the horizon the providers answer for, ranked and bounded (§22, §45.3).
///
/// The whole of `map`'s work, as one function, because the full-screen view of §23.3 redraws the
/// same projection every time the place, the zoom or the live tick changes — and a second way of
/// building it would be a second answer to what the system looks like (§49.5).
///
/// # Errors
///
/// Whatever the providers refused with while the horizon was being observed.
pub async fn projection(
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    center: &SpatialId,
    request: &MapRequest,
    now: Timestamp,
) -> Result<RecordValue, ErrorValue> {
    let horizon = observe(ctx, session, center, request, now).await?;
    let pins = session.pins().clone();
    let map = ono_spatial_query::project_map(
        session.index(),
        center,
        &horizon,
        request,
        &pins,
        session.preferences().map_node_budget,
        now,
    );
    map_record(session, &map)
}

/// The places and edges around `center`, as the providers answer for them (§2.16, §45.6).
///
/// A canonical space is declared geography whose exits the shell reads; an observed object is a
/// node of the relationship graph the v0.2 providers already assert (§31.3). Both give the same
/// shape: places at a hierarchy depth, and the edges between them.
async fn observe(
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    center: &SpatialId,
    request: &MapRequest,
    now: Timestamp,
) -> Result<MapHorizon, ErrorValue> {
    let mut horizon = MapHorizon::new();
    horizon.place(place_at(session, center, 0, None));

    if let Some(space) = ono_spatial_query::resolve::space_of(center) {
        observe_space(ctx, session, space, center, request, &mut horizon, now).await?;
    } else {
        observe_object(ctx, session, center, request, &mut horizon, now).await?;
    }
    Ok(horizon)
}

/// The horizon of a canonical space: its served children, and what lies inside each of them.
async fn observe_space(
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    here: &'static CanonicalSpace,
    center: &SpatialId,
    request: &MapRequest,
    horizon: &mut MapHorizon,
    now: Timestamp,
) -> Result<(), ErrorValue> {
    let surroundings =
        view::observe_space(ctx, session, here, request.horizon_depth() > 1, now).await?;
    for exit in surroundings.exits() {
        // §24.2 makes a group label the word `enter` takes, so an exit named after a child space
        // *is* that child: the child is one hop away and its contents are two. An exit that is
        // the place's own contents has no such intermediate place, and its members are one hop.
        let child = space::children(here.id)
            .find(|child| child.is_served() && child.label == exit.label())
            .map(|child| child.spatial_id());
        let (parent, member_depth) = match &child {
            Some(child) => {
                horizon.place(place_at(
                    session,
                    child,
                    1,
                    Some((center.clone(), HierarchyKind::Grouping)),
                ));
                (child.clone(), 2)
            }
            None => (center.clone(), 1),
        };
        for member in exit.members() {
            horizon.place(place_at(
                session,
                member,
                member_depth,
                Some((parent.clone(), HierarchyKind::Grouping)),
            ));
        }
    }
    Ok(())
}

/// The horizon of an observed object: the place it is filed under, and the relationships the
/// providers assert about it (§3.5, §31.3).
async fn observe_object(
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    center: &SpatialId,
    request: &MapRequest,
    horizon: &mut MapHorizon,
    now: Timestamp,
) -> Result<(), ErrorValue> {
    let interest =
        crate::spatial::relations::Interest::here().complete(request.horizon_depth() > 1);
    crate::spatial::relations::observe(ctx, session, center, &interest, now).await?;

    if let Some(parent) = ono_spatial_query::resolve::parent_of(session.index(), center) {
        horizon.place(place_at(session, &parent, 1, None));
        horizon.place(place_at(
            session,
            center,
            0,
            Some((parent, HierarchyKind::Grouping)),
        ));
    }
    let edges: Vec<ono_spatial_core::RelationshipEdge> = session
        .index()
        .get(center)
        .map(|entry| entry.edges().to_vec())
        .unwrap_or_default();
    for edge in edges {
        if let Some(other) = edge.other_end(center) {
            horizon.place(place_at(session, &other.clone(), 1, None));
        }
        horizon.edge(edge);
    }
    Ok(())
}

/// One horizon place, carrying the state its provider last reported (§22's `MapNode.state`).
fn place_at(
    session: &SpatialSessionState,
    id: &SpatialId,
    depth: usize,
    parent: Option<(SpatialId, HierarchyKind)>,
) -> HorizonPlace {
    let state = session.record_of(id).and_then(|record| {
        record
            .get("state")
            .and_then(|value| value.as_str().ok())
            .map(str::to_owned)
    });
    HorizonPlace::new(id.clone(), depth, parent).in_state(state)
}

/// The `ono.spatial-map/1` record of §22.
fn map_record(session: &SpatialSessionState, map: &SpatialMap) -> Result<RecordValue, ErrorValue> {
    let index = session.index();
    let mut nodes = Vec::with_capacity(map.nodes.len());
    for node in &map.nodes {
        nodes.push(Value::Record(Arc::new(node_record(index, node)?)));
    }
    let mut edges = Vec::with_capacity(map.edges.len());
    for edge in &map.edges {
        edges.push(Value::Record(Arc::new(edge_record(edge)?)));
    }
    let mut clusters = Vec::with_capacity(map.clusters.len());
    for cluster in &map.clusters {
        clusters.push(Value::Record(Arc::new(cluster_record(cluster)?)));
    }
    let mut landmarks = Vec::with_capacity(map.landmarks.len());
    for landmark in &map.landmarks {
        landmarks.push(Value::Record(Arc::new(view::landmark_record(
            index, landmark,
        )?)));
    }

    Ok(RecordValue::builder(
        schema("ono.spatial-map", 1)?,
        Provenance::local(COMPOSER, SchemaId::new("ono.spatial-map", 1)),
    )
    .set("map_id", Value::Uuid(map_id(map)))?
    .set("center", Value::string(&map.center.to_string()))?
    .set(
        "focus",
        map.focus
            .as_ref()
            .map_or(Value::Null, |node| Value::string(&node.to_string())),
    )?
    .set("scope", Value::string(&session.scope().to_string()))?
    .set("zoom_level", Value::Int(i128::from(map.zoom_level)))?
    .set("nodes", Value::list(nodes))?
    .set("edges", Value::list(edges))?
    .set("clusters", Value::list(clusters))?
    .set("landmarks", Value::list(landmarks))?
    .set("hidden", Value::Record(Arc::new(hidden_record(map)?)))?
    .set("generated_at", Value::Timestamp(map.generated_at))?
    .set("completeness", Value::string(map.completeness.as_str()))?
    // §25.1's live map subscribes to provider change events. This build polls, and saying
    // otherwise would promise a liveness nothing delivers (§2.17).
    .set("live_capable", Value::Bool(false))?
    .build())
}

/// One `ono.map-node/1` (§22's `MapNode`).
fn node_record(index: &SpatialIndex, node: &MapNode) -> Result<RecordValue, ErrorValue> {
    let object_ref = match node.space {
        // A canonical space is declared, not observed: the thing that stands for the object is
        // the registry entry, and naming it is what makes the geography checkable (§41.1, §4.1).
        Some(id) => {
            let mut reference = MapValue::new();
            reference.insert("schema".into(), Value::string(place_schema(id)));
            reference.insert("space".into(), Value::string(id));
            Value::Map(Arc::new(reference))
        }
        None => index.get(&node.id).map_or(Value::Null, |entry| {
            view::reference_record(entry.canonical_ref())
        }),
    };
    let reasons: Vec<Value> = node
        .landmark_reasons
        .iter()
        .map(|reason| Value::string(reason.as_str()))
        .collect();
    Ok(RecordValue::builder(
        schema("ono.map-node", 1)?,
        Provenance::local(COMPOSER, SchemaId::new("ono.map-node", 1)),
    )
    .set("id", Value::string(&node.id.to_string()))?
    .set("object_ref", object_ref)?
    .set("space", node.space.map_or(Value::Null, Value::string))?
    .set("label", Value::string(&node.label))?
    .set("type", Value::string(node.object_type.as_str()))?
    .set(
        "state",
        node.state.as_deref().map_or(Value::Null, Value::string),
    )?
    .set(
        "canonical_parent",
        node.canonical_parent
            .as_ref()
            .map_or(Value::Null, |parent| Value::string(&parent.to_string())),
    )?
    .set("landmark_reasons", Value::list(reasons))?
    .set("depth", Value::Int(i128::try_from(node.depth).unwrap_or(0)))?
    .build())
}

/// One `ono.map-edge/1`, carrying everything §11.4 makes inspectable.
fn edge_record(edge: &MapEdge) -> Result<RecordValue, ErrorValue> {
    Ok(RecordValue::builder(
        schema("ono.map-edge", 1)?,
        Provenance::local(COMPOSER, SchemaId::new("ono.map-edge", 1)),
    )
    .set("id", Value::string(&edge.id))?
    .set("source", Value::string(&edge.source))?
    .set("source_label", Value::string(&edge.source_label))?
    .set("target", Value::string(&edge.target))?
    .set("target_label", Value::string(&edge.target_label))?
    .set("relation", Value::string(&edge.relation))?
    .set("kind", Value::string(edge.kind.as_str()))?
    .set("confidence", Value::string(edge.confidence.as_str()))?
    .set("direction", Value::string(edge.direction.as_str()))?
    .set("provider", Value::string(edge.provenance.provider()))?
    .set(
        "provenance",
        ono_command::provenance_value(&edge.provenance),
    )?
    .set(
        "observed_at",
        edge.observed_at.map_or(Value::Null, Value::Timestamp),
    )?
    // §24.3: no change summary is invented where no event source or snapshot exists.
    .set("changed", Value::Null)?
    .build())
}

/// One `ono.map-cluster/1` (§8.2).
fn cluster_record(cluster: &MapCluster) -> Result<RecordValue, ErrorValue> {
    Ok(RecordValue::builder(
        schema("ono.map-cluster", 1)?,
        Provenance::local(COMPOSER, SchemaId::new("ono.map-cluster", 1)),
    )
    .set("id", Value::string(&cluster.id))?
    .set("label", Value::string(&cluster.label))?
    .set(
        "members",
        Value::Int(i128::try_from(cluster.members()).unwrap_or(i128::MAX)),
    )?
    .set("grouping", Value::string(cluster.grouping))?
    .set("expandable", Value::Bool(cluster.expandable()))?
    .build())
}

/// The `ono.hidden-summary/1` of §23.6.
fn hidden_record(map: &SpatialMap) -> Result<RecordValue, ErrorValue> {
    let count = |value: usize| Value::Int(i128::try_from(value).unwrap_or(i128::MAX));
    Ok(RecordValue::builder(
        schema("ono.hidden-summary", 1)?,
        Provenance::local(COMPOSER, SchemaId::new("ono.hidden-summary", 1)),
    )
    .set("count", count(map.hidden.count))?
    .set("clustered", count(map.hidden.clustered))?
    .set("aggregated", count(map.hidden.aggregated))?
    .build())
}

/// This projection's identity (§22's `map_id: Uuid`).
///
/// It is derived from what the map *is* — where it is centred, how far it is zoomed, when it was
/// made and which places it drew — rather than drawn from a random source, so two identical
/// projections carry one id and a changed projection carries another. The version and variant
/// bits are set as RFC 4122 requires of a UUID built from a hash.
fn map_id(map: &SpatialMap) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(map.center.as_str().as_bytes());
    hasher.update(map.zoom_level.to_be_bytes());
    hasher.update(map.generated_at.as_nanosecond().to_be_bytes());
    for node in &map.nodes {
        hasher.update([0x1f]);
        hasher.update(node.id.as_str().as_bytes());
    }
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// The v0.2 schema of the object a canonical space stands for (§41.1).
fn place_schema(space_id: &str) -> &'static str {
    space::space(space_id).map_or("ono.spatial-place/1", CanonicalSpace::place_schema)
}

fn schema(name: &str, version: u32) -> Result<Arc<ono_value::Schema>, ErrorValue> {
    let id = SchemaId::new(name, version);
    builtin_schemas().get(&id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("the `{id}` contract is not in this build"),
        )
    })
}

fn integer(value: Option<&Value>) -> Option<i128> {
    match value {
        Some(Value::Int(number)) => Some(*number),
        _ => None,
    }
}

/// A `--relations a,b` or `--type process,service` list, as §6.9 spells it.
fn words(value: Option<&Value>) -> Option<Vec<String>> {
    let text = text_of(value?)?;
    let words: Vec<String> = text
        .split(',')
        .map(|word| word.trim().to_owned())
        .filter(|word| !word.is_empty())
        .collect();
    (!words.is_empty()).then_some(words)
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Null | Value::Bool(_) => None,
        other => ono_value::canonical_text(other)
            .ok()
            .filter(|text| !text.is_empty()),
    }
}
