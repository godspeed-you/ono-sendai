# ADR-0558: A marker is what a token means where a person reads without colour

- Status: accepted
- Date: 2026-09-03
- Spec refs: §44 ("No functionality may depend on color alone"), §4.6, §13.1, §13.2, §49;
  ADR-0332, ADR-0419
- Decided by: agent (autonomous)

## Context

ADR-0332 gave every §44 token a `marker`: a short piece of text that carries the token's meaning
where colour cannot. It made the marker a requirement — a theme whose `ui.danger` and
`ui.success` markers are equal, or absent, is refused by name — and it checked every marker for
length and for control characters at load time. Then it recorded, in its own *What is not
delivered*:

> no renderer *emits* the markers yet. They are the theme's answer to a colourless destination
> and every theme is required to carry distinct ones, but the tables and the error renderer print
> the plain text there.

That is issue #12, and at HEAD it still held: `Theme::paint` returned the sanitised text unchanged
wherever there was no colour, and not one renderer asked for a marker. §44's closing rule was
enforced on the theme file and unused by the output.

Two things made the emission less obvious than it looks.

**Rule 1 of ADR-0332.** "A theme is consulted only where there is colour", so that "under
`NO_COLOR`, on a dumb terminal and in every pipe, every theme prints identical bytes". A marker is
a theme's text, so emitting it anywhere colourless would make those bytes theme-dependent again.

**The layout measures what it prints.** `Layout::render_styled` computes column widths from the
unpainted text and adds the escape sequences last, "so colour can never change the alignment". A
marker is not invisible: added after the widths were decided, it would push a column out.

## Decision

**A marker is emitted at `Presentation::Plain`, and nowhere else.**

`Plain` is a terminal the user asked to keep plain, or one that cannot do better: a *person* is
reading it and there is no hue to carry the meaning, which is the reader the marker exists for. A
pipe, a redirect and a script are not people — there the structure is the answer, the bytes must
not depend on which theme happens to be configured, and a mark in the middle of them is noise a
reader downstream has to strip.

So ADR-0332's rule 1 keeps its substance and loses its overreach: **a theme cannot change the
bytes of a machine destination**, which is the guarantee that was worth having. What it cannot do
either, at `Plain`, is make the output unreadable — a marker is at most four characters and
carries no control character, both checked when the theme is loaded, so the bound is on the theme
rather than on the destination.

`Presentation::marks()` is the one place that decides. `Theme::paint` becomes `mark` then
`colour`, and both halves are public because a caller that has to *measure* what it prints marks
first, lays the marked text out, and colours it afterwards — so the escape sequences are still
added last and still cannot move a column.

**Three narrowings, each because a marker that says nothing costs a column:**

1. A marker that only repeats the text it marks is not added. `ui.value.null` is marked `null`
   and a null cell already reads `null`.
2. Empty text is not marked. There is nothing there to be about.
3. The frame is not marked. `ui.table.header`, `ui.table.key` and `ui.border` name the structure
   the values sit in rather than a value, so they are coloured and never marked.

**And one renderer is given the tokens the guarantee is about.** `ui.success`, `ui.warning` and
`ui.danger` are the three §44 names a theme must keep distinguishable, and nothing in the shell
painted with them, so the guarantee had nothing to protect. `ono.action-result/1`'s `status` is
the shell's own report of what it did to one object; its three values are those three meanings,
and it is the one column where a reader has to be told good from bad. `success` is
`ui.success`, `skipped` is `ui.warning` and `failed` is `ui.danger`.

It is that field of that schema and nothing else. A service's `state`, a socket's `state` and a
package's `installed` describe a system rather than judge an action, and painting a stopped unit
as danger would be the renderer asserting more than the value says (§10.5).

## Consequences

- At a `NO_COLOR` terminal, a failed mutation reads `!! failed` and a successful one `ok success`;
  a `skipped` one reads `! skipped`. At a colour terminal they are coloured, as before. In a pipe,
  a redirect and a script nothing changed at all, so no serialisation, no acceptance case reading
  `to json`, and no downstream tool sees a byte it did not see before.
- `Theme::colour` and `Theme::mark` are public. `paint` is unchanged for every caller that does
  not measure, and gains the marker for free.
- The two tests that held ADR-0332's rule 1 over `Plain` now hold it over the machine
  destinations, and a new one holds the other half: at `Plain` a theme's marker is what
  distinguishes danger from success in the *output*, not only in the theme.
- A theme file can now change what a `Plain` terminal prints, within four characters per token.
  That is the point of a marker, and it is why the length and control-character checks of
  ADR-0332 are load-time rather than advisory.
- Still not marked: the tree renderer's connectors and labels are painted after the line has been
  shortened to the width, so a marker there could exceed it by up to five cells. No shipped theme
  marks a token the tree uses (`ui.fg`, `ui.dim`, `ui.error.code`), so nothing is lost today;
  recorded rather than half-done.

## Alternatives considered

- **Emit the marker wherever there is no colour, pipes included.** Rejected: it makes the bytes of
  a pipe depend on a user's theme, which is the one thing ADR-0332's rule 1 is for, and it puts a
  mark into text another program has to parse.
- **Emit the marker at a colour terminal too**, for a reader who cannot distinguish the hues.
  Rejected here as a decision that belongs to the reader rather than to the renderer: `NO_COLOR`
  is exactly how that reader says so, and it already selects `Plain`.
- **Mark inside `paint` alone, and let the table measure afterwards.** Rejected: the width would
  be computed from the unmarked text and the marked cell would overflow its column — the failure
  the escapes-added-last rule exists to prevent, reintroduced by the thing meant to help.
- **Give `ui.success`/`ui.warning`/`ui.danger` to a service's `state` as well.** Rejected: an
  inactive unit is not a warning, it is a fact, and a renderer that decides which system states
  are bad has left rendering.
