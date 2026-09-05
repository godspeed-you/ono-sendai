# ADR-0031: A path compares with the string that spells it

- Status: accepted
- Date: 2026-08-26
- Spec refs: §10.6, §12.2; docs/contracts/language.yaml (regex literals)
- Decided by: agent (autonomous)

## Context

`get mount | where target == "/proc"` answered `[]` on every machine. Two facts collided:

- a mount's `target` is a `Path` value, and `compare_to` had no `Path` × `String` row, so the
  comparison was a type mismatch that `where` (correctly) treated as not-true;
- expression mode has no path literal — `/proc` reads as an unclosed regex, because `/…/` is the
  regex delimiter the language reserves (docs/contracts/language.yaml).

So there was **no way to write** the most ordinary of filters. Every spelling was either a type
error or a parse error.

## Decision

**A `Path` and a `String` compare as their text**, both directions, for ordering and therefore
for equality. The coercion is text-to-text only: `Bytes` stays incomparable with `Path` even
when the bytes happen to spell one, because spec §12.2 keeps bytes distinct from text.

No path literal is added to expression mode. `/…/` stays the regex delimiter; a quoted string is
the way a path is written in a comparison, and now it means what it says.

## Consequences

- `where target == "/proc"`, `where cwd == "/home/ada"` and every comparison like them work.
- Sorting a mixed column of paths and strings is deterministic and follows the text.
- One test pins each side: `crates/ono-value/tests/values.rs` — the equality both ways round,
  the ordering, and the refusal to equate bytes with a path.

## Alternatives considered

- **A path literal in expression mode.** Rejected: `/x/` is already the regex delimiter, and a
  disambiguation rule ("a slash-delimited token is a regex unless it looks like a path") is the
  shape-decides-structure heuristic ADR-0009 exists to forbid.
- **Coerce in `where` only.** Rejected: `sort` and `==` would disagree with `where`, and a
  comparison's meaning would depend on which command evaluates it.
