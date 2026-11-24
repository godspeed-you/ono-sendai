# ADR-1055: Only a connection that was granted can be revoked

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.4.1 §9.1, §10.3, §12.5, §54.1, §59.1, §59.9; ADR-0470, ADR-0505
- Issues: #161
- Decided by: agent (autonomous)

## Context

ADR-0505 gave every live connection of a listening agent a handle the §12.5 revocation sweep can
pull: once a second, the sweep re-reads the authorization store and ends every live connection
whose fingerprint the store does not list. "Every live connection" included the ones whose client
the store had *never* listed. Those are admitted to the registry (the ceilings of §12.1 and §12.3
count them) and then refused by their own session, which reads the same store, resolves the
peer's fingerprint against it and answers the `Hello` with `remote.unauthorized` (E1202).

Between the peer's TLS handshake and its `Hello` the two decisions raced. When the sweep ran in
that window it closed the connection, `tokio::select!` dropped the session that was about to send
the refusal, and the peer met a socket that went away: `remote.unreachable` (E0601) instead of
E1202. Issue #161 measured it at 2 of 6 workspace runs at load 22–26; §54.1 asks a refusal to name
the boundary that decided, and §59.9 asks every trust failure to be deterministic.

The diagnosis was verified rather than assumed:
`crates/ono-remote/tests/limits.rs::should_refuse_a_never_authorized_client_with_a_refusal_even_when_a_revocation_sweep_runs_first`
forces the interleaving — the never-authorized peer completes mutual TLS and holds its `Hello`
until the audit trail proves a sweep ran over it (a real revocation of another client) — and
failed with `RemoteUnreachable: the peer closed the link without answering` before the change.

## Decision

**A connection is revocable only if the store it is served from granted its client.** Admission
(`ConnectionRegistry::admit`) counts a connection and gives it no revocation handle. After the
listener has read the store the session will be authorized from (once per connection, §10.3,
ADR-0470), it asks that same snapshot `store.client(fingerprint).is_some()` — the exact predicate
the sweep negates — and only then registers the handle the sweep pulls. A connection without one
is skipped by `revoke_absent`.

Why this closes the race by construction rather than narrowing it:

- *A never-authorized client is never closed by the sweep.* It holds no handle, so there is
  nothing for the sweep to send on; whatever the timing, its session runs to its refusal.
- *A never-authorized client is never served.* The session resolves the peer against the very
  snapshot the revocability question was asked of. A fingerprint that snapshot does not list is
  refused (`AuthorizedClients::authorize` → E1202) before negotiation, so there is no instant at
  which a connection is both unrevocable and authorized. A later store that *adds* the client
  does not reach this connection (§10.3): it is refused, and the next connection is the one that
  benefits from the operator's `add client-key`.
- *A revoked client still loses its connection within §12.5's bound.* A client granted by its
  snapshot registers the handle before its session starts. If the operator removes it before
  that registration, the sweep that runs after it sees the handle and the absent fingerprint and
  closes the connection; if after, likewise. In both cases the next sweep — at most
  `REVOCATION_SWEEP` (1 s) later — ends it, inside §12.5's five seconds.
  `limits.rs::should_terminate_an_established_session_when_its_authorization_is_revoked` is
  unchanged and green.

## Consequences

- E1202 arrives at a never-authorized client however the sweep and the handshake interleave; the
  audit trail no longer records a `connection_id=revoked` line for a client nothing was ever
  granted to (the new test asserts both).
- The registry's live count still includes refused connections while their refusal is written,
  so the ceilings of §12.1 and §12.3 are unchanged.
- The exit test of #161 —
  `crates/ono-cli/tests/authenticated_link.rs::should_refuse_an_authenticated_client_the_agent_never_authorized`
  and `::should_report_an_authenticated_but_unauthorized_link_as_exactly_that` — is a
  confirmation run, not the proof; the proof is the forced interleaving above.

## Alternatives considered

- **Check the store inside the sweep against "was this client ever granted".** Equivalent in
  outcome, but it needs the sweep to know each connection's initial grant, which is exactly the
  fact the handle now records; storing it twice would be a second representation.
- **Let the refusal win the `select!` (biased select, or deferring the sweep until the session
  has authorized).** Narrows the window rather than closing it: a sweep that fires while the
  session is still waiting for the `Hello` would still drop it.
- **Read the store before admission.** Would let `admit` decide revocability in one step, but it
  changes which refusal a peer over a ceiling *and* facing an unreadable store meets, for no gain;
  registering the handle after the read is the smaller change.
