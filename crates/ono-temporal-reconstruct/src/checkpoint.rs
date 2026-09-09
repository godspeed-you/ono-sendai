//! The checkpoint projection and its bounds (v0.5 §42), and §9.1's nearest-trusted selection.
//!
//! §42.2 fixes what a default local checkpoint holds and ends with the sentence that shapes this
//! module: "huge unbounded sets such as all files under `/` MUST NOT be checkpointed by
//! default". A projection therefore has a policy, and what the policy leaves out is stated as
//! coverage rather than dropped — §42.4 makes a checkpoint inherit "the coverage quality of its
//! sources", and a class nobody captured is a class a later reconstruction must call unknown
//! rather than absent.

use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;
use ono_spatial_core::{PermissionState, SpatialId, SpatialScope, SpatialType};
use ono_temporal_core::{
    Checkpoint, CheckpointId, EvidenceSource, GapReason, LedgerRead, ObjectState, RelationState,
    TemporalCompleteness, TemporalCoverage,
};
use ono_value::{ErrorValue, Provenance, SchemaId};

use crate::capability;

/// How many older checkpoints [`nearest_trusted`] will step past before giving up.
///
/// A store whose recent checkpoints all saw nothing is a store with a problem, and walking it to
/// the beginning of time would turn one refusal into an unbounded scan (§43.1's spirit).
const MAX_STEPS_BACK: usize = 64;

/// What a default local checkpoint may hold (§42.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointPolicy {
    types: BTreeSet<SpatialType>,
    max_per_class: usize,
}

impl CheckpointPolicy {
    /// The default ceiling on one state class, where the policy names no other.
    pub const DEFAULT_MAX_PER_CLASS: usize = 10_000;

    /// §42.2's own list: the state classes a default local checkpoint holds.
    ///
    /// "Spatial identity index summaries; services; processes where visible; interfaces/routes;
    /// mounts/filesystems; containers; relevant relations". The path tree is deliberately
    /// absent, which is the same sentence §14.5 states from the other side.
    #[must_use]
    pub fn default_local() -> Self {
        Self {
            types: [
                SpatialType::System,
                SpatialType::Compute,
                SpatialType::Network,
                SpatialType::Storage,
                SpatialType::Containers,
                SpatialType::Identity,
                SpatialType::Devices,
                SpatialType::Workload,
                SpatialType::Service,
                SpatialType::Process,
                SpatialType::Cgroup,
                SpatialType::Container,
                SpatialType::Namespace,
                SpatialType::Interface,
                SpatialType::Address,
                SpatialType::Route,
                SpatialType::Neighbor,
                SpatialType::Socket,
                SpatialType::Listener,
                SpatialType::Connection,
                SpatialType::Filesystem,
                SpatialType::Mount,
                SpatialType::BlockDevice,
            ]
            .into_iter()
            .collect(),
            max_per_class: Self::DEFAULT_MAX_PER_CLASS,
        }
    }

    /// A policy that holds nothing, for a caller that states its own list.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            types: BTreeSet::new(),
            max_per_class: Self::DEFAULT_MAX_PER_CLASS,
        }
    }

    /// The policy with `object_type` admitted.
    #[must_use]
    pub fn with_type(mut self, object_type: SpatialType) -> Self {
        self.types.insert(object_type);
        self
    }

    /// The policy with a different ceiling on one state class.
    #[must_use]
    pub const fn with_max_per_class(mut self, max: usize) -> Self {
        self.max_per_class = max;
        self
    }

    /// Whether objects of this type may be checkpointed at all.
    #[must_use]
    pub fn admits(&self, object_type: SpatialType) -> bool {
        self.types.contains(&object_type)
    }

    /// The ceiling on one state class.
    #[must_use]
    pub const fn max_per_class(&self) -> usize {
        self.max_per_class
    }
}

/// What a caller wants serialised into a checkpoint (§42.2).
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointRequest {
    scope: SpatialScope,
    captured_at: Timestamp,
    objects: Vec<ObjectState>,
    relations: Vec<RelationState>,
    coverage: Vec<TemporalCoverage>,
    provenance: Provenance,
}

impl CheckpointRequest {
    /// A capture of `scope` at `captured_at`, holding nothing yet.
    #[must_use]
    pub fn new(scope: SpatialScope, captured_at: Timestamp) -> Self {
        Self {
            scope,
            captured_at,
            objects: Vec::new(),
            relations: Vec::new(),
            coverage: Vec::new(),
            provenance: Provenance::local("ono.recorder", SchemaId::new("ono.temporal-event", 1)),
        }
    }

    /// The objects the capture saw.
    #[must_use]
    pub fn with_objects(mut self, objects: Vec<ObjectState>) -> Self {
        self.objects = objects;
        self
    }

    /// The relationships the capture saw.
    #[must_use]
    pub fn with_relations(mut self, relations: Vec<RelationState>) -> Self {
        self.relations = relations;
        self
    }

    /// What the sources behind the capture were able to observe (§42.4).
    #[must_use]
    pub fn with_coverage(mut self, coverage: Vec<TemporalCoverage>) -> Self {
        self.coverage = coverage;
        self
    }

    /// Who captured it (v0.2 §25.2).
    #[must_use]
    pub fn with_provenance(mut self, provenance: Provenance) -> Self {
        self.provenance = provenance;
        self
    }
}

/// A state class the policy left out, and why (§42.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedClass {
    /// The type nothing of which was checkpointed.
    pub object_type: SpatialType,
    /// How many objects of it the caller offered.
    pub count: usize,
    /// Why they were left out (§7.5).
    pub reason: GapReason,
}

