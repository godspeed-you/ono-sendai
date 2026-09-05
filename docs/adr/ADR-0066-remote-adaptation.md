# ADR-0066: Remote adaptation — the agent negotiates, runs and decodes

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.54, §1.8, §1.18; v0.2 §21.2, §21.4; ADR-0036, ADR-0052, ADR-0056, ADR-0059
- Decided by: agent (autonomous)

## Context

Spec v0.3 §1.54: "If a command executes on `prod-db`, the compatible adapter and executable
version must be evaluated for `prod-db`, not only for the local deck", and it offers three
strategies — the remote agent adapts and streams canonical values; the local shell runs a
remote raw command and decodes; raw SSH with no adaptation. The v0.2 link protocol
(ADR-0036) carries queries, subscriptions and actions; external commands typed inside a link
frame have always run locally.

## Decision

1. **Strategy 1.** The protocol gains one frame, `start-adapt` (kind 14), carrying
   `AdaptRequest { argv, demand, explain_only }`. The agent resolves the program on *its*
   `PATH`, negotiates against *its* registry (the bundled packs, probing versions as the
   shell does), runs the plan with the plan's environment, decodes on a reader thread and
   streams values and failures back on the stream; a non-zero exit is a failure after the
   records; a cancelled stream kills the child. With `explain_only` it answers one map —
   `adapted`, `state` (the §1.57 words), `argv` — and runs nothing.
2. **Inside a link frame the remote decides first.** For an external stage with a structured
   or interactive demand the shell asks the remote (`explain_only`); if the remote adapts, the
   shell asks again to run, feeds the records — marked with the host, as every remote record is
   — into the following native segment or the renderer through the streamed-seed path of
   ADR-0059; provenance therefore says which host, which adapter, which executable and
   version, and that decoding happened on the remote (`link`). If the remote does not adapt:
   under a structured demand the stage fails with `adapter.not_available` naming the host —
   never a local table pretending to be remote; under an interactive demand the program runs
   locally raw, as it always has, with the reason printed. An agent that cannot answer (an
   older build, a lost link) is the same as "does not adapt".
3. **Bytes stay classic.** A byte consumer, a redirection, `raw` — nothing changes: the
   program runs locally, as in v0.2. Strategy 3 is what the shell already was; strategy 2 is
   not implemented, because a remote raw exec is a different feature with its own security
   surface (spec §21.3's agentless mode, ADR-0036's remainder).
4. **`explain` says where.** Inside a link frame the plan does not consult the local registry;
   each external stage gets an `adaptation on <host>: <state>` line from the remote's own
   answer.

## Consequences

- `enter link prod-db; ss -tunap | where state == "established"` is prod-db's sockets
  through prod-db's `ss`, and `inspect` proves it.
- One extra round trip per adapted stage inside a frame (the explain-only ask); a link frame
  is an explicit context, and the decision is what §1.54 requires to be the remote's.
- Tests: `ono-protocol/tests/messages.rs` (the frame round trip), `ono-cli/tests/remote.rs`
  (records and provenance from the other side, `explain … on testbox`, the byte path, the
  structured refusal), acceptance case `084`.

## Alternatives considered

- Decoding locally from a remote raw run (strategy 2) — rejected for now: it needs remote
  exec, and the executable version would still have to be probed remotely.
- Consulting the local registry inside a frame — rejected: it would adapt with the wrong
  adapter for the wrong version of the wrong host's tool.
