# ADR-0551: A trace is filtered at its root, not at every node it reaches

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.2 §22 (`trace` and the relationship graph), §22.1 (Node and Edge), §22.3 (the
  useful traces: `trace connection --remote …`); v0.4.1 §2.7; `docs/spec/schemas/graph.v1.yaml`
  (`root`: "the object the trace started from"); AGENTS.md §11
- Decided by: agent (autonomous)

## Context

`crates/ono-cli/tests/options_and_selectors.rs::should_trace_nothing_else_when_no_connection_has_the_requested_remote`
was reported failing under a full parallel workspace run, returning "graphs holding unrelated
sockets — TCP listeners other suites bound, and unix sockets", and passing in isolation.

The test ran `trace connection --remote 192.0.2.1` — TEST-NET-1, never routed — and, if the
command succeeded, required **every** `ono.socket/1` node of the graph to carry that peer.

That requirement is not what the contract says, and no correct implementation can satisfy it. A
trace of a real connection on this machine returns 256 nodes: the connection, the process holding
it, that process's other sockets, its open files, its children, their sockets. Measured, on a peer
this machine really has:

```text
trace connection --remote 78.46.37.25 | to json
  256 nodes, of which 84 are ono.socket/1; their peers include 34.107.243.93,
  2a04:4e42:200::347 and dozens with none at all
```

Reaching those is the whole of what §22 is for — "the relationships the kernel actually asserts".
So the assertion was green only because the command **refused**, and it refused only because this
particular host held no connection to TEST-NET-1. The test was asserting a property of the host's
socket table. On a host where anything at all answered the selector, the assertion would have been
wrong rather than merely lucky, and that is the shape the failure report has.

## Decision

**`--remote` names the subject of the trace, and the subject of a graph is its root.**

`graph.v1` already says which field that is: `root` is "the object the trace started from". The
nodes reachable from it are relationships and are deliberately not filtered — filtering them would
produce a graph with edges to objects it does not contain, which is a graph nobody can render.

So the test asserts on `root`, and on nothing else:

- with a peer the fixture created, `trace connection --remote 127.0.0.1` succeeds and the graph is
  rooted at a socket whose `remote.address` is `127.0.0.1`;
- with a peer nothing can hold, `trace connection --remote 192.0.2.1` either refuses with the
  structured not-found of §40, or is rooted at a socket with that peer. A graph rooted at some
  other connection is the one answer that is wrong — and it is exactly the answer an ignored
  `--remote` gives.

**And the fixture owns the connection the positive half needs.** AGENTS.md §11 forbids relying on
the developer machine's real sockets unless the fixture creates them, and the negative half was
doing precisely that: it depended on the host having no TEST-NET-1 connection to keep the command
refusing. The test now binds a listener on `127.0.0.1:0`, dials it and accepts, and holds all
three sockets for the shell's whole run. The peer that exists is one this test made; the peer that
does not is one RFC 5737 guarantees.

## Consequences

Easy: the test now fails for one reason — a `--remote` the provider did not honour — and passes
whatever else the host is running. Verified by disabling the filter in
`crates/ono-provider-netlink/src/provider.rs::keep`: red at the TEST-NET-1 assertion, naming the
connection the graph was rooted at instead.

Also easy, and the point: the positive half means the negative half can no longer pass vacuously.
A `trace connection --remote` that refused *everything* would now be red, which is §65.10's rule
about a test that reports a pass it did not earn.

Hard: nothing here constrains what a trace reaches, and a future defect that pulled in an
unrelated *subject* through a relationship provider would not be caught by this test. That belongs
to the relationship providers' own suites (`crates/ono-graph/`), where an edge is asserted against
the fixture that created both of its ends.

No production code changed. The filter was correct; the assertion about it was not.

## Alternatives considered

**Keep the every-node assertion and bound the trace to depth 0.** It would make the assertion true
and would test a command nobody runs: a trace with no relationships is `get`.

**Assert that every node is *reachable* from a 192.0.2.1 connection.** Every node in a graph the
tracer built is reachable from its root by construction, so the assertion would hold for any graph
and prove nothing.

**Leave the test as it was and accept the flake.** It passes by refusing, so on the day the shell
stops honouring `--remote` it goes red for the right reason — but on a host holding any matching
connection it goes red for the wrong one, and the report that opened this ADR is that day
happening. A test that is correct only on hosts with a particular socket table is the defect
AGENTS.md §11 names.
