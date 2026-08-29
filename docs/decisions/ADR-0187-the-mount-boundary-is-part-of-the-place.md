# ADR-0187: The mount boundary is part of the place, and the Unix path tree keeps its shape

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §15.1, §15.2, §15.3, §15.4, §3.2, §3.4, §2.18, §44.3, §11.1
- Decided by: agent (autonomous, S4d/S4e)
- Supersedes the directory half of ADR-0135's parent chain

## Context

§15.3 is one normative sentence and one example: "Crossing a mount boundary MUST be
discoverable", shown as a block carrying the local path, the filesystem, the source and whether
the filesystem is remote. §3.2 lists `filesystem` among the scope kinds and §2.18 requires a
scope crossing to be observable in the trail. §44.3 asks for both in one walk: reach the mounts
without knowing their names, enter one, "see the mount boundary/source, and traverse into the
mounted directory".

Three things were missing. A path place carried no boundary at all, so `enter /mnt/backup` looked
exactly like `enter /home`. A step from one filesystem into another recorded
`scope_crossing: null`, because the only crossing the shell computed was between the *objects'*
scopes, and `ProviderBridge` projects every local observation into the one host scope. And
`parent_rules(Directory)` put `mount.backs_directory` ahead of the path — a rule nothing supplied,
so it never fired, and would have replaced a step of the Unix path tree if it had.

## Decision

**1. The boundary is a field of the place, not a line of the renderer.**
`ono.place-view/1` gains a nullable `boundary` of the new schema `ono.mount-boundary/1`, with
§15.3's four fields — `local_path`, `filesystem`, `source`, `remote` — plus the mount's
`read_only` and its `spatial_id`. Every one of them is read from the record `get mount` answered
with (§2.16): the spatial layer composes the mount table, it never reads `/proc/self/mountinfo`
itself. `ono-spatial-render` prints the block §15.3 draws, under the heading and above the exits.

**2. `remote` is decided from the filesystem type and the shape of the source.**
`ono.mount/1` has no `remote` field, and the kernel does not report one. The rule is: a known
network filesystem type (`nfs`, `nfs4`, `cifs`, `smb3`, `afs`, `ceph`, `glusterfs`, `9p`,
`sshfs`, … , with the `fuse.` prefix stripped), or a source of the shape `host:/export` or
`//host/share`. Both halves are conservative — a type this build does not know is local, and a
device path is never called remote — because §2.17 makes an honest `no` better than a guessed
`yes`.

**3. Crossing a mount is a `filesystem` scope crossing, computed second.**
`movement::crossing_between` still asks the two places' own scopes first — a host, a container, a
namespace is the outer boundary and must not be understated (§2.18) — and, only where those are
the same, asks whether the two paths sit on different mounts. The boundary is then between
`filesystem:<mount point>` scopes nested in the session's host scope, so `kind` is `filesystem`,
`entering` is true and `remote` is false: a `filesystem` crossing does not leave the host, and
saying it did would be the overstatement §2.18 warns about. Every movement uses the one function,
so `enter`, `jump` and `up` record the crossing alike.

**4. The Unix path tree keeps its shape, and the mount is where it runs out.**
`parent_rules(Directory)` becomes `[path.parent, mount.backs_directory]` (and
`docs/spec/providers/linux-procfs.yaml` says the same, because `spec-check` compares them).

- §15.1 is unconditional: "Ono MUST preserve canonical Unix filesystem paths and directory
  semantics." The parent of `/mnt/backup` is `/mnt`, mount point or not; making the mount its
  parent replaces a step of the path tree with a step of the storage hierarchy.
- §15.2's `MOUNTS -> DIRECTORY ROOTS` still holds, and now holds where it means something: `/`
  has no path above it, so `up` from `/` reaches the mount that provides it, and from there
  `filesystem.mounted_at` → FILESYSTEMS → STORAGE. That is where the two hierarchies meet.
- §15.3's "discoverable" is delivered by the boundary on the place and the crossing in the
  trail — which is what §3.2 and invariant 18 actually ask for — rather than by rerouting `up`.

**5. `mount.backs_directory` links a mount to every directory it provides.**
The relation's own doc is "the directory a mount provides", and §15.4 lists the mount boundary
among a directory place's neighbours, so the edge is drawn from the providing mount to the
directory rather than only to its mount point. With `path.parent` ahead of it in the rule chain
this changes no `up` except at a directory root, where it is the answer.

**6. Every path place observes three things, once, in one function.**
`storage::observe_place_at` is the single seam every spelling reaches — `enter /etc`,
`jump storage:/data`, `cd` under `follow_cwd`: the object itself, the mount table (so the
boundary can be named), and the enclosing directory (so `up` from a file reaches it, which
`parent_rules(File) = [path.parent]` has asked for since S1 and nothing supplied).

## Consequences

- `enter <mount>; look` shows §15.3's block; `enter /; enter <mount>; trail --json` carries a
  `scope_crossing` of kind `filesystem`; `up` from a file lands on its directory. All three are
  §44.3's walk, and acceptance case `109-spatial-storage.case` runs it in the container.
- `up` from a mount point now goes to the directory above it rather than to the mount. The mount
  is one `enter mount` away and is named in the boundary of every place under it, so nothing is
  hidden; what changed is that the path tree is not silently re-routed.
- A path place costs one extra provider question — the mount table — which the observation cache
  of ADR-0186 answers for ten seconds at a time.
- Exit tests: `spatial_storage_missing::should_show_the_source_device_and_filesystem_when_the_place_is_a_mount_boundary`
  and `…::should_record_the_boundary_crossing_when_traversing_from_the_root_into_a_mounted_directory`.

## Spec deviation

- Section: v0.4 §15.2 read together with §11.1
- Text: "STORAGE -> FILESYSTEMS -> MOUNTS -> VOLUMES/DEVICES when known -> DIRECTORY ROOTS"
- Instead: a directory's canonical parent is the directory above it in the Unix path tree, and
  the mount only where there is no directory above it.
- Why: §15.1 requires canonical Unix path semantics to be preserved, and §15.2 is a hierarchy of
  *spaces*, not a rule that every mount point is re-parented out of its own path. Reading §15.2
  as the stronger rule makes `up` from `/mnt/backup` skip `/mnt`, which is precisely the "filing
  cabinet metaphor" the §15 Intent says the storage model avoids.

## Alternatives considered

- **Give a path place its own `filesystem` scope, so identity carries the mount.** The scope is
  part of the `SpatialId`, so every directory outside `/` would change identity — a large,
  invisible break for a fact that belongs on the movement, not on the object.
- **Leave the crossing to the renderer.** §2.18 asks for it in the *trail*, which is data; a
  renderer-only marker cannot be read by `trail --json` and would not survive a pipe.
- **Keep `mount.backs_directory` ahead of `path.parent` and record it only at the mount point.**
  Then an ordinary directory has no `mount` exit at all, and §15.4's "mount boundary" neighbour
  is missing from every place except the point itself.
