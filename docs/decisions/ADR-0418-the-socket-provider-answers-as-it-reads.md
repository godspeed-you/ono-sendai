# ADR-0418: The socket provider answers as it reads

- Status: accepted
- Date: 2026-08-30
- Spec refs: v0.2 §34 (latency is a product property; the first row of an enumeration inside
  50 ms), §16.5 (partial failure), §23.2 and §28.4 (`ono.socket/1`), §11.1 (a bounded stream);
  ADR-0015 T7 (every decoder in the netlink crate is bounded)
- Decided by: agent (autonomous)

## Context

`get socket | take 1` cost the whole socket table. `read_sockets` issued all five `sock_diag`
dumps, decoded every message of every one of them into a `RecordValue`, and handed the finished
`Decoded` to the stream; `take 1` then threw away everything after the first row. The work was
already paid for by the time the first row existed.

On an ordinary host nobody notices. Acceptance case `152` builds the host spec §34 asks about —
5 000 listening Unix sockets — and measured the first row at 51 ms against the 50 ms budget of
§34, which is why that case, and the acceptance job in CI with it, shipped red with v0.4.0.

The cost splits in two, and both halves are the same mistake:

- **Decoding.** 5 000 messages became 5 000 records — schema validation, provenance and a
  `Timestamp::now()` each — at roughly 4 µs apiece. Measured on the container: 31 ms for the
  first row with two sockets on the host, 51 ms with 5 002.
- **Dumping.** Five dumps were always issued, in full, whatever the consumer wanted. An
  `inet_diag` dump is a kernel-side walk of the hash tables and costs milliseconds on a busy
  host even when it answers with nothing.

## Decision

**The socket provider reads only as far as its consumer.**

- The decoders are iterators. `inet_sockets` and `unix_sockets` walk a dump one netlink message
  at a time and yield one `Item` — a record, or the reason a message could not become one — per
  message. `decode_inet_sockets` and `decode_unix_sockets` keep their signatures and collect
  those items, so a caller that wants the whole table still gets it in one call.
- `snapshot` runs the reader on a blocking thread and hands objects to the stream in batches of
  64 over a channel that holds one batch. The channel is the backpressure: the reader parks
  while a batch is outstanding, and a consumer that stops takes the reader down with it at its
  next handover. A dump ends with a handover even when it decoded nothing, so the reader is at
  most one dump ahead of the consumer.
- **A dump nothing in the answer can come from is not issued.** The `connection` target is the
  one case that exists today: every Unix socket record carries `remote: null`, because its peer
  is an inode rather than an address, so no Unix socket can ever be a connection and the
  `unix_diag` dump is skipped for that target.

**Failures travel in the order they were met**, rather than ahead of every object as `emit`
sent them. §16.5 requires that a failure be reported rather than swallowed, and it still is: a
failure is handed over before every object read after it, so no `take` can truncate away a
failure that had already happened. What changes is that a consumer which stops early also stops
the reading, and an address family this provider never consulted has not failed. Reporting a
failure for a dump nobody asked for would be the invention §35.3 forbids, in the other
direction.

## Consequences

- Case `152` passes: on the container the first socket row costs 47 ms against a 45 ms baseline
  on the same host — the pathological table now costs 2 ms rather than 20.
- The whole table is marginally more expensive (`get socket | count` on 5 000 sockets: 54 ms
  before, 78 ms after, against a 5 000 ms budget) because every record crosses a channel. The
  trade is deliberate: the interactive path is the one §34 puts a budget on.
- `emit` still serves the interface, route and neighbour providers, which read one small table
  and have no reason to stream.
- A future target that can be answered without a dump can skip it the same way. The rule is that
  the skip must follow from the schema — as `remote: null` does — never from a guess about what
  the host is likely to hold.
- Tests: `kernel_providers::should_answer_exactly_the_bound_when_a_socket_query_asks_for_one`
  and `::should_answer_no_unix_socket_when_the_connection_target_is_asked`; the decoding
  fidelity of the iterators is held by the existing `socket_decoding` and `malformed_messages`
  suites, which did not change. The latency claim is case `152`'s to make.

## Alternatives considered

- **Push `take N` down into the query.** `take` is a pipeline transform, and the provider sees
  `Query::max()` only when a command spells a bound itself. Teaching the planner to fold a
  following `take` into the query would help exactly the shapes that end in `take` and nothing
  else — `| where … | take 1` would still read the table. Rejected as the narrower fix.
- **Decode lazily but keep issuing every dump.** Half the cost, and the half that grows with the
  host's socket table stays. Rejected.
- **Raise the budget in case `152`.** The budget is spec §34's, and the harness is not weakened
  to get a green result (AGENTS.md §14). Rejected.
