# ADR-0609: A package may be signed without a key anybody keeps

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.2 §31.36, §45.2; v0.4.1 §17.2, §47.1, §47.3;
  `docs/specs/kuang11/kuang11-plugin-installation-permissions-spec.md` (K11P) §11.3, §13.3, §18.1;
  ADR-0311, ADR-0312, ADR-0529, ADR-0606, ADR-0607
- Decided by: agent (autonomous, on the user's choice between two recorded options)

## Context

Ono signs its own releases without a key: `release.yml` holds no secret, and the whole of the
signing material is `id-token: write`, a token the run can prove (ADR-0529). A verifier checks
*which workflow on which tag* signed, not which key.

KUANG/11 packages were specified the other way. `kuang-sign` is ed25519 with a private key file
(ADR-0311), and `kuang-trust/1` maps a publisher to a public key (ADR-0312). That is a good
mechanism, verifiable offline at install time, and it has one consequence nobody wanted: to publish
a signed package, somebody must keep a private key. The Kubernetes provider reached the point of
shipping `.deb` and `.rpm` wrappers (ADR-0071) and stopped there, because the project has no such
key and does not want one: whoever can read it signs as that publisher for as long as it exists.

So the layer that installs plugins asks for exactly the custody the layer that ships the shell
abolished. This ADR closes that gap in the direction core already went.

## Decision

### 1. A package may carry a keyless signature over the same bytes

`SignedPackage::canonical_bytes()` is a line-oriented description of a package — its id, version,
publisher and the digest of every artifact file — and it is what an ed25519 signature covers today.
A keyless signature covers **exactly those bytes**, so the two forms answer one question about one
thing, and a reader comparing them is comparing like with like.

It travels as `signature.sigstore.json` beside the manifest: a Sigstore bundle, which is what
`cosign sign-blob --bundle` writes. `artifact_files` excludes it, as it already excludes
`signature.yaml`, so a signature never covers itself. A package may carry either form or both; where
both are present both must verify, because a package that is honest under one signature and not the
other is not honest.

### 2. What is verified, offline, in this order

1. The description is recomputed from the package on disk, and its SHA-256 must equal the bundle's
   `messageDigest`. A bundle that vouches for other bytes is refused before anything cryptographic
   is done with it.
2. The leaf certificate must chain to an embedded Fulcio certificate authority, for the
   code-signing usage, through `rustls-webpki` — path building and constraints belong to a library
   that does them for a living.
3. The leaf's public key must verify the bundle's signature over the description bytes.
4. The transparency-log entry's signed timestamp must verify under an embedded Rekor log key, and
   its `integratedTime` must fall **inside the certificate's validity window**. A Fulcio
   certificate lives about ten minutes, so this is the step that pins a signature to the moment a
   workflow ran rather than to whenever somebody presents it. Without it an ephemeral key that
   leaked from a run would be usable for ever.
5. The certificate's subject identity and OIDC issuer must match an entry an operator enrolled.

Steps 1 to 4 answer *is this signature valid*. Step 5 answers *do I trust that signer*. K11P §18.1
keeps those apart and so does this: a bundle that passes 1 to 4 under an identity nobody enrolled is
`signature: valid`, `trust: unknown`, exactly as an ed25519 signature from an unenrolled key is.

### 3. The trust store enrols an identity beside a key

`kuang-trust/1` gains `identities:`, each entry a publisher, an OIDC issuer, an identity (the
workflow URI, with a trailing `*` permitted so a repository's releases are enrolled once rather
than per tag) and a standing, in the same vocabulary `keys:` uses. `revoked` blocks there too.

### 4. The trust root travels with Ono, and is data

The Fulcio certificate authorities and the Rekor log keys come from Sigstore's own published
trusted root, embedded as `docs/contracts/kuang/sigstore-trust-root.json` and compiled in. No TUF
client and no network at verification time: an install must work on a machine with no route out,
which is the whole point of K11A §20. It is updated the way `webpki-roots` is, by taking a newer
published root, and the contract records the digest of what was taken and where from.

Both key types Sigstore's log has used are verified: ECDSA P-256 for `rekor.sigstore.dev` and
Ed25519 for the log that replaced it, so a bundle made before or after that rotation verifies.

### 5. What is deliberately not checked

- **The certificate transparency SCT.** It would need the CT log keys and a second proof to bind
  the same certificate a second way. The Fulcio chain establishes who was issued the certificate
  and the Rekor entry establishes when it was used; the SCT would add that Fulcio published the
  issuance. Named here so the boundary is visible rather than assumed.
- **The Merkle inclusion proof against a signed checkpoint.** The signed entry timestamp is what
  cosign's own offline verification rests on, and it is signed by the same log key the proof's
  checkpoint would be. Verifying the proof as well is a later increment, not a different answer.

Both omissions fail *closed* in the sense that matters: nothing is accepted that these checks would
have rejected on their own, because a bundle that reaches step 5 has already been bound to an
identity and a moment.

## Consequences

- **The verifier lives in `ono-cli`, not in `ono-kuang-protocol`.** A package verifies nothing; a
  host verifies packages. Putting it in the protocol crate was the first attempt and the
  acceptance image caught it: `ono-kuang-sdk` builds the example plugin for `wasm32-wasip2`, and
  `ring` compiles its assembly through `clang`, which that target's build needs and the builder
  image does not carry. The protocol keeps only `BUNDLE_FILE`, the name of the file — what a
  package may contain is the protocol's business, and checking what it contains is the host's.
- `ono-cli` gains `ring` and `rustls-webpki`, both already resolved in the graph and both already
  allowed by `deny.toml`, `rustls-pki-types` for their argument types, and `x509-parser` for the
  two certificate fields webpki does not expose. Fifteen new crates, all pure Rust.
- A publisher can release a signed package from a workflow with no secret, and the Kubernetes
  provider does exactly that (ADR-0071 there). Trusting it stays the operator's action, as it is
  for a key: nothing here ships a trust store, so a fresh machine answers `signature: valid`,
  `trust: unknown` until somebody enrols the identity. The provider's README gives that block
  verbatim.
- `kuang-sign` keeps working unchanged. Nothing about an existing signed package changes, and no
  installed package needs re-signing.
- Encoded by the tests of `ono-cli::kuang_keyless` against a real bundle: the one this project's
  own `v0.4.3` release published over its `SHA256SUMS`, which is a Fulcio-issued certificate, a
  real log entry and a real signature rather than a fixture somebody wrote to pass. The package
  path is `ono-cli/tests/plugins_signature.rs`, where a bundle covering other bytes is `invalid`
  and the package `untrusted`.

## Alternatives considered

- **The `sigstore-verify` crate.** Forty-four new crates even with every feature off, among them an
  async HTTP stack and `aws-lc-sys`, a C library needing cmake, for a verification that must run
  offline in a workspace whose release is built twice and compared byte for byte (ADR-0533). The
  cost is not the crate count; it is a C toolchain in the reproducibility path.
- **Keeping a key in a repository secret.** The fast answer, and it puts back the custody core
  removed from its own releases. A secret that can sign as the publisher for ever is a worse
  standing risk than the work in this ADR.
- **Publishing the wrappers unsigned.** ADR-0071 refuses it: a distribution package that installs
  under local-development semantics is not what a distribution package is for.
- **Verifying by shelling out to `cosign`.** A security decision that depends on an optional
  external tool being present, and on this build agreeing with whatever version is installed.
