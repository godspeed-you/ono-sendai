# ADR-0429: A release definition becomes a checklist by naming its proof

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §66.1–§66.9 (the Release Definition), §3.1–§3.4 (priority classes), §40.3
  (the fourteen acceptance families), §52.2 (single source of truth), §57 (the H0–H12 sequence),
  §58 (work-package shape), §59–§62 (the acceptance scenarios), Appendices A–J;
  AGENTS.md §9, §15; `docs/ACCEPTANCE.md` §3, §4.7
- Decided by: agent (autonomous)

## Context

`docs/ACCEPTANCE.md` §4.8 had to be written before the v0.4.1 tranche started, because it is what
"finished" means for the other hundred issues (#29). Its source is v0.4.1 §66, a nine-part
Release Definition written as prose bullets — *"Captures use shared budgets"*, *"no silent test
skips remain in covered patterns"*, *"the exact tested bytes are the published bytes"*. Each is a
true statement about a finished product and none of them is a check. Turning them into boxes
raised five questions the specification does not answer, and this ADR records the answers so the
hundred increments that fill the boxes read them the same way.

The material was: §66's nine criteria, §40.3's fourteen acceptance families, §59–§62's
twenty-six acceptance scenarios, Appendix A's twenty-two default limits, Appendices D, E, F, G, H
and I, and 101 open issues carrying the milestones H0 … H12. §4.7 — the v0.4 tranche — was the
model for form and voice, and its rule carries over unchanged: a box is ticked by a named
automated proof, never by judgement and never by reading code.

## Decision

**§4.8 is organised by the phase sequence of §57, and each box is closed by naming the proof, the
issues that deliver it and the specification sections it comes from.** Five sub-decisions follow.

**1. The spine is §57, and §66 is the coverage obligation.** §66's nine criteria are the release
definition, and they are not a work breakdown: §66.1's eight bullets are delivered by four
different phases across thirty-four issues, and §66.7's nine bullets by two. Ordering the
subsection by §66 would have produced boxes nobody could pick up, because the unit an agent picks
up is a work package (§58) and a work package belongs to a phase. So §4.8.1–§4.8.12 follow H0 …
H12, §4.8.13 holds the fourteen acceptance families of §40.3, and §4.8.14 holds §66.9. §66 is not
lost by this: §4.8.14's third box requires that every bullet of §66.1–§66.8 has at least one box
naming its proof, checked mechanically by reading §66 out of the specification and §4.8 out of
this file. The criterion that loses its box fails the gate.

**2. Every issue of the tranche is named in the box that closes it, and every box names at least
one issue.** The 101 issues carry milestones H0 … H12 and the phase, priority and normative
sections in their first line. §4.8 cites the issue number in each box, so an agent that reads a
box can `gh issue view` the work package behind it, and a reader of the tracker can find the box
their issue closes. Three boxes name several issues because the proof is one thing: §4.8.4's
revocation box, §4.8.9's flake box and §4.8.10's crate-graph box, which closes #95, #96 and #97
together because "no new dependency inversion" is a statement about the three refactors jointly.

**3. Priority is written on the box, and it decides what may stay open — never what may be
judged.** Each box opens with `P0`, `P1`, `P2` or `P3` from §3.1, taken from the issue that
delivers it. §3.2 and §3.3 make P0 and P1 mandatory; §3.4 makes P2 and P3 part of the product
contract as well, with a release candidate allowed to be cut while they are in flight. What the
priority never does is weaken the evidence rule: a P3 box is closed by a named test exactly as a
P0 box is. §4.8.14 states the one asymmetry — a P2 or P3 item may be excluded by an ADR written
before release-candidate freeze, and no such ADR may waive a §66 criterion (§66.9).

**4. Acceptance-case numbers 180–200 are reserved for the tranche and ascend with the phase
sequence.** The suite's highest number today is 171. `xtask`'s reference check treats a backticked
`NNN-kebab-name` below the highest existing number as a claim and reports it when the file is
absent, and treats a number above it as prose (`xtask/src/scan.rs::check_acceptance_case_references`,
ADR-0401). Numbering the twenty-one new cases above 171 therefore keeps the gate green while none
of them exists, and numbering them in phase order keeps it green as they land one at a time: the
case that lands leaves no lower number pointing at a file nobody wrote. An increment forced to
deliver out of that order writes the case name without backticks until its file exists, which is
the same check's documented way of recording a name as absent.

**5. Where §66 states a document, the box names the gate check that fails when the document and
the tree disagree.** Four of §66.8's five bullets and two of §66.1's are about documentation, and
a box that said "the README is accurate" would be closed by an opinion. Each is closed instead by
a check over the repository — `xtask/tests/terminology.rs` for the §19 terminology contract,
`xtask/tests/metrics.rs` for the generated counts, `xtask/tests/reference.rs` for the migration
commands resolving against the command registry, `xtask/tests/provenance.rs` for the verification
sequence being executed against a release fixture rather than printed. The same reasoning gives
§4.8.1 the boundary-inventory box: §20's security acceptance principle becomes checkable when
every boundary of §6.1 names an owning crate and a security test.

Where a proof already exists, the box names it rather than a name the checklist would have
preferred. The three H10 pinning boxes of §4.8.11 cite `xtask/tests/supply_chain.rs` and the test
names ADR-0433 delivered while this subsection was being written, and the four other workflow
boxes name the same file, so the repository keeps one scanner for what a workflow declares.

Two consequences of the form itself are worth stating, because they are easy to undo:

- **Every box is open.** `scripts/release-check.sh` greps this file for `- [ ]` and fails on the
  first one, so writing §4.8 makes the release gate red on purpose. That is the correct state for
  a tranche that has just started, and the reason §4.8 is written first rather than last.
- **§4.7's evidence harvester now stops at `### 4.8`.** `xtask/tests/spatial_evidence.rs` read
  from `### 4.7` to `## 5. Stopping rule` and would have held the v0.4 evidence against the tests
  §4.8 asks future increments to write — reporting §4.7 as rotten when it is whole. Bounding the
  passage at the next tranche's heading is the whole change; the v0.4.1 boxes are held by their
  own harvester, `xtask/tests/hardening_evidence.rs`, which is §4.8.1's first box and the last box
  of the tranche to close.

## Consequences

Easy: an agent picking up any of the 101 issues finds the box its work closes, the file and test
name it has to create, the specification sections it implements and the Appendix A figure it must
assert. A reviewer can ask one mechanical question of the finished tranche — is every box ticked,
and does every proof named resolve — and get an answer from `scripts/release-check.sh` rather than
from a reading.

Hard: §4.8 names about two hundred and sixty tests, most of which do not exist yet, so it is a set of
promises until `xtask/tests/hardening_evidence.rs` exists to resolve them. Until then a wrong file
name in a box is caught by the increment that writes the test rather than by the gate, which is
the same position §4.7 was in while it was being filled. The mitigation is the ordering:
§4.8.1's first box is written early in H0 and turns every later pointer into something the gate
follows.

Also hard: the case-number reservation is a discipline rather than a mechanism. It holds while
phases land in order, and an out-of-order delivery has to use the plain-name form deliberately.

Encoded by: `docs/ACCEPTANCE.md` §4.8 (118 boxes across fourteen subsubsections), and
`xtask/tests/spatial_evidence.rs::should_find_every_test_the_v04_checklist_names_as_a_proof`,
which still passes because the §4.7 passage now ends where §4.8 begins.

## Alternatives considered

**Organise §4.8 by §66.1–§66.9** — the release definition is the source of the boxes, so mirroring
it looks obvious. It puts §66.1's eight bullets, delivered by four phases and thirty-four issues,
into one subsubsection, and it leaves a reader of an H3 issue with no subsection to look in. The
coverage obligation of §4.8.14's third box gives §66 what mirroring would have given it, without
the cost.

**One box per issue, mechanically** — 101 boxes, one per work package, would be traceable and
would say nothing about whether the *product* is finished: several issues deliver one provable
behaviour, and §66 states four criteria (the crate graph, the status documents agreeing, the
§66.9 rules) that no single issue owns. The boxes here are proofs, and an issue is what produces
one.

**Write §4.8 after the tranche, from what was built** — this is how a checklist becomes a
description of the work instead of a definition of it, and #29 exists to prevent exactly that.
`docs/STATE.md` schedules it first for the same reason.

**Leave the case numbers unassigned until each case is written** — it removes the ordering
discipline and removes the map: an agent in H5 would have no way to know whether the materialization
case already exists under another number. Reserving the block costs one rule and buys the
subsection its acceptance-family boxes.
