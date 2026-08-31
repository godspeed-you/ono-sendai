# ADR-0425: The backlog is the issue tracker; `docs/STATE.md` is the inbox

- Status: accepted
- Date: 2026-08-31
- Spec refs: AGENTS.md §4, §8, §9, §16; docs/ACCEPTANCE.md §4.5, §4.6.5, §4.7.2; ADR-0402
- Decided by: the user

## Context

`docs/STATE.md` was the whole memory of an autonomous run. AGENTS.md §9 made it the board an
agent reads first and writes last, and its *Next up* section grew into the backlog: 27 open boxes
on 2026-08-31, each carrying a reproduction, the files and line numbers it touches, the measured
figures, the ADRs around it and the exit test that closes it. That was the right shape while the
repository was a closed loop between one user and a fleet of agents with no other surface.

It has two defects, and both had already fired by the time this was written.

**The board duplicates itself, and the duplicates go stale.** The §37 phase lists carry a box per
requirement, and *Next up* carries a class entry for the same requirement, each pointing at the
other in prose — "open, and it is C-1 above". When C-1 and C-7 were closed on 2026-08-29 by
ADR-0331 and ADR-0332, their class entries were ticked and the two phase boxes that named them
were not. The board reported two capabilities as missing that had shipped, for two days, in the
file whose entire job is to say what is missing. Nothing caught it: `state-check` (ADR-0402)
checks that *In progress* is empty and that every *Deferred* entry names an ADR, and reads
nothing else.

**A board inside the repository is invisible from outside it.** The repository is public.
`README.md` sent a reader after "known issues and the backlog" into a 3 253-line working
document, ordered by the run rather than by what a newcomer could pick up, and interleaved with
session records that are addressed to the next agent.

The alternative failure is worse and is why this ADR exists rather than a one-line rule. On
2026-08-31 the 27 open boxes were filed as issues #1–#27 with their evidence copied into the
issue bodies. That produced two backlogs holding the same 27 facts, with commit hashes and line
numbers in both — and **no step in the working loop touches GitHub**. An agent reads STATE.md,
ticks a box and commits; the issue stays open with a body that is now wrong. Dual maintenance
here does not risk drift, it guarantees it.

## Decision

**The GitHub issue tracker is the backlog of record.** One problem is one issue. Its evidence —
reproduction, files, measurements, ADRs, exit test — lives in the issue body and nowhere else.

**`docs/STATE.md` holds only what has no issue yet.** Its *Next up* section is replaced by
*Found, not yet filed*: a staging area for a problem an agent runs into while doing something
else, written down where AGENTS.md §4 forbids fixing it in the commit that found it. An entry
there is not work anybody may pick up. **The user decides when an entry becomes an issue**, and
filing it removes it from the board — so a problem is on exactly one of the two surfaces, never
both.

**What stays in `docs/STATE.md`** is what an issue tracker cannot hold: *In progress* claims
(AGENTS.md §13), *Deferred / blocked* with its ADR per entry, the session records that carry the
reasoning behind a decision the code no longer shows, the §37 phase lists, and the historical
*Done* record. The two properties ADR-0402 checks are unaffected; both sections it reads survive
unchanged.

**Task selection reads the tracker.** AGENTS.md §9's ordering rules now apply to open issues.
They are unchanged in substance — phase sequence, then what unblocks the most, then contracts
before implementations, then a broken referee outranks features — and only their subject moves.
An agent that closes an issue says so in the commit body (`Closes #NN`), which is the one place
the loop already writes prose that GitHub reads.

**`docs/ACCEPTANCE.md` remains the stopping rule**, untouched by this. It is not a backlog: §4 is
the list of what must be true before release, proven by named automated tests, and no issue may
tick one of its boxes.

## Consequences

Easy now: a reader sees the real backlog, labelled and searchable, without reading a working
document. A problem has one home, so it cannot be closed in one place and left open in another.
The staleness that hit the C9 and theme boxes needs a duplicate to exist, and after this there is
none to have.

Harder now: an agent needs `gh` and network access to see the backlog, where before it needed
only the checkout. A run in an offline worktree can still work — `docs/ACCEPTANCE.md` is the
stopping rule and is in-tree — but it cannot choose its own next task. This is accepted
deliberately: the user's model is that an agent is given its issue, and *Found, not yet filed* is
explicitly not self-service.

Also harder: the evidence in an issue body is not checksummed and no gate reads it, where a
STATE.md entry sat in a file `spec-check` walks. The mitigation is that issues are short-lived
against a board entry's lifetime — filed when the user triages, closed by the commit that fixes
them — and that anything which must survive is an ADR, which is in-tree and always was.

To revisit: if the tracker and the phase lists start restating each other the way *Next up* and
the phase lists did, the answer is generation, not prose — the discipline of ADR-0018 and
ADR-0331, where the derived surface is produced from the source and a gate fails on drift.

The 27 issues filed on 2026-08-31 are what this ADR regularises; `docs/STATE.md` gives up those
27 entries in the same commit, so the two surfaces are consistent from the moment the rule
exists.

## Alternatives considered

**Close the issues, keep `docs/STATE.md` as the only board.** The status quo ante, and coherent:
it is what AGENTS.md §9, README.md §288 and CONTRIBUTING.md already described, and it needs no
network in the loop. Rejected because it keeps the internal duplication that produced the stale
C9 and theme boxes, and leaves a public repository whose backlog is only legible to someone
willing to read a working document end to end.

**Generate the issues from `docs/STATE.md`** — an `xtask issues` that opens, updates and closes
from the boxes, with `spec-check` failing on drift. This is the repository's own idiom for a
derived surface (ADR-0018, ADR-0331) and would have kept a single source. Rejected as premature:
it is a tranche of work — box parsing, a stable box↔issue mapping, reconciliation of edits made
on either side — to automate a flow whose shape is not settled yet. It stays the answer if the
tracker and the board start restating each other.

**Thin issue bodies pointing back at `docs/STATE.md`.** Cheap, and it keeps every fact in one
place. Rejected because it only halves the problem: no fact drifts, but nothing closes the issue
when the box is ticked, so the tracker fills with resolved issues and stops being a backlog.
Every middle position between the two surfaces has this shape — the facts can be moved, the
lifecycle cannot be split.
