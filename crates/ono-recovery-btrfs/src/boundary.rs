//! Nested subvolume boundaries, and the set of subvolumes a plan must snapshot (§14.3).
//!
//! This is the module the crate exists for. §14.3 states the fact plainly — *"A snapshot of a
//! parent subvolume does not contain the live contents of nested subvolumes as ordinary
//! recursively captured data"* — and the recorded fixtures demonstrate it rather than asserting
//! it: `nested-live.txt` shows `state/` inside `/mnt/root/var/lib-app`, and
//! `nested-in-snapshot.txt` shows the same path inside a read-only snapshot of `@var`, empty.
//!
//! Two things follow, and both are here:
//!
//! - **Coverage stops at a boundary.** A snapshot of `@var` covers everything in `@var` except
//!   what lives in `@var/lib-app`, and [`SubvolumeLayout::nested_within`] names those so the
//!   asset can carry a [`RecoveryExclusion`](ono_change_core::RecoveryExclusion) for each
//!   (§55.4 case 18).
//! - **Protection follows the planned mutation scope.** [`SubvolumeLayout::required_for`] maps the
//!   paths a plan changes to the subvolumes that each need their own snapshot, and to nothing
//!   else. §62.7 forbids blanket-snapshotting the host, so a plan that does not touch `@home`
//!   does not snapshot `@home` — which is §14.3's own worked example and §59.3's scenario.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::mount::{BtrfsMount, BtrfsMounts, contains};
use crate::parse::SubvolumeEntry;

/// One subvolume, as a boundary that snapshots do not cross (§14.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubvolumeBoundary {
    id: u64,
    parent_id: Option<u64>,
    uuid: Option<Arc<str>>,
    parent_uuid: Option<Arc<str>>,
    tree_path: Arc<str>,
}

impl SubvolumeBoundary {
    /// Records a boundary directly, for a subvolume whose metadata is already in hand.
    #[must_use]
    pub fn new(id: u64, tree_path: impl Into<Arc<str>>) -> Self {
        Self {
            id,
            parent_id: None,
            uuid: None,
            parent_uuid: None,
            tree_path: tree_path.into(),
        }
    }

    /// Reads a boundary out of one `btrfs subvolume list` line (§14.3).
    #[must_use]
    pub fn from_entry(entry: &SubvolumeEntry) -> Self {
        Self {
            id: entry.id(),
            parent_id: entry.parent_id(),
            uuid: entry.uuid().map(Arc::from),
            parent_uuid: entry.parent_uuid().map(Arc::from),
            tree_path: Arc::from(entry.tree_path()),
        }
    }

    /// The stable subvolume ID (§14.1).
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// The ID of the subvolume this one is nested inside.
    #[must_use]
    pub const fn parent_id(&self) -> Option<u64> {
        self.parent_id
    }

    /// The subvolume UUID.
    #[must_use]
    pub fn uuid(&self) -> Option<&str> {
        self.uuid.as_deref()
    }

    /// The UUID this subvolume was snapshotted from (§14.2).
    #[must_use]
    pub fn parent_uuid(&self) -> Option<&str> {
        self.parent_uuid.as_deref()
    }

    /// The path inside the filesystem tree, such as `@var/lib-app`.
    #[must_use]
    pub fn tree_path(&self) -> &str {
        &self.tree_path
    }

    /// The last component of the tree path.
    #[must_use]
    pub fn name(&self) -> &str {
        self.tree_path.rsplit('/').next().unwrap_or(&self.tree_path)
    }

    /// Whether this subvolume is itself a snapshot of another one (§14.2).
    #[must_use]
    pub fn is_snapshot(&self) -> bool {
        self.parent_uuid.is_some()
    }

    /// Whether this subvolume lies inside `other` in the filesystem tree (§14.3).
    ///
    /// The comparison is by path component and strictly proper: `@var/lib-app` is nested inside
    /// `@var`, `@var` is not nested inside itself, and `@variant` is not nested inside `@var`.
    #[must_use]
    pub fn is_nested_in(&self, other: &Self) -> bool {
        let outer = other.tree_path.trim_matches('/');
        let inner = self.tree_path.trim_matches('/');
        !outer.is_empty()
            && inner
                .strip_prefix(outer)
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// The subvolumes of one filesystem, together with the mounts that make them reachable (§14.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubvolumeLayout {
    boundaries: Vec<SubvolumeBoundary>,
    mounts: BtrfsMounts,
}

impl SubvolumeLayout {
    /// Combines `btrfs subvolume list` output with the mount table it was taken against.
    #[must_use]
    pub const fn new(boundaries: Vec<SubvolumeBoundary>, mounts: BtrfsMounts) -> Self {
        Self { boundaries, mounts }
    }

