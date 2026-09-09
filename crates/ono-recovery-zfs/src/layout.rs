//! What the pool actually contains, as §13.1's discovery establishes it.
//!
//! §13.1 lists six things a plan needs resolved for every path it targets: the mount, the
//! dataset, the descendant dataset boundaries relevant to the target tree, the snapshots relevant
//! to rollback constraints, the clones and bookmarks that affect destructive rollback, and pool
//! health sufficient for the operation. [`Layout`] is all six as one value, read once and then
//! reasoned over, so the answers a plan is built from all describe the same instant.
//!
//! Two rules are properties of the types here rather than checks somebody remembers to write.
//! [`Layout::dataset_of_path`] resolves a path by *mount metadata* — Appendix B.8 forbids
//! inferring dataset identity from a path naming convention, so a directory called `tank/data`
//! inside `tank/home` resolves to `tank/home`. And [`Layout::descendants_of`] treats the dataset
//! as the snapshot boundary §13.4 says it is, so a child dataset is a separate boundary and never
//! part of its parent's.

use std::sync::Arc;

use ono_change_core::{FilesystemKind, ResolvedMount};
use ono_value::ErrorValue;

use crate::parse::{Property, number, property_of, rows};

/// One ZFS filesystem, as `zfs list -H -p -t filesystem` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dataset {
    /// The dataset name, which is its identity — `tank/data/customer`.
    pub name: Arc<str>,
    /// The `mountpoint` property, which may be `-`, `none` or `legacy`.
    pub mountpoint: Arc<str>,
    /// Whether ZFS reports it mounted right now (§13.7).
    pub mounted: bool,
    /// Bytes used, from `-p`.
    pub used: Option<u128>,
    /// Bytes available in the containing pool for this dataset.
    pub available: Option<u128>,
    /// Bytes referenced.
    pub referenced: Option<u128>,
    /// The snapshot this dataset was cloned from, where it is a clone (§13.6).
    pub origin: Option<Arc<str>>,
    /// The `canmount` property, which decides whether a restore can reach it through a mount.
    pub canmount: Arc<str>,
}

impl Dataset {
    /// The pool the dataset belongs to, which is the first component of its name.
    #[must_use]
    pub fn pool(&self) -> &str {
        self.name.split('/').next().unwrap_or(&self.name)
    }

    /// Whether the dataset has a mountpoint a restore could write a file through.
    ///
    /// `none` and `legacy` both mean ZFS will not place it, so §13.5's selective restore has no
    /// `.zfs/snapshot` path to read out of and the provider says so rather than guessing one.
    #[must_use]
    pub fn has_placed_mountpoint(&self) -> bool {
        self.mountpoint.starts_with('/')
    }
}

/// One snapshot, as `zfs list -H -p -t snapshot` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The full name, `dataset@snapshot`.
    pub name: Arc<str>,
    /// The dataset half.
    pub dataset: Arc<str>,
    /// The short half, after the `@`.
    pub short: Arc<str>,
    /// Creation time, in seconds since the epoch, as `-p` prints it.
    pub creation: Option<u128>,
    /// The GUID, which is the identity §56.1 asks to be proven exact.
    pub guid: Arc<str>,
    /// Bytes the snapshot itself uses, which §38.2 forbids rendering as the cost of keeping it.
    pub used: Option<u128>,
    /// Bytes the snapshot references.
    pub referenced: Option<u128>,
    /// Whether a destroy has been deferred because a hold exists.
    pub defer_destroy: bool,
}

/// One bookmark, as `zfs list -H -p -t bookmark` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    /// The full name, `dataset#bookmark`.
    pub name: Arc<str>,
    /// The dataset half.
    pub dataset: Arc<str>,
    /// Creation time in seconds since the epoch.
    pub creation: Option<u128>,
    /// The GUID, which is the snapshot the bookmark was taken from (§13.6).
    pub guid: Arc<str>,
}

