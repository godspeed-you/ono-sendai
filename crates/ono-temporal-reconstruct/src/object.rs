//! Reconstructed objects and the temporal metadata they carry (v0.5 §9.4).
//!
//! §9.4: "reconstructed objects retain their canonical schema plus temporal metadata", and "the
//! temporal metadata MUST NOT collide with provider fields". [`attach_temporal`] is the one
//! place that attachment happens, and it never overwrites a provider's own field: it uses the
//! object's declared `temporal` field where the schema has one, and the reserved `ono.temporal`
//! extension key where it has none (v0.2 §10.4, §31.5).

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{Projection, SpatialId, SpatialObject, SpatialScope, SpatialType};
use ono_temporal_core::{
    CoverageSummary, EventId, EvidenceSource, TemporalGap, value::temporal_metadata,
};
use ono_value::{ErrorValue, RecordValue, Value};

use crate::field::{Presence, ReconstructedField};

/// The field name §9.4's metadata takes on a schema that declares one.
pub const TEMPORAL_FIELD: &str = "temporal";

/// The reserved extension key it takes on a schema that does not (v0.2 §10.4, §31.5).
pub const TEMPORAL_EXTENSION: &str = "ono.temporal";

/// One object as reconstruction supports it at the requested instant (§9.1, §9.4).
#[derive(Debug, Clone, PartialEq)]
pub struct ReconstructedObject {
    pub(crate) id: SpatialId,
    pub(crate) object_type: SpatialType,
    pub(crate) label: Arc<str>,
    pub(crate) scope: SpatialScope,
    pub(crate) presence: Presence,
    pub(crate) record: Option<RecordValue>,
    pub(crate) fields: Vec<ReconstructedField>,
    pub(crate) observed_at: Timestamp,
    pub(crate) as_of: Timestamp,
    pub(crate) lifetime_from: Option<Timestamp>,
    pub(crate) lifetime_until: Option<Timestamp>,
    pub(crate) sources: Vec<EvidenceSource>,
    pub(crate) coverage: CoverageSummary,
    pub(crate) gaps: Vec<TemporalGap>,
}

impl ReconstructedObject {
    /// The v0.4 identity it had when it was live (§5.1).
    #[must_use]
    pub const fn spatial_id(&self) -> &SpatialId {
        &self.id
    }

    /// What kind of object it is.
    #[must_use]
    pub const fn object_type(&self) -> SpatialType {
        self.object_type
    }

    /// What a person calls it.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Whether reconstruction supports its existence at the requested instant (§9.5, §9.7).
    #[must_use]
    pub const fn presence(&self) -> Presence {
        self.presence
    }

    /// The canonical provider record, as reconstruction assembled it.
    ///
    /// `None` where every source that spoke about the object named it without producing a
    /// record — an appearance event carries an identity and a label, and that is not a row.
    #[must_use]
    pub const fn record(&self) -> Option<&RecordValue> {
        self.record.as_ref()
    }

    /// Every reconstructed field, each with its own coverage (§8.5).
    pub fn fields(&self) -> impl Iterator<Item = &ReconstructedField> {
        self.fields.iter()
    }

