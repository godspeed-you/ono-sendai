# ADR-1057: A handshake that missed its deadline is told so, within one more

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.4.1 §12.2, §12.6, §13.1, §14.1, §54.1, §59.1; ADR-0505, ADR-1056
- Issues: #181
- Decided by: agent (autonomous)

## Context

§12.2: "A peer that does not complete TLS plus Ono protocol negotiation within the timeout MUST be
disconnected", and `remote.handshake_timeout` (E1502) is the refusal §53.1 lists for it. Issue #181
found that E1502 never reached a peer. Reading the code showed two halves with two different
defects:

- **The TLS half** (`crates/ono-remote/src/listener.rs`): when the deadline passes while TLS is
  still being negotiated, the listener built E1502, audited it, printed it on the agent's stderr
  and closed the socket. The issue names this path and asks for `ono_protocol::refuse` as on the
  E1501 path beside it — but the E1501 path runs after TLS, and here there is no TLS session: the
  rustls state was dropped with the timed-out future, and the peer has proved nothing (§13.1).
- **The Ono half** (`ono_protocol::serve`): a peer that completed mutual TLS and then sent no
  `Hello` within the deadline was dropped with `Err(remote.handshake_timeout)` — no `Reject`, and
  no audit record either. This peer *does* have an authenticated channel.

There is also the hostile reading the task demanded: a peer whose handshake timed out may be a
slow-loris, and telling it why must not become a way to hold the agent longer — by not reading the
refusal, or by never hanging up after it. Every handshake-time refusal in `serve` was queued for
the connection's writer task, which writes with no deadline; a peer that never reads keeps that
task, and the socket it owns, alive for as long as it likes.

## Decision

**E1502 is said wherever there is a channel to say it on.**

- *After TLS* (`serve`, the Ono half): the deadline passing writes a `Reject` carrying E1502 and
  its `timeout_ms` (ADR-1056's `over_the_wire`), records a `connection.limit_denied` audit event
  with the peer's fingerprint and address, and returns `Ok(())` — refusing a slow peer is the
  agent doing its job, like every other refusal `serve` makes. The client's error is E1502,
  `retryable: true`, with `timeout_ms` and the remediation `refusal_guidance` supplies.
- *During TLS* (the listener): unchanged in outcome — recorded and closed, nothing written. A peer
  that has not completed mutual TLS has no authenticated channel to be told anything over
  (§13.1), and the only thing that could be written is a TLS alert, which carries no Ono code.
  What such a client reports is its own transport failure (`remote.unreachable`), which is true:
  it never reached an Ono endpoint. `ono` always sends its `Hello` immediately after TLS, so the
  half in which a real client can be *late* is the Ono half, and that is the half that now speaks.
- One constructor serves both halves: `ono_protocol::handshake_timeout_refusal`. The census row
  for `remote.handshake_timeout` moves its `decided_by` from `ono-remote` to `ono-protocol`, the
  crate that now constructs it.

**Disclosure.** The peer told E1502 has completed mutual TLS, so it is authenticated; it may not
be authorized, since the deadline is enforced before the `Hello` it would be authorized from. It
learns the code, a sentence and `timeout_ms` — a non-secret limit (§53.3) that is the agent's
setting, not the store's contents — and nothing about whether it would have been authorized.

**Every handshake-time refusal is written within a bound, not through the writer task.** `serve`
no longer starts the connection's writer task until a link is accepted. Until then, the one frame
it may write — a refusal, whichever refusal — goes through `refuse_within`, which inside a single
`handshake_timeout`:

1. writes and flushes the frame, then shuts the sending direction down;
2. reads and discards whatever the peer still sends until it hangs up — a socket closed with
   unread input is reset, and a reset can destroy the refusal before the peer reads it.

When the bound runs out the transport is dropped, whatever the peer is doing. `refuse` (the
listener's pre-negotiation path for E1501 and an unreadable store) uses the same helper after its
existing bounded wait for the `Hello`. So the agent's cost per refused peer is fixed by its own
limits — at most the deadline the peer missed plus one more for the refusal — and is the same
whether the peer reads promptly, never reads, or keeps talking. During that time the connection
holds a slot the §12.1/§12.3 ceilings count, so the sockets held this way stay bounded by them.

## Consequences

- Exit test of #181: `crates/ono-remote/tests/limits.rs::should_tell_a_client_whose_handshake_timed_out_that_it_did`
  — over real TLS sockets, a client whose `Hello` is late receives E1502 with `timeout_ms`, and the
  agent's audit names it; the test waits for the agent's own audit record, not for a duration.
- `crates/ono-protocol/tests/handshake_deadline.rs` proves the three properties with paused time
  (deterministic, no sleeps as synchronisation): the peer that never says hello reads a `Reject`
  E1502; a peer that never reads it (a sixteen-byte pipe nobody drains) is let go within twice the
  deadline; a peer that reads it and keeps sending frames is let go within the same bound.
- `ono --agent` over stdio whose caller never says hello now ends with status 0 after writing the
  refusal, where it used to end with status 1 and the timeout on stderr; the decision is in the
  refusal and, for a listening agent, in the audit trail.
- Not changed, and reported rather than fixed: the two halves of §12.2's deadline are still
  measured separately (TLS gets one `handshake_timeout`, the `Hello` another), so a peer can spend
  up to twice the figure before being refused, plus the refusal's own bound. §12.2 reads as one
  deadline for both. Both waits are bounded, and changing the split touches every listener test's
  timing; it is outside #181.

## Alternatives considered

- **Answer a TLS-phase timeout with something.** There is no Ono channel; a TLS alert is not a
  structured refusal, and anything else would be bytes to an unauthenticated peer (§13.1).
- **Keep the writer task and add a write timeout to it.** The writer task serves established
  sessions too, where a peer that reads slowly is legitimate back-pressure; a write deadline there
  would be a behaviour change for every link, not a fix for the refusal.
- **Close without draining.** Cheaper, and it reintroduces the reset that destroys the refusal
  before the peer reads it — the race `refuse` was written to avoid.
- **A separate, shorter grace for the refusal.** Would be a second number for §52.2 to centralise;
  the handshake timeout is already the figure an operator sets for "how long a stranger may hold
  this agent before it has a link".