/// One pool, as `zpool list -H -p` reports it, with the state `zpool status` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
    /// The pool name.
    pub name: Arc<str>,
    /// Total size in bytes.
    pub size: Option<u128>,
    /// Bytes allocated.
    pub allocated: Option<u128>,
    /// Bytes free, which Appendix D.3's floor is compared against.
    pub free: Option<u128>,
    /// Percent capacity used.
    pub capacity: Option<u128>,
    /// Percent fragmentation.
    pub fragmentation: Option<u128>,
    /// The health word — `ONLINE`, `DEGRADED`, `FAULTED`.
    pub health: Arc<str>,
    /// The `errors:` line `zpool status` printed for this pool, where one was read.
    pub errors: Option<Arc<str>>,
}

impl Pool {
    /// Whether the pool is healthy enough for §13.1's "proposed protection operation".
    ///
    /// `ONLINE` is the only word this provider treats as fit for creating a recovery point.
    /// §56.3 makes anything else a reason to say so rather than to snapshot onto a pool that is
    /// resilvering, degraded or reporting data errors.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.health.as_ref() == "ONLINE"
            && self
                .errors
                .as_deref()
                .is_none_or(|errors| errors == "No known data errors")
    }
}

/// One line of `mountinfo(5)`, reduced to what Appendix B.8 needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// The kernel's mount id.
    pub id: Arc<str>,
    /// Where it is mounted.
    pub mount_point: Arc<str>,
    /// The filesystem type as the kernel spells it.
    pub filesystem: Arc<str>,
    /// The mount source. For ZFS this is the dataset name, and it is the only thing Appendix B.8
    /// permits dataset identity to be read from.
    pub source: Arc<str>,
    /// The mount's root within its filesystem.
    pub root: Arc<str>,
    /// The per-mount options.
    pub options: Vec<Arc<str>>,
}

impl Mount {
    /// How Ono classifies the filesystem (Appendix B).
    #[must_use]
    pub fn kind(&self) -> FilesystemKind {
        FilesystemKind::of_mount_type(&self.filesystem)
    }

    /// The vocabulary value `ono-change-core` speaks in.
    #[must_use]
    pub fn resolved(&self) -> ResolvedMount {
        ResolvedMount::new(
            Arc::clone(&self.id),
            Arc::clone(&self.mount_point),
            Arc::clone(&self.filesystem),
            Arc::clone(&self.source),
            Arc::clone(&self.root),
        )
        .with_options(self.options.clone())
    }
}

/// The mount table a resolution is performed against (Appendix B.1, B.8).
///
/// It is a value rather than a read of `/proc` inside the resolver, so Appendix G.2's
/// deliberately misleading layouts — a directory named like a dataset, an ext4 mount called
/// `/pool` — can be stated exactly by a test and by the recorded fixture alike.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MountTable {
    mounts: Vec<Mount>,
}

impl MountTable {
    /// Reads recorded or live `mountinfo(5)` text.
    ///
    /// Lines that do not carry the ` - ` separator are skipped: a mount table with one unreadable
    /// line is still a mount table, and the resolution of a path whose mount was on that line
    /// then finds nothing, which is the refusing direction.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let mounts = text.lines().filter_map(parse_mount_line).collect();
        Self { mounts }
    }

    /// The mounts, in the order the kernel listed them.
    #[must_use]
    pub fn mounts(&self) -> &[Mount] {
        &self.mounts
    }

    /// Whether the table holds no mounts at all, which §56.3 makes a refusal rather than a guess.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }

    /// The mount whose mount point is the longest path-prefix of `path`.
    ///
    /// Longest wins because that is what the kernel resolves to: `/tank/data/customer` is served
    /// by the `tank/data/customer` mount and not by the `tank/data` one above it, and §13.4's
    /// whole point is that those are different snapshot boundaries.
    #[must_use]
    pub fn covering(&self, path: &str) -> Option<&Mount> {
        self.mounts
            .iter()
            .filter(|mount| is_beneath(path, &mount.mount_point))
            .max_by_key(|mount| mount.mount_point.len())
    }

    /// The mount of the ZFS dataset called `dataset`, where the table shows one.
    #[must_use]
    pub fn of_dataset(&self, dataset: &str) -> Option<&Mount> {
        self.mounts
            .iter()
            .find(|mount| mount.kind() == FilesystemKind::Zfs && mount.source.as_ref() == dataset)
    }
}

