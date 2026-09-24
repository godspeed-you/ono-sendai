# ADR-0895: The scratch root is read from cargo's build layout and never guessed

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.6 §15, Appendix B.7
- Issues: #143
- Amends: ADR-0890 (its rule for finding the target directory; its decision to scratch in
  cargo's target tmp directory stands)
- Decided by: agent (autonomous)

## Context

ADR-0890 located the scratch root at run time as "the nearest ancestor of the test binary that
holds a `CACHEDIR.TAG`", said this held "whatever `CARGO_TARGET_DIR` or `--target` made of the
path below it", and fell back to the workspace's `target/`. A review raised four concerns.

1. **`--target` builds.** Cargo writes a `CACHEDIR.TAG` into `target/<triple>/` as well, and the
   review expected scratch to land in the wrong directory there. Checked against cargo 1.94: for
   `cargo test --target x86_64-unknown-linux-gnu`, cargo itself compiles
   `CARGO_TARGET_TMPDIR = target/x86_64-unknown-linux-gnu/tmp` into the test (read from the
   built binary). So the nearest tag gave the right answer, and
   `crates/ono-recovery-files/tests/store.rs::should_give_a_suite_scratch_space_on_the_filesystem_cargo_builds_into`
   passes under `--target`. ADR-0890's wording was still wrong: the directory found is
   `<target>/<triple>`, not the target root, and the answer was right by accident of cargo's
   choice.
2. **Missing tags.** Cargo writes the tag only when it creates the directory. A target directory
   that already existed — restored from a CI cache, or mounted as a volume — has no tag, and
   scratch silently fell back to the workspace's `target/`.
3. **Stray tags.** An unrelated tagged directory above a binary, such as `~/.cache`, was taken
   for a target directory.
4. **`CARGO_TARGET_DIR` ignored.** The fallback used for doc tests ignored `CARGO_TARGET_DIR`.

## Decision

The scratch root is, in order:

1. `CARGO_TARGET_TMPDIR` from the environment, if set.
2. **From the binary's place in cargo's build layout.** Find the nearest ancestor of the binary
   that is a profile directory, meaning it holds both `.fingerprint` and `deps`. Its parent is
   the directory cargo names `CARGO_TARGET_TMPDIR`'s parent for that build: `<target>` normally,
   `<target>/<triple>` for a `--target` build. Scratch goes in that parent's `tmp`.
3. **For a binary outside that layout (a doc test):** `CARGO_TARGET_DIR/tmp`, resolved against
   the workspace root when the path is relative.
4. Otherwise the workspace's `target/tmp`, but only if that directory exists.
5. Otherwise a **panic** that names what was looked for and says to set `CARGO_TARGET_TMPDIR`.

No step relies on `CACHEDIR.TAG`, and nothing falls back to the system temporary directory.

## Consequences

The decision is a pure function of gathered surroundings, and unit tests in
`crates/ono-testkit/src/scratch.rs::tests` cover each branch:

- a plain build and a `--target` build;
- a target directory without a tag;
- a stray tag above the binary;
- `CARGO_TARGET_DIR` for a doc test;
- the refusal when nothing can be found.

A relative `CARGO_TARGET_DIR` is resolved against the workspace root. Cargo resolves it against
the directory it was invoked from, which is the same directory for this workspace's gate and CI,
but not necessarily for a developer invoking cargo from a subdirectory.

## Alternatives considered

**The outermost tagged ancestor** (the review's suggestion). It would pick `target/` for a
`--target` build, which is not what cargo names. It would also pick an unrelated outer tag.
