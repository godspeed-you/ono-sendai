# ADR-0274: What host-key pinning needs before it can be live

- Status: superseded by ADR-0441
- Date: 2026-08-29
- Spec refs: §21.2, §21.5, §49; ADR-0015 T5/T6, ADR-0037 §4, ADR-0245
- Decided by: agent (autonomous, `close-spat`)

## Context

`docs/STATE.md`'s B-remote-2 asks for an acceptance case in which a changed host key refuses with
`remote.host_key_changed` (E0603). This ADR records why that case cannot be written today, exactly
what would have to exist first, and that the item therefore stays open.

What exists is complete and proven at unit level:

- `ono-protocol/src/trust.rs` — `TrustStore` (in-memory and file-backed), `HostKey`, full SHA-256
  `Fingerprint`, `TrustPolicy::{Required, Pinned, Unauthenticated}`, `verify`, `pin`, `repin`, and
  `decide`, called from `RemoteLink::connect` *before* the handshake is offered, as §21.5 and
  ADR-0015 T5 require.
- Ten tests in `ono-protocol/tests/trust.rs` and three in `ono-remote/tests/trust.rs`, including
  `should_refuse_a_changed_host_key_with_the_stable_safety_code`, which asserts E0603 and that it
  is not retryable.

What does not exist is a production transport that can present a peer key.

## The obstacle, precisely

Both production transports go through `SubprocessTransport`, and `Transport::peer_key` answers
`None` for both:

- **`ssh`** spawns `ssh -o BatchMode=yes -T … ono --agent` (`ono-remote/src/transport.rs`). OpenSSH
  authenticates the host against its own `known_hosts` before a single frame crosses, and offers
  the parent process no way to learn which key it accepted. ADR-0037 §4 therefore has the transport
  answer `None` truthfully and the link use `TrustPolicy::Unauthenticated`, named in code so nobody
  enables it by accident.
- **`local`** spawns this very binary. There is no peer to authenticate; the child is this process's
  own.

`decide(Unauthenticated, …, None)` never reaches `store.verify`, so `host_key_changed` is
unreachable in production and `TrustStore::open` is called from nothing but tests.

Reading the accepted key out of `~/.ssh/known_hosts` would not fix this. It would record a
verification *OpenSSH* performed as if Ono had performed it, which is the claim ADR-0037 §4 refuses
and which §2.17 forbids in general: an unobserved fact is not a fact.

## What must exist first

**A transport that certifies its peer to this process.** Concretely, one of:

1. **An Ono-native authenticated transport over TCP** — TLS or Noise, where the peer's certificate
   or static public key *is* the `HostKey` and `Transport::peer_key` can answer it honestly. §21.2
   already anticipates one ("a future TCP transport gets `Required` with the trust store", the
   comment beside the `Unauthenticated` choice). This is the route the trust store was built for.
2. **An ssh implementation inside Ono** — a Rust ssh client rather than `ssh(1)`, so the host key
   crosses this process. That is a second authentication path beside OpenSSH's, with its own
   `known_hosts` semantics and its own cryptographic surface, and it earns a decision of its own
   before anyone starts it.

Only after one of them exists can:

- the exit test be written — an acceptance case that connects, changes the key and asserts E0603;
- **F12 be settled.** `TrustPolicy`'s default is `Required` (trust on first use), and ADR-0015 T5
  wants an unknown key refused, which is `Pinned`. Which default is right depends on whether first
  contact can be verified out of band, and that question has no answer while no transport
  authenticates at all. The contradiction is real and cannot be resolved by choosing a word.

## Decision

**B-remote-2 stays open.** Nothing is changed by this ADR. It exists so that the gap is documented
where the code is, rather than only in a progress board, and so that the next agent does not spend
the increment rediscovering that `ssh(1)` will not tell it the key.

Two things must **not** be done in the meantime, and this ADR is the place they are ruled out:

- do not copy `known_hosts` into the trust store (asserting someone else's verification);
- do not add a trust-store command surface — `get`/`set`/`forget` a pinned key — while nothing
  writes to the store on any production path. A safety control that does nothing is the defect
  class this tranche has been removing all day, and a UX over an unused store is exactly that.

## Consequences

- `docs/ACCEPTANCE.md`'s remote box keeps naming the unit-level E0603 proof, which is what is
  actually true.
- ADR-0037 §4 stands unchanged; this ADR is its scope note, not its reversal.
- ADR-0015 T5/T6 keep their unit-level tests (named in ADR-0245); neither has an acceptance case
  and neither can have one yet.