    /// Every subvolume the filesystem holds.
    #[must_use]
    pub fn boundaries(&self) -> &[SubvolumeBoundary] {
        &self.boundaries
    }

    /// The mount table the layout was resolved against.
    #[must_use]
    pub const fn mounts(&self) -> &BtrfsMounts {
        &self.mounts
    }

    /// The subvolume with this id.
    #[must_use]
    pub fn by_id(&self, id: u64) -> Option<&SubvolumeBoundary> {
        self.boundaries.iter().find(|boundary| boundary.id() == id)
    }

    /// The subvolume at this tree path.
    #[must_use]
    pub fn by_tree_path(&self, tree_path: &str) -> Option<&SubvolumeBoundary> {
        let wanted = tree_path.trim_matches('/');
        self.boundaries
            .iter()
            .find(|boundary| boundary.tree_path() == wanted)
    }

    /// Where a subvolume is visible in this mount namespace, where it is visible at all.
    ///
    /// A subvolume can be visible through several mounts at once — the fixtures' `@var` is
    /// `/mnt/root/var` through its own mount and `/mnt/top/@var` through the filesystem's top
    /// level — and the preferred answer is the one reached through the *most specific* mount,
    /// because that is the path a plan and a person will use.
    #[must_use]
    pub fn visible_path(&self, boundary: &SubvolumeBoundary) -> Option<PathBuf> {
        self.mount_for(boundary)
            .and_then(|mount| mount.visible_path(boundary.tree_path()))
    }

    /// Every path a subvolume is visible at, one per mount that shows it.
    #[must_use]
    pub fn visible_paths(&self, boundary: &SubvolumeBoundary) -> Vec<PathBuf> {
        self.mounts
            .mounts()
            .iter()
            .filter_map(|mount| mount.visible_path(boundary.tree_path()))
            .collect()
    }

    /// The mount a subvolume is best reached through, where it is visible at all.
    #[must_use]
    pub fn mount_for(&self, boundary: &SubvolumeBoundary) -> Option<&BtrfsMount> {
        self.mounts
            .mounts()
            .iter()
            .filter(|mount| mount.visible_path(boundary.tree_path()).is_some())
            .max_by_key(|mount| mount.tree_path().len())
    }

