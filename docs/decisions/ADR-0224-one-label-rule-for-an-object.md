# ADR-0224: One label rule for an object

- Status: accepted
- Date: 2026-08-29
- Spec refs: §11.5, §22.4, §27.3
- Decided by: agent (autonomous, `close-data`)

## Context

ADR-0116 left this open in its own consequences: "`ObjectRef::of` labels an object by the first
default-view column outside the identity, while `ono_graph::label_of` has a form per schema; for
a mount the first says the source device, the second the mount point. One rule should serve both."

`ProviderMutation` used both, one per branch, so the same object rendered two ways:

```text
unmount filesystem /                →  ono.mount/1[/] /dev/sda2
get mount / | unmount filesystem    →  ono.mount/1[/]

set process 1 --priority 5              →  ono.process/1[1, …] systemd
get process 1 | set process --priority 5 →  ono.process/1[1, …] process/1 systemd
```

The selector branch built an `ObjectRef`; the piped branch called `ono_graph::label_of`.

## Decision

**A mutation's target is labelled by one rule: `ObjectRef::of`'s** — the first default-view
column outside the identity. Both branches of `ProviderMutation::targets` build an `ObjectRef`,
and the piped branch takes its identity from it too.

That is the right rule for this position because the identity is already printed beside it
(ADR-0116 §1): what the label must add is the thing the identity does not show. For a mount that
is the source device; for a process the command name, without the `process/1` that repeats the
identity.

**`ono_graph::label_of` keeps its own job**, which is a different one: the standalone caption of
a node in a rendered graph, where no identity is printed and `process/1 systemd` is exactly what
a reader needs. The two are not competing implementations of one idea; they are the labels of two
different renderings, and this ADR is the record of which belongs where.

## Consequences

- `unmount filesystem /` and `get mount / | unmount filesystem` render the same `target`, and so
  do the two spellings for every other target.
- Two assertions changed with this decision, and neither was weakened:
  `storage_missing.rs::should_unmount_the_mounts_piped_in_from_get_mount` asserted the target was
  exactly `ono.mount/1[/]` and now asserts that prefix, matching the sibling test that already
  did; acceptance case 042 pinned the same literal and now checks the identity prefix *and* that
  the two spellings agree — the label is the host's mount source, which no case can pin.
- A new test asserts the agreement itself, so the two rules cannot drift apart again.

## Alternatives considered

- **Use `ono_graph::label_of` in both.** Rejected on the evidence: for a mount it repeats the
  identity and is suppressed, leaving the row with nothing a person can read; for a process it
  prints `process/1` beside `ono.process/1[1]`.
- **Make `label_of` call `ObjectRef`'s rule.** Rejected: a graph node has no identity printed
  beside it, so `systemd` alone would stop saying which process it is.