/// Whether `path` is `ancestor` or lies beneath it, comparing whole path components.
///
/// String prefixing would make `/tank/database` a child of `/tank/data`, which is exactly the
/// class of mistake §13.4 and Appendix B.8 exist to prevent.
#[must_use]
pub fn is_beneath(path: &str, ancestor: &str) -> bool {
    if ancestor == "/" {
        return path.starts_with('/');
    }
    path == ancestor
        || (path.starts_with(ancestor) && path.as_bytes().get(ancestor.len()) == Some(&b'/'))
}

/// Whether `dataset` is `ancestor` or a descendant of it, comparing whole name components.
#[must_use]
pub fn is_descendant(dataset: &str, ancestor: &str) -> bool {
    dataset == ancestor
        || (dataset.starts_with(ancestor) && dataset.as_bytes().get(ancestor.len()) == Some(&b'/'))
}

fn parse_mount_line(line: &str) -> Option<Mount> {
    let (before, after) = line.split_once(" - ")?;
    let mut head = before.split_whitespace();
    let id = head.next()?;
    let _parent = head.next()?;
    let _device = head.next()?;
    let root = head.next()?;
    let mount_point = head.next()?;
    let options = head.next()?;
    let mut tail = after.split_whitespace();
    let filesystem = tail.next()?;
    let source = tail.next()?;
    Some(Mount {
        id: Arc::from(id),
        mount_point: Arc::from(mount_point),
        filesystem: Arc::from(filesystem),
        source: Arc::from(source),
        root: Arc::from(root),
        options: options.split(',').map(Arc::from).collect(),
    })
}

/// Everything §13.1 asks discovery to resolve, read at one instant.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    datasets: Vec<Dataset>,
    snapshots: Vec<Snapshot>,
    order: Vec<Arc<str>>,
    bookmarks: Vec<Bookmark>,
    pools: Vec<Pool>,
    mounts: MountTable,
}

impl Layout {
    /// Assembles a layout from the readings of the six §13.1 questions.
    #[must_use]
    pub fn new(
        datasets: Vec<Dataset>,
        snapshots: Vec<Snapshot>,
        order: Vec<Arc<str>>,
        bookmarks: Vec<Bookmark>,
        pools: Vec<Pool>,
        mounts: MountTable,
    ) -> Self {
        Self {
            datasets,
            snapshots,
            order,
            bookmarks,
            pools,
            mounts,
        }
    }

    /// The filesystems.
    #[must_use]
    pub fn datasets(&self) -> &[Dataset] {
        &self.datasets
    }

    /// The snapshots.
    #[must_use]
    pub fn snapshots(&self) -> &[Snapshot] {
        &self.snapshots
    }

    /// The bookmarks.
    #[must_use]
    pub fn bookmarks(&self) -> &[Bookmark] {
        &self.bookmarks
    }

    /// The pools.
    #[must_use]
    pub fn pools(&self) -> &[Pool] {
        &self.pools
    }

    /// The mount table the resolution was performed against.
    #[must_use]
    pub const fn mounts(&self) -> &MountTable {
        &self.mounts
    }

    /// Whether creation order was established, which is what decides "newer" (Appendix D.5).
    ///
    /// ZFS prints creation to the second, and two snapshots taken in the same second carry the
    /// same number. The order `zfs list -s creation` emits is therefore the evidence, and without
    /// it §56.1's "whether it is the latest relevant snapshot" is a fact this provider does not
    /// have.
    #[must_use]
    pub fn has_creation_order(&self) -> bool {
        !self.order.is_empty()
    }

    /// The dataset called `name`.
    #[must_use]
    pub fn dataset(&self, name: &str) -> Option<&Dataset> {
        self.datasets
            .iter()
            .find(|dataset| dataset.name.as_ref() == name)
    }

    /// The snapshot called `name`, in full `dataset@snapshot` form.
    #[must_use]
    pub fn snapshot(&self, name: &str) -> Option<&Snapshot> {
        self.snapshots
            .iter()
            .find(|snapshot| snapshot.name.as_ref() == name)
    }

