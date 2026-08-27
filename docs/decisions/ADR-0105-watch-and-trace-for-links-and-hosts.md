# ADR-0105: `watch` and `trace` for links and hosts

- Status: accepted
- Date: 2026-08-27
- Spec refs: §18.2, §21.2, §22.1–§22.4, §31.14, §33.4, §52; ADR-0024, ADR-0034, ADR-0036,
  ADR-0078, ADR-0103
- Decided by: agent (autonomous)

## Context

ADR-0078 left five remote commands ignored — `watch link`, `watch host`, `trace link`,
`trace host` — because the watch runtime and the tracer both start from provider records and
the link table was rendered text (ADR-0078's note, `docs/STATE.md`). ADR-0103 made `link` and
`host` records of `ono.shell`. What remained was the event schemas, the relationships a link
and a host have, and the two contracts still marked `planned` (`ono.host.watch`,
`ono.link.trace`; spec §52 marks both "?").

## Decision

### The watches are the generic runtime over the session provider

`ono.link-event/1` and `ono.host-event/1` have the envelope of `ono.process-event/1` and carry
the object under `link` / `host` (ADR-0078's rule). The generic poll loop of ADR-0034 snapshots
`ono.shell`'s tables at the default cadence (`--every` overrides), so the first event is the
`snapshot` of every link held or host known, and a link added, torn down or re-established in
another statement of the session shows up as `added`, `removed`, `changed` at the next poll —
the tables are republished before every pipeline. `source` is `poll`; the provider has no
event source, and says so.

`watch host` polls the host sources: the files are re-read at each interval, so a host recorded
by `add host` in a parallel session appears. It does not probe reachability — the contract's
summary says "reachability changes", and a poll that connected to every known host every two
seconds would be the kind of invisible cost spec §18.2 forbids. Reachability is `test host`'s
(ADR-0104), one host at a time, on request; a watch that probes is a later decision, taken
when a cheap liveness check exists on the link protocol.

### Trace edge sets — exact, from the shell's own bookkeeping

| Subject | Relation | Target | Read from | Confidence |
|---|---|---|---|---|
| `ono.host/1` | `link` | `ono.link/1` | the link table, `host == name`; edge metadata `transport` | exact |
| `ono.link/1` | `offers` | `ono.provider/1` | the link record's `providers` (the handshake's answer, spec §21.2) | exact |

`HostLinks` and `LinkProviders` live in `crates/ono-graph/src/kernel/remote.rs` and join
`kernel_relationships`, so `trace host prod-db` draws the host, its link and the providers the
far side offers, and `trace link prod-db` starts one hop in. The link node's summary carries
`transport`, `mode` and `state`, which is how §33.4's summary — transport, agent, providers —
is a graph rather than prose. A definition that was never established offers nothing: absence,
not a failed read (spec §10.5).

`ono.provider/1` (id, targets, available; identity `id`) is written for these nodes. A provider
is an object with a stable identity — every record's provenance names it — and a graph node
needs a kind that means something. Only the id is known across a link today, so the other
fields are nullable and the node's summary carries the id; no command enumerates providers
yet, and when one does the schema is already there.

### The two `planned` cells are delivered

`ono.host.watch` and `ono.link.trace` become `experimental` (spec §52: a contract with an
implementation, not yet a compatibility promise), their `validation_required` goes with
`planned`, and `ono.link-event/1` leaves `deferred.yaml` in the same commit (ADR-0078's rule).

## Consequences

- `watch link | take 1 | select kind | to json` is `[{"kind":"snapshot"}]` once a link is held;
  `watch host --every 1s | take 1` likewise; `trace host testbox` has an `ono.link/1` node, an
  edge and `linux.procfs` among the nodes. Tests: `remote_missing.rs`
  (`should_begin_a_link_watch_with_a_snapshot`, `should_list_the_held_link_in_the_watch_snapshot`,
  `should_begin_a_host_watch_with_a_snapshot`,
  `should_trace_a_host_to_its_link_and_negotiated_providers`,
  `should_trace_a_link_to_its_transport_agent_and_providers`).
- `watch link` with no link held streams nothing until one is made in another statement —
  a live table of an empty table, as `watch job` would be.
- The "multiplexed streams" of `ono.link.trace`'s summary are not drawn: the protocol does not
  expose its open streams as objects, and drawing a count would be a number without an
  identity. Left open in `docs/STATE.md`.

## Alternatives considered

- **A `watch host` that probes every host at each interval.** Rejected: an invisible network
  cost per poll, and a probe over ssh is seconds, not milliseconds (spec §34).
- **Provider nodes as `ono.endpoint/1` or a bare label.** Rejected: an endpoint is an address,
  and a node without a kind cannot be told from any other.
- **Carrying the provider ids in the link node's summary only.** Rejected: spec §22.4 draws
  the facts as nodes, and a list inside a cell is prose.
