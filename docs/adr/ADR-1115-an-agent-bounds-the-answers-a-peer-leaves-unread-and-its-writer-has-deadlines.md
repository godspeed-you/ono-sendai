# ADR-1115: An agent bounds the answers a peer leaves unread, and its writer has deadlines

- Status: accepted
- Date: 2026-11-25
- Spec refs: v0.4.1 §12.1, §12.3, §12.5, §12.6, §14.1, §18.5; spec §18.5; ADR-0015 T7, ADR-1095
- Issues: #308 (reviews PRF-2, ARC-13, SEC-1, SEC-2)
- Decided by: agent (autonomous)

## Context

`connection.rs` queues every outbound frame on an unbounded channel and argues that credit bounds
it: no data frame leaves before its consumer granted room. That argument covers data. It does not
cover what an agent writes without credit because its peer asked: the `end` that answers a probe
(ADR-1095, new in v0.6.6, announced to every authorized connection) and the refusal of a stream
beyond `max_streams`. A peer that sends probes and never reads makes the agent queue one frame per
probe for ever — the security review reproduced 2 million unread probes growing an agent to
467 MB.

The same review found the writer itself unbounded in time. A peer that stops reading leaves the
writer blocked in `write_all`; when the session ends — the caller hangs up, a revocation drops it
(§12.5), the serving loop returns and its registry slot is released (§12.1) — the writer task
still owns the socket and the queue, outside every ceiling. That part predates v0.6.6
(`git show 42cf37d6:crates/ono-protocol/src/connection.rs`: the same unbounded queue and the same
`write_all` with no deadline); the probe made it cheap to exploit.

## Decision

**An allowance of unread answers.** Frames no credit governs are queued through
`FrameSink::send_uncredited`, which counts them until the writer has written them. At most
`MAX_UNCREDITED_FRAMES` (1024) may wait at once. The 1025th is refused; the agent ends the
connection with `ProtocolError::UnreadAnswers` — `remote.protocol_mismatch` (E0602), the code the
link's other enforced bounds (`CreditExceeded`, `TooManyStreams`, oversized frames) already carry,
not retryable — and records `connection.limit_denied` with that code, the audit kind §14.1's
trail already has for a connection ended at a limit. No new error code, no new audit kind. The
probe's answer and both frames of a stream refusal count; credited data and the client's control
frames (cancel, credit) do not — they are bounded by credit and by the peer's own window, and a
full queue must never block a cancel (§18.5). An honest peer reads what it asks for and never
comes near the allowance: a probe pass is one probe per link.

**Two deadlines on the writer.** While the session lives, one write that makes no progress for
`WRITE_DEADLINE` (30 s) ends the connection: an honest reader drains the socket continuously (the
client's reader task forwards every frame as it arrives), so a write stalled that long is a peer
that is not reading. Once the session has ended (`FrameSink::hangup`, which every end of a
serving loop and every dropped `Link` calls), whatever is still queued gets at most the handshake
timeout to leave — the one figure §12.2 already gives an unresponsive peer, configured by
`limits.remote_handshake_timeout_ms` — and then the socket is dropped and the rest is never
written. So a non-reading peer holds its socket at most that long past its registry slot or its
revocation.

## Consequences

- Memory per connection for frames nobody granted credit for is bounded by 1024 small frames;
  the queue for credited data stays bounded by credit, as before.
- A revoked or ended session releases its socket within the handshake timeout whatever the peer
  does. The "writer task has no write deadline after `serve` returns" entry of the run's notes is
  closed by this decision.
- A link whose network stalls for more than 30 s mid-write is ended rather than waited on; the
  holder re-dials (agent-held links back off and reconnect, ADR-1097).
- Tests: `crates/ono-protocol/tests/probe.rs::should_end_the_connection_of_a_peer_that_asks_for_probes_and_never_reads_the_answers`
  (a 1 KiB socket, 1280 unread probes: the agent ends the connection with the bound and the audit
  record; on the old code the serving task never ended),
  `…::should_let_go_of_a_peer_that_stops_reading_once_the_session_has_ended` (400 unread answers
  behind a 1 KiB socket, the caller hangs up: past the deadline the peer reads again and receives
  less than it asked for, because the writer let the socket go; on the old code it received all
  6,400 bytes). `docs/contracts/hardening/security_boundaries.yaml` lists the `Message::Probe`
  dispatch path with the allowance as its guard.

## Alternatives considered

- **A bounded writer channel for everything.** A full queue would block a cancel behind the data
  it cancels (the reason the module gave for leaving the queue unbounded), and would make credited
  producers wait twice.
- **Rate-limiting probes.** Bounds the rate, not the queue: a peer that never reads still grows
  it, only slower.
- **A new error code (`remote.peer_not_reading`).** The brief forbids a new taxonomy where the
  link's enforced bounds already have one; the message names the bound.
- **A configurable allowance.** No operator has a reason to tune it; an honest peer uses a
  handful. A figure in the code, named here, is the smaller surface.
