# ADR-0548: The baseline freezes the state that exists, not the one that is gone

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §2.6, §32.3, §32.4, §37.2, §50.1, §52.2, §57 (H0), Appendix F.4, Appendix H;
  ADR-0451, ADR-0489, ADR-0490, ADR-0527, ADR-0529, ADR-0544, ADR-0547; spec §35.3
- Issues: #30 — consumes #103's manifest (ADR-0451) and #83/#84's baseline (ADR-0489, ADR-0490)
- Decided by: agent (autonomous)

## Context

§57's phase H0 asks for a starting point, and the issue says why:

> Nothing in v0.4.1 can be shown to have improved without a recorded starting point. Freeze,
> **before any hardening lands**: the current test/performance snapshot; the current release
> artifact hashes; the current release workflow inputs.

It was worked **last**. H0's other issues landed first, then H1 through H12 — twelve phases,
ninety-nine issues, a green gate, 128 acceptance cases. By the time this increment ran, the
"before" it was written to freeze was twelve phases old and unreachable: the binary that would be
measured is the hardened one, the workflow inputs are the pinned ones H10 pinned, and the test
counts are the ones H8's truthfulness work produced.

Measuring today's tree and filing the figures as the starting point is not a shortcut. It is
v0.4.1 §2.6 broken in the plainest way the specification states it:

> Where a value cannot be determined, the system MUST report it as unknown rather than
> substituting a plausible value.

Meanwhile, both things H0 wanted frozen had already been built, by the phases that needed them:

- **H7** wrote `docs/contracts/hardening/performance_baseline.json` — §32.4's regression baseline, six
  metrics per benchmark, on a named reference environment, with the one-minute load average of the
  run that produced each figure (ADR-0489, ADR-0490).
- **H10** wrote `cargo xtask build-manifest` → `dist/build-inputs.json`, every field of Appendix H,
  read from the tree and the run rather than typed (ADR-0451). That ADR left a note for this one:
  *"When #30 lands, its baseline should be a captured manifest rather than a second, hand-written
  list of the same facts."*
- **H11** built `SHA256SUMS`, the provenance and the two-build comparison — over artifacts that do
  not exist yet, because no `v*` tag has been pushed (ADR-0527, ADR-0529).

## Decision

**`docs/baselines/v0.4.1.json` is a snapshot of the finished tranche, written by
`cargo xtask baseline --write`, and it restates nothing.**

### 1. It says which state it is, in the file

The `note` field and `captured.state` both say it: this is the tranche complete, not the tranche
starting. A reader who finds a file called a baseline and assumes it is a *before* will draw
exactly the wrong conclusion from every figure in it, and the fix for that is one sentence in the
file rather than a convention somebody has to know.

#30's exit test survives the change of meaning intact — *"a machine-readable baseline file in the
repository that H7 and H11 both consume rather than re-derive"* — because what makes a baseline
useful to v0.5 is that it is a fixed point, not that it precedes something.

### 2. It binds the two files that hold the figures, and copies neither

§52.2 is the constraint that decides the shape:

> A number such as `max_connections = 32` MUST not be independently typed into five files if one
> contract can generate the others.

So the snapshot holds **no performance figures**. It names every benchmark, profile and
temperature the regression baseline records, with the commit each was measured at, and the gate
resolves each name into `performance_baseline.json` and requires all six of §32.3's metrics to be
present there. The check runs in both directions: a benchmark the snapshot names that the baseline
does not hold is a figure nobody can look up, and a benchmark the baseline holds that the snapshot
does not name is a snapshot of what somebody remembered.

The Appendix H inputs are **captured from the generator**, by calling
`provenance::build_inputs` — the same function `cargo xtask build-manifest` calls — so the two can
never disagree, and the gate holds the captured object to the field set the generator produces. A
manifest captured on a developer machine has no tag and no workflow run identity and says `null`
for both, which is what ADR-0451 already decided and what §35.3 requires.

### 3. The counts are history, and the gate reads them as history

`tests.at_capture` carries the nine repository metrics of §50 as they stood at the capture commit.
The gate checks that the recorded **keys** are exactly the metrics `cargo xtask metrics` computes —
so the snapshot cannot invent a count nothing produces, or quietly drop one — and does **not**
compare the values to the tree.

That is deliberate and it is the one place this file could have gone wrong. §50.1 already makes
the README's generated block the live figure, checked against the tree on every gate run
(ADR-0544). A snapshot whose counts had to equal the present would be that same number typed
twice, which is §52.2 again, and it would turn every added test into a two-file change for no
gain. A snapshot is a moment; the moment is allowed to pass.

### 4. The artifact hashes are null, with a reason, because there is no release

