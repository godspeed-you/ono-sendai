# ADR-0035: A trace is one observation, and its expansions share it

- Status: accepted
- Date: 2026-08-26
- Spec refs: §22.1, §22.3, §34; ADR-0025
- Decided by: agent (autonomous)

## Context

Wiring `trace` into the shell exposed a cost the crate's own tests never saw: expanding one node
asks several relationship providers for "every process" or "the socket with this inode", and the
providers answered each question by enumerating the machine afresh. A trace of pid 1 — the first
thing anyone tries — cost `nodes × processes` full procfs reads: twenty-seven seconds, where
spec §34 budgets interactive moments in milliseconds.

## Decision

**One trace is one observation of the machine.** The provider set built for a single trace run
shares one snapshot per target (`SharedSnapshots` in `ono-graph`): the first expansion that needs
"every process" enumerates once, every later expansion reads the same answer, and a pinned
lookup — a socket by inode — is found in the shared snapshot rather than re-dumping the socket
table. The cache lives exactly as long as the provider set that `kernel_relationships` built,
which the trace command builds per invocation, so the next trace observes the machine afresh.

This is not staleness sneaking in: a trace's edges already claim coherence with each other
(spec §22.1's confidence model assumes both ends were seen together), and reading the machine
once per trace is *more* coherent than re-reading it between two expansions of the same walk.

Measured on this machine, `trace process 1`: **27.2s before, 1.1s after.**

Alongside: `Graph::from_record` revives a graph from its `ono.graph/1` record — `to_value`'s
inverse — so a graph that travelled a pipeline draws exactly the trees the live one draws, and
the sink renders any single `ono.graph/1` value as trees, never as a table (spec §13.6).

## Consequences

- `trace` is interactive. The remaining cost is one enumeration per target touched, the same
  order as `get process` itself.
- Memory per trace is one snapshot per target — the same order as the graph being built.
- Tests: the round trip is pinned in `ono-graph/tests/graph.rs`; the command in
  `ono-command/tests/trace.rs` (a graph rooted at the named object; a trace of nothing is an
  error naming what was asked); the tree rendering in `ono-cli/tests/native.rs`.

## Alternatives considered

- **Push pinned selectors down instead.** Right and complementary — the netlink provider could
  filter by inode kernel-side — but it fixes one lookup, not the `nodes × processes` shape, and
  the shared observation fixes both.
- **A TTL cache inside each provider.** Rejected: a time-based cache answers "how stale may a
  trace be", which nobody asked; scope-based sharing answers "what is one observation", which is
  the actual semantic unit.
