# ADR-1096: `watch host` probes the held link, and dials nothing

- Status: accepted
- Date: 2026-11-24
- Spec refs: spec §18.2, §21.2, §22.1–§22.4, §34; v0.4.1 §2.1, §10.2; ADR-0105, ADR-1095
- Issues: #219 (a)
- Decided by: agent (autonomous)

## Context

`ono.host.watch`'s summary is "Emit host reachability changes as a live stream", and ADR-0105
delivered a watch that polls the host *sources* and reports a snapshot: a host appearing in a file
was a change; whether it answered was not. ADR-0105 rejected probing every known host at every
interval — an invisible network cost per poll, seconds per host over ssh — and left a probing
watch "a later decision, taken when a cheap liveness check exists on the link protocol". ADR-1095
built that check: `ono.link.probe/1`, answered by the agent's loop without a provider, bounded by
a 2 s timeout and reused for 5 s.

## Decision

**The probe is the shared mechanism; there is no second one.** `ono.host/1` gains three fields —
`reachability`, `reachability_reason`, `reachability_since` — filled from the latest probe of the
link this session holds to the host. Reading the host table measures the held links first (the
same `spatial::links::measure_links` pass the link map uses, with its reuse window and its
process-wide serialization), so `get host` reports reachability now and `watch host`, whose
generic poll loop reads the table at each interval, reports it at every poll. A transition is a
`changed` event of the existing envelope; `reachability_since` moves only on a transition, so a
host that stays reachable emits nothing after its snapshot — one event per change, not per probe.

**Nothing is dialled.** A host this session holds no link to is `unknown`, and its reason says
`test host` is what dials one. Opening connections from a watch would be ADR-0105's rejected
design under another name; reachability without a link is a question the user asks explicitly.

**Three states, never merged** — "no evidence" is not "unreachable" (§35.3):

| state | evidence |
|---|---|
| `reachable` | a probe of the held link was answered |
| `unreachable` | a probe went unanswered within `PROBE_TIMEOUT`, or the link failed (the far side is gone) |
| `unknown` | no link held; a definition not established; a link this session no longer follows; an agentless link (no agent to ask); an agent that does not offer the probe; a probe that failed for another reason, which the reason names |

Transport reachability is not authorization (v0.4.1 §2.1), and the probe keeps them apart by
reading nothing: a client authorized to connect but not to observe still learns the host is
reachable, because the connection the probe travels on was already authorized. A client whose
access is revoked has its connection closed (§12.5), which is the link failing — `unreachable`
with the reason saying the far side is gone.

*Amended in the v0.6.6 remediation (review TST-3).* The decision first declared a fourth state,
`unauthorized`, for "a far side that answered and refused the probe". No agent produces that
answer — this build's agent answers every authorized connection's probe in its loop, and a
connection that is not authorized is never established — so no test could produce the value and
the contract would have declared a state nothing yields (#200's shape). It is removed from
`ono.host/1`; a probe some other far side refused is `unknown`, and the reason names the code.

**Bounds, cancellation and backpressure are the watch runtime's own.** A poll that probes waits at
most 2 s (one pass, all links side by side); a dead link is detected without sending anything (a
failed link is re-evaluated at once rather than reported reachable for the rest of the reuse
window); a cancelled watch drops the snapshot it was collecting, and the producer task that holds
the in-flight probe ends within the probe's own timeout. The probe traffic of a watch is at most
one probe per held link per 5 s, whatever `--every` says.

## Consequences

- Tests: `crates/ono-cli/tests/watch_host.rs` — reachable through a held link; `unknown` and
  nothing dialled for a host without one; `unknown` for an agentless link; a real second agent
  (`ono --agent --listen 127.0.0.2:0`, its own home and identity) that dies mid-watch produces
  exactly one `changed` event to `unreachable` and nothing before it; one that is frozen
  mid-watch — the connection stays open, so only the probe can tell — produces exactly one
  `changed` to `unreachable` and, thawed, exactly one back to `reachable`; a client authorized
  without observe still sees the host reachable. Acceptance case
  `456-watch-host-reports-reachability-through-the-held-link.case`.
- `get host` now costs a probe per held agent link (bounded and reused as above). A listing that
  shows reachability measures it; one that would show an old figure would be the carried-forward
  number ADR-1095 forbids.
- `ono.host/1`'s `transport` enum still lists `[ssh, local]` while a link may be `tcp`; that is
  older than this decision and is reported, not changed, here.

## Alternatives considered

- **Probe every known host by connecting.** ADR-0105's rejected design: invisible cost, seconds
  per host, and a connection attempt is not a liveness check of anything this session holds.
- **A provider subscription for `host`.** Would label polling as `subscription` — the source field
  would lie (spec §18.2 requires polling to be explicit).
- **Emit the probe time on every poll.** Each probe would be a `changed` event; a reader watching
  for reachability would drown in timestamps. `reachability_since` keeps the evidence's time and
  moves only when the evidence does.

*Amended in the v0.6.6 remediation (review ARC-3).* The fields `reachability` and `reachability_reason` were first declared
`required` in place in a `/1` schema, which v0.2 §10.4 counts as a versioned schema change
("adding a required field"). Following the repository's precedent (ADR-0134, ADR-0996), they are
declared `nullable` instead: this build always produces them, and a consumer of `ono.host/1` that
predates them stays valid. No version bump.