    /// The pool called `name`.
    #[must_use]
    pub fn pool(&self, name: &str) -> Option<&Pool> {
        self.pools.iter().find(|pool| pool.name.as_ref() == name)
    }

    /// The dataset that holds `path`, resolved through mount metadata (Appendix B.8).
    ///
    /// The answer comes from the kernel's mount table and is then confirmed against `zfs list`,
    /// so a directory *named* like a dataset resolves to the dataset it lives in and an ext4
    /// mount called `/pool` resolves to no ZFS dataset at all.
    #[must_use]
    pub fn dataset_of_path(&self, path: &str) -> Option<&Dataset> {
        let mount = self.mounts.covering(path)?;
        if mount.kind() != FilesystemKind::Zfs {
            return None;
        }
        self.dataset(&mount.source)
    }

    /// The datasets strictly beneath `ancestor` — §13.4's separate snapshot boundaries.
    #[must_use]
    pub fn descendants_of(&self, ancestor: &str) -> Vec<&Dataset> {
        self.datasets
            .iter()
            .filter(|dataset| {
                dataset.name.as_ref() != ancestor && is_descendant(&dataset.name, ancestor)
            })
            .collect()
    }

    /// The descendants of `ancestor` whose mount point lies inside `path`.
    ///
    /// This is §13.3's "several descendant datasets must be protected at one logical point": the
    /// tree the operator named, cut at the dataset boundaries that actually fall inside it.
    #[must_use]
    pub fn descendants_in_tree(&self, ancestor: &str, path: &str) -> Vec<&Dataset> {
        self.descendants_of(ancestor)
            .into_iter()
            .filter(|dataset| {
                dataset.has_placed_mountpoint() && is_beneath(&dataset.mountpoint, path)
            })
            .collect()
    }

    /// The snapshots of `dataset`, in the order ZFS created them.
    #[must_use]
    pub fn snapshots_of(&self, dataset: &str) -> Vec<&Snapshot> {
        let mut listed: Vec<&Snapshot> = self
            .snapshots
            .iter()
            .filter(|snapshot| snapshot.dataset.as_ref() == dataset)
            .collect();
        listed.sort_by_key(|snapshot| self.creation_rank(&snapshot.name));
        listed
    }

    /// The snapshots of the same dataset ZFS created after `name` (Appendix D.5).
    #[must_use]
    pub fn newer_snapshots(&self, name: &str) -> Vec<&Snapshot> {
        let Some(target) = self.snapshot(name) else {
            return Vec::new();
        };
        let rank = self.creation_rank(name);
        self.snapshots_of(&target.dataset)
            .into_iter()
            .filter(|snapshot| {
                snapshot.name.as_ref() != name && self.creation_rank(&snapshot.name) > rank
            })
            .collect()
    }

    /// Whether `name` is the newest snapshot its dataset holds (§56.1).
    #[must_use]
    pub fn is_latest_snapshot(&self, name: &str) -> bool {
        self.newer_snapshots(name).is_empty()
    }

    /// The bookmarks of `dataset`.
    #[must_use]
    pub fn bookmarks_of(&self, dataset: &str) -> Vec<&Bookmark> {
        self.bookmarks
            .iter()
            .filter(|bookmark| bookmark.dataset.as_ref() == dataset)
            .collect()
    }

    /// The datasets that are clones of `snapshot`, as their `origin` property names it (§13.6).
    #[must_use]
    pub fn clones_of(&self, snapshot: &str) -> Vec<&Dataset> {
        self.datasets
            .iter()
            .filter(|dataset| {
                dataset
                    .origin
                    .as_deref()
                    .is_some_and(|origin| origin == snapshot)
            })
            .collect()
    }

    /// Where a snapshot sits in ZFS's own creation order, or the end when it is not listed.
    fn creation_rank(&self, name: &str) -> usize {
        self.order
            .iter()
            .position(|listed| listed.as_ref() == name)
            .unwrap_or(usize::MAX)
    }
}

