# ADR-0532: The bytes that were tested are the bytes that ship

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §48.4 (artifact identity), §49.1 (build once, promote after proof), §49.2 (no
  hidden local release step), §49.3 (release candidate versus final), §49.4 (failure atomicity),
  §62.6 (package identity), §43.2 (first-party scripts for critical logic)
- Issues: #110 — closes the H11 milestone; builds on #106 (ADR-0528), #108 (ADR-0530), #109
  (ADR-0531)
- Decided by: agent (autonomous)

## Context

The release workflow already built once and published what it built, and §48.4 asks for something
stronger than a pipeline that ought to make that true:

> The artifact tested by package validation MUST be the same bytes later uploaded to the release.
> The workflow MUST NOT rebuild packages after tests and then upload the untested rebuild.

§62.6 says how anyone would know: *the exact artifact installed in package smoke tests MUST hash
identically to the later published asset.* Nothing hashed anything across that boundary. A rebuild
inserted between the tests and the upload would have carried the same name, the same version and
the same metadata, and every other check in the release would have passed.

§49.4 asks for the other half: a failed publishing step should not leave a release that looks
complete.

## Decision

**Package validation records what it installed, publication compares against that record, and the
release is a draft until its asset inventory has been checked by digest.**

### The record crosses the job boundary

`scripts/package-check.sh` writes `target/package-check.sha256` — the SHA-256 of each package it
installed (ADR-0531). The `package` job uploads it as `tested-<arch>`; `publish` downloads every
one of them into `tested/` and runs:

```bash
cargo xtask checksums --dir dist --verify --tested tested
```

`check_tested_bytes` asks both directions:

- every artifact the record names is in the release **at the digest it was recorded with** — the
  same name at different bytes is the rebuild §48.4 forbids;
- every `.deb` and `.rpm` in the release appears in some record — an artifact package validation
  never installed does not ship.

The record goes to `target/`, not to `dist/`, because `dist/` is what two builds are compared byte
for byte (ADR-0527) and a record of the check is not an artifact of the build.

### Publication is a draft until the inventory verifies

`scripts/publish-release.sh` does five things in an order that is the whole guarantee:

```text
verify -> draft -> upload -> check what is attached, by digest -> publish
```

The inventory check downloads the assets back from the release and compares each against the local
bytes, then runs `sha256sum --check` over the downloaded copy of `SHA256SUMS`. **By digest rather
than by name**: an upload that truncated has the right name and the wrong bytes, and a count of
files cannot tell the difference. Until that passes, the release stays a draft — so a failure
leaves something visibly unfinished rather than something that looks complete.

It is a first-party script rather than `softprops/action-gh-release`, which it replaces. §43.2
asks that critical release logic live in repository-owned code with Actions orchestrating it, and
the draft-upload-verify-publish order is exactly the kind of logic that rule is about. It also
means a maintainer can read the publishing rule in one file rather than in an action's
documentation.

It verifies the release again before drafting, even though the workflow verified it one step
earlier. A script a maintainer can run must not assume its caller checked; the workflow step
exists so §49.1's order is visible in the pipeline rather than only inside a file.

### §49.2 and §49.3, which needed no new machinery and did need a test

Every step of the release is a script in this repository or an action pinned by commit, started by
a tag push. Nothing waits for a maintainer to run something locally, and no step that proves
anything is conditional — so a final tag reruns the complete check even where a release candidate
passed. Both were already true, and neither was asserted anywhere;
`should_promote_an_already_tested_artifact_rather_than_rebuilding_it` is where they now are. It
was green the moment it was written, which is worth saying rather than hiding: it is a regression
guard over properties this phase did not have to create, and the properties it guards are exactly
the ones a later convenience — "just rebuild it in `publish`, it is the same commit" — would take
away.

## Consequences

Easy: the release cannot substitute an untested artifact without failing, and it cannot half-
publish. Case `199-release-provenance` proves the identity check in the container, on the real
`ono` binary, by rebuilding it under the same name and requiring the refusal.

Hard: publication now needs `gh` and a token with `contents: write`, which the `publish` job
already had. And the inventory check downloads every asset back, which costs a minute on a release
and is the only way to know that what GitHub stored is what was uploaded.

Encoded by:

- `xtask/tests/packaging.rs::should_publish_the_same_bytes_package_validation_installed` — a
  rebuilt package and an unvalidated package are each refused by name, and the record is shown to
  travel from the job that tested to the job that publishes;
- `xtask/tests/supply_chain.rs::should_promote_an_already_tested_artifact_rather_than_rebuilding_it`
  — the publishing job builds nothing, takes its artifacts from the job that proved them, and
  skips no proof for any tag;
- `::should_publish_the_release_only_after_the_asset_inventory_verifies` — the five steps, in
  order, with the inventory checked by digest;
- acceptance case `199-release-provenance`.

## Alternatives considered

**Trust the artifact upload/download round trip.** GitHub's own artifact store is reliable, and
that is not the failure §48.4 is about. The failure is a maintainer or a future workflow edit
inserting a rebuild between the tests and the upload, which no storage guarantee addresses.

**Compare by artifact name and size.** Cheaper, and it accepts a package whose contents changed
without changing its length — which is exactly what a substituted build of the same commit looks
like. §62.6 says "hash identically" in as many words.

**Publish with `action-gh-release` and check the inventory afterwards.** The check would then run
on a release that is already visible, which is §49.4's failure mode with an extra step rather than
without one.

**Have `publish` rebuild from the tag "to be sure it matches".** This is the exact thing §48.4
forbids, and it is a plausible-sounding change somebody will propose. It is why the test that
forbids it is written even though it was green on the day it was written.