§57 H0 asks for "the current release artifact hashes". There are none. No `v*` tag exists, so the
release workflow has never run, so no artifact has ever been built by it — the same fact that
keeps §4.8.11's signature box and §4.8.12's verification box open (ADR-0529).

`artifacts.hashes` is therefore `null` and `artifacts.reason` says all of that in the file, and
**the gate refuses a null with no reason**. An empty list would have been the easy answer and it
would have read as "nothing was published", which is a claim; a bare `null` is a question nobody
asked. §2.6 and spec §35.3 both ask for the third thing, which is an absence that explains itself.
Re-running `cargo xtask baseline --write` after the first tag records the digests.

## Consequences

Easy: `cargo xtask baseline` prints the snapshot and `--write` commits it, so re-capturing after a
re-measurement or a first release is one command. `spec-check` validates it on every gate run, so
a benchmark that leaves the regression baseline turns the gate red here rather than being noticed
by a reader.

Hard: the snapshot is deliberately not a comparison target. Nothing in it can be regressed
*against*, because the figures live in `performance_baseline.json` and `perf::Baseline::compare`
is what compares them (ADR-0489). Somebody looking for "the file the regression gate reads" will
find this one first and it is not that file; the `checked_by` and `written_by` fields on each
section point at the one that is.

Also: it is the one machine-readable contract in the repository that lives outside
`docs/contracts/hardening/`, so `registries.yaml` does not index it (ADR-0547). That is right —
`docs/contracts/` is the public contract surface and a snapshot of one tranche is a record, not a
contract — but it means the "every contract is indexed" property has one deliberate exception, and
this paragraph is where it is written down. Its validator, `xtask::baseline::check`, runs in
`spec-check` beside the indexed ones.

Encoded by: `xtask/tests/perf.rs::should_read_the_frozen_v041_baseline_and_find_every_metric_it_declares`,
`::should_report_a_frozen_baseline_naming_a_benchmark_nobody_measured`,
`::should_report_a_frozen_baseline_that_leaves_a_measured_benchmark_out`,
`::should_report_a_frozen_baseline_that_leaves_an_absent_artifact_hash_unexplained`,
`::should_capture_the_frozen_baseline_from_the_sources_rather_than_from_a_second_list`.

## Spec deviation

- Section: v0.4.1 §57, Phase H0
- Text: "freeze a v0.4.1 baseline test/performance snapshot; … record current release artifact
  hashes and workflow inputs" — under a phase whose deliverables are ordered "**No production fix
  lands before the corresponding failure proof where practical**", i.e. before the hardening work.
- Instead: the snapshot records the tranche **after** H1–H12, and says so in its own `note`. The
  release artifact hashes are `null` with a written reason rather than a figure.
- Why: the increment ran after the tranche was complete. The state §57 H0 names no longer exists
  in the working tree, and the only way to produce numbers for it would be to measure the hardened
  build and label the result "before", which §2.6 forbids and which would make every later
  comparison meaningless in a way nobody could detect. The purpose §57 gives the deliverable —
  *"nothing in v0.4.1 can be shown to have improved without a recorded starting point"* — is
  served for v0.5 by a fixed point that is honestly labelled; it cannot be served retroactively
  for v0.4.1 by a fabricated one. The improvement v0.4.1 actually made is recorded where it was
  made: in the ADRs of each phase, in the boxes of `docs/ACCEPTANCE.md` §4.8, and in the four
  failure proofs of #31 that were committed red and are now green.

## Alternatives considered

**Reconstruct the "before" by checking out the pre-tranche commit and measuring it.** It is
technically possible: `git checkout` the commit before H1, run `cargo xtask perf`. Rejected on
three grounds. It needs a second release build this machine has no disk for (ADR-0527 records the
same constraint for the rebuild comparison). The figures would be measured on today's kernel and
today's load, so they are not the historical numbers either, only differently wrong. And nothing
would consume them: the regression gate compares against `performance_baseline.json`, and a second
baseline that no comparator reads is a file, not a control.

**Write nothing, and close #30 as delivered by H7 and H10.** Defensible — both artifacts exist and
both are gate-checked. Rejected because #30's exit test asks for *one* file H7 and H11 both
consume, and because "delivered by two other issues" is a claim a reader has to reconstruct. A
binding file that resolves into both, and that the gate holds to them, makes the claim checkable.

**Copy the six metrics per benchmark into the snapshot so it stands alone.** Rejected by §52.2 and
by the concrete failure mode: two files of the same figures diverge at the first re-measurement,
and the one nobody compares is the one that lies.

**Compare the recorded counts against the tree, so the snapshot cannot go stale.** Rejected: that
is not a snapshot, it is a second copy of the README's generated block with a re-capture step
attached to every added test. The keys are checked and the values are dated.
