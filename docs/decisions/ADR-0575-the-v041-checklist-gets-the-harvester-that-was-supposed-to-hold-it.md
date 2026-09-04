# ADR-0575: The v0.4.1 checklist gets the harvester that was supposed to hold it

- Status: accepted
- Date: 2026-09-04
- Spec refs: v0.4.1 §3, §3.1, §3.4, §57, §66.1–§66.9; ADR-0137, ADR-0401, ADR-0402, ADR-0427,
  ADR-0515
- Issues: none (found while reading `docs/ACCEPTANCE.md` §4.8 against the tree)
- Decided by: agent (autonomous)

## Context

`docs/ACCEPTANCE.md` §4.8 is the definition of done for the v0.4.1 tranche, and its very first box
says what holds it:

> **P0 · Every proof this subsection names resolves.** `xtask/tests/hardening_evidence.rs` holds
> §4.8 the way `xtask/tests/spatial_evidence.rs` holds §4.7 … This box closes last: it is the
> mechanical statement that no other box here is ticked by nothing (#29, §3).

That file exists and holds two tests — the fourteen acceptance families, and the finite timeout
every v0.4.1 case states. Neither of the three tests the box names had been written. §4.7 has had
its harvester since ADR-0137; §4.8 has had none, so for the whole tranche every tick in it was a
claim nobody checked.

The tranche closed all 101 of its issues under that gap, and the gap shows: §4.8 stands at 78
ticked boxes of 118, and of the forty that are open, twenty-five name only proofs that exist. The
nine boxes of §4.8.2 name test names their delivering increments never used — the checklist says
`authentication.rs::should_refuse_the_tls_handshake_when_the_client_presents_no_certificate` and
the tree says
`client_authentication.rs::should_refuse_a_tls_client_that_presents_no_certificate`. The mutual
authentication is there; the pointer to it is not. A harvester would have failed on the commit that
introduced the drift.

## Decision

### 1. One harvester per checklist, on one set of shared helpers

`named_tests`, `locate`, `declared` and `assert_proofs_exist` move from
`xtask/tests/spatial_evidence.rs` into `xtask/tests/support/mod.rs`, and both harvesters call
them. §39.1's rule against a second copy of a helper (ADR-0427, ADR-0515) applies to the guards
themselves: two harvesters that had each grown their own reader of the same document would drift
the way the boxes did. `locate` takes the name of the checklist it is resolving for, so its failure
names the subsection a reader has to open.

### 2. What the §4.8 harvester holds

Seven tests, of which §4.8.1 and §4.8.14 name six by name:

- `should_read_the_v041_checklist_apart_from_the_v04_one` — §4.7's passage ends at `### 4.8` and
  §4.8's begins there, so neither harvester answers for the other's boxes;
- `should_find_every_test_the_v041_checklist_names_as_a_proof` — every `file.rs::should_…` §4.8
  names exists, lives under a crate's `tests/` or under `xtask/tests/`, and is not `#[ignore]`d;
- `should_find_every_acceptance_case_the_v041_checklist_names` — a case in backticks is a claim the
  referee collects it, a case in prose is a name recorded absent, and both are checked (ADR-0401);
- `should_find_every_p0_and_p1_box_of_the_v041_checklist_ticked` — §66.9's binding criterion, read
  from the priority §3.1 makes each box carry rather than from the tracker's labels;
- `should_find_a_dated_adr_for_every_box_the_checklist_leaves_open` and
  `should_refuse_an_exclusion_adr_dated_after_the_release_candidate_freeze` — §66.9's only
  exception, held on the box rather than on an issue;
- `should_find_a_box_for_every_bullet_of_the_release_definition` — every bullet of §66.1–§66.8 has
  a box in §4.8 that names its proof.

### 3. The release-candidate freeze is a date in the checklist, not a date in a test

§66.9 allows an exclusion only through "an ADR made before release candidate freeze" and never says
when the freeze is. §4.8.14 now states it — **2026-09-04** — and the harvester reads it from there.
A test carrying its own copy would be a second source for the one date the rule turns on.

### 4. The last test checks the mapping, not the tick

`should_find_a_box_for_every_bullet_of_the_release_definition` asserts that every §66 bullet has
exactly one box in §4.8 opening with the title the reviewer mapped it to, and that a box left open
names an ADR. It deliberately does *not* assert that the box is ticked.

That is not a weakening. `scripts/release-check.sh` greps this file for `^- \[ \]` and fails on the
first one, so "every box is ticked" is already enforced for the whole checklist by the release gate
itself, and asserting it a second time here would only decide *where* the same failure is reported.
What no other check holds is the mapping: a criterion of §66 whose box was rewritten, merged into a
neighbour or dropped disappears silently, and the checklist still reads as complete. So this test
holds the mapping, and §66.9's "an exclusion cannot waive a release criterion" is the conjunction of
three checks — the P0/P1 test, the exclusion test, and the release gate's own grep.

### 5. Three of the seven are committed red

§57's rule is the failure before the fix, and AGENTS.md §7's is that a red test is `#[ignore]`d with
a `// REASON:` and an entry under *Deferred*. `should_find_every_test_the_v041_checklist_names_as_a_proof`
is red because of the drift it exists to find, and its report is the worklist for the increment that
reconciles §4.8.2. The two §4.8.14 tests are red because forty boxes are open. Each names the
increment that un-ignores it.

## Consequences

Easy: the drift that produced §4.8.2's nine dangling boxes cannot recur unnoticed — the next
rename fails the gate on the commit that makes it, which is what §4.7 has had since ADR-0137.

Hard: the checklist is now load-bearing text. A box whose title is edited breaks the §66 mapping
table, and that is deliberate — a criterion is supposed to be hard to lose — but it means editing
§4.8's prose is a code change and has to be run through the gate like one.

Also hard: three ignored tests exist until the tranche's boxes are reconciled and ticked. They are
the only ones in the workspace besides ADR-0496's, and §4.8.12's "the status documents agree" box
cannot be ticked while any of them remains — which is the pressure that closes them.

Encoded by `xtask/tests/hardening_evidence.rs` in full, and by
`xtask/tests/spatial_evidence.rs::should_report_a_checklist_proof_that_no_longer_exists`, which is
the shared helpers' own guard and now covers both harvesters.

## Alternatives considered

**Rename the tests to match the checklist.** The checklist was written before the work and guessed
at names; the tests were written with the work and describe what they assert
(`should_refuse_a_tls_client_that_presents_no_certificate` says more than
`should_refuse_the_tls_handshake_when_the_client_presents_no_certificate`). Renaming eleven tests so
a document that guessed wrong can stay unedited is the tail wagging the dog, and §11 asks that a
test name describe the behaviour rather than the plan.

**Tick §4.8's boxes first and write the harvester afterwards.** That is the order that produced the
problem: every tick between the tranche's start and now was made without a check, and doing forty
more of them the same way would leave the same hole for the next tranche to fall into.

**Let `should_find_a_box_for_every_bullet_of_the_release_definition` also require the tick.** It
would then be impossible to run before the release run that closes #107 and #115 — the two boxes
that wait on a signature only a `v*` tag can produce — so the test that guards §66's mapping would
itself have to stay ignored through the release. Splitting the two questions keeps the mapping
guarded on every gate run.
