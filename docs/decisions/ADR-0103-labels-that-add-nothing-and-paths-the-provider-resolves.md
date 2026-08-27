# ADR-0103: A label the identity already shows is left off; a path the command does not act on is resolved

- Status: accepted
- Date: 2026-08-27
- Spec refs: §11.5, §16.5, §17.3, §22.4, §23.5
- Decided by: agent (autonomous, integrating `implementation-storage` into `implementation`)

## Context

Three families reached `ProviderMutation` on separate branches with three rules for a selector
that names an object:

- files (ADR-0082 §1): a `path` selector is acted on, never resolved — `write file` names a file
  that does not exist yet, and every filesystem call takes a path;
- network (ADR-0088 §2, §4): a selector nothing answers to still goes to the provider, and the
  `ActionResult` `target` renders as `<identity> <label>` whenever a label is known and differs
  from the identity;
- storage (ADR-0098 §1): a creating verb resolves nothing; `unmount filesystem <path>` resolves
  the path through the storage provider to the mount it detaches, and its rows read
  `ono.mount/1[/]` (`storage_missing.rs`, acceptance case 042).

Merged literally, `unmount filesystem /` took the files rule — a `path` selector — and acted on
`ono.filesystem/1[/]` instead of the mount, and every path-identified row grew a redundant
label: `ono.mount/1[/] /`, `ono.file/1[a.txt] a.txt`.

## Decision

### 1. The label is appended only when it says more than the identity shows

`ActionOutcome::into_record` renders `<identity> <label>` when the label is neither the identity's
own rendering nor one of its identity values as written (`ObjectId::shows`). `ono.socket/1[620332]
tcp/127.0.0.1:45801` keeps its label; `ono.mount/1[/]` and `ono.file/1[a.txt]` stand alone. This
refines ADR-0088 §4, whose intent — "the label is what the row is read by" — a label repeating the
identity does not serve.

### 2. The path rule of ADR-0082 §1 applies to a command acting on its target's own objects

A `path` selector is acted on unresolved when the contract's input names no stream of another
schema. `remove file` (`stream<ono.file/1>`) and `write file` (`bytes | string`) act on the path.
`unmount filesystem` takes `stream<ono.mount/1>`: the path is the provider's to resolve to the
mount it names, and a path no mount answers to reaches the provider unresolved under ADR-0088 §2,
which reports it as not found. The order of the rules in `ProviderMutation::targets` is: a
creating verb (ADR-0098 §1), then the path rule as scoped here, then resolution through the
provider with the unresolved fallback.

## Consequences

- `storage_missing.rs::…piped_in_from_get_mount` and case 042 pass unchanged.
  `…unmounting_the_root_filesystem_unprivileged` names a resolved selector, whose row carries
  the `ObjectRef` label under ADR-0088 §4 (`ono.mount/1[/] /dev/sda2`); it now asserts the
  identity as the row's prefix. `files_missing.rs` and `network_missing.rs` are unaffected, as
  no test pinned the `<identity> <label>` form.
- `ObjectRef::of` labels an object by the first default-view column outside the identity, while
  `ono_graph::label_of` has a form per schema; for a mount the first says the source device,
  the second the mount point. One rule should serve both (STATE.md, next up); this ADR does not
  move it.
- A provider whose identity values are opaque (a pid, an inode number) keeps its label on every
  row; one whose identity is the name a person uses shows it once.

## Alternatives considered

- Editing the storage assertions to `ono.mount/1[/] /` — accepts a row that says the same thing
  twice on every path-identified object, and leaves `ono.filesystem/1[/]` naming a mount.
- Resolving every path through the provider first — the file provider's `resolve` describes the
  path and fails when it is absent, which is exactly the file `write` is about to create; and it
  rejects the list a glob resolved to (ADR-0081).
