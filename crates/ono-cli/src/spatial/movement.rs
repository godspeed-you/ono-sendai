//! Movement through history and hierarchy: `back`, `up`, `jump` and `trail`
//! (spec v0.4 §6.5, §6.6, §6.7, §20, §29.2, §40, §44.6, §46).
//!
//! §53 settles what separates them, and this module is that separation made executable:
//!
//! - **`back` follows history.** It walks the trail the session actually made, in reverse, and
//!   §2.4 makes that reversibility an invariant rather than a convenience. Where the previous
//!   place is gone it is §20.3 that decides what happens, not this module's judgement.
//! - **`up` follows the canonical hierarchy.** Never a relationship edge (§43.2): the canonical
//!   parent is the one `ono-spatial-core` computes from the rule chain of the place's type, which
//!   is why `up` from a socket reaches its network collection and `back` reaches the process that
//!   owns it (§6.6, §44.6).
//! - **`jump` resolves without adjacency**, across scopes, and therefore has to say where it came
//!   from and where it went (§6.5).
//! - **`trail` reads what the other three write** (§6.7).
//!
//! The trail itself lives in [`ono_spatial_core::NavigationTrail`] and the session that owns it in
//! [`crate::spatial::session`]. Both are per process, which is the strongest form of §29.2: a
//! called script cannot change the caller's place because it has no access to it.

use std::collections::BTreeSet;
use std::sync::Arc;

use jiff::Timestamp;
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture};
use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_spatial_core::{
    BackOutcome, Movement, NavigationStep, ScopeBoundary, SpatialId, SpatialScope,
};
use ono_spatial_index::SpatialIndex;
use ono_value::{ErrorValue, Provenance, RecordValue, SchemaId, Value, builtin_schemas};

use crate::spatial::commands::{COMPOSER, resolved_place, text_of, with_pins};
use crate::spatial::session::{SpatialSessionState, spatial_session};

/// Whether the session can still say what a place is (§20.3's "no longer exists").
///
/// A canonical space is declared geography and is always there (§4.1). Anything else is a place
/// exactly as long as the index holds it. This build has no liveness check behind that — the
/// tombstones of §10.3 are a later phase — so it answers what it can actually know, and a `back`
/// onto a place the session never lost sight of returns rather than refusing (ADR-0152).
fn still_a_place(index: &SpatialIndex, id: &SpatialId) -> bool {
    index.contains(id) || ono_spatial_query::resolve::space_of(id).is_some()
}

/// `back` — the movement that follows navigation history (spec v0.4 §6.6, §20.3, §2.4).
#[derive(Debug)]
pub struct Back;

impl CommandImpl for Back {
    fn id(&self) -> &str {
        "ono.place.back"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("back"))
    }

    fn invoke_async<'a>(&'a self, _ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let now = Timestamp::now();
            let mut session = spatial_session().await;

            // Which of the places behind us are still places is decided before the trail is
            // touched, so the walk itself never reaches into the index it is being read against.
            let alive: BTreeSet<SpatialId> = session
                .trail()
                .history()
                .filter(|id| still_a_place(session.index(), id))
                .cloned()
                .collect();
            match session.trail_mut().back(now, |id| alive.contains(id)) {
                BackOutcome::Returned { .. } => {}
                BackOutcome::Skipped { skipped, .. } => {
                    // §20.3 step 2: skip to the nearest valid previous place "only after
                    // informing the user". The movement succeeded, so this is a notice on stderr
                    // rather than a failure — a script that navigates must not die because a
                    // process it visited has since exited.
                    eprintln!(
                        "{}: {} place{} on the trail no longer exist{}; went back past {}",
                        ono_core::SHORT_NAME,
                        skipped.len(),
                        if skipped.len() == 1 { "" } else { "s" },
                        if skipped.len() == 1 { "s" } else { "" },
                        skipped
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                BackOutcome::AllGone { skipped } => {
                    return Err(ErrorValue::new(
                        ErrorCode::SpatialDestinationGone,
                        format!(
                            "every place behind this one is gone: {}",
                            skipped
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )
                    .with_help(
                        "the trail keeps the record of where they were; `home` returns to the \
                         root of this host (spec v0.4 §20.3, §6.6)",
                    ));
                }
                BackOutcome::Empty => {
                    return Err(ErrorValue::new(
                        ErrorCode::SpatialHistoryEmpty,
                        "this session has not moved yet, so there is nowhere to go back to",
                    )
                    .with_help(
                        "`up` moves to the canonical parent instead, which is a different \
                         question (spec v0.4 §6.6, §40)",
                    ));
                }
            }
            Ok(Outcome::Values(ValueStream::from_values(Vec::new())))
        })
    }
}

/// `up` — the movement that follows the canonical hierarchy (spec v0.4 §6.6, §11.3, §43.2).
#[derive(Debug)]
pub struct Up;

