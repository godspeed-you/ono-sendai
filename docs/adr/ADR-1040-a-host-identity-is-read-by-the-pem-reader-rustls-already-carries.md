# ADR-1040: A host identity is read by the PEM reader rustls already carries

- Status: accepted
- Date: 2026-11-24
- Spec refs: spec §45.1, §45.4; v0.4.1 §7.2, §8.1, §8.3; ADR-0353, ADR-0449
- Issues: #172
- Decided by: agent (autonomous)

## Context

`ono-remote` read the two PEM blocks of a peer identity file — `link_identity.pem`, or the file
`--agent --host-key` names — with `rustls-pemfile`. That crate was archived in August 2025
(RUSTSEC-2025-0134, unmaintained), its code having moved into `rustls-pki-types` as
`rustls_pki_types::pem::PemObject`. `deny.toml` carried a waiver for the advisory that expires on
2027-03-01, and `xtask` turns the gate red when a waiver's date passes, so the migration had a
deadline whether or not anyone scheduled it.

The file is a host's own identity: a peer pinned its certificate, and the key in it is what this
host proves it holds. Replacing the reader must not change which certificate and which key are
taken from a file, nor turn a refusal into an acceptance.

## Decision

**The identity file is read with `rustls::pki_types::pem::PemObject`, reached through the
`rustls::pki_types` re-export, and `rustls-pemfile` leaves the workspace with its waiver.** No new
dependency arrives: `rustls-pki-types` is already in the graph under `rustls` and `rustls-webpki`,
already in `[workspace.metadata.supply-chain.cryptographic]`, and its entry now names this second
role. The waiver is deleted, not extended.

The reading is the one `rustls-pemfile` did, because `PemObject` is that code:

- the certificate is the **first** `CERTIFICATE` section in the file, and the key the **first**
  private key of the three forms a TLS stack takes (PKCS#8 `PRIVATE KEY`, PKCS#1
  `RSA PRIVATE KEY`, SEC1 `EC PRIVATE KEY`), wherever each stands; every other section is skipped;
- a section that does not decode (no end marker, bad base64) refuses the file as `parse.syntax`
  naming it; so does a file with no certificate or no usable key (an `ENCRYPTED PRIVATE KEY` is not
  one), and such a file is never replaced by a generated identity;
- a file holding several keys presents the first. If that key does not belong to the certificate,
  presenting the identity fails — the certificate is never paired with a later key instead.

What the refusal messages say after the colon is the PEM reader's own wording and is not a
contract; the code, the named file and the `no CERTIFICATE block` / `no PRIVATE KEY block` phrases
are.

## Consequences

The workspace no longer depends on an unmaintained crate, and the advisory waiver list is empty.
Nothing changes for an operator: the same files load, the same files are refused, the same
fingerprint is printed.

The behaviour is pinned at the boundary, written and green against the old reader before the swap
and unchanged by it: `crates/ono-remote/tests/peer_identity.rs::should_read_the_same_identity_whichever_order_its_two_blocks_are_in`,
`::should_refuse_an_identity_file_without_a_certificate_as_a_syntax_error`,
`::should_refuse_an_identity_file_whose_key_is_not_a_private_key_it_can_use`,
`::should_refuse_a_malformed_pem_block_as_a_syntax_error`,
`::should_refuse_an_identity_path_where_nothing_can_be_read_or_created`,
`::should_present_the_first_private_key_when_a_file_holds_several`, beside the existing permission
and key-material cases of the same suite.

## Alternatives considered

**Extend the waiver.** The advisory is "unmaintained", not a vulnerability, so another year would
have been defensible on paper. It is also how a waiver becomes permanent, which is what ADR-0449's
expiry dates exist to stop, and the replacement is the same code under a maintained name.

**Depend on `rustls-pki-types` directly.** A `[dependencies]` line would make the use visible in
`ono-remote/Cargo.toml`. It would also be a second route to a crate `rustls` re-exports at the
version it was built against; reading through `rustls::pki_types` keeps the identity's types and
the TLS configuration's types the same types by construction.

**A hand-written PEM parser.** Two blocks of base64 is little code, and it is exactly the kind of
code §45.4 asks a project not to write twice.
