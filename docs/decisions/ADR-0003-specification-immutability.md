# ADR-0003: The specification is enforced as immutable

- Status: accepted
- Date: 2026-08-26
- Spec refs: none — this is a process decision about the spec, not from it
- Decided by: user instruction, mechanism chosen by the agent

## Context

The initial specification is the fixed reference every later artifact is measured against. An
agent that edits it to resolve an ambiguity destroys exactly that: afterwards nobody can tell
what was specified and what was invented, and every ADR that cites a section becomes unreliable.

The risk is not malice. It is an agent thirty increments into a long run, finding a sentence
that contradicts what it just built, and "fixing" the document because that is the smaller
diff. A rule in a document is easy to forget at that moment. The rule needs to be checked.

## Decision

- `docs/ono_sendai_shell_spec_v0.2.md` MUST NOT be edited, amended, reformatted, renamed,
  regenerated or replaced by any agent, for any reason (AGENTS.md section 5.1).
- Ambiguities, silences, inconsistencies and outright errors in the spec are resolved through
  ADRs. An ADR that departs from spec text carries a `Spec deviation` heading naming the
  section, quoting the sentence and stating the replacing rule.
- `docs/spec.sha256` records the checksum of the specification. `cargo xtask spec-check`
  verifies it on every gate run, so any modification turns the gate red immediately rather than
  being discovered later in a diff nobody reads.
- The checksum is verified with `sha256sum` from coreutils rather than a hashing crate, which
  keeps the "no third-party dependencies yet" decision of ADR-0001 intact. It is present on the
  developer machine, in CI and in the runtime container.
- Restoring the file is the only correct response to a red immutability check. Updating
  `docs/spec.sha256` is the user's action, taken when they deliberately replace the spec.

## Consequences

- The set of ADRs carrying a `Spec deviation` heading is the complete, greppable list of every
  point where the product differs from its specification. That list is the input for the next
  spec revision, whenever the user chooses to make one.
- Agents cannot quietly resolve a contradiction by rewriting history; they must state the
  contradiction, choose, and leave the evidence in place.
- A spec that is wrong stays wrong in the repository until the user revises it. This is
  intended: an accurate record of what was asked for is worth more than a tidy document.
- `sha256sum` becomes a hard requirement of the gate. If a future environment lacks it, the
  check fails loudly rather than silently passing.

## Alternatives considered

- **State the rule in AGENTS.md only** — rejected: unenforced rules decay across a long
  autonomous run, and this one fails silently when it is broken.
- **A `sha2` crate dependency** — rejected: it would breach ADR-0001 for a check that a
  coreutils binary already performs.
- **A git pre-commit hook** — rejected: hooks are not installed by cloning, so the check would
  be absent exactly where an agent runs unattended. The gate is the thing every agent already
  runs.
- **Allow edits with a mandatory changelog entry** — rejected: it makes the reference mutable,
  which is the property being protected.
