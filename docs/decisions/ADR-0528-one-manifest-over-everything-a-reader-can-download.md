# ADR-0528: One manifest over everything a reader can download

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §47.1 (required release files), §47.2 (checksum manifest), §46.1 (checksum
  manifests are a reproducibility target), §48.2 (the manifest matches the uploaded file),
  §67.7 (the command a user types)
- Issues: #106 — consumed by #107 (what the signature covers) and #108 (what provenance binds)
- Decided by: agent (autonomous)

## Context

§47.1 asks every release to publish `SHA256SUMS` beside the packages, and §47.2 asks it to cover
*every* downloadable executable or package artifact, in deterministic order. The release published
packages and, since ADR-0451, `build-inputs.json`, and no digests at all.

Two questions had to be answered rather than assumed.

**What counts as an artifact.** A release directory holds the packages, the input manifest, the
checksum manifest, a signature over it and a provenance document that quotes its digests. A
manifest that listed itself would be unverifiable; one that listed its own signature or the
provenance would be circular.

**What "matches the uploaded files" means.** `sha256sum -c` answers one direction: every listed
file is present and hashes correctly. It cannot answer the other: whether a file is present and
*unlisted*. That is the direction an asset reaches a release unattested through, and §48.2 asks
for it by name.

## Decision

**`cargo xtask checksums` writes `SHA256SUMS` over a release directory, and `--verify` checks it
in both directions.**

- **Everything is listed except four names and the provenance**: `SHA256SUMS`, `SHA256SUMS.sig`,
  `SHA256SUMS.pem`, `SHA256SUMS.sigstore.json` and `build-provenance.json`. Every other file in
  the directory is something a reader downloads, so every other file is listed — including
  `build-inputs.json`, which is a published asset and not an exception. The chain has no gap:
  the manifest covers the artifacts, the signature covers the manifest, and the provenance binds
  the artifacts *and* the manifest's own digest (ADR-0530).
- **The order is byte order of the file name**, which is what `LC_ALL=C sort` gives and what makes
  the manifest a deterministic function of the artifacts alone. §46.1 names checksum manifests as
  a reproducibility target, and this is how it is met: not by rebuilding the manifest twice and
  comparing, but by there being nothing in it that could differ.
- **The format is `sha256sum`'s own** — digest, two spaces, name. §67.7 shows a user typing
  `sha256sum -c SHA256SUMS`, so the test proves the manifest by running exactly that.
- **`--verify` fails on an unlisted artifact**, on a listed artifact that is absent, and on one
  whose bytes no longer hash to what the manifest records.

The manifest is written **once, in `publish`**, after both architectures and the input manifest
are in one directory — because "every artifact in the release" is only knowable there. It is not
written per architecture: two partial manifests are not one manifest, and merging them would be a
third implementation of the same sort.

`scripts/release-check.sh` writes and then verifies it, so an artifact missing from the manifest
fails the release check locally as well as in the workflow.

## Consequences

Easy: #107 has one file to sign. #108 has one list of digests to bind. A user has one command.

Hard: the manifest is a property of the *directory*, so a release directory that still holds a
previous version's packages produces a manifest that lists them. On a fresh runner that cannot
happen; on a developer machine `dist/` accumulates, and `release-check.sh` will list what is
there. That is the truthful answer for that directory, and the alternative — filtering by a
version pattern — would make the manifest silent about a file that is genuinely present.

Encoded by `xtask/tests/provenance.rs`:

- `should_list_every_downloadable_artifact_in_the_checksum_manifest` — five assets of a release
  fixture, every one listed, the manifest not listing itself, and `sha256sum -c --strict`
  verifying it;
- `should_order_the_checksum_manifest_deterministically` — the same artifacts written into two
  directories in opposite orders produce byte-identical manifests, sorted;
- `should_fail_the_release_check_when_an_artifact_is_absent_from_the_manifest` — an asset that
  arrives after the manifest was written, and an asset whose bytes changed afterwards, are both
  refused by name.

## Alternatives considered

**Generate the manifest in each `package` job.** Each would then describe one architecture, and
the release would carry two files called `SHA256SUMS` that collide on upload. §47.2 says *every*
downloadable artifact in the release, which is a statement about the release and not about a job.

**Use `sha256sum *` in the workflow.** It is one line, and it is the line that would have listed
`SHA256SUMS` itself, ordered by shell glob expansion under whatever locale the runner had, with no
verification of the second direction. §43.2 asks for first-party code in critical release logic;
this is what that rule is for.

**Exclude `build-inputs.json` as "not an executable or package".** §47.2's words are "every
downloadable executable/package artifact", and the input manifest is downloadable. Leaving it out
would mean the one file that says what the release was made of is the one file a reader cannot
check.