/// Reads `zfs list -H -p -t filesystem -o name,mountpoint,mounted,type,used,available,referenced,origin,canmount`.
///
/// # Errors
///
/// `recovery.provider_unavailable` when a row does not carry the nine requested columns.
pub fn datasets(program: &str, text: &str) -> Result<Vec<Dataset>, ErrorValue> {
    rows(program, text, 9)?
        .into_iter()
        .map(|fields| {
            let field = |index: usize| fields.get(index).copied().unwrap_or("-");
            Ok(Dataset {
                name: Arc::from(field(0)),
                mountpoint: Arc::from(field(1)),
                mounted: field(2) == "yes",
                used: number(field(4)),
                available: number(field(5)),
                referenced: number(field(6)),
                origin: (field(7) != "-").then(|| Arc::from(field(7))),
                canmount: Arc::from(field(8)),
            })
        })
        .collect()
}

/// Reads `zfs list -H -p -t filesystem -o name,origin`, which is how clones are found (§13.6).
///
/// # Errors
///
/// `recovery.provider_unavailable` when a row does not carry two columns.
pub fn origins(program: &str, text: &str) -> Result<Vec<(Arc<str>, Option<Arc<str>>)>, ErrorValue> {
    Ok(rows(program, text, 2)?
        .into_iter()
        .map(|fields| {
            let name: Arc<str> = Arc::from(fields.first().copied().unwrap_or("-"));
            let origin = fields.get(1).copied().unwrap_or("-");
            (name, (origin != "-").then(|| Arc::from(origin)))
        })
        .collect())
}

/// Reads `zfs list -H -p -t snapshot -o name,creation,used,referenced,guid,defer_destroy`.
///
/// # Errors
///
/// `recovery.provider_unavailable` when a row does not carry the six requested columns, or when a
/// name does not carry the `@` that makes it a snapshot.
pub fn snapshots(program: &str, text: &str) -> Result<Vec<Snapshot>, ErrorValue> {
    rows(program, text, 6)?
        .into_iter()
        .map(|fields| {
            let field = |index: usize| fields.get(index).copied().unwrap_or("-");
            let name = field(0);
            let (dataset, short) = name.split_once('@').ok_or_else(|| {
                ono_change_core::error::tool_failed(
                    program,
                    "a snapshot listing carried a name without the `@` that separates the dataset \
                     from the snapshot",
                )
            })?;
            Ok(Snapshot {
                name: Arc::from(name),
                dataset: Arc::from(dataset),
                short: Arc::from(short),
                creation: number(field(1)),
                used: number(field(2)),
                referenced: number(field(3)),
                guid: Arc::from(field(4)),
                defer_destroy: field(5) == "on",
            })
        })
        .collect()
}

/// Reads `zfs list -H -p -t snapshot -o name,creation -s creation` into creation order.
///
/// # Errors
///
/// `recovery.provider_unavailable` when a row does not carry two columns.
pub fn creation_order(program: &str, text: &str) -> Result<Vec<Arc<str>>, ErrorValue> {
    Ok(rows(program, text, 2)?
        .into_iter()
        .map(|fields| Arc::from(fields.first().copied().unwrap_or("-")))
        .collect())
}

/// Reads `zfs list -H -p -t bookmark -o name,creation,guid`.
///
/// # Errors
///
/// `recovery.provider_unavailable` when a row does not carry three columns, or when a name does
/// not carry the `#` that makes it a bookmark.
pub fn bookmarks(program: &str, text: &str) -> Result<Vec<Bookmark>, ErrorValue> {
    rows(program, text, 3)?
        .into_iter()
        .map(|fields| {
            let field = |index: usize| fields.get(index).copied().unwrap_or("-");
            let name = field(0);
            let (dataset, _) = name.split_once('#').ok_or_else(|| {
                ono_change_core::error::tool_failed(
                    program,
                    "a bookmark listing carried a name without the `#` that separates the dataset \
                     from the bookmark",
                )
            })?;
            Ok(Bookmark {
                name: Arc::from(name),
                dataset: Arc::from(dataset),
                creation: number(field(1)),
                guid: Arc::from(field(2)),
            })
        })
        .collect()
}

