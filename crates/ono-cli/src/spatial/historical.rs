//! The world as it was: a spatial index built from evidence about one instant, and nothing else
//! (spec v0.5 §9.7, §14.1, §14.3, §14.4, §14.5, §55.2, §55.9).
//!
//! §14.1 makes `look`, `near`, `find place`, `map`, `enter`, `follow`, `jump`, `up` and `home`
//! evaluate against historical state where evidence permits. They already do all of their work
//! against a [`SpatialIndex`], a [`MapHorizon`] and an instant, and `ono-spatial-query`'s
//! selection, ranking and projection are pure functions of exactly those. So the historical half
//! of v0.5 is not a second spatial layer: it is a second index, and the same functions answer
//! about it.
//!
//! # Why present state cannot leak into it
//!
//! §14.3 forbids a current-only exit from appearing in a historical neighbourhood and §55.2
//! prohibits "rendering today's graph with an old timestamp". That is a property of this type
//! rather than a filter applied after the fact:
//!
//! - [`HistoricalWorld::reconstruct`] is the only constructor, and its inputs are a
//!   [`LedgerRead`], a scope and an instant. There is no argument, field or method through which
//!   a live [`SpatialIndex`], a [`crate::spatial::SpatialSessionState`] or a provider registry
//!   could reach it, so the index cannot contain something no evidence put there.
//! - The objects come from [`ReconstructedWorld::objects`] filtered on
//!   [`Presence::Present`], which is reconstruction's own answer about the instant.
//! - The edges come from [`ReconstructedWorld::relations`], which yields only edges supported at
//!   the instant — §14.3's rule is an API property of the reconstruction crate, and this module
//!   re-derives nothing.
//! - An edge is recorded only where **both** ends are places this world holds, so a historical
//!   exit always leads to a historical place.
//! - [`HistoricalWorld::index`] hands out a shared reference. Nothing can absorb into it after
//!   construction.
//!
//! The one place a present-day fact is admitted is [`HistoricalWorld::find`], because §14.4 asks
//! for it — and it arrives marked, as a `ResolutionBasis::ResolutionAid` with no anchor and no
//! instant, which is the distinction §14.4 makes a MUST.
//!
//! # Where the coordinate comes from
//!
//! This module holds no temporal state. §55.7 keeps temporal logic out of `ono-cli` and §4 gives
//! a session exactly one coordinate; the shell's temporal session owns it and installs itself
//! here once through [`install`], so the spatial commands read one coordinate rather than
//! keeping a second.

use std::sync::{Arc, OnceLock};

use jiff::{Span, Timestamp};
use ono_core::ErrorCode;
use ono_spatial_core::{
    HierarchyKind, Neighborhood, RelationType, RelationshipEdge, SpatialId, SpatialObject,
    SpatialScope, SpatialType, ValidityWindow, space,
};
use ono_spatial_index::{FreshnessPolicy, PinRegistry, SpatialIndex};
use ono_spatial_query::{
    HorizonPlace, MapHorizon, MapRequest, NeighborhoodRequest, Resolution, SelectorContext,
    SpatialMap,
};
use ono_temporal_core::{CoverageSummary, LedgerRead, TemporalGap};
use ono_temporal_query::search::{HistoricalPlaceMatch, PresentAliases, find_place_at};
use ono_temporal_reconstruct::{
    Presence, ReconstructedWorld, ReconstructionRequest, Reconstructor, SourceMatrix,
};
use ono_value::{ErrorValue, Provenance, RecordValue, SchemaId, Value};

/// How the spatial layer reaches the session's active temporal coordinate (§4, §14.1).
///
/// The shell's temporal session implements it and installs itself once. Nothing here holds a
/// coordinate of its own: §4 gives a session one, and a spatial layer that kept a second would
/// be able to disagree with the prompt about what time it is.
pub trait TemporalEvidence: Send + Sync + std::fmt::Debug {
    /// The instant every spatial command evaluates at, or `None` in the present (§4.1).
    fn coordinate(&self) -> Option<Timestamp>;

    /// The ledger a historical world is reconstructed from (§3.1).
    fn ledger(&self) -> Arc<dyn LedgerRead>;
}

