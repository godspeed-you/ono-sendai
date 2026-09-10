//! The first-party ZFS recovery provider of Ono-Sendai v0.6 (spec §13, Appendix D.1-D.5).
//!
//! §13 makes ZFS a first-party reference provider because its snapshot semantics map naturally
//! onto protected change. The interesting part of the section is everywhere that map is
//! misleading, and this crate is mostly those places:
//!
//! - **The dataset is the boundary (§13.4).** `/`, `/var`, `/home` and `/data` on separate
//!   datasets are separate snapshot boundaries, and a snapshot of one protects none of the
//!   others. [`layout::Layout::dataset_of_path`] resolves through the kernel's mount table and
//!   `zfs list`'s own mount metadata, never through the shape of a path (Appendix B.8), and
//!   [`render::BoundaryReport`] prints §13.4's `NOT PROTECTED BY` block from what is actually
//!   there.
//! - **One point in time is not one recursive flag (§13.3).** Several datasets protected at one
//!   logical point are named, each of them, in one atomic `zfs snapshot`, so nothing outside the
//!   assets is snapshotted; Appendix D.1 wants one concrete reference per dataset, so
//!   [`RecoveryProvider::plan_protection`](ono_change_core::RecoveryProvider::plan_protection) emits one protection action per dataset and recovery
//!   planning reasons about each dataset separately.
//! - **No flag is ever added silently (§13.6).** No argument vector this crate builds contains
//!   `-r` or `-R`, and [`RecoveryProvider::restore_with`](ono_change_core::RecoveryProvider::restore_with)
//!   refuses every spelling of either. Where a rollback would destroy newer snapshots or
//!   bookmarks, each destruction is a named action an operator accepts, and where a clone of a
//!   newer snapshot stands in the way the plan says so and stops.
//! - **Nothing is promised online (§13.7).** A rollback that needs an unmount, a reboot or a
//!   boot-environment switch says so before apply.
//! - **Twelve facts before a destructive path (§56.1).** [`checklist::SafetyChecklist`] starts
//!   with every fact unproven, and §56.3's `recovery.plan_incomplete` is what a caller gets when
//!   any of them could not be established.
//!
//! Every ZFS command runs through [`ono_change_core::ToolRunner`] as a program and an argument
//! vector (§12.3): [`ProcessRunner`] in production, and in the deterministic suite the recorded
//! output of the real tools, replayed through `ScriptedRunner` from `tests/fixtures/`.

#![forbid(unsafe_code)]

pub mod checklist;
mod floor;
pub mod layout;
pub mod naming;
pub mod parse;
mod process;
mod provider;
pub mod render;

pub use checklist::{FactEvidence, SafetyChecklist, ZfsFact};
pub use floor::FreeSpaceFloor;
pub use layout::{
    Bookmark, Dataset, Layout, Mount, MountState, MountTable, Pool, Snapshot, is_beneath,
    is_descendant,
};
pub use process::{DEFAULT_TIMEOUT, ProcessRunner};
pub use provider::{
    CLONE_MOUNT_ROOT, CP, GUID_FINGERPRINT, LS, MOUNTINFO, PROVIDER_ID, RootDatasetCase,
    VALIDATED_VERSIONS, ZFS, ZPOOL, ZfsProvider,
};
pub use render::{BoundaryReport, render_asset};
