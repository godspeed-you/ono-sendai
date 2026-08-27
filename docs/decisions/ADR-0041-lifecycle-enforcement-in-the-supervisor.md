# ADR-0041: Lifecycle enforcement in the supervisor

- Status: accepted
- Date: 2026-08-26
- Spec refs: §31.8, §31.15, §31.17, §31.34, §31.63; ADR-0022 §5, ADR-0040
- Decided by: agent (autonomous)

## Context

`docs/spec/kuang/lifecycle.v1.yaml` fixes six states and a transition table, but its table is
written along the command path (`install`, `enable`, `load`, …) and leaves three questions a
running supervisor must answer: how `degraded` composes with `active`, where a runtime
protocol violation lands, and what a crash does to the state.

## Decision

### 1. `degraded` is a memory, not a dead end

The type (`ono_kuang_protocol::Lifecycle`) tracks the loaded/degraded flag and the invocation
count separately and projects the flat six-value state spec §31.8's `get plugin` column shows.
Consequences the tests encode:

- a degraded package can begin invocations (`degraded -> active`), because spec §31.8 defines
  degraded as *loaded* with optional capabilities unavailable — it runs, minus the disabled
  features;
- when the last invocation of a degraded package ends, the state returns to `degraded`, not
  `loaded` — a denial does not disappear because an invocation finished (spec §31.63);
- `unload` is legal from `degraded` exactly as from `loaded`, and never while invocations are
  active — draining is the caller's job and the type refuses to forget it.

### 2. A protocol violation quarantines

Spec §31.8 reaches `quarantined` through "trust, integrity or policy failure"; spec §31.34
classifies protocol violations with `restart: never`. The supervisor reads a runtime protocol
violation — malformed frame, oversized frame declaration, emission beyond credit, a handle
that is not the plugin's, an unknown method, an unsolicited response — as a policy failure
discovered late: code that does not hold to the negotiated contract is code the policy no
longer permits to load. The instance is killed, every stream closes with
`runtime.protocol_violation`, every pending invocation resolves as failed, and the package is
`quarantined` with the reason recorded. `load plugin` on a quarantined package fails with the
reason rather than offering to continue (ADR-0015 rule 4).

A *schema* violation in emitted output does not quarantine: it closes the offending stream
with `runtime.schema_violation` and leaves the instance loaded, because spec §31.34 separates
the two classes — invalid output degrades the contribution, misframing breaks the trust that
any message means what it says.

### 3. A crash unloads

An instance that exits or breaks its pipes outside a shutdown is `runtime.trap`: streams close
with the error, invocations resolve failed, the lifecycle drains to `enabled`. It is not
quarantine — spec §31.34 permits automatic restart of stateless plugins, which a quarantined
state would forbid; restart policy itself is deferred (the supervisor does not restart today,
it reports).

## Consequences

The conformance suite asserts each path as an outcome: `should_quarantine_a_plugin_that_emits_
beyond_credit`, `…_that_breaks_framing`, `…_that_declares_an_oversized_frame` (quarantine),
`should_close_the_stream_when_output_leaves_the_declared_schema` (loaded, stream failed),
`should_load_degraded_when_an_optional_capability_is_denied` and the lifecycle unit tests
(degraded memory). Revisit when restart policy lands: `automatic-if-stateless` needs the
state-class declaration wired to the restart decision.

## Alternatives considered

- **Violation → crash semantics (back to `enabled`)** — rejected: a package that misframes
  once will misframe again; reloading it without an operator decision turns a security
  boundary into a retry loop, and spec §31.34 says `restart: never` for the class.
- **A flat enum with `active` erasing `degraded`** — rejected: the denial would vanish from
  `get plugin` for exactly as long as the package is doing something — the moment an operator
  most wants to see it.
