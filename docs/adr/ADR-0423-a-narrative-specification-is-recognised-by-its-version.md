# ADR-0423: A narrative specification is recognised by its version, not by `shell_spec`

- Status: accepted
- Date: 2026-08-31
- Spec refs: v0.5 §0; AGENTS.md §5.1, §5.2; supersedes the naming rule of ADR-0026
- Decided by: agent (autonomous)

## Context

ADR-0026 made the specification a set — one base plus enhancements — and gave `spec-check` two
guarantees over that set: every narrative specification has a `sha256sum` line, and `AGENTS.md`
enumerates every enhancement by name. It closed with "the set can keep growing; nothing in the
rules is specific to `v0.3`."

That was true of the rules and false of the discovery. `xtask/src/narrative.rs` found a
specification by testing its file name for the substring `shell_spec`, and
`xtask/src/scan.rs` exempted the immutable documents from the acceptance-case reference check by
testing for the prefix `docs/specs/ono_sendai_shell_spec_`. Both matched the shape of the three names
that existed rather than the thing being named.

On 2026-08-31 the user added `docs/ono_sendai_spec_v0.5_temporal_causal_systems_interface.md` —
the Temporal & Causal Systems Interface, 4 147 lines. The name dropped the `shell_` element. The
consequences were exactly the two ADR-0026 was written to prevent:

- the document had no checksum line, so nothing would notice it being edited;
- no instruction file named it, so no agent had a reason to read it;

and this time neither was discovered by a red gate, because the guard did not see the file at
all. `scripts/gate.sh` printed `gate: green` over a repository holding an unguarded, unread
specification. A guard that goes quiet exactly when the thing it guards against happens is worse
than no guard: ADR-0026 records that the first enhancement was found by a failing build, and the
second by nothing.

The v0.4 document arrived the same way and was *renamed* on `main` to add the infix. That is a
fix nobody can rely on and one no agent may perform — AGENTS.md §5.1 forbids renaming a
narrative specification, so the harness cannot depend on the user having named it a particular
way.

**Postscript, same day.** While this decision was being written the user renamed v0.5 on `main`
too (`c4ca548`, content untouched), so all four documents carry `shell_spec` again and
`docs/specs/spec.sha256` follows the new path. That does not restore the old rule. It is the second
time in two enhancements that the harness was made correct by hand after the fact, which is the
argument for a rule that does not need the hand.

## Decision

**A file in `docs/` is a narrative specification when its name starts with `ono_sendai_`,
contains `spec_v` and ends in `.md`.** The product name and the version are what every one of
these documents carries; the words between them are the user's prose and are not load-bearing.

**The base is the lowest version, not the name that sorts first.** `narrative_specs` orders by
the `(major, minor)` parsed out of the name and falls back to the name only to break a tie; a
name announcing no version sorts last, because nothing unversioned can be the base. The previous
order was lexicographic, which put `ono_sendai_shell_spec_v0.2.md` first by the accident that
`h` precedes `p` — correct today, and correct for no reason.

`is_narrative_spec` is one predicate, exported from `narrative` and used by `scan` as well, so
the discovery rule and the immutability exemption cannot drift apart again.

The wording follows the code: AGENTS.md §5.2, README.md and the `spec-check` restore hint now
say `docs/ono_sendai_*spec_v*.md` where they said `docs/*_shell_spec_*.md`.

## Consequences

- With discovery widened, `spec-check` went red against the tree for the two real defects, and
  they were fixed in the same increment: the v0.5 checksum line is in `docs/specs/spec.sha256` and
  AGENTS.md §5.2 enumerates the document. The gate is green again for a reason.
- v0.5 is under the guard but not implemented. `docs/STATE.md` records it as the next tranche;
  `docs/ACCEPTANCE.md` has no §4.8 yet, and writing one from v0.5 §48 and §56 is that tranche's
  first task.
- Two tests in `xtask/tests/scan.rs` asserted the case-reference exemptions with names numbered
  above the highest case in their fixture. The check skips those as prose rather than as
  exemptions, so both passed no matter what the exemption did. They now use in-range names and
  fail when the exemption is removed.
- Recording a checksum for a document the agent did not write remains what ADR-0026 said it is:
  bringing a file *under* §5.1, not out from it.

## Alternatives considered

- **Rename the file to match the guard**, as happened for v0.4. Rejected: AGENTS.md §5.1 forbids
  an agent renaming a narrative specification, and a rule the user must remember to spell
  correctly is the defect, not the cure.
- **Enumerate the specification file names in the harness.** Rejected: it makes adding an
  enhancement a code change, and a list nobody updates fails the same silent way.
- **Match every `docs/*.md` that is not a known harness document.** Rejected: `STATE.md`,
  `ACCEPTANCE.md` and the ADRs would each need an exception, and the next document added to
  `docs/` would be demanded as a specification.