    /// The subvolume that actually holds `path` (§14.1, Appendix B.9).
    ///
    /// The answer is the boundary whose visible path is the longest prefix of `path`, taken over
    /// every mount that shows it. That is what makes a plain directory resolve to its
    /// *containing* subvolume: Appendix B.9's `looks-like-a-subvol` sits inside `@`, has no
    /// metadata of its own, and is therefore answered with `@`.
    #[must_use]
    pub fn containing(&self, path: &Path) -> Option<&SubvolumeBoundary> {
        let target = path.to_string_lossy().into_owned();
        self.boundaries
            .iter()
            .filter_map(|boundary| {
                self.visible_paths(boundary)
                    .into_iter()
                    .filter_map(|visible| {
                        let visible = visible.to_string_lossy().into_owned();
                        contains(&visible, &target).then_some(visible.len())
                    })
                    .max()
                    .map(|depth| (depth, boundary))
            })
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, boundary)| boundary)
    }

    /// Every subvolume whose contents a snapshot of `boundary` would not hold (§14.3, §59.3).
    ///
    /// Two shapes count, and missing the second is how a plan comes to claim that a root snapshot
    /// covers `/var`:
    ///
    /// - a subvolume nested inside this one in the filesystem tree, such as `@var/lib-app` inside
    ///   `@var`;
    /// - a subvolume mounted *beneath* this one's mountpoint, such as the separate `@var` mounted
    ///   at `/var` inside the root subvolume's tree. It is a sibling in the filesystem tree and a
    ///   hole in the snapshot all the same, because what the snapshot holds at `var/` is the empty
    ///   directory the mount covers up.
    ///
    /// So the comparison is made on visible paths where the mount table gives them, and falls back
    /// to tree paths where it does not. These are exactly the holes in what a snapshot of
    /// `boundary` covers, and naming each one is what lets the asset say what it does not hold
    /// instead of implying it holds everything beneath the path (§55.4 case 18).
    #[must_use]
    pub fn nested_within(&self, boundary: &SubvolumeBoundary) -> Vec<&SubvolumeBoundary> {
        let outer = self.visible_path(boundary);
        let mut nested: Vec<&SubvolumeBoundary> = self
            .boundaries
            .iter()
            .filter(|candidate| candidate.id() != boundary.id())
            .filter(|candidate| match (outer.as_ref(), self.visible_path(candidate)) {
                (Some(outer), Some(inner)) => inner.starts_with(outer) && inner != *outer,
                _ => candidate.is_nested_in(boundary),
            })
            .collect();
        nested.sort_by(|left, right| left.tree_path().cmp(right.tree_path()));
        nested
    }

    /// The subvolumes a plan changing `paths` must each snapshot separately (§14.3, §59.3).
    ///
    /// The result is §14.3's worked example computed rather than recited: a plan touching
    /// `/etc/nginx/nginx.conf` and `/var/lib/app/state.db` needs `@root` and `@var`, and does not
    /// need `@home`. A path inside a nested subvolume produces its own entry — the parent's
    /// snapshot does not reach it, and §55.4 case 19 requires it to receive a snapshot of its
    /// own.
    ///
    /// Paths that resolve to no subvolume are returned in [`RequiredProtection::unresolved`]
    /// rather than dropped. §56.3 makes an unmapped path a refusal for the caller to raise, and a
    /// silently shorter list is how a plan comes to be called protected while a target is not.
    #[must_use]
    pub fn required_for(&self, paths: &[&Path]) -> RequiredProtection {
        let mut required: Vec<RequiredSubvolume> = Vec::new();
        let mut unresolved: Vec<Arc<str>> = Vec::new();
        for path in paths {
            let text: Arc<str> = Arc::from(path.to_string_lossy().into_owned());
            let Some(boundary) = self.containing(path) else {
                unresolved.push(text);
                continue;
            };
            match required
                .iter_mut()
                .find(|entry| entry.boundary.id() == boundary.id())
            {
                Some(entry) => {
                    if !entry.changed.contains(&text) {
                        entry.changed.push(text);
                    }
                }
                None => required.push(RequiredSubvolume {
                    boundary: boundary.clone(),
                    visible_path: self.visible_path(boundary),
                    changed: vec![text],
                    nested: self
                        .nested_within(boundary)
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>(),
                }),
            }
        }
        required.sort_by(|left, right| left.boundary.tree_path().cmp(right.boundary.tree_path()));
        RequiredProtection {
            required,
            unresolved,
        }
    }
}

/// One subvolume a plan must snapshot, and what a snapshot of it would leave out (§14.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredSubvolume {
    boundary: SubvolumeBoundary,
    visible_path: Option<PathBuf>,
    changed: Vec<Arc<str>>,
    nested: Vec<SubvolumeBoundary>,
}

impl RequiredSubvolume {
    /// The subvolume.
    #[must_use]
    pub const fn boundary(&self) -> &SubvolumeBoundary {
        &self.boundary
    }

    /// Where it is mounted, where it is mounted at all.
    #[must_use]
    pub fn visible_path(&self) -> Option<&Path> {
        self.visible_path.as_deref()
    }

    /// The planned changes that land in it.
    #[must_use]
    pub fn changed(&self) -> &[Arc<str>] {
        &self.changed
    }

    /// The nested subvolumes a snapshot of it would not contain (§14.3).
    #[must_use]
    pub fn nested(&self) -> &[SubvolumeBoundary] {
        &self.nested
    }
}

/// What protection a plan's changed paths require, and what could not be mapped (§14.3, §56.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredProtection {
    required: Vec<RequiredSubvolume>,
    unresolved: Vec<Arc<str>>,
}

impl RequiredProtection {
    /// The subvolumes that each need their own snapshot.
    #[must_use]
    pub fn required(&self) -> &[RequiredSubvolume] {
        &self.required
    }

    /// The paths no subvolume could be resolved for.
    #[must_use]
    pub fn unresolved(&self) -> &[Arc<str>] {
        &self.unresolved
    }

    /// Whether a given subvolume is in the required set.
    #[must_use]
    pub fn includes(&self, tree_path: &str) -> bool {
        let wanted = tree_path.trim_matches('/');
        self.required
            .iter()
            .any(|entry| entry.boundary.tree_path() == wanted)
    }
}