static EVIDENCE: OnceLock<Arc<dyn TemporalEvidence>> = OnceLock::new();

/// Installs the session's temporal evidence, once per process.
///
/// Returns whether this call is the one that installed it. A second call changes nothing, which
/// is what keeps one session to one coordinate.
pub fn install(evidence: Arc<dyn TemporalEvidence>) -> bool {
    EVIDENCE.set(evidence).is_ok()
}

/// The active coordinate and the evidence behind it, or `None` in the present (§4.1).
///
/// `None` is the present, and the present is what every v0.4 spatial command already answers
/// about. A session with no temporal evidence installed is in the present by construction.
#[must_use]
pub fn active() -> Option<Active> {
    let evidence = EVIDENCE.get()?;
    let at = evidence.coordinate()?;
    Some(Active {
        at,
        ledger: evidence.ledger(),
    })
}

/// The ledger this session records into and reads history from, where one is installed.
///
/// It is the same ledger in the present and in the past: §13.5's change engine reads it while the
/// session stands in the present, and §9.1's reconstruction reads it while the session stands in
/// the past. A session with none has no v0.5 evidence at all, and the commands say so rather than
/// answering an empty history (§2.17).
#[must_use]
pub fn ledger() -> Option<Arc<dyn LedgerRead>> {
    EVIDENCE.get().map(|evidence| evidence.ledger())
}

/// The session's historical coordinate, and the ledger that can answer about it.
#[derive(Debug, Clone)]
pub struct Active {
    at: Timestamp,
    ledger: Arc<dyn LedgerRead>,
}

impl Active {
    /// The instant the session is standing at (§4.2).
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }

    /// The ledger behind it.
    #[must_use]
    pub fn ledger(&self) -> &dyn LedgerRead {
        self.ledger.as_ref()
    }

    /// The ledger behind it, as a handle something long-lived can keep.
    #[must_use]
    pub fn ledger_handle(&self) -> Arc<dyn LedgerRead> {
        Arc::clone(&self.ledger)
    }

    /// The world of `scope` at this coordinate.
    ///
    /// # Errors
    ///
    /// Whatever §34 refusal the ledger raises.
    pub fn world(&self, scope: &SpatialScope) -> Result<HistoricalWorld, ErrorValue> {
        HistoricalWorld::reconstruct(self.ledger.as_ref(), scope, self.at)
    }
}

/// The provider id the historical spatial layer signs its own composition with.
const COMPOSER: &str = "ono.temporal";

/// How long a historical entry stays fresh.
///
/// A reconstructed observation is as old as the instant it was made at, and the instant the index
/// is read at is that same instant — so any policy that measured age against a wall clock would
/// call the whole world stale. The span is wide enough that nothing reconstructed is ever stale
/// relative to the coordinate it was reconstructed for, which is the honest reading: the past
/// does not go out of date (ADR-0681).
const HISTORICAL_TTL_DAYS: i64 = 365 * 100;

/// The world of one scope at one instant (§9.1, §14.1).
#[derive(Debug)]
pub struct HistoricalWorld {
    at: Timestamp,
    scope: SpatialScope,
    index: SpatialIndex,
    world: ReconstructedWorld,
    /// Empty. A pin is a present-day convenience the ranking consults; a historical projection
    /// ranks on what the evidence carries, and a pin made today is not evidence about then.
    pins: PinRegistry,
}

impl HistoricalWorld {
    /// Reconstructs `scope` at `at` and builds the index the spatial query layer reads (§9.1).
    ///
    /// # Errors
    ///
    /// Whatever §34 refusal the ledger raises.
    pub fn reconstruct(
        ledger: &dyn LedgerRead,
        scope: &SpatialScope,
        at: Timestamp,
    ) -> Result<Self, ErrorValue> {
        // §21.1: a source answers about the past only where it said it can. Nothing has declared
        // anything to this reconstruction, so §14.5's four supports are absent and a filesystem
        // tree is refused — which is the default the specification asks for.
        let sources = SourceMatrix::new();
        let world = Reconstructor::new(ledger)
            .with_sources(&sources)
            .reconstruct(&ReconstructionRequest::new(scope.clone(), at))?;
        Ok(Self::of(world, scope.clone(), at))
    }

