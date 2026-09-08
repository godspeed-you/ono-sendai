# ADR-0740: The v0.5 evidence harvester holds ticked boxes, and its rules live where they can be mutation-tested

- Status: accepted
- Date: 2026-09-08
- Spec refs: `docs/ACCEPTANCE.md` §3, §4.8.1, §4.11; v0.4.1 §39.1, §39.2; v0.5 §48; ADR-0137,
  ADR-0401, ADR-0427, ADR-0515
- Decided by: agent (autonomous)

## Context

`docs/ACCEPTANCE.md` §4.11 is the v0.5 tranche's definition of done: 123 boxes, every one of them
open, each naming the automated proof that will close it. Three harvesters already hold the
subsections before it — `xtask/tests/spatial_evidence.rs` (§4.7), `xtask/tests/hardening_evidence.rs`
(§4.8) and `xtask/tests/permission_evidence.rs` (§4.9, §4.10) — and §4.11's own preamble names
`xtask/tests/temporal_evidence.rs` as its fourth. Following the three precedents answers most of
the question. Three things it does not answer forced a decision.

**§4.11 is written entirely in advance.** The three earlier harvesters were written over
checklists that were at least partly delivered, and they resolve *every* proof a subsection names,
ticked or not — `support::assert_proofs_exist` walks the whole passage. Applied to §4.11 that
rule reports 90-odd files nobody has written yet, on the first increment of the tranche, which
makes the guard unusable exactly when the checklist most needs one.

**A guard that resolves nothing looks identical to a guard that works.** With every box open, a
harvester that silently matched no boxes at all would be green, and would stay green through the
first ticked box, and through the box after that. That is the defect `hardening_evidence.rs` was
written against, reappearing one subsection later in a form its own floor assertions
(`assert_proofs_exist(..., 150)`) cannot detect, because the floor is a count of *named* proofs
and §4.11 names plenty.

**§4.11 names its proofs in a different vocabulary from §4.8.** §4.8 writes
`` `file.rs::should_do_the_thing` ``; §4.11 mostly writes a whole path with no test name
(`crates/ono-temporal-query/tests/why.rs`), sometimes a directory
(`crates/ono-temporal-render/tests/`), sometimes a scanner function
(`xtask/src/scan.rs::check_bounded_channels`), sometimes a document
(`docs/contracts/hardening/performance_baseline.json`), and twice a bare command
(`cargo run -p xtask -- perf`). `support::named_tests` reads one of those forms and drops the
rest.

## Decision

### 1. The harvester holds *ticked* boxes, and proves it bites on a scratch copy

A ticked box's proofs must all resolve. An open box's proofs are what its delivering increment
owes, and `scripts/release-check.sh` — which fails on the first `- [ ]` in the file — is what
refuses to call the tranche finished until they exist. So the harvester never reports an open
box, and it never reads a tick as a verdict of its own.

The price of that leniency is that on the day it is written the check passes over an empty set.
It is paid in full by a companion test for every rule: each check is run a second time against a
scratch `docs/ACCEPTANCE.md` (`ono_testkit::scratch()`, the fixture style of
`xtask/tests/contracts.rs`) mutated to carry the exact defect the rule exists for — a ticked box
whose test function nobody wrote, whose file nobody wrote, whose test is `#[ignore]`d, whose
acceptance case the referee does not collect, whose `xtask` task does not exist, and a ticked box
naming no proof at all. A rule without such a companion is not delivered.

### 2. The rules live in `xtask/src/evidence.rs`, not in the test binary

The three earlier harvesters keep their parsing in the test file or in
`xtask/tests/support/mod.rs`. Neither home works here. A rule that has to run against a scratch
tree needs the repository root as a parameter, and `support`'s helpers close over the real
`repo()`; and a fourth copy of the box parser in a fourth test binary is precisely the duplication
v0.4.1 §39.1 forbids and `xtask::scan::check_duplicate_helpers` reports (ADR-0427, ADR-0515).

