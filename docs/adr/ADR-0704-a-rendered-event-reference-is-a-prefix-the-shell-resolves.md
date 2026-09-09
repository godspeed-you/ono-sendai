# ADR-0704: A rendered event reference is a prefix the shell resolves

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §11.5, §11.6, §12.2, §16.2, §34 (`temporal.ambiguous_event`), §39.3;
  v0.4 §39.3; ADR-0620, ADR-0660
- Decided by: agent (autonomous)

## Context

§11.6 makes a reference mandatory on a rendered event:

> Rendered events MUST expose stable references usable in subsequent commands

and spells one `@e42`. An `EventId` is a content digest — `e` plus 24 hexadecimal digits — so the
full reference is 26 characters. A timeline row is a clock, a subject, what happened and a source
tag; at 80 columns a 26-character reference takes a third of the line, and at 40 columns (v0.4
§39.3 requires 40 to work) it takes two thirds.

The specification's own `@e42` is three characters, so an abbreviation is what §11.6 has in mind.
An abbreviation is only honest if it can be typed back.

## Decision

**1. A row prints the reference the session issued, where the record carries one.** ADR-0660 mints
a short reference in `ono-temporal-query::search` as the shortest prefix of the digest that names
one event inside a session, and §20.4's completion offers the same string. A renderer that derived
a second spelling would show a reader two names for one event, so the session's own reference wins
whenever it travels with the event. `ono.temporal-event/1` declares no field for it today; the row
reads a `reference` field where the producer supplies one, which a namespaced extension of v0.2
§10.4 already allows, and the field is what this package asks the contract to grow.

**2. Where none travelled, a row derives `@e` and the first eight digits of the digest.** Eight
hexadecimal digits distinguish every event in any window a person is reading, and the derived form
resolves through the same prefix rule as the minted one.

**3. The shell resolves a prefix, and refuses an ambiguous one.** `at event @e…`, `inspect event
@e…` and `why event @e…` match on the prefix; two events sharing it are
`Ono-Sendai-E1306 temporal.ambiguous_event`, which §34 already defines and ADR-0660 already
raises. Refusing is the honest answer and the error exists for it.

**4. The reference outranks the source tag when a row is tight.** §11.6 is a MUST and §11.5's
"Source tags SHOULD be abbreviated" is a SHOULD, so a row that cannot hold everything drops the tag
first, shortens the description second, and keeps the reference. Below the width at which the
clock, the subject and the reference fit together, the row keeps what happened and the reference is
read from the full-screen view instead.

**5. Matching is prefix matching in both directions.** `RenderOptions::cursor` and
`RenderOptions::expanded` hold references, and a stored value identifies the event whose id it
prefixes, whether it was stored as `@e4f3a2c1`, `e4f3a2c1` or the full digest. So a view stores
what a row printed.

## Consequences

- **This binds another package.** `ono.temporal-timeline/1`'s events must carry the session
  reference for the two spellings to become one, and the CLI is where the allocator of ADR-0660 and
  the record meet. Until the field exists, a timeline row shows a derived eight-digit prefix while
  completion shows the shorter minted one; both resolve to the same event, and a reader sees two
  spellings of it.
- Eight digits of a SHA-256 prefix collide at roughly one pair in 65 000 events by the birthday
  bound, so a busy window will occasionally produce two rows with the same reference. That is the
  case `temporal.ambiguous_event` exists for, and it is visible rather than silent.
- The constant lives in one place (`REFERENCE_DIGITS`), so widening it is one edit.

## Alternatives considered

- **The full 26-character reference on every row.** Rejected on §39.3's 40-column requirement: the
  row would hold nothing else.
- **A per-window ordinal — `@1`, `@2`.** Rejected: it is not stable across two renderings of
  overlapping windows, and §11.6 asks for a stable reference.
- **No reference in the default rendering, only in `--view`.** Rejected: §11.6 says rendered
  events, and the default rendering is the one most readers see.
