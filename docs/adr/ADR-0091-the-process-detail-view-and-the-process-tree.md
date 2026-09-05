# ADR-0091: `inspect process` is the producer's query read closer; `--tree` nests children as an extension field

- Status: accepted
- Date: 2026-08-27
- Spec refs: §9.1, §10.4, §10.5, §23.1, §27.1, §33.1, §35.3; ADR-0015 T13, ADR-0021, ADR-0076
- Decided by: agent (autonomous)

## Context

Spec §9.1 gives `inspect process <pid|object>` the output `ProcessDetail` and §33.1 shows the
view: the Process fields plus the parent by name, the cgroup, open files and sockets.
`docs/contracts/commands/process.yaml` advertised `ono.process-detail/1` for it, `deferred.yaml`
carried the schema as a phase-D debt, and no implementation bound `ono.process.inspect`.

`ono_provider_api::Provider` has one read primitive, `snapshot(&Query)`, and the procfs provider
answers it with `ono.process/1`. A second primitive for "one object, read closer" would have to
be added to the trait, the registry, the remote protocol and the KUANG/11 bridge for one command.

`get process --tree` was parsed and dropped (STATE.md: "provider options are silently
ignored"). `ono.process/1` has no field for children, and every record is built against its
schema — an unknown field name is `type.unknown_field`.

## Decision

### 1. `ono.process-detail/1`

`docs/contracts/schemas/process-detail.v1.yaml`: every `ono.process/1` field, same identity
`(pid, started)`, plus `parent` (`ref<ono.process/1>` — pid, name and start time of the parent;
null for init), `cgroup` (the unified-hierarchy path from `/proc/<pid>/cgroup`; null on a kernel
without cgroups), `open_files` (`list<path>` — descriptors that resolve to a live path; pipes,
anonymous inodes and deleted files are not files), and `sockets` (`list<int>` — socket inodes,
the `inode` of `ono.socket/1`, so the two join). A descriptor table this user may not read
puts the `io.permission_denied` error *in* both fields, as the provider does for `cwd` and
`exe` (spec §10.5, the crate's rule). §33.1's `open_files 127` is what a renderer shows for a
list; the data is the list.

### 2. `inspect` is the producer's query with `detail` set

`InspectCommand` (`crates/ono-command/src/impls/inspect.rs`) builds the contract's query as
`get` would — the frame's implicit selector included (ADR-0076) — adds the option
`detail = true`, and the procfs provider answers that query with `ono.process-detail/1` records
instead of `ono.process/1` ones. Same enumeration, same selector semantics, one more read per
process. The provider advertises both schemas.

The object piped in is asked for by **every non-null identity field it carries** — `pid` and
`started` — as field selectors, so `get process 4419 | inspect process` cannot inspect a
process that reused the pid in between (ADR-0015 T13). Inspecting nothing is the provider's
own `io.not_found` (E0301) for a pid, and `resolve.target_not_found` when no object was named
at all; the command never answers with an empty stream, because a detail view of nothing is
not a value.

### 3. `--tree` nests children under the extension key `children`

With `tree = true` the procfs provider reads the whole table, then emits only the roots —
processes whose parent is not in the stream — and nests each process's children beneath it as
the extension field `children` (spec §10.4, `RecordBuilder::set_extra`): a list of
`ono.process/1` records, themselves carrying `children`. A record in a flat stream carries no
such key at all, which is the difference between "not asked for" and "unknown" (null). The
tree is built deepest-first without recursion, so a pathological chain of parents cannot
overflow the stack. Selectors and `--user`/`--group` still filter the records that go into the
tree; a filtered-out parent makes its children roots.

## Consequences

- `inspect process 1`, `get process 1 | inspect process`, `inspect process 4000000` (E0301)
  are proven by `crates/ono-cli/tests/processes_missing.rs`; `get process --tree` by
  `options_and_selectors_missing.rs::should_nest_children_under_their_parents_when_tree_is_requested`.
- `ono.process-detail/1` leaves `deferred.yaml`; `crates/ono-value` embeds it.
- Any provider can deliver `inspect <target>` by honouring `detail` in its `snapshot` and
  advertising a detail schema; the command crate binds `ono.process.inspect` today and the
  same implementation serves the next target.
- `children` is not in `ono.process/1`; `where children …` is not type-checked. A tree that
  needs a typed field is a schema version.

## Alternatives considered

- **A `detail` primitive on the provider trait.** Every registry, bridge and protocol layer
  would grow a method for one verb. Rejected.
- **`open_files`/`sockets` as counts.** They are what §33.1 renders, but a count is what a
  pipeline cannot use; a list renders as a count wherever a column needs one. Rejected.
- **A `children` field in `ono.process/1`.** Every flat record would carry `children: null`
  — an unknown that is not unknown. Rejected in favour of the extension key.
- **A separate `ono.process-tree/1` node schema.** Roots would not be processes, so `--tree |
  where name == …` would match nothing. Rejected.
