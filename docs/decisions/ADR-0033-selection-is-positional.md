# ADR-0033: Interactive selection is positional addressing, not a cursor

- Status: accepted
- Date: 2026-08-26
- Spec refs: §6.4, §13.4, §20.2, §37 E, §37 J
- Decided by: agent (autonomous)

## Context

Phase E's deliverable list (§37) names "interactive table selection". The defining section is
§13.4, and it is written in MAY: a rendered collection "MAY expose an ephemeral selection
cursor", selection "MUST never change pipeline data by itself", and "actions based on selection
require explicit input such as `enter`, `inspect`, or `@` reference". §6.4 adds that its `@`
syntax is "intentionally marked open for validation" and that "the semantics matter more than
the exact token."

## Decision

**Selection in Phase E is positional: `@N` names row N of the last shown result, `@-1` names the
previous result, and both are ordinary values a pipeline can start with.** Every row a table
shows is numbered by its position, so anything visible is addressable — `@2 | inspect`,
`@3 | kill process` — which is exactly the "explicit input" §13.4 requires an action to have.

**The ephemeral visual cursor is Phase J work.** §37 J exists for "advanced TUI views, only
where semantics justify them"; a cursor is presentation over the same addressing, changes no
semantics, and building it now would put TUI machinery ahead of the phases that deliver
capability. Nothing about `@N` will change when a cursor arrives: the cursor will *set* the
selection `@` refers to, as §6.4 sketches.

The §13.4 invariants hold in this form trivially: positional addressing cannot change pipeline
data, and every action on a selection is an explicit command the user typed.

## Consequences

- Objects can be investigated without restating selectors — the Phase E criterion — with what
  is already keyboard-first: run, look, `@2 | inspect`.
- `@` bare (the cursor's referent) stays a structured error naming what would give it meaning.
  When Phase J adds the cursor, that error stops being reachable interactively.
- The retained results that make this work are bounded by count and by values per result;
  retention of secret-bearing values follows the policy work of spec §17.5 and is on the board.

## Alternatives considered

- **Build the cursor now.** Rejected: §13.4 makes it optional, §37 J claims it, and a cursor
  without the rest of J's TUI investment would be a toy that still needs `@N` underneath.
- **No selection until J.** Rejected: it would leave "actions based on selection" with no
  explicit input form at all, and §20.2's result reuse is Phase E's own deliverable.
