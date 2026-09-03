# ADR-0567: A host stream is pulled, and the context is published

- Status: accepted
- Date: 2026-09-03
- Spec refs: §31.12, §31.13, §31.14, §31.15, §31.19, §31.37, §31.64, §31.73; ADR-0022, ADR-0040, ADR-0566
- Decided by: agent (autonomous)

## Context

Issue #3 (c): of spec §31.12's sixteen host API domains, the supervisor served seven. Every
domain that answers with more than one value — `objects.query`, `schemas.list`,
`relations.query`, `history.query` — is declared in `protocol.v1.yaml` as returning
`handle<stream>`, and the plugin reads such a stream with `streams.next`. Nothing produced a
host stream and nothing served `streams.next`, so none of those domains could be built. This
ADR settles the two pieces every one of them stands on, and delivers the two smallest domains
with them.

## Decision

**1. A host stream is pulled, with the plugin's credit.** The supervisor keeps a table of the
streams it has opened for the plugin, by handle. `streams.next {handle, max}` answers at most
`max` values and says whether the stream is complete; a complete stream is dropped from the
table on the read that drains it. `streams.cancel {handle}` drops a host stream, or marks a
plugin output stream cancelled, and a handle the host never opened is a protocol violation —
the plugin is quarantined, because naming a handle it was never given is a defect in the
package and not a condition of the pipeline. This is ADR-0022 §8's pull-based flow in the
other direction: the plugin decides how much it is ready for, and the host sends no more.

**2. A stream is opened over what the host already has.** `schemas.list` produces its records
before it answers with the handle, and the plugin pulls them. The table carries the values it
holds, whether the stream is complete, and the terminal error if there was one — the shape a
live source needs too, so `objects.query` (the next increment) opens a stream whose values
arrive as the provider produces them, into the same table and behind the same `streams.next`.

**3. The context is published, not queried.** The supervisor has no session. `context.get`
answers from a source the loader is handed — `LoadConfig.context` — and the shell publishes
its context into it before every pipeline, beside the plugin home and the state directory it
already publishes: `cwd`, the innermost object frame as `{target, identity}`, the link host
when the session is inside a link, the host name, whether a person is at the terminal and
whether output is redirected. Nothing beyond it: no environment, no history — those are their
own domains with their own capabilities. The test host hands a fixed context, so a conformance
run is deterministic (§31.73).

**4. A schema record says where it came from.** `schemas.get` and `schemas.list` describe a
schema as fields with their types, units, nullability and docs, the identity and its fallback,
the default view, and an `origin` — `core` for `ono.*`, `package` for the package's own
contributions, `provider` otherwise (§31.64). Both cost `schema.read`; the id or the prefix is
the audited target.

## Consequences

- `objects`, `relations`, `history` and the rest of the stream-shaped domains have their
  transport. Each still needs its host-side source — the provider registry, the graph, the
  history — which the shell will hand the loader the way it hands the context.
- `streams.next` reads only what the table already holds. A live source that has produced
  nothing yet answers an empty page rather than waiting; the `deadline` the contract declares
  is accepted and, for a materialised stream, has nothing to wait for. The live increment
  gives it meaning.
- The example package gained `context`, `schemas` and `schema`; the conformance suite proves
  the fixed context, the prefix-filtered schema stream pulled two at a time, one schema's
  fields, and deny-by-default for all three.

## Alternatives considered

- **Pushing host values to the plugin as unsolicited frames.** Rejected: the plugin would have
  to buffer without bound, which is the overflow §31.15 forbids in the other direction.
- **Answering `schemas.list` with the whole list inline.** Rejected: the contract says a
  stream, and the first stream-shaped domain is where the mechanism has to exist.
- **Letting the supervisor read the session's context through a callback into the shell.**
  Rejected: the session is not shared, and a callback from the actor's task into the evaluator
  would be a lock across the boundary. Publishing is one write per pipeline and no coupling.
