# ADR-0862: v0.6.2 is its milestone, and each issue is its requirement

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.6.1 §3.3, §23, §24, §31; AGENTS.md §5.2, §9; ADR-0425
- Decided by: agent (autonomous)

## Context

The user asked for v0.6.2 to be implemented "based on the existing v0.6.2 specification in the
repository". No narrative specification for v0.6.2 exists on any branch: `docs/specs/` ends at
v0.6.1 among the implemented releases, and no `docs/specs/ono_sendai_*spec_v0.6.2*.md` was ever
committed. What exists is the GitHub milestone `v0.6.2`, with this description:

> Verification Foundation. Scope: the test suite and acceptance harness; CI, packaging and release
> tooling; binary size and footprint. Purpose: establish a reliable test, build and release
> foundation before making larger semantic changes.

and 24 open issues: #124–#127 (binary size), #139, #141, #143, #145, #146, #151, #155, #160,
#162, #164–#166, #169, #185, #188, #189, #196, #204, #215 and #218. Each carries its evidence and,
in nearly every case, an explicit exit test.

Only the user adds a narrative specification (AGENTS.md §5.2), so an agent may not write one to
fill the gap.

## Decision

v0.6.2 is specified the way v0.6.1 §3.3 specifies a patch release, minus the narrative document:

1. **The milestone `v0.6.2` is the release inventory.** Its description is the scope statement.
2. **Each issue body is a normative requirement**, and its *Exit test* (or *What closes it*) is
   the acceptance criterion. Where an issue proposes a mechanism and also states an exit test,
   the exit test is binding and the mechanism is a suggestion an ADR may replace.
3. **v0.6.1's cross-cutting rules apply unchanged**: no new product capability beyond what an
   issue asks for (§24), the quality gates of §31, compatibility with v0.6.x behaviour (§32).
   #127's core build is the one issue that adds a build configuration; the default build stays
   the full product.
4. **The traceability record is `docs/releases/v0.6.2.md`**: every issue, the commits and ADRs
   that close it, the tests and acceptance cases that prove it, and the verification result.
5. The release is 0.6.2: the workspace version, the release note and the run record follow the
   v0.6.1 pattern.

## Consequences

- An issue closes only with the commit that meets its exit test (`Closes #NN`, AGENTS.md §9).
- A problem found on the way that is not in the milestone goes to `docs/STATE.md` → *Found, not
  yet filed*, not into the release (v0.6.1 §24's scope rule).
- Should the user later add a v0.6.2 narrative specification, it outranks this record (AGENTS.md
  §5), and reconciling the code with it is ordinary work.

## Alternatives considered

**Stop and ask for the specification.** The milestone states scope and purpose, and every issue
states its exit test; nothing a specification would add is needed to decide what "done" means.
AGENTS.md §8 forbids idling on a question the repository answers.

**Write a v0.6.2 specification.** Only the user adds one (AGENTS.md §5.2).