/// Reads `zpool list -H -p -o name,size,alloc,free,capacity,fragmentation,health`.
///
/// # Errors
///
/// `recovery.provider_unavailable` when a row does not carry the seven requested columns.
pub fn pools(program: &str, text: &str) -> Result<Vec<Pool>, ErrorValue> {
    rows(program, text, 7)?
        .into_iter()
        .map(|fields| {
            let field = |index: usize| fields.get(index).copied().unwrap_or("-");
            Ok(Pool {
                name: Arc::from(field(0)),
                size: number(field(1)),
                allocated: number(field(2)),
                free: number(field(3)),
                capacity: number(field(4)),
                fragmentation: number(field(5)),
                health: Arc::from(field(6)),
                errors: None,
            })
        })
        .collect()
}

/// Reads the `pool:`, `state:` and `errors:` lines of `zpool status` (§13.1's pool health).
#[must_use]
pub fn pool_status(text: &str) -> Vec<(Arc<str>, Arc<str>, Option<Arc<str>>)> {
    let mut statuses: Vec<(Arc<str>, Arc<str>, Option<Arc<str>>)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(name) = trimmed.strip_prefix("pool: ") {
            statuses.push((Arc::from(name.trim()), Arc::from("UNKNOWN"), None));
        } else if let Some(state) = trimmed.strip_prefix("state: ")
            && let Some(last) = statuses.last_mut()
        {
            last.1 = Arc::from(state.trim());
        } else if let Some(errors) = trimmed.strip_prefix("errors: ")
            && let Some(last) = statuses.last_mut()
        {
            last.2 = Some(Arc::from(errors.trim()));
        }
    }
    statuses
}

/// Folds `zpool status` state and error lines into the pools `zpool list` reported.
#[must_use]
pub fn with_status(mut listed: Vec<Pool>, status: &str) -> Vec<Pool> {
    for (name, state, errors) in pool_status(status) {
        if let Some(pool) = listed.iter_mut().find(|pool| pool.name == name) {
            pool.health = state;
            pool.errors = errors;
        }
    }
    listed
}

/// Reads the `mounted`, `mountpoint`, `readonly` and `snapdir` properties of a dataset (§13.7).
#[must_use]
pub fn mount_state(listed: &[Property], dataset: &str) -> MountState {
    MountState {
        mounted: property_of(listed, dataset, "mounted").map(|entry| entry.value.as_ref() == "yes"),
        mountpoint: property_of(listed, dataset, "mountpoint")
            .filter(|entry| !entry.is_absent())
            .map(|entry| Arc::clone(&entry.value)),
        read_only: property_of(listed, dataset, "readonly")
            .map(|entry| entry.value.as_ref() == "on"),
        snapdir: property_of(listed, dataset, "snapdir").map(|entry| Arc::clone(&entry.value)),
    }
}

/// What ZFS says about where a dataset is placed and whether it is there now (§13.7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MountState {
    /// Whether the dataset is mounted, or `None` when the property could not be read.
    pub mounted: Option<bool>,
    /// Where it is placed, where the property named a path.
    pub mountpoint: Option<Arc<str>>,
    /// Whether it is read-only, which blocks a restore into it (Appendix G.2).
    pub read_only: Option<bool>,
    /// The `snapdir` property, where it was read.
    pub snapdir: Option<Arc<str>>,
}

impl MountState {
    /// Whether the mount requirement of §56.1 could be established at all.
    #[must_use]
    pub const fn is_established(&self) -> bool {
        self.mounted.is_some() && self.mountpoint.is_some()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_not_call_a_sibling_directory_a_child_of_the_one_beside_it() {
        assert!(
            !is_beneath("/tank/database", "/tank/data"),
            "§13.4: boundaries are compared by whole path components"
        );
        assert!(is_beneath("/tank/data/customer", "/tank/data"));
        assert!(is_beneath("/tank/data", "/tank/data"));
    }

    #[test]
    fn should_not_call_a_similarly_named_dataset_a_descendant() {
        assert!(
            !is_descendant("tank/database", "tank/data"),
            "Appendix B.8: identity is compared by name components, never by string prefix"
        );
        assert!(is_descendant("tank/data/customer", "tank/data"));
    }
}
