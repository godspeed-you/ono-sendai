# ADR-0503: A client is a key, and a failure is one task

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.1, §2.3, §11.3, §12.3, §12.6, §14.1, §65.2, Appendix A; ADR-0501, ADR-0502
- Decided by: agent (autonomous)

## Context

§12.3 is one line of policy and one line of implementation guidance, and the second is the whole
decision:

> ```text
> max_connections_per_client = 4
> ```
> The limit is keyed by authenticated client fingerprint, not source IP.

§11.3 says why, in the section about what may never stand in for identity:

> Loopback, RFC1918/private address space, Unix user identity inferred from source port, source IP
> allowlists, or "same LAN" MUST NOT substitute for cryptographic client authentication.

A per-address ceiling fails in both directions at once. One client with several addresses walks
past it; several clients behind one address are refused for each other's traffic. It is the cheaper
thing to implement, which is why it is the thing to say no to explicitly.

§12.6 is the neighbouring rule and the one Ono was failing:

> One malformed, unauthorized or slow client MUST NOT terminate the listener or consume unbounded
> tasks. Accept-loop errors are reported and the listener continues unless the listening socket
> itself becomes unusable.

The pre-H3 accept loop completed each TLS handshake inline, so a peer that stalled did not
terminate the listener — it stopped it, which is worse in every way that matters and better only
on paper.

## Decision

### 1. The per-client ceiling is counted over fingerprints in the live registry

`ConnectionRegistry` holds each live connection with the fingerprint the TLS handshake proved, and
`admit` counts the entries matching the arriving peer's. The source address is recorded, in the
audit event, for correlation and for nothing else — which is §65.2's rule and §14.2's purpose for
the field.

The registry is a map rather than a counter because §12.5 needs a handle on each connection, not a
number (ADR-0505). Counting per fingerprint is a filter over the same map, so the two requirements
are one structure.

### 2. One connection is one task, and the accept loop holds nothing

The accept loop does exactly two things: take a socket, take a pending-handshake slot. Everything
after that — TLS, admission, the store read, the session — happens on a task spawned for that
connection, whose outcome nothing waits on. A panic inside it is contained by the runtime; a
refusal, a timeout and an abrupt disconnect are recorded there and end there. §12.6's "consume
unbounded tasks" is answered by the ceilings of ADR-0502: the number of tasks is bounded by
`max_connections + max_pending_handshakes`, because a task cannot exist without a slot.

### 3. The listening socket has three ways to be finished, and everything else is one peer

An accept error is fatal only when the socket itself is gone: `EBADF`, `ENOTSOCK`, `EINVAL`. Every
other error — a peer that reset between the kernel's accept queue and ours, a process out of file
descriptors, a transient refusal — is about one connection, is recorded, and the loop continues.
The distinction lives in `io::ErrorKind` rather than in a message, which is why
`TlsListener::accept_tcp` reports the operating system's error unwrapped rather than as an
`ErrorValue` (§53.2: policy is decided on codes, never on strings).

### 4. A poisoned lock is not a way for one connection to close the agent

The registry's state is behind a `std::sync::Mutex`, and a task that panicked while holding it
would poison it — turning one failed connection into every later connection failing, which is
§12.6's failure wearing a different name. The lock is taken with
`unwrap_or_else(PoisonError::into_inner)`, and it is safe to: every mutation under it is a single
insert, remove or increment, so there is no half-written state for a panicking holder to leave
behind.

## Consequences

Easy: one client cannot fill the global ceiling, and a second client behind the same NAT is
unaffected by the first. A failing connection costs one task and one log line.

Hard: a legitimate operator running five terminals against one agent from one machine meets the
ceiling, because five terminals from one identity is what four connections per client means. The
figure is Appendix A's, it is configurable through `limits.remote_connections_per_client`, and the
refusal names both.

Also hard: `Fingerprint` equality is now load-bearing for a resource decision as well as for a
trust decision. It is a SHA-256 digest compared as a fixed-size value, which is the same comparison
the authorization store already makes.

Encoded by `crates/ono-remote/tests/limits.rs::should_refuse_a_fifth_connection_from_one_authenticated_fingerprint`,
`::should_key_the_per_client_ceiling_on_the_fingerprint_rather_than_the_address`,
`::should_keep_accepting_after_one_connection_fails`, and
`::should_leave_every_other_session_intact_when_one_connection_is_aborted`.

## Alternatives considered

**Key the ceiling on the source address, or on address and fingerprint together.** §12.3 says
fingerprint and §11.3 says why. Adding the address as a second key would reintroduce exactly the
two failures above for the clients it applied to.

**A `tokio::sync::Semaphore` for each ceiling instead of a registry.** Semaphores count, and
counting is all §12.1 and §12.3 need. §12.5 needs to reach into a live connection, and a semaphore
cannot be asked which permits belong to which key. Two mechanisms for one set of facts is the
drift §52.2 is about, applied to code.

**Assert failure isolation by asserting that nothing panicked.** A test that only checks for the
absence of a panic passes against a listener that has silently stopped accepting. The tests here
make a connection fail and then require the *next* one to be served — and the isolation test
additionally requires the failing peer to still be failing at that moment, so a listener that had
to finish with it first fails rather than passing slowly.
