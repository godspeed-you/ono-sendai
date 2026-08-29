# ADR-0137: What closes a v0.4 release box

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §52 (release criteria), §52.1, §52.2, §52.3, §44, §43, §34, §2, §35, §50 S11;
  AGENTS.md §10, §15; `docs/ACCEPTANCE.md` §3, §4.6, §5
- Decided by: agent (autonomous)

## Context

`docs/ACCEPTANCE.md` §4 is the machine-checkable definition of finished, and until now it ended
at §4.6, the v0.3 tranche. The v0.4 Spatial Systems Interface was therefore invisible to
`scripts/release-check.sh`: the run could have ended with 175 ignored tests and ten unrun
scenarios in the tree. §4.7 closes that hole.

Writing it forced three questions §4.6 answered only by example.

1. v0.4 §52.2 asks for a **security review** and §52.3 for **dogfooding**. Neither is a test.
   `docs/ACCEPTANCE.md` §3 and AGENTS.md §15 forbid a box that judgement alone can tick.
2. v0.4 §52.2's second bullet — "unit/property/integration/PTY tests pass" — is one sentence
   over the four test layers of §43.1–§43.4, each with its own checklist of required coverage.
   One box would hide four unfinished checklists behind one tick.
3. Several §52 requirements have no candidate proof in the tree at all: the §34 budgets have no
   container case, and nothing mechanically stops a `.case.v04` scenario from being left out of
   the suite for ever, because the referee cannot see a file it does not collect.

## Decision

**1. Evidence for a requirement no test can prove is named in advance, and a test guards the
evidence.** For the two such requirements in v0.4:

- *Security review* (§52.2, §51 SEC-S01): the review is an ADR that extends the T1–T15 threat
  table of ADR-0015 with one row per §35 boundary (§35.1–§35.5), **each row naming a passing
  test**. `xtask/tests/spatial_evidence.rs` asserts that every test the table names exists and
  is not ignored. The reviewer's judgement chooses the rows; the suite closes the box.
- *Dogfooding* (§52.3): a session of at least an hour on a host the author did not prepare,
  recorded as `docs/dogfood/v0.4-<date>.md` — what was asked, what the shell answered — with
  every defect it produced filed in `docs/STATE.md` and either fixed or deferred with an ADR.
  The box is ticked when the record exists, its defects are closed or deferred with a reason,
  and cases `090`–`099` are green. The scenario half of §52.3 is guarded mechanically: the same
  `xtask` test asserts the README-v0.4 house rule that no case types the name of the object it
  is supposed to discover, which is what makes those cases evidence for the *qualitative*
  statement rather than only for §44.

**2. A one-sentence criterion covering several checklists becomes one box per checklist.** §52.2
bullet 2 is §4.7.2's four boxes for §43.1 (thirteen unit areas), §43.2 (seven properties), §43.3
(the nine fixture elements) and §43.4 (the nine PTY checks). The mapping is stated in §4.7.2 so
the count difference is not mistaken for an invention. For the record: **§52.2 lists nine
bullets, not ten**, and §4.7.2 holds thirteen boxes — the nine, with bullet 2 expanded to four,
plus §52.3.

**3. The tranche must build three artifacts that do not exist yet**, and §4.7 names them so the
delivering increment knows they are part of the work, not a later chore:

- `xtask/tests/spatial_evidence.rs` — the drift guard, modelled on `xtask/tests/adapter_evidence.rs`:
  no `*.case.v04` left behind, every §43.1 area covered, every security-review row's test real,
  no case naming its own discovery target.
- `docker/acceptance/cases/100-spatial-performance-budgets.case` — the §34 budgets measured in
  the container at their real figures, in the shape of `060-performance-budgets`. The two
  in-suite timing tests use a ten-times tolerance on purpose (a wall-clock assertion that tight
  is flaky on shared hardware); they keep the gate honest against catastrophic regressions and
  explicitly do **not** tick a §4.7.5 box.
- `docs/dogfood/v0.4-<date>.md` — the record above.

**4. No box is ticked by a phase report.** v0.4 §50's phases (S1–S11) order the work;
`docs/STATE.md` tracks them. A completed phase never ticks a box in §4.7 — only a named test
running un-ignored in the gate, or a case running in the container, does.

## Consequences

- `scripts/release-check.sh` needs no change: it greps `docs/ACCEPTANCE.md` for `^- \[ \]` and
  fails on the first hit, so §4.7's 66 boxes entered the stopping rule the moment they were
  written. §5 now says so, so no later subsection is added in the belief that it must be
  registered somewhere.
- The run cannot end while any of the 175 spatial tests is ignored or any scenario still carries
  `.case.v04`: the first is caught by the *no release-blocking defects* box and `spec-check`'s
  unfinished-work scan, the second by the *acceptance scenarios pass* box and its `xtask` guard.
- Boxes name tests that do not exist yet. That is deliberate — a definition of done written from
  the specification, not from the code — but it means a box's named proof may be renamed by the
  increment that writes it. Renaming a proof in §4.7 is part of that increment, exactly as
  updating `docs/spec/` is.
- Three artifacts are now required work items. If a later increment finds a better shape for one
  of them, it supersedes this ADR rather than quietly dropping the requirement.

## Alternatives considered

- *Leave §52.2's review and §52.3 as prose outside the checklist.* Rejected: AGENTS.md §15 makes
  an unticked box the reason the run continues, and a requirement outside the checklist is a
  requirement nothing enforces.
- *One box per §52 bullet, no more.* Rejected for bullet 2 only, for the reason in Decision 2;
  everywhere else §4.7 keeps the bullet-to-box mapping one to one.
- *Tick the security review from a checklist inside the ADR itself.* Rejected: a checklist a
  human ticks is judgement with extra steps. The rows must name tests the gate runs.
- *Add a `## 4.7` scan line to `scripts/release-check.sh`.* Unnecessary — the grep is generic —
  and a per-section list would be one more place to forget a tranche.