    /// The world a reconstruction already produced, indexed.
    #[must_use]
    fn of(world: ReconstructedWorld, scope: SpatialScope, at: Timestamp) -> Self {
        let mut index = SpatialIndex::new(FreshnessPolicy::uniform(
            Span::new().days(HISTORICAL_TTL_DAYS),
        ));
        for object in world.objects() {
            if !object.presence().is_present() {
                continue;
            }
            let Some(projected) = project(object, &scope) else {
                continue;
            };
            // A conflict here would be two reconstructed objects claiming one provider identity,
            // which the reconstruction already reconciled: the loser is dropped rather than
            // allowed to overwrite, because §5.1 makes the v0.4 identity authoritative.
            let _ = index.register(projected, object.observed_at());
        }
        for relation in world.relations() {
            // §14.3: an exit at `T` leads to a place at `T`. An edge whose far end this world
            // does not hold is not drawn, so a historical exit can never point into the present.
            if !index.contains(relation.from()) || !index.contains(relation.to()) {
                continue;
            }
            let Some(relation_type) = RelationType::new(relation.relation()) else {
                continue;
            };
            let mut edge = RelationshipEdge::new(
                relation.from().clone(),
                relation.to().clone(),
                relation_type,
                relation.confidence(),
                Provenance::local(COMPOSER, SchemaId::new("ono.spatial-relation", 1)),
                relation.valid_from().unwrap_or(at),
            );
            if let Some(from) = relation.valid_from() {
                edge = edge.valid(ValidityWindow::new(Some(from), relation.valid_until()));
            }
            index.record_edge(edge);
        }
        Self {
            at,
            scope,
            index,
            world,
            pins: PinRegistry::new(),
        }
    }

