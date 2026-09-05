# ADR-0439: `ono/2` is a different protocol, and there is no way back to `ono/1`

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.1, §2.9, §4.2, §4.3, §13.2, §13.3, §13.4, §65.1; spec §43;
  ADR-0006, ADR-0353, ADR-0437
- Decided by: agent (autonomous)

## Context

§13.4 asks the direct transport to "advance from the existing `ono/1` ALPN token to a token that
unambiguously represents the mutual-authentication contract, for example `ono/2`", and offers one
alternative: keep `ono/1` and bump the protocol version instead, in which case "an ADR MUST
demonstrate that downgrade cannot occur before client authentication".

Around it, §13.3 forbids the client from ever retrying without a certificate, §13.2 forbids a peer
from requesting a legacy unauthenticated mode after mutual TLS is up, and §4.2 governs what a
person hitting the break is told:

> The preferred implementation is to bump the direct-link trust/protocol capability such that a
> missing mutual-authentication capability produces `remote.protocol_mismatch` or a new stable
> authentication-specific error before any provider operation.

This is a compatibility break by design, so the decision is not *whether* to break but where the
break lands and what it says.

## Decision

**The ALPN token becomes `ono/2`, and `ono/1` is neither offered nor accepted.** The alternative
§13.4 permits — keep `ono/1`, bump the Ono protocol version — was rejected rather than argued for,
because the demonstration it would require cannot honestly be made. The Ono protocol version is
negotiated in the `Hello`/`Accept` exchange, which happens *inside* the TLS session; §13.1
requires mutual TLS to complete before a `Hello` is accepted, so at the moment the version is
negotiated the authentication question is already settled and the version number can say nothing
about it. A token that named a one-sided link would then still be on the wire naming a two-sided
one. ALPN is the only field in this handshake that is agreed before any certificate is exchanged,
which makes it the only field where the contract can be named in time.

**A v0.4.1 client refuses a server that does not ask it for a certificate, and can tell.**
`rustls::client::ResolvesClientCert::resolve` is called exactly when the server sends a
`CertificateRequest`, so the client's certificate resolver records whether it was ever asked, and
`connect` refuses when it was not. Without that flag a client could complete an `ono/2` handshake
against something that named `ono/2` and requested nothing, send a `Hello`, and be answered by a
peer that never learned who it was — §2.1's word "authenticated" applied to a link where only one
direction of the proof happened.

**There is no fallback path, because there is no way to express one.** `connect(address,
identity)` takes the identity by reference and non-optionally; there is no builder default, no
`Option`, and no sibling function that omits it. §13.3's "MUST NOT retry" is therefore not a rule
the code follows, it is a state the code cannot reach, and the test that proves it counts TCP
connections rather than inspecting a code path: one attempt, one connection.

**No legacy diagnostic mode exists, which is the strongest reading of §13.3's third sentence.**
That sentence conditions on "if a legacy diagnostic mode exists" and constrains one that does —
an explicit command path or a flag containing `legacy` or `unauthenticated`, a high-visibility
warning, never selected by `link`. Nothing in v0.4.1 needs one, so nothing was built, and the
condition is unmet rather than satisfied. Issue #39 keeps it that way from the CLI side.

**The refusal is `Ono-Sendai-E0605` / `remote.peer_unauthenticated`, kind `safety`, never
retryable.** §4.2 offers `remote.protocol_mismatch` or a new authentication-specific code, and
this is the second. E0602 would have been cheaper and is the wrong sentence: it is kind `provider`
and its help says "upgrade the agent, or connect in agentless mode where the local side does the
work", which is advice to *route around* the refusal. A person who hits E0605 is upgrading a fleet
and needs to know which end is old and that the link they thought was authenticated was not; the
help says so, points at `--transport ssh` — where §4.3 keeps a different and honestly described
trust source — and offers nothing that would drop the certificate.

Both ways of meeting an old server land on it: `no_application_protocol` from a server that only
speaks `ono/1`, and a completed handshake with no certificate request. Neither is a
`remote.unreachable` (E0601): the port answered, and calling that unreachable would send a person
to check their firewall.

## Consequences

Easy: the break is loud, single-code and non-interactive, which is what §2.9 and §4.2 both ask
for. A mixed fleet fails at the link rather than silently linking without authentication, and the
message names the reason.

Hard: **v0.4.0 and v0.4.1 cannot make a direct TCP link in either direction.** A v0.4.0 client
offering `ono/1` is refused by a v0.4.1 listener (no shared protocol), and a v0.4.1 client is
refused by a v0.4.0 listener (E0605). §4.2 requires the first and permits the second only through
"a legacy compatibility mode outside the normal `link` path", which this release does not have.
The migration is: upgrade the listening agent first, then the clients. Nothing is lost in the
meantime — `--transport ssh` and `--transport local` are untouched, because they never went
through this module.

Also hard: the `asked` flag is an observation of rustls's internal call order, not of the wire. It
is correct for every current rustls version and it is the only signal the client API offers, but
it is a coupling worth knowing about: if `resolve` ever started being called speculatively, the
check would silently stop meaning anything.
`should_refuse_a_server_that_does_not_ask_for_a_client_certificate` is what would notice, because
it stands up a real server that requests nothing.

Encoded by: `crates/ono-remote/tests/downgrade_resistance.rs::should_speak_the_mutual_authentication_token_and_no_older_one`,
`::should_refuse_a_server_that_does_not_ask_for_a_client_certificate`,
`::should_not_try_again_after_a_server_refuses_mutual_authentication`,
`::should_refuse_a_client_that_asks_for_the_older_protocol_token`,
`::should_know_the_key_of_a_server_it_did_agree_to_speak_to`.

## Alternatives considered

**Keep `ono/1` and bump the Ono protocol version.** §13.4's stated alternative. Rejected on the
argument above: the version is negotiated inside the TLS session, after the authentication
question has already been answered, so it cannot carry a claim about that answer to a peer that
has not yet decided whether to prove anything.

**Offer both `ono/2` and `ono/1`, preferring `ono/2`.** Interoperable, and exactly the shape of
downgrade §13.3 exists to prevent: an attacker who can influence the ClientHello or answer the
port gets to pick the weaker one, which is the definition of a downgrade attack.

**Map the refusal to `remote.protocol_mismatch` (E0602).** Permitted by §4.2 and needs no new
code. Rejected: its kind is `provider`, its help recommends agentless mode as a way around it, and
conflating "the agent speaks an older wire format" with "this link is not authenticated" makes the
one refusal that matters unbranchable in a script.

**Warn and continue when the server asks for no certificate.** Rejected by §2.9 and §13.3
together; a warning on an unauthenticated link is a link nobody notices is unauthenticated.
