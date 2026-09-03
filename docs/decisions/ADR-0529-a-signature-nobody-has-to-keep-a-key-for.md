# ADR-0529: A signature nobody has to keep a key for

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §47.1 (required release files), §47.3 (signing model), §47.5 (user
  documentation), §43.2 (first-party scripts), §43.3–§43.4 (permissions and pull-request trust),
  §44.2 (tool versions, signing tools named explicitly), §45.4 (a signing dependency needs a
  recorded review), §2.3 (fail closed), §67.7 (the command a reader types)
- Issues: #107 — builds on #106 (ADR-0528), extended by #108
- Decided by: agent (autonomous)

## Context

§47.3 states a preference and a condition:

> The reference implementation SHOULD use keyless OIDC-backed signing through Sigstore/Cosign or
> an equivalent system that does not require a long-lived private signing key stored as a
> repository secret.

The condition is the hard part in this repository, because ADR-0433 closed the doors a secret
would otherwise have travelled through: `pull_request_target` is banned, and `contents: write`
lives on the `publish` job alone. A long-lived signing key would have to be a repository secret,
and a repository secret is a thing that has to be kept, rotated, revoked and kept away from every
workflow a fork can start — §43.4's whole subject. §45.4 additionally requires a recorded review
before a signing dependency is introduced at all.

## Decision

**Keyless Sigstore signing, through cosign, in the `publish` job, over `SHA256SUMS`.**

There is no private key in this repository and none in its secrets. cosign asks GitHub's OIDC
provider for a short-lived token proving *which workflow of which repository on which ref* is
asking, exchanges it at Fulcio for a certificate valid for ten minutes, signs with it, and records
the signature in Rekor. The signing material never outlives the run. What a reader verifies
against is an identity:

```text
https://github.com/godspeed-you/ono-sendai/.github/workflows/release.yml@refs/tags/v…
issued by  https://token.actions.githubusercontent.com
```

### §45.4's review, recorded

- **What it is.** `cosign`, pinned at `3.1.3` in `[workspace.metadata.release-tools]` and
  installed by `sigstore/cosign-installer` pinned at commit `6f9f1778…` (v4.1.2). §44.2 names
  signing tools explicitly, so the register was taught the installer-action spelling
  (`<tool>-release: "v<version>"`) rather than the tool being left out of it.
- **Where it runs.** The `publish` job only, which is reachable by a tag push and by nothing a
  fork can start. That job now holds `id-token: write` beside the `contents: write` it already
  had, and no other job holds either.
- **What it can do if compromised.** Sign a blob as this repository's release workflow, during a
  run of that workflow. It cannot sign outside one, because the token it needs does not exist
  outside one — which is the property a stored key does not have.
- **Custody, rotation, revocation.** None to define, and that is the point of choosing this model
  over the alternative §47.3 allows. Fulcio's certificate expires in ten minutes; Rekor's entry is
  the durable record. The identity a reader pins is the workflow path, which this repository
  controls through the same review every other file gets.

### Both halves are first-party scripts

`scripts/sign-release.sh` signs and **verifies its own signature before it returns**. A signature
that cannot be checked is worse than none, and it has to fail the release rather than reach a
reader. It refuses to run at all without an OIDC identity in the environment, because a locally
made signature would attest to a developer's machine.

`scripts/verify-release.sh` is the reader's side and the release's own check, in one file: the
digests through `sha256sum --check --strict` (§67.7's command, run as documented), then the
signature through `cosign verify-blob` constrained to the identity above, then the provenance
(ADR-0530). It fails closed — a missing signature is a failure, not a skip — and `--without-
signature` / `--without-provenance` exist for the local release check, which has no OIDC identity
to produce either.

Neither needs a proprietary service and neither needs this repository checked out, which is
§47.5's requirement of the verification instructions.

## Consequences

### The box this leaves open, deliberately

**§4.8.11's `#107` box stays unticked, and `should_verify_the_published_signature_over_the_
checksum_manifest` does not prove what its name promises.** The honest statement:

- keyless signing needs an OIDC token that exists only inside a run of this repository's release
  workflow, and verification needs a route to Fulcio and Rekor. The gate has neither, and
  `scripts/acceptance.sh` runs with networking disabled by design (§40.2);
- what the test therefore owns is the **verification path**: that `verify-release.sh` asks for a
  keyless verification constrained to this repository's release workflow and GitHub's issuer,
  that it never hands cosign a key, that it reports what the tool reports rather than deciding
  for itself, and that a missing bundle is a refusal. A stand-in `cosign` on `PATH` implements the
  one property the real tool implements — the bundle was made over exactly these bytes — which is
  what makes `should_fail_verification_when_the_checksum_manifest_is_altered` a real test of the
  refusal rather than of a string;
- faking the outside world is what AGENTS.md §11 permits; faking our own layer is not, so the box
  is not ticked on the strength of it.

The end-to-end proof — a real Sigstore signature over a real `SHA256SUMS`, verified from a clean
machine with the published instructions — is produced by the first tag push, by the `publish` job
signing and then verifying itself. That is the run that closes the box, and §66.5 asks for green
rather than for existence. H8 left `#94` open for the same reason and that was right.

### What is proven now

- the release workflow signs the checksum manifest keyless, with `id-token: write` on the
  publishing job alone and no repository secret anywhere in the file;
- the verification is identity-constrained, keyless, and fails closed on an absent signature, an
  altered manifest and an artifact whose bytes changed;
- cosign's version is an input the gate enforces and the Appendix H manifest records.

Encoded by `xtask/tests/provenance.rs::should_verify_the_published_signature_over_the_checksum_
manifest` and `::should_fail_verification_when_the_checksum_manifest_is_altered`, and by
`xtask/tests/supply_chain.rs::should_find_an_exact_version_for_every_release_tool`, which now
covers cosign.

## Alternatives considered

**A long-lived key in a repository secret.** §47.3 permits it *if* an ADR defines custody,
rotation, revocation and offline verification. Every one of those four is a standing obligation
somebody has to meet for as long as the project exists, and §43.4 then makes the secret a
permanent hazard to reason about in every workflow change. Keyless has none of the four and its
failure mode — Sigstore being unreachable — fails the release rather than the reader.

**GitHub's `actions/attest-build-provenance`.** The same Sigstore machinery behind a smaller
surface, and it produces an attestation verified with `gh attestation verify`, which is a GitHub
client. §47.5 asks that verification not require a proprietary service. `cosign verify-blob`
against a published bundle is checkable by anyone with cosign and the file.

**Sign the packages individually instead of the manifest.** Four signatures where one suffices,
and a reader would have to check each. The manifest already binds every artifact by digest
(ADR-0528), so one signature over it covers all of them — which is the shape §47.1 asks for by
listing `SHA256SUMS` and `SHA256SUMS.sig` side by side.

**Verify a real signature in the gate by generating a key pair with `cosign generate-key-pair`.**
It would be green and it would prove the wrong thing: the key-based path, which this project does
not use, with a key this project does not have. A test that exercises a code path nobody ships is
a test that can pass while the shipped path is broken.
