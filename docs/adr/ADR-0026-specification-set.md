# ADR-0026: The specification is a base plus enhancements

- Status: accepted
- Date: 2026-08-26
- Spec refs: §0; AGENTS.md §5.1, §14
- Decided by: agent (autonomous)

## Context

The harness was built when `docs/` held exactly one narrative specification, and `spec-check`
encoded that as a rule: *"several narrative specifications found; exactly one is authoritative"*.

The user then added `docs/specs/ono_sendai_shell_spec_v0.3_external_command_adapters.md` beside the
base. The addition is deliberate — its filename says it enhances one subsystem, not that it
replaces the whole document — but the gate refused it, and CI on `main` went red. AGENTS.md §14
puts the referee ahead of the feature, so the rule had to be decided properly rather than worked
around.

Two things went wrong beyond the red gate, and both matter more than the rule itself:

- the new specification carried no entry in `docs/specs/spec.sha256`, so the immutability guarantee of
  §5.1 did not cover it at all;
- no instruction file mentioned it, so no agent had any reason to read it. It was found by a
  failing build, which is not a mechanism anyone should rely on.

## Decision

**The specification is a set: one base plus zero or more enhancements.**

- The **base** is the earliest by name, today `ono_sendai_shell_spec_v0.2.md`. It governs
  everything an enhancement does not speak about.
- Every other `docs/*_shell_spec_*.md` is an **enhancement layered on the base**. Where the two
  overlap, the later version wins, and the ADR that implements the overlapping part cites both
  sections.
- **All of them are immutable** under AGENTS.md §5.1. There is no such thing as a narrative
  specification an agent may edit.

`spec-check` enforces three rules, each of which failed to hold when the first enhancement
arrived (`xtask/src/narrative.rs`, tested in `xtask/tests/narrative.rs`):

1. at least one narrative specification exists;
2. **every** narrative specification has a `sha256sum` line in `docs/specs/spec.sha256` — a file with no
   entry is a file nothing would notice being edited;
3. the instruction files name the base, and `AGENTS.md` additionally names **every** enhancement —
   an enhancement the authoritative instruction set does not enumerate is one no agent reads.

Rule 3 is the one that would have surfaced the new document immediately, and by name.

## Consequences

- Adding an enhancement specification stays the user's action. Recording its checksum and
  enumerating it in `AGENTS.md` becomes the agent's first task afterwards, ahead of feature work,
  because until both are done the gate is red.
- Recording the checksum of a specification the agent did not write is not a weakening of §5.1.
  The checksum is what makes the file unwritable; adding the line brings the new file *under* the
  guard rather than out from it. AGENTS.md §14 reserves `docs/specs/spec.sha256` for the user in the
  case it was written for — a deliberate *replacement* of the base — and that case is unchanged:
  an agent never rewrites an existing line, only appends one for a file it found unguarded.
- The set can keep growing. Nothing in the rules is specific to `v0.3`.
- CI on `main` stays red until the harness fix reaches it. Promoting `implementation` is the
  user's decision (AGENTS.md §12.1), so the fix waits there; `implementation` itself is green.

## Alternatives considered

- **Treat the newest specification as the only authoritative one.** Rejected: the new document
  covers external command adapters and is silent on the language, the pipeline, KUANG/11 and
  everything else. Reading it as a replacement would discard the base and, with it, the product.
- **Concatenate them into one file.** Rejected outright: it would mean writing to a narrative
  specification, which §5.1 forbids absolutely.
- **Relax `spec-check` to ignore extra specifications.** Rejected: that is the version of this
  change that makes the gate green and the project worse. The two real defects — an unguarded
  file and an unread file — would both have survived it.