impl CommandImpl for Up {
    fn id(&self) -> &str {
        "ono.place.up"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("up"))
    }

    fn invoke_async<'a>(&'a self, _ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let now = Timestamp::now();
            let mut session = spatial_session().await;
            let here = session.current_place().clone();

            // §11.3: one canonical parent, deterministic, computed from the place's own rule
            // chain — the same answer the place view already declares under `canonical_parent`,
            // because asking twice is how a hierarchy stops being one.
            let Some(there) = ono_spatial_query::resolve::parent_of(session.index(), &here) else {
                return Err(ErrorValue::new(
                    ErrorCode::SpatialNoParent,
                    "this place is the top of the canonical hierarchy of this host",
                )
                .with_help(
                    "`back` returns through navigation history instead, which is a different \
                     question (spec v0.4 §6.6, §7.1, §40)",
                ));
            };
            let crossing = crossing_between(&session, &here, &there);
            let mut step = NavigationStep::new(now, here, there, Movement::Up);
            if let Some(crossing) = crossing {
                step = step.crossing(crossing);
            }
            session.trail_mut().record(step);
            Ok(Outcome::Values(ValueStream::from_values(Vec::new())))
        })
    }
}

/// `jump <selector>` — movement without adjacency (spec v0.4 §6.5, §20.1, §20.4).
#[derive(Debug)]
pub struct Jump {
    pins: Option<crate::spatial::PinStore>,
}

impl Jump {
    /// The implementation registered against `ono.place.jump`.
    #[must_use]
    pub fn new(pins: Option<crate::spatial::PinStore>) -> Self {
        Self { pins }
    }
}

impl CommandImpl for Jump {
    fn id(&self) -> &str {
        "ono.place.jump"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("jump"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let selector = ctx
                .arguments()
                .selector("selector")
                .and_then(text_of)
                .ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::SpatialNotFound,
                        "`jump` needs a place to move to",
                    )
                    .with_help(
                        "`find place <text>` finds the places this shell can reach, and \
                             `jump @<pin>` reaches a pinned one (spec v0.4 §6.5, §20.4)",
                    )
                })?;
            let now = Timestamp::now();

            let mut session = spatial_session().await;
            with_pins(&mut session, self.pins.as_ref(), now).await?;
            let here = session.current_place().clone();

            // §20.4's `jump @edge-proxy`: the pin was resolved when the store was read, so the
            // bookmark names a place rather than a spelling to guess at again.
            let there = if let Some(name) = selector.strip_prefix('@') {
                pinned_place(&session, name)?
            } else {
                resolved_place(ctx, &mut session, &here, &selector, now).await?
            };

            if here == there {
                return Ok(Outcome::Values(ValueStream::from_values(Vec::new())));
            }
            // §6.5: "MUST visibly record the source and destination in the trail" — and §3.2 adds
            // the boundary, where the teleport crossed one.
            let crossing = crossing_between(&session, &here, &there);
            let mut step = NavigationStep::new(now, here, there, Movement::Jump);
            if let Some(crossing) = crossing {
                step = step.crossing(crossing);
            }
            session.trail_mut().record(step);
            Ok(Outcome::Values(ValueStream::from_values(Vec::new())))
        })
    }
}

/// The place a `@<bookmark>` names (§20.4).
///
/// # Errors
///
/// `spatial.not_found` when no pin carries the name; `spatial.destination_gone` when the pin is
/// there but nothing answers for it any more — §20.4 keeps the pin and reports the state rather
/// than deleting what the user chose.
fn pinned_place(session: &SpatialSessionState, name: &str) -> Result<SpatialId, ErrorValue> {
    let Some(pin) = session.pins().get(name) else {
        let known: Vec<&str> = session
            .pins()
            .pins()
            .map(ono_spatial_index::Pin::name)
            .collect();
        return Err(ErrorValue::new(
            ErrorCode::SpatialNotFound,
            format!("no pin is called `{name}`"),
        )
        .with_help(if known.is_empty() {
            "`pin --name <name>` marks the current place (spec v0.4 §20.4)".to_owned()
        } else {
            format!("this session's pins are {}", known.join(", "))
        }));
    };
    let id = pin.spatial_id().clone();
    if still_a_place(session.index(), &id) {
        return Ok(id);
    }
    Err(ErrorValue::new(
        ErrorCode::SpatialDestinationGone,
        format!(
            "the pin `{name}` marks `{}`, and nothing answers for it now",
            pin.selector()
        ),
    )
    .with_help("the pin is kept; `unpin` removes it (spec v0.4 §20.4)"))
}

/// The scope boundary a movement between two places crosses, where it crosses one (§3.2, §2.18).
fn crossing_between(
    session: &SpatialSessionState,
    from: &SpatialId,
    to: &SpatialId,
) -> Option<ScopeBoundary> {
    let here = scope_of(session, from);
    let there = scope_of(session, to);
    here.boundary_to(&there)
}

/// The scope a place belongs to: its own where it was observed, the session's where it is
/// declared geography (§3.2, §4.1).
fn scope_of(session: &SpatialSessionState, id: &SpatialId) -> SpatialScope {
    session.index().get(id).map_or_else(
        || session.scope().clone(),
        |entry| entry.object().scope().clone(),
    )
}

/// `trail` — the navigation history as §20.1 records it (spec v0.4 §6.7, §29.1).
#[derive(Debug)]
pub struct Trail;