    /// The instant this world is the world at (§9.4's `as_of`).
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }

    /// The boundary reconstructed.
    #[must_use]
    pub const fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// The index the spatial query layer reads. It holds only what evidence put there.
    #[must_use]
    pub const fn index(&self) -> &SpatialIndex {
        &self.index
    }

    /// The reconstruction behind it, for coverage, gaps and field-level knowledge.
    #[must_use]
    pub const fn reconstruction(&self) -> &ReconstructedWorld {
        &self.world
    }

    /// The composed coverage of the whole answer (§8.5).
    #[must_use]
    pub const fn coverage(&self) -> &CoverageSummary {
        self.world.coverage()
    }

    /// The gaps that materially affect it (§7.5, §18.6).
    #[must_use]
    pub fn gaps(&self) -> &[TemporalGap] {
        self.world.gaps()
    }

    /// Whether the object was there at the instant, and how well that is known (§9.5, §9.7).
    #[must_use]
    pub fn presence_of(&self, id: &SpatialId, object_type: SpatialType) -> Presence {
        // A canonical space is declared geography rather than an observed object (v0.4 §7.1), so
        // it is there whenever the shell is: `home` and `up` reach the root at every instant.
        if ono_spatial_query::resolve::space_of(id).is_some() {
            return Presence::Present;
        }
        self.world.presence_of(id, object_type)
    }

    /// The archived record of one place, where a source produced one.
    #[must_use]
    pub fn record_of(&self, id: &SpatialId) -> Option<&RecordValue> {
        self.world.object(id).and_then(|object| object.record())
    }

    /// §9.7's refusal, or `None` where the place was there.
    ///
    /// §9.7: "If the current v0.4 place did not yet exist at `T`, `look` MUST report `place not
    /// known at requested time` with coverage explaining whether this means known-absent or
    /// simply unknown. It MUST NOT automatically jump elsewhere." The refusal carries the
    /// distinction as metadata, so a script branches on it and a renderer draws it; the caller
    /// navigates nowhere, which is what returning a refusal rather than a place guarantees.
    #[must_use]
    pub fn not_known_here(&self, id: &SpatialId, object_type: SpatialType) -> Option<ErrorValue> {
        let presence = self.presence_of(id, object_type);
        if presence.is_present() {
            return None;
        }
        let headline = self.coverage().headline();
        let detail = match presence {
            Presence::Absent => {
                "coverage over the window was complete enough to prove it was not there"
            }
            _ => "no source covered this place over the window, so nothing is known either way",
        };
        Some(
            ErrorValue::new(
                ErrorCode::TemporalNotRecorded,
                "place not known at requested time",
            )
            .with_metadata("at", Value::Timestamp(self.at))
            .with_metadata("presence", Value::string(presence.as_str()))
            .with_metadata("coverage", Value::string(headline_word(headline)))
            .with_metadata(
                "gaps",
                Value::Int(i128::try_from(self.gaps().len()).unwrap_or(0)),
            )
            .with_help(format!(
                "{detail} — `now` returns to the present, and the place is unchanged until you \
                 navigate (spec v0.5 §9.7)"
            )),
        )
    }

    /// §14.5's refusal for a kind of place whose historical structure nothing supports.
    ///
    /// `None` where the type is not part of the path tree, or where a source did carry it.
    #[must_use]
    pub fn structure_refusal(&self, object_type: SpatialType) -> Option<ErrorValue> {
        if !ono_temporal_reconstruct::is_path_structure(object_type) {
            return None;
        }
        if self
            .world
            .objects()
            .any(|object| object.object_type() == object_type && object.presence().is_present())
        {
            return None;
        }
        Some(
            ErrorValue::new(
                ErrorCode::TemporalUnsupportedSource,
                format!(
                    "historical filesystem structure is not supported here: nothing in this \
                     session's evidence carries the {} tree at {}",
                    object_type.as_str().to_lowercase(),
                    self.at
                ),
            )
            .with_metadata("at", Value::Timestamp(self.at))
            .with_metadata("object_type", Value::string(object_type.as_str()))
            .with_help(
                "a recorder checkpoint, a filesystem snapshot provider, audit or inotify evidence \
                 sufficient for reconstruction, or a KUANG/11 provider can carry it; current \
                 directory contents are not the past (spec v0.5 §14.5)",
            ),
        )
    }

    /// The neighbourhood of `center` at this instant (§14.1, §14.3).
    #[must_use]
    pub fn neighborhood(&self, center: &SpatialId, request: &NeighborhoodRequest) -> Neighborhood {
        ono_spatial_query::neighborhood_of(&self.index, center, request, &self.pins, self.at)
    }

    /// The map around `center` at this instant (§14.1, §14.2).
    #[must_use]
    pub fn map(&self, center: &SpatialId, request: &MapRequest, budget: usize) -> SpatialMap {
        let horizon = self.horizon(center);
        ono_spatial_query::project_map(
            &self.index,
            center,
            &horizon,
            request,
            &self.pins,
            budget,
            self.at,
        )
    }

    /// The horizon around `center`, built the way `map.rs::observe` builds the live one.
    ///
    /// The live builder asks the providers; this one reads the reconstruction, which is the whole
    /// of the difference. The shape is the same, because a projection that received a differently
    /// shaped horizon would draw a differently shaped map (§45.4).
    #[must_use]
    pub fn horizon(&self, center: &SpatialId) -> MapHorizon {
        let mut horizon = MapHorizon::new();
        horizon.place(self.place_at(center, 0, None));

        if let Some(here) = ono_spatial_query::resolve::space_of(center) {
            for child in space::children(here.id).filter(|child| child.is_served()) {
                let child_id = child.spatial_id();
                horizon.place(self.place_at(
                    &child_id,
                    1,
                    Some((center.clone(), HierarchyKind::Grouping)),
                ));
                for member in self.filed_under(&child_id) {
                    horizon.place(self.place_at(
                        &member,
                        2,
                        Some((child_id.clone(), HierarchyKind::Grouping)),
                    ));
                }
            }
            for member in self.filed_under(center) {
                horizon.place(self.place_at(
                    &member,
                    1,
                    Some((center.clone(), HierarchyKind::Grouping)),
                ));
            }
            return horizon;
        }

        if let Some(parent) = ono_spatial_query::resolve::parent_of(&self.index, center) {
            horizon.place(self.place_at(&parent, 1, None));
            horizon.place(self.place_at(center, 0, Some((parent, HierarchyKind::Grouping))));
        }
        let edges: Vec<RelationshipEdge> = self
            .index
            .get(center)
            .map(|entry| entry.edges().to_vec())
            .unwrap_or_default();
        for edge in edges {
            if let Some(other) = edge.other_end(center) {
                horizon.place(self.place_at(&other.clone(), 1, None));
            }
            horizon.edge(edge);
        }
        horizon
    }

    /// The places whose canonical parent is `parent`, as this world files them.
    fn filed_under(&self, parent: &SpatialId) -> Vec<SpatialId> {
        self.index
            .entries()
            .filter(|entry| {
                ono_spatial_query::resolve::parent_of(&self.index, entry.object().spatial_id())
                    .as_ref()
                    == Some(parent)
            })
            .map(|entry| entry.object().spatial_id().clone())
            .collect()
    }

    /// One horizon place, carrying the state the archived record reported (§22's `MapNode.state`).
    fn place_at(
        &self,
        id: &SpatialId,
        depth: usize,
        parent: Option<(SpatialId, HierarchyKind)>,
    ) -> HorizonPlace {
        let state = self.record_of(id).and_then(|record| {
            record
                .get("state")
                .and_then(|value| value.as_str().ok())
                .map(str::to_owned)
        });
        HorizonPlace::new(id.clone(), depth, parent).in_state(state)
    }

    /// The place a selector names in this world (§14.1's `enter`, `jump`, `follow`).
    #[must_use]
    pub fn resolve(&self, selector: &str, context: &SelectorContext) -> Resolution {
        ono_spatial_query::resolve(&self.index, selector, context, self.at)
    }

    /// `find place` at this instant (§14.4).
    ///
    /// The historical index answers first and its matches carry the event that supports them.
    /// `present` is today's index, and what it contributes is a candidate rather than a claim:
    /// a match reached only that way is a resolution aid with no anchor and no instant.
    ///
    /// # Errors
    ///
    /// Whatever §34 refusal the ledger raises.
    pub fn find(
        &self,
        ledger: &dyn LedgerRead,
        query: &str,
        present: &SpatialIndex,
    ) -> Result<Vec<HistoricalPlaceMatch>, ErrorValue> {
        find_place_at(ledger, &self.scope, self.at, query, &IndexAliases(present))
    }
}

