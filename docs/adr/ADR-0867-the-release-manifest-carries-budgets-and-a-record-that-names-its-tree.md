# ADR-0867: The release manifest carries budgets and a record that names its tree

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §35.3, §43.2, §47.4, §50.1, Appendix H; ADR-0451, ADR-0530, ADR-0864,
  ADR-0866
- Issues: #125
- Decided by: agent (autonomous)

## Context

ADR-0864 put the sizes of `docs/baselines/binary-size.yaml` into the release input manifest as
`binaries.<binary>.stripped_bytes`. An independent review found them detached from the release:
the `inputs` job of `.github/workflows/release.yml` builds nothing, so the figures were whatever a
release build of some earlier tree measured, presented beside the release's own commit and tag as
if they described its bytes. Nothing kept the record current either: `spec-check` holds it to the
budget, not to the tree.

Three things constrain the fix:

- The manifest is Appendix H's list of what a release is *given*, and the workflow writes it
  "before it makes anything" (ADR-0451). A size is an output.
- The provenance binds artifact digests and is generated "from the Appendix H input manifest and
  this run's own identity — never from anything the build produced" (ADR-0530).
- The binaries inside the packages are measured where they are packaged, on each release runner
  (ADR-0866), and that is the only place their bytes and a check meet.

## Decision

1. **The manifest carries inputs, and names the tree of anything else.** `binaries` becomes:
   - `budget_bytes.<binary>.<triple>` — every row of `build_budgets`, the ceiling this release's
     binaries are held to where they are packaged. That is an input of the release.
   - `recorded` — `record` (the file), `commit` (the last commit that wrote it, or `null` when the
     working tree's record differs from it), `measured` (a sentence saying these figures come
     from a release build of `commit`, not from this release), and `stripped_bytes.<binary>.<triple>`.

   A release's own sizes are in the log of the job that packaged it, one `binary-size:` line per
   binary, each held to its budget there (ADR-0866). Measuring them into the manifest would make
   the manifest wait for the build it describes the inputs of, and putting them into the
   provenance would break its rule of reading nothing the build produced.
2. **CI keeps the record current where it builds what ships.** `scripts/binary-size.sh
   --check-record` fails when the record holds no figure for a binary it just measured, or one
   more than one percent of the build away from it. `scripts/package.sh --check-record` and
   `scripts/build-core.sh --check-record` pass it through, and `ci.yml` uses both: the packaging
   job holds the x86_64 `ono` and `kuang-compile` to the record on every push, the core job the
   musl `ono`. One percent is ten times what two builds of one tree differ by across linkers and
   build images (0.1 %, ADR-0865), and a change that moves the binary further has changed the
   figure the README and the manifest state. The fix is `cargo xtask metrics --write` after a
   release build, in the same commit.
3. **The release workflow does not pass `--check-record`.** The aarch64 figures are held to the
   record by nobody on a push, because nothing builds aarch64 on a push; failing a tagged release
   on a stale figure would block the release on documentation. The manifest's `commit` says how
   old the aarch64 figure is; the budget is enforced regardless.

## Consequences

- A reader of the manifest can no longer mistake the recorded sizes for the release's: the field
  says whose they are, and the release's own are in its packaging logs.
- A commit that moves a shipped binary by more than one percent fails CI's packaging or core job
  until it carries the re-recorded figure. That is the cost of a record that stays true; the
  message names the command.
- Local runs of `package.sh` and `build-core.sh` do not check the record unless asked, so a
  developer's packaging is not blocked by a figure somebody else moved.
- Tests: `xtask/tests/provenance.rs` —
  `should_carry_the_budgets_and_a_recorded_size_that_names_its_own_tree_into_the_manifest`;
  `xtask/tests/metrics.rs` — `should_fail_a_record_more_than_a_percent_from_the_build_that_ships`,
  `should_keep_the_record_current_in_the_ci_jobs_that_build_what_ships`;
  `xtask/tests/harness.rs` —
  `should_refuse_to_package_against_a_stale_record_when_asked_to_keep_it_current`.

## Alternatives considered

- **Build before writing the manifest, and measure.** Reorders Appendix H: the manifest would
  describe the inputs of a build that already happened, from its outputs.
- **Upload each runner's sizes and let `publish` add them to the provenance.** Correct bytes, but
  the provenance then reads a build product, which ADR-0530 rules out, and it would be a third
  place sizes live.
- **Drop the sizes from the manifest.** Honest, and it loses the budgets, which *are* inputs.
- **Hold the record exactly.** Two builds of one tree differ by the linker and the image; an exact
  record would fail every push that built it somewhere else.