impl CommandImpl for Trail {
    fn id(&self) -> &str {
        "ono.place.trail"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("trail"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments();
            let json = arguments.flag("json");
            let compact = arguments.flag("compact");
            let session = spatial_session().await;

            if compact {
                // §20.2: "full breadcrumbs MAY occupy a status line or be shown by `trail`". The
                // breadcrumb is the canonical hierarchy path of where the session is standing —
                // orientation, not history, which the steps below carry in full.
                return Ok(Outcome::Values(ValueStream::from_values(vec![
                    Value::string(&breadcrumb(&session)),
                ])));
            }

            let mut rows = Vec::with_capacity(session.trail().steps().len());
            for step in session.trail().steps() {
                rows.push(Value::Record(Arc::new(step_record(&session, step)?)));
            }
            if json {
                let document = ono_value::to_json_data(&Value::list(rows));
                let text = serde_json::to_string(&document).map_err(|error| {
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("the trail could not be written as JSON: {error}"),
                    )
                })?;
                return Ok(Outcome::Values(ValueStream::from_values(vec![
                    Value::string(&text),
                ])));
            }
            Ok(Outcome::Values(ValueStream::from_values(rows)))
        })
    }
}

/// The breadcrumb of §20.2: the hierarchy path of the current place, host first.
fn breadcrumb(session: &SpatialSessionState) -> String {
    let here = session.current_place();
    let path = ono_spatial_query::place_path(session.index(), here);
    let mut parts: Vec<String> = path.split('/').map(str::to_owned).collect();
    if let Some(name) = display_name(session, here) {
        // The path names the place's parent chain (§27.2); the place itself closes the trail.
        if parts.last() != Some(&name) {
            parts.push(name);
        }
    }
    parts.join(" > ")
}

/// One `ono.navigation-step/1` record (§20.1, ADR-0150).
fn step_record(
    session: &SpatialSessionState,
    step: &NavigationStep,
) -> Result<RecordValue, ErrorValue> {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.navigation-step", 1))
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                "the `ono.navigation-step/1` contract is not in this build",
            )
        })?;
    let relation = step
        .word()
        .map(str::to_owned)
        .or_else(|| step.relation().map(ToString::to_string));
    Ok(RecordValue::builder(
        schema,
        Provenance::local(COMPOSER, SchemaId::new("ono.navigation-step", 1)),
    )
    .set("timestamp", Value::Timestamp(step.timestamp()))?
    .set("movement", Value::string(step.movement().as_str()))?
    .set("from", Value::string(&step.from().to_string()))?
    .set("to", Value::string(&step.to().to_string()))?
    .set("from_ref", optional(canonical_ref(session, step.from())))?
    .set("to_ref", optional(canonical_ref(session, step.to())))?
    .set("from_name", optional(display_name(session, step.from())))?
    .set("to_name", optional(display_name(session, step.to())))?
    .set("relation", optional(relation))?
    .set(
        "relation_id",
        optional(step.relation().map(ToString::to_string)),
    )?
    .set(
        "scope_crossing",
        step.scope_crossing().map_or(Value::Null, crossing_record),
    )?
    .set("host", Value::string(session.scope().host_scope().id()))?
    .build())
}

fn optional(text: Option<String>) -> Value {
    text.map_or(Value::Null, |text| Value::string(&text))
}

/// The `<type>/<key>` spelling of §11.2 — the same one `enter` and `jump` take back.
fn canonical_ref(session: &SpatialSessionState, id: &SpatialId) -> Option<String> {
    if let Some(space) = ono_spatial_query::resolve::space_of(id) {
        return Some(space.id.to_owned());
    }
    let entry = session.index().get(id)?;
    let kind = entry.object().object_type().as_str().to_ascii_lowercase();
    // The first identity field is the key a person types: a process's pid, a socket's inode, a
    // mount's target. The rest of the identity is what makes the id unique and lives in `from`
    // and `to`, which is where a reader who needs certainty looks (§3.1, §10.2).
    let key = entry
        .canonical_ref()
        .id()
        .values()
        .first()
        .and_then(|value| ono_value::canonical_text(value).ok())?;
    Some(format!("{kind}/{key}"))
}

/// What a person calls the place, where this session still knows it.
fn display_name(session: &SpatialSessionState, id: &SpatialId) -> Option<String> {
    if let Some(space) = ono_spatial_query::resolve::space_of(id) {
        return Some(space.label.to_owned());
    }
    session
        .index()
        .get(id)
        .map(|entry| entry.object().display_name().to_owned())
}

/// The crossing, as a reader of the trail sees it (§3.2, §2.18).
fn crossing_record(boundary: &ScopeBoundary) -> Value {
    let mut map = ono_value::MapValue::new();
    map.insert("kind".into(), Value::string(boundary.kind().as_str()));
    map.insert("from".into(), Value::string(&boundary.from().to_string()));
    map.insert("to".into(), Value::string(&boundary.to().to_string()));
    map.insert("entering".into(), Value::Bool(boundary.is_entering()));
    map.insert("remote".into(), Value::Bool(boundary.is_remote()));
    Value::Map(Arc::new(map))
}
