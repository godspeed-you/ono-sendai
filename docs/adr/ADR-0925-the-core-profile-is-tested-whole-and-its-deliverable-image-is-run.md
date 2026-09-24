# ADR-0925: The core profile's suite runs whole, and its deliverable image is run as it ships

- Status: accepted
- Date: 2026-09-24
- Spec refs: AGENTS.md §10, §11; ADR-0910, ADR-0913 (corrected here)
- Issues: #127
- Decided by: agent (autonomous)

## Context

An independent review of #127 found three gaps between what ADR-0913 and the commits claimed and
what was true.

1. The CI `core-build` job ran only `--test core_build` and the storage tests of
   `ono-provider-linux`. The claim that tier test files carry `#![cfg(feature = "<tier>")]` held
   only for the 17 files that failed to compile. Files that compiled but drove a compiled-out
   tier failed at runtime, so the core suite could not be run whole.
2. ADR-0913 §3 and `docker/README.md` said the harness busybox is "under /opt/harness/bin and
   nowhere else" and "not on any path `ono` was installed on". `/opt/harness/bin` is on `PATH`,
   so `ono` finds busybox applets whenever a case runs an external program. Cases 021, 023 and
   356 rely on exactly that.
3. The deliverable `core` stage, with its `ENTRYPOINT` and `USER`, was never run. Acceptance
   runs the `core-acceptance` stage with its own command.

## Decision

1. **The core suite runs whole.**
   - A test file that exercises a tier carries that tier's `cfg` at file level.
   - A test in a mixed file carries the `cfg` of the tier it drives. `full` is used where a test
     needs several tiers or the one-line full version.
   - The generated provider conformance suite is gated on `full` by its generator, because its
     declarations are the full product's providers.
   - No assertion changes, so the full build compiles and runs every test as before.
   - The CI job runs `cargo test -p ono-cli --no-default-features --features core` in full and
     `cargo test -p ono-provider-linux --no-default-features` in full.
2. **The harness is described as it is.** Busybox lives in `/opt/harness/bin`, which is on
   `PATH` after `/usr/local/bin`. It is what the case script runs, and it is what `ono` runs when
   a case asks for an external program. It never stands in for a native command. The
   deliverable `core` stage contains no busybox. The Dockerfile comment, `docker/README.md` and
   case 355's comment say so. This corrects the two sentences of ADR-0913 §3 quoted above. The
   rest of ADR-0913 stands.
3. **The deliverable is run.** `scripts/build-core.sh --stage <dir> --run-image`:
   - builds the `core` stage under a tag unique to the run;
   - runs it with no arguments beyond `ono`'s own, so the image's `ENTRYPOINT` and `USER` are
     what run;
   - checks that `--version` names the core build;
   - checks that PID 1 is `/usr/local/bin/ono`, running as `case` in `/home/case`;
   - removes the image again.

   The CI job runs it. It costs one image build from an already staged context.

## Consequences

- A tier test that is added without a `cfg` fails the core job, and the fix is a `cfg`, not a
  changed assertion.
- The PTY completion test of the user selector has a 150 ms completion budget. Under a heavily
  loaded machine (load around 35 on 8 cores) it failed inside the whole suite and passed alone.
  That is recorded, not changed (AGENTS.md: never raise a budget to pass).
- Tests:
  - `crates/ono-cli/tests/core_build.rs` (18 tests), plus the whole ono-cli suite in the core
    profile;
  - `build-core.sh --run-image` was checked against a deliberately wrong `USER`, which it
    refused.
