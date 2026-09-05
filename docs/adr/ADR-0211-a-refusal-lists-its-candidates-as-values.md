# ADR-0211: A refusal lists its candidates as values, not as newlines in its message

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.4 §27.2, §29.3, §40; v0.2 §16.1, §16.2, §49; ADR-0015 T1
- Decided by: agent (autonomous, `S11c`)

## Context

`docs/dogfood/v0.4-2026-08-28.md` finding 4. `follow files` at a process holding several files
printed its whole candidate list on one line:

```text
ono: Ono-Sendai-E1002 spatial.ambiguous_selector `files` reaches 3 places:\u{a}  /dev/null  ono:stable:962e…\u{a}  /tmp/…/held.conf  ono:stable:7f49…
```

The refusal is right and the list is what §27.2 and §29.3 ask for. What was wrong is the render
boundary: `ono_render::sanitise` escapes **every** control character, newlines included, and
ADR-0015 T1 is why — "a table cell holding `evil\nroot 1` then rendered as two terminal lines,
the second of them indistinguishable from a real row". A filename may contain a newline, and
acceptance case `048` holds that line.

So the message carried two kinds of newline that the boundary could not tell apart: the ones the
shell wrote between candidates, and the ones a name might bring with it. Escaping both is what
made the list unreadable; escaping neither would let a filename forge a line.

## Decision

**The shell does not write its structure into a string. It carries it as a value.**

A refusal that lists things puts them in the error's metadata under `details`, a list of strings,
and keeps its message to the one-line summary that says how many there are. Concretely:

- `Resolution::require`'s ambiguity and fuzzy refusals (`ono-spatial-query`) and `ambiguous_edge`
  (`ono-cli`) build `details` instead of a `\n`-joined message;
- `Reporter::error` prints the message, then each `details` entry on its own line indented by
  two, **each sanitised on its own**, then the help.

The two kinds of newline are then never in the same string: the structure is the list, and the
data lives inside its elements, where the existing blanket escape still applies unchanged. A
candidate whose display name carries an escape byte or a newline is still shown as `\u{1b}` and
`\u{a}` and cannot forge a line of its own. Nothing about `sanitise`, about case `048`, or about
any other message changes — a `\n` that reaches the reporter inside a message is still escaped,
because the reporter has no way to know who put it there.

The renderer shows at most ten entries and then `… N more`. The count is already in the message,
the whole list stays on the error value for a script that catches it, and a diagnostic that fills
the screen with ninety rows is not a diagnostic (§2.9's bounded horizon, applied to a refusal).

## Consequences

- `catch e { $e }` now reads the candidates as a real list instead of parsing a paragraph, and
  `$e | to json` carries them under `metadata.details` — a gain §40 asks for and the old shape
  could not give.
- `details` is a convention on `ErrorValue`, not a new field: any refusal that has a list to show
  can use it, and the reporter needs no knowledge of which refusal it is looking at.
- Two unit assertions in `crates/ono-spatial-query/tests/resolution.rs` read the candidate rows
  out of the message and the help; they now read them out of `details`. That is the assertion
  change a contract decision is allowed to make, and it lands in this commit.
- The tests that encode it:
  `crates/ono-cli/tests/spatial_relationships_missing.rs::should_break_a_listing_refusal_into_lines_while_still_escaping_what_the_names_carry`
  (both halves in one test, because separating them is the defect), the two updated resolution
  tests, and acceptance case `103` assertion `s4b-b5`.

## Alternatives considered

- **Let the reporter split the message on `\n` and sanitise each line.** Rejected: it gives a
  filename the power to add a line to a diagnostic, which is ADR-0015 T1 in the place where
  attacker-controlled text most reliably reaches a terminal.
- **Sanitise the embedded names where the message is built, then split on `\n` in the reporter.**
  Sound, but it needs the sanitiser in `ono-spatial-query`, which has no business depending on a
  renderer — and it leaves the invariant "this string's newlines are trustworthy" as a rule
  nothing enforces. Carrying the list as a list makes the same guarantee structural.
