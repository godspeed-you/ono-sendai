# ADR-0353: The Ono transport certifies its peer, and the certificate is the pinned key

- Status: accepted
- Date: 2026-08-29
- Spec refs: §21.2, §21.5, §49; ADR-0015 T5/T6, ADR-0037 §4, ADR-0274
- Decided by: agent (autonomous, `close-remote`)

## Context

ADR-0274 recorded the honest blocker under B-remote-2: the trust store of ADR-0015 T5/T6 was
complete and proven at unit level, and consulted by nothing in production, because both production
transports run behind an `ssh` subprocess whose `peer_key` is truthfully `None` (ADR-0037 §4).
`remote.host_key_changed` (E0603) was unreachable outside a fixture, and F12 — whether the default
policy should refuse an unknown key or record it — could not be settled while no transport
authenticated at all.

ADR-0274 named the two ways out and said which one the trust store was built for: "an Ono-native
authenticated transport over TCP — TLS or Noise, where the peer's certificate or static public key
*is* the `HostKey`". This ADR takes that route.

## Decision

1. **The transport is TCP with TLS 1.3, in `crates/ono-remote/src/tls.rs`.** rustls with the `ring`
   provider, TLS 1.3 only, ALPN `ono/1`. Key exchange and record protection are exactly what
   `ono-protocol`'s `transport` module says a shell must not write twice; this module supplies them
   and answers the one question `ono-protocol` asks a transport.

2. **The peer's end-entity certificate *is* the `HostKey`, algorithm `tls-x509`.** TLS 1.3's
   `CertificateVerify` is a signature made during this handshake, over a transcript of this
   handshake, with the key belonging to the certificate the peer sent. This process verifies it
   with bytes it saw itself, so `Transport::peer_key` reports a fact rather than somebody else's
   claim — which is the whole distinction ADR-0037 §4 refused to blur by reading `known_hosts`.

   Pinning the whole certificate rather than the public key inside it is the strict reading: a
   re-issued certificate is a new key to Ono even when the key is the same. It is what a person can
   check by hand from one file, and re-trusting is meant to be deliberate anyway.

3. **There is no certificate authority and no name check, deliberately.** A PKI answers "does
   somebody I trust vouch for this name?"; §21.5's "explicit host trust" answers "is this the key
   this host had last time?", and that is the trust store's decision, made above the transport, on
   the key the transport established the peer really holds. A name check against a self-signed
   certificate would check nothing, so the certificate carries the fixed name `ono.invalid` — a
   name that cannot resolve, so it cannot be mistaken for a claim about DNS. The custom verifier is
   named `PinIsTheAnchor` and does exactly two things: it performs the handshake signature check
   (through rustls' own `verify_tls13_signature`), and it declines to invent an authority.

4. **A host's identity is a file it owns**, `HostIdentity::open_or_create`, one CERTIFICATE and one
   PRIVATE KEY PEM block, generated with `rcgen` on first use and written `0600`. It survives a
   restart, because an identity that changed on every start would make every peer that pinned the
   host refuse it — which is a denial of service dressed as security.

5. **`ring` rather than the default rustls provider.** The default (`aws-lc-rs`) needs an assembler
   and CMake; an agent has to be installable on the machine it is meant to run on, and the release
   container builds with the toolchain a Rust build already has.

6. **The listening side authenticates nobody at the transport.** A client presents no certificate;
   who it is comes from the link protocol's own identity (§21.2). Client certificates are a
   different decision — least privilege by identity — and this ADR does not pre-empt it.

## Consequences

Easy: `TrustStore`, `TrustPolicy` and E0603 are live on a production path for the first time.
`crates/ono-remote/tests/tls.rs::should_pin_a_host_on_first_contact_and_then_refuse_a_changed_key`
is the refusal ADR-0274 said could not be written yet, over a transport that authenticates. F12 can
now be decided rather than deferred (ADR-0354).

Hard: the workspace has a cryptography dependency where it had none — rustls, ring, rcgen — and a
build that must keep working in the release container. The pin is per certificate, so rotating a
certificate is a re-pin even when the key did not change; that is stated in the module
documentation rather than smoothed over. Nothing about `ssh` changes: ADR-0037 §4 stands, and an
ssh link stays `Unauthenticated` by name, because OpenSSH still will not tell this process the key.

Encoded by: `crates/ono-remote/tests/tls.rs` (5 tests).

## Alternatives considered

- **A Rust ssh client inside Ono** — ADR-0274's option 2, rejected here: a second authentication
  path beside OpenSSH's, with its own `known_hosts` semantics and its own cryptographic surface,
  bought for the same property TLS gives with a stack that already exists.
- **Noise** — a good fit and a smaller surface, rejected because it would need a handshake pattern,
  a key format and a framing decision of Ono's own, where TLS 1.3 brings all three with a
  specification and a decade of review.
- **RFC 7250 raw public keys instead of certificates** — closer to what is meant ("a key, not a
  name") and rejected for now: it needs both ends configured for it, and it buys only a shorter
  pinned blob. It can replace the certificate under the same `HostKey` question later, and the
  algorithm name in the store is what would change.
- **Pinning the SubjectPublicKeyInfo out of the certificate** — rejected: it needs certificate
  parsing this crate otherwise does not do, to make re-issuing a certificate *not* a re-pin, which
  is a convenience bought with a weaker statement about what was checked.
