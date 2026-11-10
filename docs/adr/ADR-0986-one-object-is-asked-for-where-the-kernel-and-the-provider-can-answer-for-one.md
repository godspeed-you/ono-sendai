# ADR-0986: One object is asked for where the kernel and the provider can answer for one

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §11.2, §22.2, §27.1; v0.4.1 §34.4, §36.1; ADR-0497, ADR-0576, ADR-0947; issues #296, #187
- Decided by: agent (autonomous)

## Context

v0.4.1 §34.4: "A local neighborhood query SHOULD NOT require construction of the complete
system graph when provider APIs can answer the neighborhood incrementally. Any unavoidable
global build MUST be visible in `explain`." ADR-0947 delivered the second sentence. The first
was unmet in two places a user meets constantly (#296):

- `trace process <pid>` read the whole process table to find one parent and a few children —
  once per trace, shared between the nodes it expands, and still every process of the host.
- `enter process/<pid>`, a selector that names one object by its key, swept the whole `process`
  target on an index miss.

Both have an incremental answer. The kernel lists a process's children under
`/proc/<pid>/task/<tid>/children`, the process provider reads one pid when it is given one, and
a `<type>/<key>` selector carries the field its provider is keyed on.

## Decision

**The process tree is read from the kernel's own list.** `linux.process-tree` reads the children
of the subject from `/proc/<pid>/task/<tid>/children` and asks the process provider for those
pids and for the parent, one pinned question each. Where the kernel publishes no such list — a
kernel without `CONFIG_PROC_CHILDREN`, a process that has gone — the process table is read
once per trace, as before, and the tree is the same either way.

**A `<type>/<key>` selector asks for the object it names.** On an index miss the provider that
serves the type is asked with a selector on the field such an object is named by, the key
converted to the type the schema declares. The target is swept only where that finds nothing:
`process/nginx` is a name, and a name is found by the sweep (ADR-0497).

**What stays whole, and why.** A process's sockets are joined through the socket table, which
the kernel answers by dump and not by inode; a user's processes, a service's processes and a
mount's users are questions about a whole target. A bare name on an index miss is swept: an
`enter` that stopped early would say `not_found` about something that is there (ADR-0576).
So `trace process` and `enter` keep `acquisition: {scope: global}` — the declaration says what
the command may cost, and neither became local for every argument.

## Consequences

Measured on this build's host with 2,330 processes, debug build, median of five runs:

| command | before | after |
|---|---|---|
| `trace process <pid> --depth 1` | 0.47 s | 0.11 s |
| `trace process <pid>` | 9.25 s | 3.63 s |
| `enter process/<pid>` | 1.16 s | 0.14 s |

The fixture test states the machine-independent form: tracing one process of a table of two
hundred asks the provider for three records.

Hard: the children file is, in the kernel's own words, reliable only while the children do not
exit; a child that exits between the listing and the question is left out, which is the tree
at the moment of the question. A selector miss by bare name is unchanged — that is #187, and
its cost is the enumeration itself.

Encoded by `crates/ono-graph/tests/relationships.rs::should_ask_for_the_parent_and_the_children_alone_when_the_kernel_lists_the_children`,
`::should_read_the_process_table_once_when_the_kernel_does_not_list_children`,
`crates/ono-cli/tests/spatial_contracts.rs::should_ask_for_the_one_object_a_typed_selector_names_rather_than_its_whole_target`
and acceptance case `143-one-object-is-asked-for`.

## Alternatives considered

**Push a `ppid` selector into the process provider.** The provider would read the same file;
the graph kernel already reads `/proc` for descriptors and control groups, and a selector on a
nullable field is not something the pushdown contract of ADR-0980 admits.

**Pin every lookup of every kernel** — the user of a process, the filesystem of a mount. A
provider that answers a selector by filtering its whole target would then be enumerated once
per node instead of once per trace, which is the twenty-seven-second trace the shared snapshot
was introduced against.

**Declare `trace process` local.** It reads the socket table whenever the process holds a
socket.
