//! What each source can say about the past (v0.5 §21.1), and the filesystem refusal of §14.5.
//!
//! §14.5 is a refusal rather than a feature. "Ono MUST therefore show only historical filesystem
//! structure supported by: recorder checkpoints; filesystem-specific snapshot providers;
//! explicit audit/inotify/FSEvents-like evidence sufficient for reconstruction; KUANG/11
//! providers", and "no generic v0.5 implementation may pretend that current directory contents
//! represent the past". [`SourceMatrix::structure_support`] is those four bullets and nothing
//! else; a source with nothing but [`TemporalCapabilities::current_snapshot`] gets `None`.

use std::collections::BTreeMap;

use ono_spatial_core::SpatialType;
use ono_temporal_core::{EvidenceSource, TemporalCapabilities};

/// Which of §14.5's four supports carries a historical filesystem structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StructureSupport {
    /// A recorder checkpoint serialised the structure at an instant (§42.2).
    Checkpoint,
    /// A filesystem-specific snapshot provider answers about the past directly (§21.4).
    SnapshotProvider,
    /// An audit or inotify stream is exhaustive enough to reconstruct from (§21.5).
    AuditEvidence,
    /// A KUANG/11 package contributes the history (§37.3).
    KuangProvider,
}

impl StructureSupport {
    /// The name `inspect` spells when it shows what a historical path rests on.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            StructureSupport::Checkpoint => "checkpoint",
            StructureSupport::SnapshotProvider => "snapshot_provider",
            StructureSupport::AuditEvidence => "audit_evidence",
            StructureSupport::KuangProvider => "kuang_provider",
        }
    }
}

/// What each source declares it can answer about time (§21.1).
///
/// A source this does not name declares nothing, which is the honest default: §21.5 forbids
/// advertising `exhaustive_events` "merely because events usually arrive", so an unstated
/// capability is a capability the source does not have.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMatrix {
    declared: BTreeMap<EvidenceSource, TemporalCapabilities>,
}

impl SourceMatrix {
    /// A matrix in which no source has declared anything.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The matrix with `source`'s declaration added.
    #[must_use]
    pub fn with(mut self, source: EvidenceSource, capabilities: TemporalCapabilities) -> Self {
        self.declared.insert(source, capabilities);
        self
    }

    /// What `source` declared, or the all-false default.
    #[must_use]
    pub fn capabilities(&self, source: &EvidenceSource) -> TemporalCapabilities {
        self.declared
            .get(source)
            .cloned()
            .unwrap_or_else(TemporalCapabilities::none)
    }

    /// Which of §14.5's four supports `source` provides, or `None` for none of them.
    #[must_use]
    pub fn structure_support(
        &self,
        source: &EvidenceSource,
        from_checkpoint: bool,
    ) -> Option<StructureSupport> {
        if from_checkpoint {
            return Some(StructureSupport::Checkpoint);
        }
        if source.is_plugin() {
            return Some(StructureSupport::KuangProvider);
        }
        let capabilities = self.capabilities(source);
        if capabilities.historical_query {
            return Some(StructureSupport::SnapshotProvider);
        }
        if capabilities.exhaustive_events {
            return Some(StructureSupport::AuditEvidence);
        }
        None
    }
}

/// Whether §14.5's refusal applies to objects of this type.
///
/// "Normal filesystems do not retain arbitrary historical directory trees", which is true of the
/// path tree and not of the mounts and filesystems that carry it — §42.2 checkpoints those by
/// name, so the refusal is exactly the two types that make up a directory tree.
#[must_use]
pub const fn is_path_structure(object_type: SpatialType) -> bool {
    matches!(object_type, SpatialType::File | SpatialType::Directory)
}
