# ADR-0251: The acceptance image is built from the sources it copies

- Status: accepted
- Date: 2026-08-29
- Spec refs: AGENTS.md §10, §14 ("the referee outranks the feature"); `docs/ACCEPTANCE.md` §1
- Decided by: agent (autonomous)

## Context

`docker/Dockerfile` keeps cargo's `target/` in a BuildKit cache mount so the acceptance suite is
cheap enough to run on every increment. Cargo decides what to rebuild from **mtimes**, and
`COPY` preserves the host's. A source file edited *before* the cached artifacts were written is
therefore older than them, cargo declares its crate fresh, and the image is built — successfully,
with no warning of any kind — around the *previous* binary while carrying the *new* source.

This was observed rather than reasoned about. An image built from a tree that implements the
`prompt.vcs` setting shipped an `ono` that answered `there is no setting 'prompt.vcs'`; the
builder stage held `crates/ono-cli/src/repl.rs` with the new code at mtime 05:21 and
`/usr/local/bin/ono` at 05:26.

The consequence is the worst kind a harness can have: every case in the suite passes, and what
it graded was the code of some earlier run. A referee that cannot notice it is grading the wrong
program makes every claim of progress behind it worthless (AGENTS.md §14).

## Decision

**The build stamps the workspace sources with its own clock before compiling:**

```dockerfile
find crates xtask docs/contracts Cargo.toml Cargo.lock -type f -exec touch {} + && cargo build --release …
```

Only the workspace is stamped. The registry's sources keep their mtimes, so dependencies stay
cached and each build recompiles `crates/` and `xtask/` and nothing else — a couple of minutes,
which is the price of the suite grading the tree it was given.

The rule is asserted rather than trusted:
`xtask/tests/packaging.rs::should_stamp_the_workspace_before_building_when_the_image_caches_its_target_directory`
fails if the build step keeps the cache mount without stamping, or stamps after compiling. It is
conditional on the cache mount, so removing the mount — the other correct answer — leaves it green.

## Consequences

An acceptance run now grades the tree it was handed. The cost is a workspace recompile per image
build; the dependency graph, which is the expensive part, stays cached.

Every acceptance result recorded before this fix carries an asterisk: it was correct only if the
sources it exercised happened to be newer than the cached artifacts. Nothing is known to have
been wrongly ticked — the boxes that were closed name workspace tests as well as cases — but the
possibility is why the fix outranks the features it was found beside.

## Alternatives considered

- **Dropping the target cache mount.** Correct, and it makes every image build a full release
  compile of the dependency graph. Rejected on cost; the guard permits it, so a future agent may
  choose it.
- **`CARGO_INCREMENTAL=0` alone**, which is already set. It does not help: incremental compilation
  is a different mechanism from the fingerprint's mtime comparison.
- **Trusting BuildKit's layer cache to invalidate the RUN step.** It does invalidate it — the step
  re-runs. It is cargo, inside the step, that decides nothing needs rebuilding.
