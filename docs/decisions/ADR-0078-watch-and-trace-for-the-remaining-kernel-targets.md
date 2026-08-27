# ADR-0078: `watch` and `trace` for the remaining kernel targets

- Status: accepted
- Date: 2026-08-27
- Spec refs: §18.2, §18.3, §22.1–§22.4, §23.6, §28.5–§28.7, §31.14; ADR-0024, ADR-0034
- Decided by: agent (autonomous)

## Context

ADR-0034 built the generic watch runtime — snapshot first, poll-and-diff by identity, `source`
on every event — and bound it to process, socket and service only; the other watch commands
stayed `planned` with their event schemas deferred. Likewise `trace` walked the relationships of
spec §22.2 (process tree, descriptors, sockets, unit → process, mount → device) but had no
expansion for a user, an interface, a route, a mount's users or a file's holders, which spec
§22.3 names as the questions `trace` exists to answer (`trace file … --users`). The RED suites
under `crates/ono-cli/tests/*_missing.rs` pin the observable shape; this ADR records what was
free to decide.

## Decision

### Event envelopes are per target, keyed by the target's own name

Every `ono.<target>-event/1` schema has the envelope of `ono.process-event/1` — `kind`, `at`,
`changed`, `source` — and carries the object under the field named after the target: `user`,
`group`, `interface`, `route`, `mount`, `file`. `watch route | where route.interface == "lo"`
therefore reads as `watch process | where process.cpu > 20` does, and the runtime derives the
field from the contract's `target` rather than from a table.

### The cadence is ADR-0034's two seconds, for every target

No target gets a cadence of its own. The account database, the routing tables and the mount
table change rarely and cost little to snapshot; a file tree is the expensive one, and it is
bounded by the tree the user named. `--every` remains the override, and every event says
`source: poll` because none of these providers subscribes yet (the netlink and inotify
subscriptions stay in `docs/STATE.md` → *Next up*, as ADR-0034 left them).

### A file watch observes what is beneath the path

`watch file <path>` is `get file <path>` re-run — one entry, whose mtime moves — only if a
watch of a directory were meant to report the directory. It is not: spec §18.2's example is a
tree, and the contract's `--recursive` says "watch the whole tree". So the watch queries the
file provider with the `root` selector — the walk `find file` performs — one level deep unless
`--recursive`, and a file created under the path arrives as an `added` event carrying the new
`ono.file/1`. Identity is `(device, inode)`, so a rename is a `changed` event and a replacement
is a `removed` plus an `added`.

### The delivered commands leave `planned`

A command this build answers is `experimental` (spec §52: a contract with an implementation,
not yet a compatibility promise), and its `validation_required` flag goes with `planned`. The
event schemas move out of `docs/spec/schemas/deferred.yaml` in the same commit as their
watch, so `spec-check` never sees a stale deferral or an unwritten promise.

### Trace edge sets — exact unless stated

| Subject | Relation | Target | Read from | Confidence |
|---|---|---|---|---|
| `ono.user/1` | `runs` | `ono.process/1` | process `user.uid == uid` | exact |
| `ono.user/1` | `primary-group` | `ono.group/1` | the account's `primary_group.gid` | exact |
| `ono.user/1` | `member-of` | `ono.group/1` | the group's own `members` list | exact |

Identity is numeric (spec §23.6): a process belongs to a user by uid, never by a name two
accounts could share. Targets are listed in pid / gid order so two traces of one machine draw
the same graph (ADR-0034's determinism, applied to graphs).

Further families (storage, network, files) extend this table in the ADRs that deliver them
(ADR-0079, ADR-0080).

## Consequences

- `watch user | take 1 | select kind | to json` is `[{"kind":"snapshot"}]` on every machine,
  and `trace user root` has pid 1 and gid 0 among its nodes on every Linux system.
- `trace user` of an account that owns many processes expands each of them one hop further
  (their children, descriptors, sockets), bounded by `DEFAULT_MAX_NODES`; the shared per-trace
  snapshots of `ono-graph` keep that to one enumeration per target.
- Tests: `crates/ono-cli/tests/identity_missing.rs` (watch user/group, trace user) and
  `crates/ono-graph/tests/relationships.rs` (the user providers against fixtures).

## Alternatives considered

- **A generic `ono.object-event/1`.** Rejected again for the reason ADR-0034 gave: the
  pre-flight field check of spec §11.3 needs the object's field list under a known name.
- **Per-target cadences (files fast, accounts slow).** Rejected: an invisible difference in
  cost between two spellings of the same verb is exactly what spec §18.2 forbids; `--every` is
  the explicit knob.
- **Relating a user to processes by name.** Rejected: names are not identity (spec §23.6), and
  the kernel reports the uid.
