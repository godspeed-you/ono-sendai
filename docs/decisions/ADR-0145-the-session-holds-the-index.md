# ADR-0145: The session holds the place and the index

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §29.2, §33.1, §33.2, §34, §45.4, §45.6, §46, §46.1; ADR-0141 (extended here)
- Decided by: agent (autonomous)

## Context

ADR-0141 decided that `find place` builds its index per command and throws it away: a search asks
only the targets its query needs, and nothing is carried between commands. §46 requires more of a
*session*: a current place, a trail, pins and view preferences, all of which outlive one command.
§34 budgets a repeated `look` at 50 ms, which is only reachable if the second `look` reads what the
first one learned (§33.1: the index is an in-memory cache).

Nothing in the shell can hold it. The command table the evaluator builds is a `OnceLock<CommandTable>`
and a `CommandImpl` is handed an `Invocation`, not the `Session`.

## Decision

**`SpatialSessionState` is a process-wide singleton in `ono-cli`**, behind an asynchronous mutex,
holding the trail (whose head is the current place), the scope every observation of this process
belongs to, the spatial index, the provider bridge that fills it, the pins and the view
preferences. A fresh process starts at the local SYSTEM root with an empty trail (§46.1).

Per process is the strongest form of §29.2's guarantee. "A script MUST NOT silently change the
caller's interactive spatial context": a called script is another `ono` process and therefore
another state, so there is no shared place for it to change.

The lock is `tokio::sync::Mutex` because the commands that hold it reach providers while they do,
and a blocking guard held across an `await` is a deadlock waiting to happen.

**The index is a cache and the providers stay authoritative** (§33.2). `look` and `near` observe
what the current place needs before they project it, so a second `look` at the same place reads the
index; `enter` resolves against what is already known and observes only when the declared answer
misses (§27.1 resolves visible children first, and a canonical space's children are declared).

**The renderer stays outside.** `ono-spatial-render` is handed the finished `ono.place-view/1`
record and turns it into lines; it asks nothing and computes nothing, which is §45.4's "MUST NOT
invent semantic nodes/edges" in the only form that can be enforced. The shell's sink recognises the
schema and routes it there, exactly as it already routes `ono.graph/1` to the tree layout.

## Consequences

- ADR-0141 still holds for `find place`: a search plans its own targets and its own index, because
  the session's index is a cache of *places*, not an answer to an arbitrary predicate.
- The session's index grows for the life of the process. Freshness is tracked per entry (§33.3), and
  the eviction policy a long-lived interactive session will want is a later phase's, not this one's.
- The spatial state is not persisted: §46.1 makes `start at local SYSTEM root` the default and
  `spatial.trail.persist = false` the setting. Pins persist, and they are read from the store on
  every command that ranks by them.
- Tests that encode it: `spatial_topology_missing::should_keep_the_same_spatial_id_for_the_root_across_separate_sessions`,
  `spatial_navigation_missing::should_describe_the_current_place_as_a_structured_view_when_look_runs_without_a_tty`.

## Alternatives considered

- **A field on `Session`** — rejected: the command table is static and a `CommandImpl` never sees the
  `Session`. Threading it through would change the signature every native command is written to.
- **Rebuilding the index per command, as ADR-0141 does** — rejected by §34: `look` twice would ask
  the providers twice, and the second answer is the one the budget is about.
- **A `std::sync::Mutex`** — rejected: the guard is not `Send`, and every spatial command awaits a
  provider while it holds the state.
