# ADR-0505: Revocation reaches the connection, not only the next one

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §9.2, §9.7, §10.3, §12.1, §12.3, §12.5, §14.1, §54.1; ADR-0466, ADR-0468,
  ADR-0470, ADR-0473, ADR-0501, ADR-0502, ADR-0503
- Decided by: agent (autonomous)

## Context

§12.5 sets one MUST, one SHOULD and one escape:

> Removing an authorized client MUST prevent all new connections immediately.
>
> The reference implementation SHOULD also close existing direct-TCP connections for that
> fingerprint within 5 seconds. If live revocation is not implemented, `remove client-key` MUST say
> clearly that existing connections remain valid until disconnect and an ADR MUST record the
> limitation.

H2 took the escape, and said exactly why (ADR-0470):

> tearing down a live connection needs a registry of live connections and a cancellation path into
> each, which is phase H3's connection semaphore (§12.1, §12.3) and does not exist yet. When H3
> builds that registry, live revocation becomes a small addition on top of it, and this ADR should
> be superseded rather than extended.

H3 builds that registry, because §12.1 and §12.3 need it (ADR-0503). So the deferral's stated
condition has expired, and the question is live again rather than settled.

## Decision

**Live revocation is implemented.** `remove client-key` refuses the key's next connection and ends
every session it currently holds.

### 1. A sweep, once a second, over the registry the ceilings already keep

`ListeningAgent::run` spawns a task that re-reads the authorization store on a one-second interval
and calls `ConnectionRegistry::revoke_absent`, which signals every live connection whose fingerprint
the store no longer lists. The signal is a `oneshot` the connection's task selects on beside its own
session future; when it fires, the session future is dropped, the transport with it, and the socket
closes. That is what "the connection is terminated" means to the peer.

One second, for §12.5's five: it leaves room for the read, the sweep and the session noticing, and
costs one `stat` and a small parse per second on a host that is already serving a network socket.

The registry is `ConnectionRegistry` — the same map the global and per-client ceilings are counted
in. There is no second structure, which is why this is a small addition rather than a feature.

### 2. ADR-0470's other decision stands

The `AuthorizationContext` is still built once per accepted connection and is still immutable for
that connection's life. §10.3 requires that, and live revocation does not touch it: a running
connection's *grant* never changes, and what changes is whether the connection continues to exist.
Those are different questions, and conflating them would reintroduce the TOCTOU §10.3 rules out.

So ADR-0470 is superseded in the one paragraph that deferred this, and in nothing else.

### 3. ADR-0470's first objection, answered rather than dismissed

> a running connection may be mid-action, and killing it half way through a mutation leaves the far
> side in a state nobody chose

True, and it is the reason §12.5 says SHOULD rather than MUST. Two things make it the lesser risk
here. An operator who types `remove client-key` has decided that this client should stop, and
"stop after the mutation it is currently running" is not a promise the shell can keep anyway — the
next request would arrive a millisecond later. And an action that is interrupted mid-flight is a
condition every remote client already has to survive, because a network drops connections without
asking; a revocation that closes a socket is indistinguishable, to the far side, from the cable
being pulled.

What is *not* done is killing an action in progress on the agent's own side. The session future is
dropped, which cancels the work it owns through the cancellation the protocol already threads
(ADR-0015), rather than leaving a detached task running for a client that no longer exists.

### 4. A store that will not parse does not revoke anybody

A malformed store authorizes nobody (ADR-0466), and a sweep that acted on that reading would close
every session because a file was momentarily half-written. So the sweep skips a store it cannot
read and tries again a second later. Refusing *new* connections is the fail-closed response to an
unreadable store, and that is unchanged; ending established ones on the strength of a parse error
would be a self-inflicted outage.

### 5. `remove client-key` says what it now does

> `<fingerprint>` is revoked; its next connection is refused and any session it holds is closed

§12.5's MUST about the message is a MUST about accuracy, not about a particular sentence: the
operator has to learn what happened where they made it happen. The message changes with the
behaviour, in the same increment.

## Consequences

Easy: "revoke" means what a person expects it to mean. The five-second window is met with four to
spare, and the mechanism is a sweep over a map that had to exist anyway.

Hard: a live connection can now end for a reason that is not the peer's doing, and the peer learns
it as a closed socket rather than as a stated refusal. There is no frame for "you have been
revoked" — inventing one would tell a client something about the host's policy after deciding it
may know nothing (§59.1) — so the audit trail carries `connection.disconnected` with
`error_code=remote.unauthorized`, and the peer's own next connection is refused with the reason.

Also hard: the sweep is a wall-clock timer in a process that has otherwise very few. It is aborted
with the accept loop, and what the tests wait on is the registry emptying rather than an elapsed
duration — a state the agent reaches on its own (ADR-0459).

Encoded by `crates/ono-remote/tests/limits.rs::should_terminate_an_established_session_when_its_authorization_is_revoked`
and `crates/ono-cli/tests/client_keys.rs::should_refuse_the_next_connection_after_a_client_key_is_removed`,
with case `188-listening-agent-stays-bounded` asserting the message and the refusal at the product.

## Alternatives considered

**Defer again, and record the limitation.** The honest option if the registry had not been built,
and it has been. §12.5's escape is conditional on live revocation "not being implemented", and the
condition ADR-0470 gave for revisiting — H3's registry — is exactly what this milestone delivered.

**Check the store on every request instead of sweeping.** It would revoke faster and it is the
TOCTOU §10.3 forbids: a connection's policy would change under a request that had already been
authorized against a different one. §10.3's sentence is about the grant; this decision is about the
connection, and keeping them separate is what makes both rules simple.

**Push a signal from `remove client-key` to the agent rather than polling.** The command runs in a
different process from the agent and often on a different terminal; a signalling path between them
is an IPC surface, with a permission model of its own, to save one file read per second.

**Close the connection at the transport with a `Reject` frame first.** The client is being told
that its authorization is gone, which is a fact about the host's policy that §59.1 says a refused
peer does not get. The closed socket says everything the peer is owed, and the peer's next
connection is refused with a reason.
