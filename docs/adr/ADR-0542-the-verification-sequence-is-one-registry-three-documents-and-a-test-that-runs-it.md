# ADR-0542: The verification sequence is one registry, three documents and a test that runs it

- Status: superseded by ADR-0545 (in part: the `cosign` invocations, the published file names and the claim that no release path existed to describe)
- Date: 2026-09-03
- Spec refs: v0.4.1 §47.1, §47.2, §47.3, §47.4, §47.5, §67.7, §20
- Decided by: agent (autonomous)

## Context

§47.5: *"The Wiki/install documentation MUST show how to verify checksums and signatures before
package installation. Verification instructions SHOULD fit in a short copyable sequence and MUST
not require a proprietary service."* §67.7 sketches two commands and says "the project
documentation provides the exact supported command for the chosen signature mechanism".

Nothing published them. `.github/workflows/release.yml` today builds packages, validates them and
attaches them to a release; it writes no `SHA256SUMS`, no signature and no provenance. Those are
issues #106, #107 and #108, and they belong to the phase that owns the release path — which means
this documentation had to be written against §47's text rather than against a working workflow.

Three copies of the sequence exist by construction: the repository's own reference, the README,
and the Wiki's Install page. Written independently they would be three sequences within a year.

## Decision

**One registry.** `docs/contracts/hardening/release_verification.yaml` holds §47.1's file list and the
five steps, each with its command, what it proves, what to do when it fails, the programs it
needs, and whether this repository's tests run it. `docs/reference/release-verification.md` is
rendered from it; the README and the Wiki's Install page carry the same commands and are compared
against it by the gate. A hand-written copy that drifted is worse than none — a reader would run
the wrong command and believe the right thing.

**The order is a checked property.** The signature over the manifest is verified first and the
artifacts are checked against the manifest second, and the gate reports the reverse. Done the
other way the sequence proves only that the download was not corrupted in transit: a manifest an
attacker wrote agrees perfectly with the artifacts that attacker also wrote. This is the one part
of the sequence where getting the order wrong looks exactly like getting it right.

**The steps are executed, not printed.** `xtask/tests/release_verification.rs` builds a release
directory with `sha256sum`, runs the executable steps against it, then alters one byte of an
artifact and asserts the sequence refuses it — §20's shape applied to the release. A third test
removes one artifact and asserts the sequence still answers about the other, because a reader
downloads one package and not eight, and that is what `--ignore-missing` is for.

**Two steps are marked as not executed, with the reason in the registry.** `cosign` is not a build
dependency of this repository and verifying a real signature reaches Sigstore's transparency log
over the network, which the acceptance container does not have (§40.2). The registry says so per
step rather than leaving a reader to notice; a step nothing runs is a step that works until
somebody needs it, and saying which ones those are is the least this can do.

**What is specified and what is guessed.** §47.1 fixes the three file names, §47.3 states the
preference for keyless Sigstore/Cosign over a long-lived key in a repository secret, §47.4 fixes
the seven fields provenance binds, and §67.7 sketches the interaction. **Everything else in the
`cosign` invocations is this ADR's decision**, and it is recorded here so the phase that builds
the signing can contradict it deliberately:

- `SHA256SUMS.pem` is published beside `SHA256SUMS.sig`. §47.1 lists two files; keyless signing
  produces a short-lived certificate, and without it a reader has neither a public key nor a
  certificate and cannot verify anything. This is an addition to §47.1's list, not a departure
  from it.
- `--certificate-identity-regexp` is anchored to
  `https://github.com/godspeed-you/ono-sendai/.github/workflows/release.yml@refs/tags/v`. A
  signature that verifies without an identity check is a valid signature by anybody; the regexp is
  the whole of the check.
- `--certificate-oidc-issuer https://token.actions.githubusercontent.com`, because the workflow
  runs on GitHub Actions.
- The provenance is verified with `cosign verify-blob-attestation --type slsaprovenance --bundle
  build-provenance.intoto.jsonl`. §47.1 offers `build-provenance.json` or `.intoto.jsonl` "or
  equivalent"; the `.intoto.jsonl` bundle is what cosign writes and reads without a second tool.

If #107 lands a different mechanism, this registry is what changes, and all three documents move
with it in the same commit.

## Consequences

- A reader has a sequence, and the sequence has been run. The signature and provenance steps have
  not: they need #107's signing and #108's provenance to exist before anything can execute them
  against a real artifact set. `docs/ACCEPTANCE.md` §4.8.12's box stays open and says exactly
  that, rather than being ticked on a documented intention.
- §4.8.12 named the tests as living in `xtask/tests/provenance.rs` and being carried by case
  `199`. Both belong to the release phase; these tests are in `xtask/tests/release_verification.rs`
  and the box now names them there.
- `PROPRIETARY` is a short concrete list rather than a judgement about what the word means. Six
  named verification services, each of which would make the ability to check a download depend on
  an account with a company. Sigstore is deliberately not on it: it is an OpenSSF project with a
  public transparency log, and §47.3 names it.
- The sequence has a length budget — twenty-four lines, excluding the install step — because
  §47.5's "short copyable sequence" is a requirement and a reader who has to scroll is a reader
  who skips it.

## Alternatives considered

- **Wait for #107 and document what it built.** Rejected: §66.8 makes documented release
  verification a release criterion, and the phase that builds the signing needs a specification of
  what a reader will be told, not the other way round.
- **Use `gh attestation verify`.** Rejected: it needs the `gh` CLI and a GitHub login, which makes
  verifying a download depend on an account with the platform the download came from — the shape
  §47.5's "no proprietary service" is written against.
- **Publish a long-lived public signing key.** Rejected by §47.3, which prefers keyless OIDC
  precisely so that no private key sits in a repository secret; and an ADR defining custody,
  rotation and revocation would be the alternative it demands.
- **Let each document write the sequence in its own voice.** Rejected: three copies is the
  condition, not the choice, and the only question was whether they are compared.