/// A projected checkpoint, and what the policy kept out of it (§42.2).
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointProjection {
    checkpoint: Checkpoint,
    excluded: Vec<ExcludedClass>,
}

impl CheckpointProjection {
    /// The checkpoint itself, ready for [`ono_temporal_core::LedgerWrite::write_checkpoint`].
    #[must_use]
    pub const fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }

    /// The checkpoint, taken by value.
    #[must_use]
    pub fn into_checkpoint(self) -> Checkpoint {
        self.checkpoint
    }

    /// The state classes the policy left out (§42.2).
    #[must_use]
    pub fn excluded(&self) -> &[ExcludedClass] {
        &self.excluded
    }
}

/// Projects a bounded checkpoint from what a set of sources currently see (§42.2).
///
/// A class the policy does not admit is left out whole, and a class larger than the policy's
/// ceiling is left out whole as well: a truncated set would carry a completeness it does not
/// have, and §9.6 forbids a result that implies its rows are the whole list. Each exclusion adds
/// an `unavailable` coverage interval, so a reconstruction from this checkpoint reports that
/// class as unknown rather than as absent (§7.4).
#[must_use]
pub fn project_checkpoint(
    request: &CheckpointRequest,
    policy: &CheckpointPolicy,
) -> CheckpointProjection {
    let mut by_type: BTreeMap<SpatialType, Vec<&ObjectState>> = BTreeMap::new();
    for object in &request.objects {
        by_type.entry(object.object_type).or_default().push(object);
    }

    let mut kept: Vec<ObjectState> = Vec::new();
    let mut excluded: Vec<ExcludedClass> = Vec::new();
    let mut dropped_ids: BTreeSet<SpatialId> = BTreeSet::new();
    let mut coverage = request.coverage.clone();

    for (object_type, objects) in by_type {
        let reason = if policy.admits(object_type) {
            (objects.len() > policy.max_per_class()).then_some(GapReason::NotRecorded)
        } else {
            Some(GapReason::Unsupported)
        };
        match reason {
            None => kept.extend(objects.into_iter().cloned()),
            Some(reason) => {
                let source = objects
                    .first()
                    .map_or_else(EvidenceSource::recorder, |object| object.source.clone());
                for object in &objects {
                    dropped_ids.insert(object.id.clone());
                }
                excluded.push(ExcludedClass {
                    object_type,
                    count: objects.len(),
                    reason,
                });
                coverage.push(TemporalCoverage {
                    scope: request.scope.clone(),
                    capability: capability::existence(object_type),
                    from: request.captured_at,
                    until: request.captured_at,
                    completeness: TemporalCompleteness::Unavailable,
                    sampling_interval: None,
                    source,
                    permission: PermissionState::Unknown,
                });
            }
        }
    }

    // An edge whose end was left out points at nothing a reader could enter, so it goes with it.
    let relations = request
        .relations
        .iter()
        .filter(|edge| !dropped_ids.contains(&edge.from) && !dropped_ids.contains(&edge.to))
        .cloned()
        .collect();

    CheckpointProjection {
        checkpoint: Checkpoint {
            checkpoint_id: CheckpointId::of(&request.scope, request.captured_at),
            scope: request.scope.clone(),
            captured_at: request.captured_at,
            coverage,
            objects: kept,
            relations,
            provenance: request.provenance.clone(),
        },
        excluded,
    }
}

/// Whether a checkpoint carries anything a reconstruction may stand on (§42.4).
///
/// §42.4: "each checkpoint inherits the coverage quality of its sources. A checkpoint is not
/// globally authoritative." A checkpoint every one of whose sources was unavailable or refused
/// therefore carries no state worth selecting — it is a record that nothing could be seen. A
/// checkpoint that declares no coverage at all makes no such statement and stays selectable; its
/// weakness shows up in the composition instead, where an undeclared capability is `unknown`.
#[must_use]
pub fn is_trusted(checkpoint: &Checkpoint) -> bool {
    checkpoint.coverage.is_empty()
        || !checkpoint.coverage.iter().all(|interval| {
            matches!(
                interval.completeness,
                TemporalCompleteness::Unavailable | TemporalCompleteness::PermissionDenied
            )
        })
}

/// The nearest trusted checkpoint at or before `at` for `scope` (§9.1, step 1).
///
/// §42.1 partitions checkpoints "by host/scope so reconstructing one service does not require
/// deserializing an entire federated environment", and the ledger's own selection honours that,
/// so a checkpoint for another host is never a candidate. Where the nearest candidate is not
/// trusted, the search steps to the instant before it and asks again, up to a bounded number of
/// steps.
///
/// # Errors
///
/// Returns whatever the ledger reports — a §34 store refusal.
pub fn nearest_trusted(
    ledger: &dyn LedgerRead,
    scope: &SpatialScope,
    at: Timestamp,
) -> Result<Option<Checkpoint>, ErrorValue> {
    let mut cursor = at;
    for _ in 0..MAX_STEPS_BACK {
        let Some(candidate) = ledger.checkpoint_before(scope, cursor)? else {
            return Ok(None);
        };
        if is_trusted(&candidate) {
            return Ok(Some(candidate));
        }
        let Ok(earlier) = Timestamp::from_nanosecond(candidate.captured_at.as_nanosecond() - 1)
        else {
            return Ok(None);
        };
        cursor = earlier;
    }
    Ok(None)
}
