# ADR-0011: Name resolution order and namespaces

- Status: accepted
- Date: 2026-08-26
- Spec refs: §6.5, §15.4, §19.6, §31.5, §31.22, §43
- Decided by: agent (autonomous)

## Context

Spec §6.5 gives a "suggested default" resolution order and requires that it be "explicit and
inspectable", that users be able to force a namespace, and that ambiguity be resolvable. The
spelling `ono:get process` / `exec:get process` is marked as possibly changing. Resolution is
the single most user-visible piece of shell behaviour after parsing, and every later phase —
completion, `explain`, KUANG/11 command contribution — depends on it being pinned down.

## Decision

### Order

A head word resolves in exactly this order, first match wins:

```text
1. language keyword or control form       let fn if for while match try return break continue use
2. user function                          defined by `fn`, in the innermost scope that has one
3. alias                                  expanded once, then re-resolved from step 1
4. native command                         the command registry, including KUANG/11 contributions
5. external executable on PATH            the first hit, with the usual executable-bit rules
6. resolve.command_not_found (E0101)      with discovery suggestions
```

Aliases expand exactly once and the expansion is re-resolved from the top, so an alias may shadow
a native command and may name an external program, but a cycle terminates rather than hanging.

### Namespaces

A qualified head forces a stage of the order and skips the rest:

| Prefix | Resolves in |
|---|---|
| `ono:` | native commands only (step 4) |
| `exec:` | external executables only (step 5) |
| `fn:` | user functions only (step 2) |
| `<module>:` | commands contributed by that module or KUANG/11 package (§19.6, §31.22) |

The spelling of §6.5 is kept as written. A qualified name that does not resolve is
`resolve.command_not_found` (E0101) and is never silently retried in another namespace: forcing a
namespace is a statement about intent, and quietly ignoring it would defeat the purpose.

A head containing `/` is never a namespace and never searches `PATH`: `./build.sh` and
`/usr/bin/env` are paths, exactly as in every other shell.

### Ambiguity

Two commands claiming the same unqualified name is `resolve.ambiguous` (E0103), listing the
qualified name of every candidate so the user can copy one. This can only arise between
contributed commands, since steps 1–5 are ordered; the core never allows two natives to claim one
name (§40.1). Publisher-namespaced ids of §31.5 mean a package can always be addressed
unambiguously.

### Inspectability

`explain <command>` prints the resolution it *would* perform: which step matched, what was
skipped and why, the absolute path of an external hit, and the contributing package of a native.
This is what makes the order "explicit and inspectable" as §6.5 requires — the order is not
documented behaviour that might drift from the code, it is reported by the code.

### Not found

`resolve.command_not_found` carries suggestions from an edit-distance search over the union of
native commands, functions, aliases and `PATH` entries (§15.4), capped and sorted, and computed
only on the failure path so it costs nothing when the command exists.

## Consequences

Easy: the order is short enough to memorise and is reported rather than described; a user can
always force what they mean; a KUANG/11 package can never silently take over `get`.

Hard: alias expansion happening before native lookup means an alias can shadow a core command.
That is deliberate — it is the user's shell — and `explain` makes it visible.

Encoded by: resolution tests in `crates/ono-cli` and the `explain` acceptance cases.

## Alternatives considered

- **Native before alias** — rejected: it would make an alias unable to override a core command,
  which is the main reason people write aliases.
- **Falling back to another namespace when a qualified name misses** — rejected: it turns an
  explicit statement of intent into a suggestion and reintroduces exactly the ambiguity §6.5
  asks to remove.
- **A new spelling for namespaces (`@ono/get`, `ono::get`)** — rejected: §6.5's spelling is
  already written down, `:` does not collide with anything in the grammar of ADR-0009, and a
  change would buy nothing.
