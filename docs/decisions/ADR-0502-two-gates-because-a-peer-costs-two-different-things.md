# ADR-0502: Two gates, because a peer costs two different things

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.3, §12.1, §12.2, §12.6, §13.1, §14.1, §53.1, §53.2, §54.1, §59.1,
  Appendix A, Appendix B; ADR-0125, ADR-0473, ADR-0501
- Decided by: agent (autonomous)

## Context

§12.1 fixes a ceiling and, in the same paragraph, the hard part of applying it:

> A listening agent MUST have a hard limit on concurrent accepted authenticated connections.
> […] The limit MUST include connections that completed TCP accept but are still in TLS/protocol
> handshake state, using a separate handshake semaphore if required to prevent handshake
> exhaustion.

The two halves pull against each other. Counting handshaking connections against the ceiling means
deciding whether to admit a peer *before* knowing who it is — and a peer refused before TLS cannot
be told anything, because there is no authenticated channel to tell it over (§13.1). Refusing after
TLS gives a peer that is over the ceiling a stable error it can act on, and costs a handshake per
refused peer, which is precisely the exhaustion §12.2 is about.

§12.2 adds a second requirement that is often mistaken for the same one:

> ```text
> max_pending_handshakes = 16
> handshake_timeout = 10 seconds
> ```

A ceiling without a timeout is a listener one silent peer can hold a slot of for ever. A timeout
without a ceiling is a listener a flood can make start sixteen thousand handshakes on. Neither
substitutes for the other.

Ono arrived at H3 with none of it. `serve_authenticated` accepted in a loop, completed each TLS
handshake **on the accept loop itself**, and spawned a task per connection. So one peer that
completed TCP and then said nothing stopped the agent answering anybody, for as long as it cared to
stay silent — which is a denial of service that costs the attacker one socket.

## Decision

### 1. The pending-handshake ceiling gates the accept, and refuses silently

`ConnectionRegistry::begin_handshake` is taken before anything is spent on a peer. Over the
ceiling, the socket is dropped and an audit event is recorded, and the peer is told nothing —
§13.1 leaves no channel to tell it over, and manufacturing one would be spending exactly what the
ceiling is protecting. `docs/spec/hardening/remote_limits.yaml` records `refusal: null` for this
row so the asymmetry is written down rather than discovered.

The slot is released the moment the peer is admitted, not when its session ends. Holding it for the
session's life would make §12.2's sixteen a second, much lower global ceiling.

### 2. The global and per-client ceilings gate admission, and refuse with a stable error

After TLS, the peer's fingerprint is a fact this process verified, so `ConnectionRegistry::admit`
can apply both ceilings and the refusal can be a `Reject` frame carrying
`remote.connection_limit` — §54.1's "a refusal should tell the user which boundary made the
decision", and §59.1's rule that a refusal before negotiation discloses nothing else: no provider,
no schema, no capability, no target.

So an agent holds at most `max_connections + max_pending_handshakes` sockets, bounded, and never a
number a peer chooses. That is the reading §12.1's own sentence sanctions — the separate semaphore
is what the sentence offers, and this is what it is for.

### 3. TLS and Ono negotiation are bounded by one figure, in two places

The TLS handshake is wrapped in `tokio::time::timeout(limits.handshake_timeout(), …)` by the
listener. The Ono `Hello` that follows is wrapped in the same figure inside `ono_protocol::serve`,
which is where the read happens. §12.2 says "TLS plus Ono protocol negotiation", and a deadline on
only the first half would leave a peer that completed TLS and then said nothing holding a
connection for ever.

Two places rather than one because the two reads are in two crates, and the figure they share is
the one `Limits` field (ADR-0501) rather than two constants.

### 4. Two error codes, in the reserved H3 block

| Code | Selector | Kind | Raised when |
| --- | --- | --- | --- |
| `Ono-Sendai-E1501` | `remote.connection_limit` | `resource` | the global or the per-client ceiling was reached |
| `Ono-Sendai-E1502` | `remote.handshake_timeout` | `timeout` | TLS plus negotiation did not finish in time |

`resource` rather than `safety` for the first. §53.1 lists it in the remote family and §12 is
titled *Connection and Resource Limits*; what happened is that a counter reached its ceiling, and a
script that treats it like `remote.unauthorized` would retry never — when this is a refusal
worth retrying: a slot is released whenever a session ends, so it carries `retryable: true` where
every refusal in §9 carries `false`. `remote.unreachable` is retryable too and says nothing about
why; the distinction a script needs is the code (§53.2), and the guidance
`refusal_guidance` attaches tells the person which of the two ceilings to wait on.

One code for both ceilings rather than two. A caller's response to either is the same — wait, or
raise the ceiling — and the metadata carries `ceiling: agent | client` and the figure for anyone
who wants to tell them apart. ADR-0125's rule the other way round applies too: two codes for one
response would make a script that matched one of them incomplete.

### 5. The eighth audit class is raised

`AuditKind::ConnectionLimitDenied` was declared by H2 (ADR-0474) and raised nowhere. Every refusal
above records it, with the source address, the fingerprint where one was proved, and the code. It
is §14.1's eighth class, and the last of the eight to become reachable.

## Consequences

Easy: the accept loop does nothing but accept, so its throughput does not depend on how slow the
slowest peer is. A ceiling is one number in `Limits`, applied in one place.

Hard: an agent can hold `32 + 16` sockets rather than 32. That is bounded and it is more than a
naive reading of Appendix A suggests, so the registry says so and this ADR is where the arithmetic
lives.

Also hard: a peer refused by the pending ceiling learns only that its connection closed. That is a
deliberate asymmetry and the one place in v0.4.1 where a refusal is not explainable to the peer it
refuses — §54.1 is about the user, and the audit trail is where this user reads it.

Encoded by `crates/ono-remote/tests/limits.rs::should_refuse_the_connection_past_the_global_ceiling_and_keep_serving_the_rest`,
`::should_release_a_slot_when_a_connection_closes`,
`::should_refuse_a_seventeenth_pending_handshake`,
`::should_drop_a_handshake_that_has_not_completed_within_the_timeout`, and case
`188-listening-agent-stays-bounded`.

## Alternatives considered

**Apply the global ceiling at accept, so the arithmetic is exactly Appendix A.** The thirty-third
peer would then meet a socket that closes without a word, and `remote.connection_limit` would exist
in the registry and never reach a client. §54.1 and §53.1 both want it to.

**Complete TLS for every peer and refuse afterwards, with no pending ceiling.** Every refusal then
costs a full handshake, which is the cheapest denial of service there is and the one §12.2 names.

**One deadline covering TLS, negotiation and the first request.** It would turn a slow first query
into a dropped connection, and §12.2's deadline is about negotiation. A request budget is a
different limit with different consequences and is not one this section asks for.

**Assert the timeout by measuring it.** ADR-0252 and ADR-0459 both say what a duration assertion is
worth on a loaded machine. What the tests assert is that a peer which says nothing is dropped and
that its slot comes back — states the agent reaches on its own, waited for rather than timed.