So the rules are library code — `checklist`, `checkboxes`, `proofs`, `check_ticked_proofs`,
`check_checklist_shape`, `check_case_block`, `progress`, each taking the root and the passage —
and `xtask/tests/temporal_evidence.rs` is the suite that applies them twice, to the repository and
to the mutants. They return `xtask::scan::Problem`, the same type every other repository rule
reports, so a later increment can chain them into `cargo run -p xtask -- spec-check` without
rewriting them.

### 3. A proof is a path, a case or a command, and each is resolved as far as it can be

* **A path** — anything under `crates/`, `xtask/`, `fuzz/`, `docker/`, `docs/` or `scripts/`, or
  a bare `file.rs` in the short form §4.8 uses — must exist. A trailing slash names a directory. A
  `::item` suffix must be declared as `fn item` in that file and must not be `#[ignore]`d. A `.rs`
  path must sit under a workspace member, or the gate never runs it.
* **A case** follows ADR-0401: backticked once the file exists, in prose while it does not. A
  ticked box's case must exist.
* **A command** is resolved to the claim it actually makes about the tree:
  `cargo run -p xtask -- <task>` says `xtask/src/main.rs` dispatches that task, and the harvester
  checks that much. What the run then *measures* is deliberately not resolved — a budget is the
  gate's verdict, not the harvester's — and the test that skips it says so.

### 4. The 230–279 block is exclusive in both directions, and citing an older case is not a breach

No case file numbered 230–279 may exist that §4.11 does not name, and no case §4.11 still owes may
be numbered outside the block. An earlier tranche's case named as a regression proof is left
alone: §4.11.11 cites v0.4's `060-performance-budgets` and `100-spatial-performance-budgets` for
"v0.4's current-state budgets do not regress", and those exist and are backticked, which is what
the convention asks of a case that has been written. The rule is therefore stated over what the
checklist *owes* — a case named in prose — rather than over every number it mentions.

## Consequences

* §4.11 cannot be ticked on a proof that does not exist, is not run by the gate, or is ignored;
  and the harvester's own ability to notice that is itself under test, which is what stops it
  becoming decoration during the many increments where it resolves nothing.
* The first increment to tick a §4.11 box gets its resolution checked for free, with no change to
  this suite.
* Boxes stay in the shape `scripts/release-check.sh` reads: at the left margin where `^- \[ \]`
  finds them, with a bolded title, under `#### 4.11.N` headings that run in order. An indented box
  would exempt itself from the stopping rule silently, which is the one way §4.11 could be
  "finished" without being done.
* The tranche's progress is printed rather than judged (`cargo test -p xtask -- --show-output`),
  because this suite runs in the gate on every increment and a tranche in progress must stay
  buildable.
* What it still cannot check: that a resolved test is a *good* test, or that a named command
  passes. Those are the gate's and the container's jobs, and the harvester says so where it stops.
* Encoded by `xtask/tests/temporal_evidence.rs` (26 tests, 14 of them mutation companions) over
  `xtask/src/evidence.rs`.

## Alternatives considered

**Reuse `support::assert_proofs_exist` over the whole passage, as §4.7 and §4.8 do.** Rejected: it
reports every undelivered file in a checklist that was deliberately written before its tranche, so
the tranche would begin with a red gate that no increment could turn green except by deleting the
checklist's detail. It also reads only the `file.rs::test` form, which is a minority of §4.11's
proofs.

**Keep the parsing in `xtask/tests/temporal_evidence.rs` and test the rules through the real
document only.** Rejected: with 123 open boxes there is nothing for the rules to resolve, so
every check would pass whether or not it worked. A harvester whose correctness cannot be
demonstrated on the day it is written will not be demonstrated later either.

**Put the shared parts in `xtask/tests/support/mod.rs`.** Rejected here: the module's helpers are
written against the real repository root, and giving them a root parameter would change what the
three existing harvesters do — a behaviour change dressed as a refactor (AGENTS.md §11). Library
code takes the root without touching them.

**Tick-independent case exclusivity over every number §4.11 mentions.** Rejected: it would report
§4.11.11's two v0.4 regression cases as cases numbered outside the block, which is the checklist
being right and the rule being wrong.
