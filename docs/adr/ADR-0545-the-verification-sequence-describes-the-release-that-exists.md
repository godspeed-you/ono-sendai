# ADR-0545: The verification sequence describes the release that exists

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §47.1, §47.3, §47.4, §47.5, §66.5, §67.7; supersedes part of ADR-0542;
  builds on ADR-0528, ADR-0529, ADR-0530
- Decided by: agent (autonomous)

## Context

ADR-0542 wrote the verification sequence against §47's text, because the release path did not
exist yet and §66.8 makes documented release verification a release criterion. It said so
explicitly, listing four things it had decided rather than read: `SHA256SUMS.pem` published beside
a detached `SHA256SUMS.sig`, an `--certificate`/`--signature` pair of cosign flags, a
`build-provenance.intoto.jsonl` bundle, and `cosign verify-blob-attestation --type slsaprovenance`
for the provenance.

H11 then landed. Every one of the four is different in the release that now exists:

- the signature is a **keyless Sigstore bundle**, `SHA256SUMS.sigstore.json`, carrying the
  signature, the short-lived certificate and the transparency-log entry in one file — so there is
  no detached `.sig`, no `.pem`, and no public key for a reader to fetch (ADR-0529);
- the provenance is `build-provenance.json`, signed the same way into
  `build-provenance.json.sigstore.json`, rather than an in-toto attestation (ADR-0529, ADR-0530);
- `build-inputs.json` is a published asset and is listed in the manifest like any other file
  (ADR-0528), which the sequence had not mentioned at all;
- `scripts/verify-release.sh` already exists and is the same script the release runs on itself,
  which is a better thing to point a reader with the repository at than four commands.

Documentation that describes a release nobody built is the failure §51 is about, and it does not
stop being that because the release later arrives shaped differently.

## Decision

**`docs/contracts/hardening/release_verification.yaml` now describes what `scripts/verify-release.sh`
and `scripts/sign-release.sh` actually do**, and the README, the Wiki's Install page and
`docs/reference/release-verification.md` move with it, as they already did.

The reader's sequence is four commands and needs no repository checkout:

1. download the artifact, `SHA256SUMS`, `SHA256SUMS.sigstore.json`, `build-provenance.json` and
   `build-provenance.json.sigstore.json`;
2. `cosign verify-blob --bundle SHA256SUMS.sigstore.json` with the identity regexp and the issuer
   `scripts/verify-release.sh` uses, byte for byte;
3. `sha256sum --check --strict --ignore-missing SHA256SUMS`;
4. the same `cosign verify-blob` over the provenance, and a `grep` for the artifact's own digest
   in it.

`--ignore-missing` is the one deliberate difference from the release's own check.
`verify-release.sh` runs `sha256sum --check --strict` over a complete `dist/`, where an absent
file is a failure; a reader has downloaded one package out of several, where an absent file is
normal. Both are `--strict`, so a malformed manifest line fails either way.

**§47.1's "or equivalent" is now read as an equivalence rather than a spelling.** The check that
held the registry to a file literally named `SHA256SUMS.sig` was wrong: §47.1 says "`SHA256SUMS.sig`
or equivalent verifiable signature", and a Sigstore bundle is the equivalent. It now requires a
signature over the manifest and a provenance to be published, and does not care which of §47.1's
spellings they use.

**The documents say that no release has been signed.** Every one of them — the registry header,
the generated page, the README and the Wiki — states that the signature and provenance steps have
not been proven end to end, that keyless signing needs a token existing only inside a release run
and Sigstore over a network §40.2 denies the container, and that the first `v*` tag is the run
that proves them. §66.5 asks for green rather than for existence, and ADR-0529 left #107's box
open for exactly this reason. A verification page that reads as though it had already been
exercised would be the documentation half of the same overstatement.

## Consequences

- ADR-0542 is superseded **in part**: its structure — one registry, three documents, a test that
  runs the executable steps, the order as a checked property, the `PROPRIETARY` list, the length
  budget — stands unchanged and is what made this correction a data edit rather than a rewrite.
  What it guessed about cosign and the file names is replaced by this record.
- The rewrite cost one YAML file, one README section, one Wiki section and a regenerated page,
  because all four are rendered or compared from the same rows. That is the property the
  single-registry decision was for, tested by the first thing that changed under it.
- `docs/ACCEPTANCE.md` §4.8.12's verification box stays open, and now says which half is proven —
  the sequence exists, is consistent across three documents, and its checksum step runs against a
  fixture and against a tampered one — and which half is not.
- A reader with the repository is pointed at `scripts/verify-release.sh`, which additionally
  cross-checks the manifest against the provenance. That check is `cargo xtask provenance --verify`
  and needs the workspace, so it is named rather than printed as part of the copyable sequence.

## Alternatives considered

- **Amend ADR-0542.** Forbidden by AGENTS.md §8: an accepted record is not edited, and a decision
  that turned out to be about a different release is exactly what superseding is for.
- **Keep both spellings in the registry, so the documentation works either way.** Rejected: a
  reader would meet two sets of commands and have to work out which release they had. One release
  exists; the documentation describes it.
- **Point readers at `scripts/verify-release.sh` and print nothing else.** Rejected by §47.5,
  which asks for a short copyable sequence: a reader who downloaded a `.deb` does not have the
  repository, and telling them to clone it to check a download is a worse answer than four
  commands.
