# ADR-0434: An identity is named for the peer it proves to, and says only its fingerprint

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §7.1, §7.2, §7.3, §8.4, §56.1; AGENTS.md §7, §11; ADR-0353, ADR-0430
- Decided by: agent (autonomous)

## Context

`ono-remote` had one identity type, `HostIdentity`, and one place it could stand: the listening
side of a TLS link, holding the certificate a client pins and the key the host proves it holds.
v0.4.1 §7.1 removes that asymmetry — "both endpoints MUST present a certificate and prove
possession of the corresponding private key during TLS 1.3 negotiation" — so the same object now
stands on both ends of the same wire, and §7.2 asks that it stop being named for one of them:

> The implementation SHOULD generalize the current host-only identity abstraction into a
> transport-neutral `PeerIdentity` or equivalent concept.

§7.2 also enumerates what the object carries — "algorithm, certificate/public material, private
key, fingerprint, storage location and creation metadata where available" — and then constrains
what may leave it: "the public contract is the fingerprint. The private key MUST never be
serialized into ordinary structured pipeline output, logs, diagnostics or crash messages."

Issue #32's title reads *generalize `HostKey` into a transport-neutral `PeerIdentity`*, which is a
looser paraphrase of the same paragraph than the paragraph is. Two different types are in play and
only one of them is host-named by mistake, so the scope of the rename had to be decided rather
than assumed.

## Decision

**`HostIdentity` becomes `PeerIdentity`, and moves out of `tls` into its own module.** It is the
private object one end holds. It gains the two members §7.2 lists that the old type could not
answer — `path()`, the storage location, and `created()` — plus `algorithm()` and `certificate()`
for the public material, and `host_key()` becomes `peer_key()` because that is who it is a key
for. `tls::TlsListener::bind` takes it by the same reference it always did, and reaches the
certificate and key through one crate-private `material()` accessor instead of two private
fields, so the module that speaks TLS is the only one that ever sees the key again.

The module is `identity.rs` rather than a section of `tls.rs` because the object is now
transport-neutral by requirement, and a type whose file is named after one transport is a type
that will grow a second transport's special case inside that file.

**`ono_protocol::HostKey` keeps its name.** It is not the host-only abstraction §7.2 is about: it
is the key *a peer proved it holds*, its documentation has said exactly that since ADR-0353 ("a
peer's public key, as the transport authenticated it"), and `Transport::peer_key` — the one method
that produces one — is already named for the peer. Renaming it would rewrite the signatures of
`TrustStore::verify`, `pin`, `repin` and `decide` and edit the thirteen trust tests in
`ono-protocol/tests/trust.rs` and `ono-remote/tests/trust.rs` without changing one observable
behaviour, which AGENTS.md §11 says is the shape of a change that should not need to touch tests
at all. The word `host` in `HostKey` and in the trust store's file format also carries meaning
that is still true and still needed: the store records a key *per host name*, because that is what
a person pins and what E0603 compares. §7.2 asks for a peer identity, and this repository now has
one; it does not ask for the trust store's vocabulary to change, and H2 will add the client-side
authorization store beside it under its own name.

**`Debug` is written by hand, and the fingerprint is what it prints.** `PeerIdentity` renders as
its algorithm, its fingerprint, its storage location and the literal `private_key: "<withheld>"`.
Deriving `Debug` on a type with a private key in it is a promise about every field a later
maintainer adds; today `rustls-pki-types` happens to elide the secret in its own `Debug`, which
means the derived rendering was *accidentally* safe and the safety belonged to a dependency rather
than to this crate. §7.2 puts the requirement here, so the implementation is here.

## Consequences

Easy: the client side of §7.1 (issue #36) needs no second identity type, and the client identity
file of §8.1 (issue #33) is the same object read from a different path. `TlsListener::bind`'s
caller cannot reach the private key at all any more, because the fields are gone from its module.

Hard: `HostIdentity` and `host_key()` were public API of `ono-remote`. Both are renamed rather
than aliased, because an alias would leave the host-only name in the crate's documentation as a
supported spelling and §7.4's neighbouring rule — that internal names must not misdescribe what
they hold — argues the other way. The change is source-breaking for `ono-cli`, which is in this
workspace and updated in the same increment, and for nothing else.

The private-key rendering test passes the moment it compiles, which is unusual for a RED phase and
deliberate. Issue #32 names it as the exit test, and what it guards is the hand-written `Debug`
this ADR introduces: the RED that justified the increment was the two description tests, which
failed because a derived `Debug` printed the DER of the certificate and neither the fingerprint
nor the file, and the type had no `path`, `created` or `algorithm` to print.

Encoded by: `crates/ono-remote/tests/peer_identity.rs::should_never_render_private_key_material_in_any_rendering_or_diagnostic`,
`::should_describe_itself_by_fingerprint_algorithm_and_storage_location`,
`::should_carry_the_public_material_and_the_storage_location_of_the_file_it_came_from`.

## Alternatives considered

**Keep `HostIdentity` and add `PeerIdentity` as an alias.** No source breakage at all. Rejected:
two names for one type is exactly the ambiguity §65.1 lists as a failure mode, and the whole point
of §7.2 is that the host-only name stopped being true.

**Rename `HostKey` to `PeerKey` with a compatibility alias.** Closer to issue #32's title.
Rejected on the reasoning above: the type is already peer-named in its method and its
documentation, the rename buys no behaviour, and a `pub type HostKey = PeerKey` alias would be the
same two-names-one-type problem one crate down.

**Leave the identity in `tls.rs`.** One fewer module. Rejected: §7.2 asks for transport-neutral,
and the private key would stay reachable as a field from every function in the TLS module rather
than through one accessor.
