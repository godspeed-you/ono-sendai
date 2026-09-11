# ADR-0853: The local gate tests the packages an increment can break

- Status: accepted
- Date: 2026-09-11
- Spec refs: AGENTS.md §7, §10; v0.4.1 §38.3 (skip verification); ADR-0563, ADR-0852
- Decided by: agent, at the user's request

## Context

AGENTS.md §7 runs the gate before every commit, so the gate on a developer machine runs far more
often than CI does, and its time is multiplied by every increment. Measured on 2026-09-11 on an
idle eight-core machine, `scripts/gate.sh` at 25a50496 with a clean tree and the packaging suite
unselected:

- **10m02 in total, 8m24 of it the test step after a 40-second compile.** Format, lint, fuzz,
  supply chain, contracts and docs took about 55 seconds together.
- **`cargo test` runs the 568 test binaries of the workspace one after another.** The results
  report 428 seconds inside the binaries; process start-up and doc-tests are the rest.
- **The time is not spread evenly.** 506 binaries finish in under a second. Four take 201 of the
  428 seconds, and inside them a handful of tests: seven of `change_gates.rs` plan a restart of
  fifty services at 94 to 99 seconds each, one of `change_views.rs` does the same at 40, two of
  `change_claims.rs` take 31, one of `spatial_first_output.rs` 34.
- **A quarter of the commits touch no Rust a test outside xtask runs.** Of the last 300 commits on
  `implementation`, 71 changed only decision records, the state board, scripts and similar files
  only xtask reads, and 6 more changed xtask besides; 223 changed something else.
- **Nearly every Rust change reaches `ono-cli`, and `xtask` depends on `ono-cli`.** A change to a
  crate therefore covers most of the test time whatever is selected.

So a selection by what changed saves most of the test step on about a quarter of the commits and
little on the rest. The slow tests above are what the rest pays for; they are separate
increments (see *Consequences*).

## Decision

1. **Without a selection of its own, the gate's test step covers the packages the increment can
   break.** `cargo xtask affected [PATH...]` takes the changed paths and prints `cargo test`
   package arguments. `scripts/gate.sh` hands it the working tree's difference from `HEAD`,
   untracked files included, both sides of a rename, and tests what it prints.
2. **The rules**, in `xtask::affected`:
   - a file inside a package selects that package and every package that depends on it,
     transitively and by any kind of dependency;
   - a file of `HARNESS` — `docs/STATE.md`, `docs/ACCEPTANCE.md`, the decision records, the
     releases, the specifications, the generated reference, `scripts/`, `docker/acceptance/`,
     `README.md`, `AGENTS.md`, `CLAUDE.md`, `LICENSE`, `deny.toml` and a few more directories of
     `docs/` — selects `xtask`, the only package that reads them;
   - `xtask` is in every selection, because its tests read the sources of every package;
   - **anything else selects every package** — the lockfile, the toolchain, `docs/contracts/`
     (compiled by three build scripts and read by tests across the workspace),
     `docs/MIGRATION.md` (read by an ono-cli test), the workflows (the fuzz crate's tests read
     `fuzz.yml`), a file no rule knows;
   - **no changed file selects every package**, because a run on a clean tree is a re-run after
     the commit and is asked about the tree.
3. **The harness list checks itself.** `xtask/tests/affected.rs` reads every string literal of
   every Rust source outside xtask and fails when one names a `HARNESS` path, however many
   directories up it starts. It found `fuzz.yml` on its first run, which is why the workflows are
   not on the list.
4. **Every package, always, where it matters that nothing was left out.** `ONO_TESTS=all` covers
   every package, and so does `ONO_CANONICAL_CI=1`. CI's test parts name their selection
   explicitly (ADR-0852), so CI is untouched. A `git` that cannot answer covers every package.
5. **A selection builds every workspace binary first**, as ADR-0852's parts do, because tests
   find the binaries of other packages in the target directory.

## Consequences

- A commit that touches only harness files runs xtask's tests instead of the workspace's; with
  the packaging suite unselected that is seconds instead of eight minutes. A Rust change runs
  about what it ran before.
- What can reach a commit untested locally is a package outside the selection that a rule
  misplaced. The self-check covers a path spelled as a literal; a path assembled from pieces
  (`join("docs").join("adr")`) escapes it, and CI still runs every package before anything
  reaches `main`.
- A selection can compile third-party dependencies with fewer features than the workspace does,
  as ADR-0852 notes for CI's parts; cargo keeps both variants, so the cost is a one-off compile.
- The packaging selection of ADR-0563 is unchanged and applies inside whatever is selected.
- **The slow tests remain the larger lever for Rust changes**, and they are recorded in
  `docs/STATE.md` → *Found, not yet filed*: planning a restart costs about 0.75 seconds per
  target because each target reads every unit and every process again, and `change_claims.rs`
  waits for the 30-second default verification timeout.

## Alternatives considered

- **The same rules in `gate.sh` with `jq`.** Shorter, and untestable in the way the rest of the
  harness is tested; the rules decide which tests run, so they get tests of their own.
- **Running the test binaries side by side** — a runner that builds with `cargo test --no-run` and
  starts the binaries in parallel, or `cargo-nextest`. The largest lever for Rust changes, and
  140 of the 461 test files carry a timeout, a budget or an elapsed-time assertion; which of them
  a neighbour would move has to be settled first, and ADR-0852 records why nextest itself does
  not fit.
- **Merging each crate's integration tests into one binary.** Fewer links and parallel tests
  inside the binary, at the cost of a restructuring of the whole suite and the same load
  question.
- **Selecting inside `ono-cli` by test file.** Its tests exercise the shell end to end; a mapping
  from source files to test files would be a guess.
