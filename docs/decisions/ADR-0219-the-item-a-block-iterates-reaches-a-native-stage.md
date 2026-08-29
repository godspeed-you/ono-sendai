# ADR-0219: The item a block iterates reaches a native stage, and a mutation reads its selectors

- Status: accepted
- Date: 2026-08-29
- Spec refs: §6.4, §11.5, §19.4; ADR-0009, ADR-0071 §1, ADR-0085 §2
- Decided by: agent (autonomous, `close-data`)

## Context

Spec §19.4's own example is `each { restart service @ }`. What the shell did:

```text
get process | where pid == 1 | each { echo @.pid }          → 1
get process | where pid == 1 | each { get process @.pid }   → cannot read field `pid` on null
get process | where pid == 1 | each { stop process @ }      → `stop process` needs something to act on
```

Two separate causes, one behind the other.

1. `each` binds the item with `session.bind("@", item)`, which makes it a *variable* named `@`.
   Word expansion reads it (`echo @.pid` works), and so does the statement evaluator. But the
   scope a native stage is invoked with — built by `stage_scope` — copied the session's bindings
   as variables and never set `Scope::current`, so `Expr::CurrentValue` fell through to the
   invocation's current value, which is `Value::Null` before any row flows. `Scope::with_current`
   already existed for exactly this, documented "it matters for a nested block, where `@` names
   the item the block iterates (spec §19.4)", and nothing called it.

2. Even with `@` bound, `stop process @.pid` failed — and so did `stop process $p` outside any
   block, and `remove file $f`. ADR-0009 keeps a words-mode argument in its parsed form until a
   layer knows what it means; an argument written as an expression binds as
   `Binding::Expressions`. ADR-0085 §2 has the *producer* evaluate those against the invocation's
   scope. The mutation family never did, so `arguments().selector("pid")` — which answers only
   for a `Binding::Value` — was `None`, and the command reported that nothing had been named.

## Decision

**`stage_scope` binds `@` as the scope's current value**, not only as a variable, whenever the
session carries one. Inside a block, `@` is the item for every stage of every pipeline in it.

**A mutation resolves its written arguments once, before it acts**: `ProviderMutation::run`
calls `BoundArguments::evaluated(scope)`, which turns every `Binding::Expressions` into the value
it evaluates to, and reads its selectors and options from that. This is ADR-0085 §2's rule for
producers, applied to the family that reads values rather than expressions. An expression-mode
command must not do this — its expressions are evaluated per row, against the row — and none
does.

A parameter written more than once evaluates to the list of its values, which is the shape a
repeatable selector already carries.

## Consequences

- `each { stop process @.pid }`, `each { restart service @ }`, `stop process $p`,
  `remove file $f` and `--since (now() - 1h)` on a mutation all work, and answer one
  `ActionResult` per target as spec §11.5 requires.
- An argument that cannot be evaluated — an undefined variable, `@` where nothing is current —
  fails before the mutation resolves any target, so a refused argument never leaves a bulk
  half-run.
- The interactive selection `@` (ADR-0050) is unaffected: `each`'s binding shadows it only for
  the block's duration, exactly as the statement evaluator already had it.

## Alternatives considered

- **Evaluate every words-mode command's expression arguments in the binder.** Rejected for now:
  the scope is built *after* binding, because it pre-runs the nested pipelines the arguments
  contain, and inverting that is a larger change than the defect calls for. The producer and the
  mutation families both resolve at invocation, which is where the scope exists.
- **Have `each` substitute `@` textually before the block runs.** Rejected: it would make `@`
  a macro rather than a value, and `@` bound to a record has no text.
