//! Historical collections (v0.5 §9.6).
//!
//! §9.6: `get process` in historical context "MUST return the best supported process set for `T`
//! and attach collection-level coverage. If the source cannot prove complete enumeration, the
//! result MUST NOT imply that the returned rows are the complete process list." The second
//! sentence is why [`ReconstructedCollection::is_enumeration_proven`] exists as a field of the
//! answer rather than as something a caller infers from a row count.

use std::sync::Arc;

use ono_spatial_core::{SpatialId, SpatialType};
use ono_temporal_core::{TemporalCompleteness, TemporalGap};

/// The set of objects of one type that reconstruction supports at the requested instant (§9.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructedCollection {
    pub(crate) object_type: SpatialType,
    pub(crate) capability: Arc<str>,
    pub(crate) members: Vec<SpatialId>,
    pub(crate) completeness: TemporalCompleteness,
    pub(crate) enumeration_proven: bool,
    pub(crate) gaps: Vec<TemporalGap>,
}

impl ReconstructedCollection {
    /// The type the collection holds.
    #[must_use]
    pub const fn object_type(&self) -> SpatialType {
        self.object_type
    }

    /// The coverage capability the collection's completeness was read from (§8.1).
    #[must_use]
    pub const fn capability(&self) -> &Arc<str> {
        &self.capability
    }

    /// The identities reconstruction supports at the requested instant.
    #[must_use]
    pub fn members(&self) -> &[SpatialId] {
        &self.members
    }

    /// How many are in it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether reconstruction supports nothing of this type.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// What the sources covering this class composed to over the window (§8.5).
    #[must_use]
    pub const fn completeness(&self) -> TemporalCompleteness {
        self.completeness
    }

    /// Whether the sources could prove the enumeration is complete (§9.6).
    ///
    /// False is the honest default. A caller rendering a list whose enumeration is unproven says
    /// so; it does not print a row count as if it were the whole list.
    #[must_use]
    pub const fn is_enumeration_proven(&self) -> bool {
        self.enumeration_proven
    }

    /// The gaps that materially affect the enumeration (§7.5).
    #[must_use]
    pub fn gaps(&self) -> &[TemporalGap] {
        &self.gaps
    }
}
