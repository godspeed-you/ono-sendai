# ADR-0527: Two builds of one commit, and the member that differs

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §46.1 (definition), §46.5 (rebuild verification), §46.6 (cross-architecture),
  §62.4 (build twice), §65.11 (mutable release inputs), §49.1 (build once, promote after proof)
- Issues: #105 — builds on #104 (ADR-0526), consumed by #106 and #110
- Decided by: agent (autonomous)

## Context

§46.5 asks for two builds in fresh clean environments and a comparison, and then asks for one
thing more that is easy to skip: *a diagnostic identifying which files or archive members differ,
where tooling permits.* Two hashes that disagree tell a maintainer that a release is not
reproducible and nothing about why. A `.deb` is an `ar` archive of three members and an `.rpm` is
four concatenated sections, so "where tooling permits" reaches further than the file hash for both
of the formats this project publishes.

Nothing in the repository built anything twice. `scripts/package.sh` also wrote to `target/` and
`dist/` by name, so two builds could not exist side by side to be compared.

## Decision

**`scripts/rebuild-check.sh` builds every publishable artifact twice in two deliberately
different clean environments and hands the two directories to `cargo xtask compare-builds`, which
compares them byte for byte and names the differing archive member.**

### What is varied between the two builds

Everything a build may see and may not embed:

```text
a:  LC_ALL=C.UTF-8      TZ=UTC                umask 022
b:  LC_ALL=de_DE.UTF-8  TZ=Australia/Eucla    umask 077
    + its own TMPDIR, its own CARGO_TARGET_DIR, its own output directory
```

`Australia/Eucla` is +08:45 on purpose: a timezone whose offset is not a whole hour catches a
formatter that rounds where UTC would hide it.

`scripts/package.sh` gained `--dist <dir>` and now reads `CARGO_TARGET_DIR`, which is what lets
two builds of one commit exist at once. Both builds are handed the *same* `SOURCE_DATE_EPOCH`,
derived once: two builds that disagreed about the date would be comparing two commits.

### What the comparison reports

`xtask::reproducibility::compare` returns one `Difference` per way the two disagree:

- an artifact one build produced and the other did not;
- for a `.deb`, the `ar` member — `debian-binary`, `control.tar.xz`, `data.tar.xz` — that differs,
  with both lengths, both digests and the offset of the first differing byte inside it;
- for an `.rpm`, which of `lead`, `signature header`, `header`, `payload` differs, the same way;
- for anything else, the offset of the first differing byte;
- **and the mode the artifact file itself was written with.**

That last one is not decoration. It came out of a probe: with the determinism block of ADR-0526
removed from `scripts/package.sh`, the two packages were *still* byte-identical, and the second
build's files were `0600` because its umask was `077`. Identical bytes behind a different mode are
still two different downloads, and §46.4 asks for deterministic modes without restricting the rule
to archive members.

### Where it runs

- **`scripts/release-check.sh`** runs the packaging comparison after `scripts/package-check.sh`,
  so the local release gate builds every package twice.
- **`.github/workflows/release.yml`** does the stronger version. A new `rebuild` job builds the
  same commit **on a second runner** — a different machine, a different container daemon, a
  different temporary filesystem — and a `reproducibility` job compares the two, **once per
  architecture** (§46.6). `publish` now waits for it, so no release ships before the comparison
  has finished. A second runner is the freshest clean environment this repository can reach, and
  it costs no disk on either of them, which two target directories on one runner would.

## Consequences

### What the two builds differed in

**At the packaging layer: nothing, and that is a measurement rather than an assumption.** The two
tools were probed with the determinism block removed, under a German locale at +08:45 with a
private umask, and both packages came out identical. Specifically:

- `cargo-deb` 3.7.0 and `cargo-generate-rpm` 0.21.0 both honour `SOURCE_DATE_EPOCH`. The rpm's
  `BUILDTIME` is the epoch, and it emits no `BUILDHOST` at all — the two fields that classically
  make an rpm unreproducible.
- both write uid 0 / gid 0 with no `uname`/`gname` strings, so `dpkg-deb` prints `0/0`.
- both sort the assets a glob expands, so readdir order does not reach the archive.

What had to change for the comparison to be possible or honest was therefore not in the tools:
`scripts/package.sh` had to stop naming `target/` and `dist/` literally; the determinism inputs
had to be fixed (ADR-0526) or the second build's `0600` files would have differed; and the
*reader* had to be given `TZ=UTC`, because `dpkg-deb --contents` renders mtimes in the reader's
timezone and misread a correct archive on the first run.

### The limit this check does not reach

**The comparison covers every published artifact, and the compiled binary inside them is built
once per environment.** Locally `rebuild-check.sh` packages one binary twice, because a second
release compile means a second target directory and this machine does not have the disk for one.
In the release workflow the two builds are two runners, so there the binary *is* compiled twice
and the comparison does cover it — which is where the claim needs to hold, and where a failure is
a release blocker rather than a developer inconvenience. A local run therefore proves the
packaging layer and CI proves the whole chain; the two are the same script.

Encoded by:

- `xtask/tests/packaging.rs::should_produce_identical_hashes_for_two_clean_builds_of_one_commit`
  — runs the real script and requires the two directories to hold the same artifacts at the same
  digests;
- `::should_name_the_differing_archive_member_when_a_seeded_difference_is_introduced` — flips one
  byte inside a `.deb`'s `data.tar.xz` and requires the diagnostic to name both the artifact and
  the member;
- `::should_require_reproducibility_of_every_supported_architecture_separately` — the workflow
  rebuilds and compares per architecture, and `publish` waits for it;
- `xtask/src/reproducibility.rs`'s unit tests, including
  `notices_when_two_builds_agree_about_the_bytes_and_not_about_the_mode`, which is the probe
  finding turned into a rule.

## Alternatives considered

**Compare with `diffoscope`.** It is the right tool and it is a large Python dependency with a
long transitive tail, pulled into a release-critical path that §43.2 asks to keep first-party and
§45 asks to keep justified. Three hundred lines of Rust that read `ar` and the rpm section layout
cover both formats this project publishes, and it is the code the gate already tests.

**Build twice in one job with two target directories.** Cheaper in CI minutes and strictly worse
as evidence: the same machine, the same daemon, the same kernel, the same cache. §46.5 says *fresh
clean environments*, and two runners are two of them.

**Compare only the file hashes.** It is what §46.5's first sentence asks for and it stops exactly
where a maintainer's work starts. The seeded-difference test exists to keep the diagnostic honest:
it is not enough for the check to go red, it has to say `data.tar.xz`.