    /// One reconstructed field by name.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&ReconstructedField> {
        self.fields.iter().find(|field| field.name() == name)
    }

    /// The instant the state used here was actually observed.
    #[must_use]
    pub const fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    /// When the object was first supported at or before the requested instant (§5.2).
    #[must_use]
    pub const fn lifetime_from(&self) -> Option<Timestamp> {
        self.lifetime_from
    }

    /// When it was observed to end. `None` while its lifetime is still open at the requested
    /// instant — §5.4: `now` never revives a tombstone, and nothing here ends a lifetime that
    /// no source ended.
    #[must_use]
    pub const fn lifetime_until(&self) -> Option<Timestamp> {
        self.lifetime_until
    }

    /// Every source that contributed to this object (§9.4).
    pub fn sources(&self) -> impl Iterator<Item = &EvidenceSource> {
        self.sources.iter()
    }

    /// The composed coverage behind it (§8.5).
    #[must_use]
    pub const fn coverage(&self) -> &CoverageSummary {
        &self.coverage
    }

    /// The gaps that materially affect it (§7.5).
    #[must_use]
    pub fn gaps(&self) -> &[TemporalGap] {
        &self.gaps
    }

    /// Always true here: every object this crate returns was reconstructed rather than read
    /// from the present (§9.4).
    #[must_use]
    pub const fn is_reconstructed(&self) -> bool {
        true
    }

    /// §9.4's `temporal` sub-record.
    ///
    /// # Errors
    ///
    /// Returns `ono.provider_schema_violation` where the gap contract is not in this build.
    pub fn temporal_metadata(&self) -> Result<Value, ErrorValue> {
        temporal_metadata(
            self.as_of,
            &self.coverage,
            self.is_reconstructed(),
            &self.sources,
            &self.gaps,
        )
    }

    /// The canonical record with §9.4's metadata attached.
    ///
    /// # Errors
    ///
    /// Returns `temporal.not_recorded` where no source produced a record for this object, and
    /// `ono.provider_schema_violation` where the gap contract is not in this build.
    pub fn to_record(&self) -> Result<RecordValue, ErrorValue> {
        attach_temporal(self.archived()?, self.temporal_metadata()?)
    }

    /// The archived record as a v0.4 spatial object, for a historical map (§14.1).
    ///
    /// The projection is taken at [`observed_at`](Self::observed_at) rather than at the
    /// requested instant, so the object's lifetime says when it was seen and not when it was
    /// asked about. The identity is unchanged either way: `Projection`'s digest covers the
    /// record's identity components and never the observation time, which is what lets a
    /// historical place and its live self be the same place (§5.1).
    ///
    /// # Errors
    ///
    /// Returns `temporal.not_recorded` where no source produced a record for this object, and
    /// `spatial.identity_conflict` where the archived record declares no identity.
    pub fn project(&self) -> Result<SpatialObject, ErrorValue> {
        Projection::new(self.scope.clone(), self.observed_at)
            .project_as(self.archived()?, self.object_type)
    }

    /// The archived record, or §34's `temporal.not_recorded` where no source produced one.
    fn archived(&self) -> Result<&RecordValue, ErrorValue> {
        self.record
            .as_ref()
            .ok_or_else(|| ono_temporal_core::error::not_recorded(&self.scope, self.as_of, &[]))
    }
}

/// A subject a source named that Ono could not reconcile to a canonical identity (§5.5).
///
/// §5.5: such an event "may remain an unresolved temporal subject and MUST be rendered as such
/// rather than attached to a guessed object". It therefore never becomes a
/// [`ReconstructedObject`] and never contributes state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedSubject {
    pub(crate) source: EvidenceSource,
    pub(crate) described: Arc<str>,
    pub(crate) events: Vec<EventId>,
}

impl UnresolvedSubject {
    /// The source that named it.
    #[must_use]
    pub const fn source(&self) -> &EvidenceSource {
        &self.source
    }

    /// What the source called it — `pid 1842`, `nginx.conf`.
    #[must_use]
    pub fn described(&self) -> &str {
        &self.described
    }

    /// The events that named it, so a reader can go and look (§11.6).
    #[must_use]
    pub fn events(&self) -> &[EventId] {
        &self.events
    }
}

/// Attaches §9.4's temporal metadata to a record without colliding with a provider field.
///
/// The schema's own `temporal` field is used where it declares one. Where it does not, the
/// metadata goes into the reserved `ono.temporal` extension key: v0.2 §10.4 makes extensions
/// namespaced and §31.5 reserves `ono.*` to this project, so neither route can overwrite a field
/// a provider owns.
///
/// # Errors
///
/// Returns whatever rebuilding the record reports; a schema that declares `temporal` with an
/// incompatible shape is the one case, and it is a contract defect rather than a caller's input.
pub fn attach_temporal(record: &RecordValue, metadata: Value) -> Result<RecordValue, ErrorValue> {
    let mut builder =
        RecordValue::builder(Arc::clone(record.schema()), record.provenance().clone());
    for field in record.schema().fields() {
        if let Some(value) = record.get(field.name()) {
            builder = builder.set(field.name(), value.clone())?;
        }
    }
    for (key, value) in record.extra().iter() {
        builder = builder.set_extra(key, value.clone());
    }
    if record.schema().field(TEMPORAL_FIELD).is_some() {
        return Ok(builder.set(TEMPORAL_FIELD, metadata)?.build());
    }
    Ok(builder.set_extra(TEMPORAL_EXTENSION, metadata).build())
}

/// §9.4's metadata as it was attached, whichever route it took.
#[must_use]
pub fn temporal_of(record: &RecordValue) -> Option<Value> {
    if record.schema().field(TEMPORAL_FIELD).is_some() {
        return record
            .get(TEMPORAL_FIELD)
            .filter(|value| !value.is_null())
            .cloned();
    }
    record.extra().get(TEMPORAL_EXTENSION).cloned()
}