/// Today's index, seen as the name source §14.4 permits and nothing more.
///
/// It is a newtype rather than an implementation on [`SpatialIndex`] so that reaching the present
/// from a historical answer is a deliberate act with a name, visible at every call site.
pub struct IndexAliases<'a>(pub &'a SpatialIndex);

impl PresentAliases for IndexAliases<'_> {
    fn resolve_alias(&self, text: &str) -> Vec<(SpatialId, SpatialType, Arc<str>)> {
        self.0
            .search(text)
            .into_iter()
            .map(|entry| {
                let object = entry.object();
                (
                    object.spatial_id().clone(),
                    object.object_type(),
                    Arc::from(object.display_name()),
                )
            })
            .collect()
    }
}

/// The archived record as a v0.4 spatial object (§5.1).
///
/// `ono.file/1` declares no identity — a path is a reference and hard links share one inode — so
/// a filesystem place is derived the way v0.4 §42.3 derives one, and everything else projects
/// from its own record. A filesystem place only reaches this function where §14.5's supports
/// carried it; the reconstruction refuses the rest before this module ever sees them.
fn project(
    object: &ono_temporal_reconstruct::ReconstructedObject,
    scope: &SpatialScope,
) -> Option<SpatialObject> {
    if ono_temporal_reconstruct::is_path_structure(object.object_type()) {
        let path = object
            .record()?
            .get("path")
            .and_then(|value| ono_value::canonical_text(value).ok())?;
        return Some(
            ono_spatial_core::Projection::new(scope.clone(), object.observed_at()).derive(
                object.object_type(),
                SchemaId::new("ono.file", 1),
                "path",
                &path,
                Provenance::local(COMPOSER, SchemaId::new("ono.file", 1)),
            ),
        );
    }
    object.project().ok()
}

/// The word §8.5's headline carries into a refusal's metadata.
const fn headline_word(headline: ono_temporal_core::HeadlineCoverage) -> &'static str {
    match headline {
        ono_temporal_core::HeadlineCoverage::Complete => "complete",
        ono_temporal_core::HeadlineCoverage::Partial => "partial",
        ono_temporal_core::HeadlineCoverage::Uncertain => "uncertain",
    }
}
