# ADR-0052: Output demand is derived from the consumer, in the planner

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.4, §1.5, §1.53, §1.57; v0.2 §42
- Decided by: agent (autonomous)

## Context

Spec v0.3 §1.5 says the demand model "MUST be part of execution planning, not an after-the-fact
renderer trick", and §1.4 gives four situations — a structured consumer, the interactive
renderer, an external byte consumer, a file redirection — without saying how a planner decides
between them for every consumer the language allows, or where the decision lives. ADR-0027
chose to compute the demand backwards from the consumer before any adapter exists, so that the
adapters of the later increments plug into a planner that already knows the answer.

## Decision

1. `ono-adapter` is the crate of the External Command Adaptation Layer. It holds planning
   vocabulary only — `OutputDemand`, `Consumer`, later negotiation, plans and provenance — and
   depends on nothing that spawns. `ono-command` depends on it for the plan; `ono-cli` for
   execution. Spawning stays in `ono-process` (v0.3 §1.7).

2. The demand of an external stage is a pure function of what is attached to its stdout,
   `OutputDemand::for_consumer`, decided in this order:

   | Consumer | Demand |
   |---|---|
   | a stdout redirection to `/dev/null` | `Discard` |
   | a stdout redirection to any other path, or onto another descriptor (`>&2`) | `RawBytes` |
   | the next stage is a process (or a value head) | `RawBytes` |
   | the next stage is a native command whose declared input admits `bytes` | `RawBytes` |
   | … admits `string` but not `bytes` | `Text` |
   | … admits neither: a command over values | `Structured { schema }` — the schema when the declaration names exactly one, else none |
   | no next stage and the shell's stdout is a terminal | `Interactive` |
   | no next stage and the shell's stdout is a pipe or file | `RawBytes` |

   A redirection wins over the pipe, as it does in POSIX. The *declared* input of the consumer
   decides, not the type the plan threaded into it: `where` is defined over objects even when
   the stage before it is a program, and that is exactly the case adaptation exists for.

3. "Terminal" means the process's stdout is a terminal, whether the session is the REPL or
   `ono -c` typed at a prompt. Both already render tables the same way in v0.2, and a demand that
   differed between them would make `ono -c 'ps aux'` at a terminal behave unlike the same line
   typed in. A script, a redirected `-c` and a pipe see `RawBytes`, which is the v0.3 §1.4
   guarantee that adaptation never changes what scripts see.

4. The plan reports the demand of every external stage as a `demand` row with the reason in
   parentheses — `` structured (`where cpu > 20` consumes objects) ``,
   `bytes (stdout is not a terminal)`, `discard (stdout goes to /dev/null)` — and as a `demand`
   field of the structured plan. Native stages have no stdout of their own and carry none.

5. `ono_command::plan` keeps its signature and assumes a stream; `plan_for` takes the
   `Stdout` kind. `explain` uses `plan_for` with the real stdout, so what it prints is what
   would run.

## Consequences

- ADAPT-002 onwards negotiate against a demand that is already settled; the executor in
  `ono-cli` will read the same function, so `explain` and execution cannot disagree about it.
- `Text` is rare today (no first-party contract is declared over `string` alone) but exists
  because v0.3 §1.5 lists it and a plugin command may declare it.
- Tests: `crates/ono-adapter/tests/demand.rs` (the table above),
  `crates/ono-command/tests/explain.rs` (the plan), `crates/ono-cli/tests/builtins.rs` (the
  rendering), acceptance cases `070` and `071` (in the container, at a pipe and at a PTY).
- Known divergence, noted in `docs/STATE.md`: the plan resolves a head by the registry alone,
  so `printf x | sort` is planned as `ono.data.sort` while the executor runs `/usr/bin/sort`
  (ADR-0028). ADAPT-002 needs the executor's resolution in the plan anyway and fixes it there.

## Alternatives considered

- Deriving the demand in the renderer or at execution time — forbidden by v0.3 §1.5, and it
  would let `explain` and the run disagree.
- A demand computed from the *producer* ("`ps` can produce records") — inverts §1.4; a
  `ps aux | grep` would be adapted and scripts would change.
- Making `Interactive` require an interactive session rather than a terminal — rejected under
  point 3.
