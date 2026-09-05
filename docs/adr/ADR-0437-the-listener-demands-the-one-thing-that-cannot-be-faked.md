# ADR-0437: The listener demands the one thing that cannot be faked, and nothing else

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §0.5.1, §2.1, §2.2, §7.1, §7.4, §9.1, §13.1, §56.1, §58.1 (H1-WP2), §59.1;
  spec §21.5; ADR-0274, ADR-0353, ADR-0430, ADR-0434
- Decided by: agent (autonomous)

## Context

This is work package H1-WP2, and the defect it closes is the one v0.4.1 was written around
(§0.5.1): `TlsListener::bind` built its `rustls::ServerConfig` `with_no_client_auth()`, and
`TlsListener::accept` returned a transport whose `peer_key` was `None` with a comment saying so —
"the listening side authenticates nobody". Whoever dialled the port completed the handshake and
read the provider inventory, and the only thing they said about themselves was the protocol
`Identity`, a string they chose.

The failure proof for it already existed and was `#[ignore]`d: ADR-0430's
`should_refuse_a_tls_client_that_presents_no_certificate`, which builds a rustls client
`with_no_client_auth()` and reads the inventory back. Its red output at HEAD was
`Ok(["fixture.demo", "fixture.absent"])`.

Two questions had to be answered to make it green: **what the server verifies about a client
certificate**, and **whether a client can still connect without one**.

## Decision

**The listener requires a client certificate, and what it verifies is the possession proof and
the encoding — nothing else.** `ProofIsTheWholeCheck` implements
`rustls::server::danger::ClientCertVerifier` with `client_auth_mandatory() = true`, an empty
`root_hint_subjects()`, a `verify_client_cert` that parses the end-entity certificate and
otherwise asserts, and TLS 1.2/1.3 signature verification delegated to the crypto provider.

This is the exact mirror of `PinIsTheAnchor`, the server-certificate verifier ADR-0353 wrote for
the client side, and the mirror is the point. §7.1 says the certificate "MAY remain self-signed"
and that "Ono's trust model is explicit key/fingerprint trust, not a public certificate-authority
hierarchy", so there is no path to build and no name to check. What is left is the one fact a
handshake can establish on its own: a `CertificateVerify` signature over this handshake's
transcript, made with the private key belonging to the certificate the peer just sent. rustls
performs it; the verifier's job is to not add a check that would check nothing.

`verify_client_cert` parses the certificate rather than waving it through, because rustls hands
those bytes over unparsed and asks the implementer to handle invalid data. Bytes that are not a
certificate carry no public key, so no proof of possession can exist for them, and refusing at
that point makes the diagnostic name the encoding rather than the signature.

**The empty `root_hint_subjects()` is deliberate and is not an oversight.** RFC 8446 reads an
empty `certificate_authorities` list as "send whatever certificate you have", which is exactly
right when there are no authorities. A non-empty hint list would be a claim about a hierarchy
that does not exist.

**Authentication stops at the transport; authorization is not here.** The verifier accepts any
peer that proves possession, and the resulting key goes to `Transport::peer_key` for the trust
store above to decide about. §9.1 is explicit that a valid certificate "proves only that the
connecting process holds a private key", §59.1 requires an *unknown* client to be refused before
provider negotiation, and §56.1 gives `ono-remote` "no authorization policy semantics beyond
transporting authenticated identity". So the `authorized_clients` store is phase H2's, and
nothing in this increment pretends to be it: a listening agent today authenticates every client
and authorizes all of them, which is strictly more than it did yesterday and strictly less than
§59.1 needs. That gap is issue #40's, not a silent omission.

**`connect` takes a `&PeerIdentity` and there is no sibling that omits it.** Presenting a
certificate is not a mode of the client; it is what the client is. §13.3 forbids retrying without
one, and the cheapest way to keep a rule like that is to leave no expressible way to break it —
there is no `Option`, no builder default and no second function. ADR-0439 carries the rest of the
downgrade argument.

## Consequences

Easy: `TlsTransport::peer_key()` is `Some` on both ends of every accepted direct link, so
`TrustStore::decide` runs on the listening side for the first time and phase H2 has an identity
to hang a policy on. The four negative cases §58.1 names are all covered by one code path.

Hard: this is the compatibility break of §4.2. A v0.4.0 client cannot connect to a v0.4.1
listener — it presents no certificate and rustls refuses it with a `CertificateRequired` alert
before any Ono frame. §4.2 requires exactly that ("MUST fail safely rather than silently
downgrade authentication"), and ADR-0439 carries how a person who hits it is told why. SSH-carried
stdio agents are untouched: they never went through this module, `SubprocessTransport::peer_key`
is still truthfully `None`, and §4.3 keeps that model deliberately.

Also hard: writing the negative tests honestly took more machinery than the assertions. Building
the malformed and mismatched-key clients through `ClientConfig::with_client_auth_cert` made both
tests pass at HEAD, because rustls validates the pair *in the client* — `BadEncoding` and
`KeyMismatch` respectively — so the suite would have proved that rustls is careful rather than
that the Ono listener demands anything. Both now go through a `ResolvesClientCert` that presents
whatever the test hands it, which puts the adversary on the wire where the server has to answer
it. Their red output at HEAD was the same `Ok(["fixture.demo", "fixture.absent"])` as the H0
proof's. This is the same rule ADR-0430 states from the other side: a proof has to arrange its
failure where the requirement lives.

The wrong-ALPN case was already green before the fix, because ALPN enforcement predates it. It is
kept because §58.1 names it, and because ADR-0439 moves the token it enforces.

Encoded by: `crates/ono-remote/tests/client_authentication.rs::should_refuse_a_tls_client_that_presents_no_certificate`
(ADR-0430's proof, un-ignored, assertion unchanged),
`::should_refuse_a_tls_client_whose_certificate_is_malformed`,
`::should_refuse_a_tls_client_that_cannot_prove_it_holds_the_key_it_presents`,
`::should_refuse_a_tls_client_that_asks_for_another_application_protocol`,
`::should_report_the_key_the_accepted_client_proved_it_holds`.

## Alternatives considered

**`WebPkiClientVerifier` with the agent's own certificate as the root.** rustls ships it, and it
would need no `danger` module. Rejected: it builds a path to a trust anchor, which means the
client's certificate would have to be issued by the agent — a certificate authority, in a design
§7.1 says is not one, and an enrolment step §8 does not have.

**`client_auth_mandatory() = false`, with the peer key reported when present.** Would keep v0.4.0
clients working and let policy refuse them a layer up. Rejected: §13.1 requires mutual TLS to
complete *before* an Ono `Hello` is accepted, and §7.4 forbids the canonical agent from having a
mode where client authentication is off. Optional authentication is that mode with extra steps.

**Verify the certificate's validity dates.** Free, and it feels like diligence. Rejected: the
certificate is a key wrapper, not an assertion by anyone, and `rcgen::generate_simple_self_signed`
picks the dates. Expiring a pinned identity on a schedule nobody chose would break links for a
reason §8.6 explicitly rules out ("Ono MUST NOT auto-rotate this key based solely on age").
