# ADR-0023: The context stack — what it changes, and what it must never change

- Status: accepted
- Date: 2026-08-26
- Spec refs: §4.2, §6.4, §14, §17.2, §20.2, §21; ADR-0010, ADR-0011, ADR-0013
- Decided by: agent (autonomous)

## Context

Spec §14 gives the frame shape and three examples, and then states the constraint that decides
everything else: "Context is an ergonomic tool, not hidden global magic" (§14.5). A context stack
that quietly changes what a command means is the feature going wrong, and it is the way this
feature usually goes wrong. §14.3 adds the other half: "If a command is unsupported in a context,
Ono-Sendai MUST say why rather than silently falling back to global scope."

Phase E has to make those two sentences mechanical rather than aspirational, before the first
frame is pushed.

## Decision

### A frame narrows; it never redirects

A context frame may contribute exactly three things:

1. **implicit selectors**, which narrow a query — `enter service nginx` makes `get process` ask
   for that service's processes;
2. **an execution location**, which decides *where* a provider call or an external process runs
   (a link frame, §14.4);
3. **an environment overlay**, visible in `$env` and inherited by children.

A frame may **not** change which command a name resolves to, alter a command's semantics, or
substitute one provider for another. Resolution stays the fixed order of ADR-0011 in every
context, so a user who has learned what `get process` means never has to ask which context they
are in to know it. That is the mechanical form of §14.5.

### Every frame's contribution is visible, and reversible by writing it out

Anything a frame contributes can be written explicitly. `enter service nginx` then `get process`
is exactly `get process --service nginx`, and `explain` prints the second form when asked about
the first. A context that could express something no explicit command could would be magic by
definition.

### An unsupported command in a context is an error, never a widening

Spec §14.3 is a prohibition on the most tempting shortcut: if `enter service nginx` is active and
a command has no meaning for a service, it fails with `resolve.target_not_found` naming the frame
and what it would have needed — it does not quietly run globally. Falling back would mean a
command sometimes acting on one service and sometimes on the whole machine depending on state the
user cannot see, which is precisely the failure mode `sudo` taught everyone to fear.

### The prompt shows every frame that changes where or what

Spec §4.2 requires the link to be unambiguous and §17.2 requires elevation to be impossible to
miss. The rule: **a frame that changes execution location, privilege, or the set of objects a
command will act on appears in the prompt.** A frame that changes only the working directory
appears as the path segment it already is. Nothing that narrows a mutation is ever invisible.

### `leave` cannot leave the ground

`leave` at the bottom of the stack is a no-op with a diagnostic, not an error and not a fall
through to something else. A stack that can be popped past its base is a stack that will be.

### Frames are session state, not shell state

The stack lives in the session and is not inherited by children: an external command receives the
environment overlay, because that is what an overlay is for, and knows nothing of the object
context, because a process has no way to honour one. A subshell starts at the same frame its
parent was in, and popping in the child does not pop in the parent.

### Structured results are addressed by the same identity everything else is

`@`, `@-1` and `@3` (spec §6.4, §20.2) refer to retained results by position; the values in them
carry the `ObjectId` they always had, so `@3 | kill process` signals the object that was shown
rather than whatever now occupies that row. Retention is bounded by count and by bytes, and a
value that a secret policy redacted is redacted in the retained copy too (spec §17.5, §20.2).

## Consequences

Easy: context is learnable in one sentence — it fills in arguments you would otherwise type — and
`explain` will always show you which ones. A user can work entirely without it, which §14.5
requires.

Hard: implicit selectors have to be threaded into every provider query rather than applied as a
post-filter, or `enter service nginx` would enumerate every process and discard most. The
`Selector` push-down in `ono-provider-api` already exists for this.

Must be revisited in phase H, where a link frame makes the execution location remote and the
prompt's job is load-bearing rather than informative.

Encoded by: the context tests in `crates/ono-cli`, and the acceptance cases for `enter`/`leave`,
the prompt, and the unsupported-command refusal.

## Alternatives considered

- **Context as a set of defaults a command may override** — rejected: "override" implies the
  command sometimes wins and sometimes does not, and which one is invisible. Narrowing composes;
  defaults do not.
- **Falling back to global scope for a command a context cannot narrow** — rejected by spec §14.3,
  and rightly: it makes the blast radius of a mutation depend on state the user cannot see.
- **Frames inherited by child processes** — rejected: a process cannot honour an object context,
  so passing one would be a promise nothing keeps.
