# ADR-0979: A stated width is the width of every view

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.2 §4.6; v0.4 §23.2, §39.3; ADR-0166; issue #209
- Decided by: agent (autonomous)

## Context

ADR-0166 made the textual map honour `COLUMNS` "wherever it is stated, including for redirected
output", because a map's whole point is to fit, and left every other view at the fixed eighty
columns a redirected stream is laid out for. It rejected the general rule for one reason: "it
would change table output the rest of the suite pins at 80 columns, and that is a separate
change with its own tests."

The result was two answers to one question — how wide is the reader? — depending on which view
was asked (#209). Two suites had meanwhile been written as if the general rule held: they set
`COLUMNS` to 240 and 200 for redirected change views "wide enough to draw a whole line", and
the setting did nothing.

## Decision

**`COLUMNS`, where the environment states it, is the width of every view, on a terminal and off
one.** Without it a view on a terminal takes the terminal's width, and a redirected view is laid
out at eighty columns.

Determinism is unchanged in kind: v0.2 §4.6 asks that redirected output not depend on who is
watching, and it does not — it depends on the environment the command was given, as it already
did for the locale and `NO_COLOR`. A run with no `COLUMNS` in its environment prints what it
printed before.

This supersedes the map-only clause of ADR-0166; the rest of ADR-0166 stands.

## Consequences

Easy: `COLUMNS=160 ono -c 'get service' > report.txt` is a report 160 columns wide; a pipeline
into `less -S` can be told how wide to be; the map and the table beside it agree.

Hard: a script that exports `COLUMNS` — an interactive shell that passes it on, a CI runner that
sets it — gets redirected output at that width where it got eighty. Golden files recorded with
`COLUMNS` in the environment and compared without it, or the reverse, differ. The acceptance
harness's `columns:` header sets `COLUMNS` for the whole case, so a redirected command inside
such a case is now laid out at the case's width.

Encoded by `crates/ono-cli/tests/redirected_width.rs` and, for the map, by the tests ADR-0166
names.

## Alternatives considered

**Make the map follow the others** — eighty columns off a terminal whatever `COLUMNS` says. One
rule as well, and it takes away the only way to draw a redirected map that fits a narrow
reader, which acceptance case 105 proves at forty columns (§39.3).

**Leave the two rules.** ADR-0166 recorded the asymmetry as provisional, and two test suites
had already assumed it away.

**A dedicated variable or flag (`--width`).** `COLUMNS` is what every terminal program reads
for exactly this, and a second spelling would have to say which of the two wins.
