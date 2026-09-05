# ADR-0106: Agentless mode is recorded and reported; `explain` shows the remote execution context

- Status: accepted
- Date: 2026-08-27
- Spec refs: §11.3, §17.1, §21.3, §21.4, §42.2; ADR-0036, ADR-0037 §6, ADR-0066 §4, ADR-0103
- Decided by: agent (autonomous)

## Context

`link host prod-db --agentless` accepted the flag and ignored it: the link went up in agent
mode and nothing said so. Spec §21.3 — "Fallback MUST be visible because semantics and
performance may differ" — is a requirement on visibility, and an accepted-and-ignored flag is
the opposite of visible. ADR-0037 §6 deliberately left the agentless mode itself (a reduced
provider set over plain ssh command execution) to a later increment, and this build still has
no such provider set: every link is served by `ono --agent` on the far side.

Separately, spec §42.2 shows what `explain` prints for a destructive plan while connected — an
`EXECUTION CONTEXT` naming the link as remote and a `MUTATION` block with `operation signal
TERM` and the privilege — and the plan printed only the per-stage rows, with nothing about the
link and nothing about what `stop` actually sends.

## Decision

### 1. `--agentless` is recorded on the link and reported everywhere the link is described

`SessionLink.agentless` is set by the flag; `ono.link/1 mode` is `agentless` (ADR-0103's
field), so `get link`, `watch link`, `trace link` and the host record's link all carry it.
The link summary line and `explain`'s context row say the rest plainly: *agentless —
requested; this build serves it through the agent*. The mode is therefore what the user asked
for and what the link will use once the fallback exists, and every place that shows it also
shows who answers today. Until the agentless provider set of ADR-0037 §6 lands, no link runs
without the agent; a host without `ono` installed fails the handshake as it always did.

### 2. `explain` inside a link prints the execution context; a mutation prints its effect

After the per-stage plan, `explain` prints:

- `EXECUTION CONTEXT` whenever the session stands in a link frame: `link <host> (remote)`,
  `transport`, `mode` (with the note above when agentless), `identity` (the user this side
  announced to the handshake).
- `MUTATION` for every stage whose capability risk changes the world (spec §17.1), inside a
  link or not: the stage, `operation` (the effect: `signal TERM` for `stop process`, the
  bound or default signal for `kill process` and `send signal`, `<verb> <target>` otherwise),
  `targets` (the stage's input type — §42.2's "dynamic Stream<Process>"), `risk` (with
  `+ remote` inside a link), `privilege`.

The blocks are printed by the shell (`builtin.rs`), beside the `adaptation on <host>` lines
of ADR-0066 §4, because the plan itself is built without the session's frames; the rows use
the plan's own alignment so `risk         mutate` still reads as one line. The structured form
(`explain … | to json`, `ono.execution-plan/1`) is unchanged: the context is the session's,
and moving it into the plan value is a contract change for a later increment.

## Consequences

- `link host testbox --transport local --agentless; get link | to json` carries
  `"mode":"agentless"`; `explain get process` inside that link prints the mode; `explain stop
  process 1` inside a link names `testbox (remote)`, `signal TERM` and the privilege. Tests:
  `remote_missing.rs` (`should_keep_the_agentless_mode_visible_in_the_link_table`,
  `should_explain_that_a_query_runs_in_agentless_mode`,
  `should_explain_the_remote_context_of_a_mutation`,
  `should_explain_the_effect_of_a_remote_mutation`).
- A local `explain stop process 1` gains the `MUTATION` block too — the effect of a mutation
  is worth a line wherever it runs.
- `docs/STATE.md` keeps the agentless provider set under *Next up*: when it lands, the mode
  becomes what the link runs, and the "served by the agent" note goes.

## Alternatives considered

- **Refusing `--agentless` with `provider.unsupported`.** Rejected: honest, but it makes a
  documented option unusable and leaves nothing to inspect; recording the request and saying
  who answers keeps the flag meaningful and the fallback visible when it exists.
- **Putting the context into `ExecutionPlan` via `PlanContext`.** Deferred: every caller
  constructing `PlanContext` would change for a row only the shell can fill; the printed form
  is what §42.2 specifies, and the value form can follow when a consumer needs it.
