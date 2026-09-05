# ADR-0073: When a terminal is too narrow for a table

- Status: accepted
- Date: 2026-08-27
- Spec refs: §13.2, §13.3, §4.6, §35.5
- Decided by: agent (autonomous)

## Context

Spec §13.2: "the renderer SHOULD choose a compact table when records are homogeneous and
terminal width permits. It MAY switch to stacked records for very narrow terminals." The
layout already had both forms, but the switch fired only when a column could not keep four
cells — so at 30 columns the process table survived as `sy...  null  16...  on...`, every cell
a truncation marker. Truncation was visible (§13.3), and useless. The RED test
`should_stack_records_instead_of_truncating_a_table_when_the_terminal_is_narrow` in
`crates/ono-cli/tests/data_missing.rs` states the user's expectation: at 30 columns a process
renders stacked, `pid 1` on one line, nothing cut. The spec does not say where "permits"
ends; this ADR does.

## Decision

1. **Width permits a table while every column that had to give up cells keeps at least
   eight.** Columns start at their natural width; when the row overflows, the widest columns
   shrink first, exactly as before. The moment the column to be cut next is already at eight
   cells, the layout stops and renders stacked records instead. Eight cells show a pid, a
   percentage, a byte size or a short name whole; below that a column shows mostly `...`.
2. **A column that is not cut is never the reason to stack.** Narrow columns keep their
   natural width, so a table of short identifiers beside one long path still shortens the path
   — `get service` at 80 columns keeps its table with a 24-cell `NAME`.
3. **A single-column table never stacks for width.** Stacking one column shows nothing the
   table does not, so it shortens down to the marker, as before (`MIN_COLUMN_WIDTH` = 4).
4. **Stacked records show every value whole where it fits**, one `label value` line per field,
   label lowercased and padded to the widest header; a value wider than the remaining cells is
   still shortened with the visible marker (§13.3).

The threshold is a constant of `ono-render` (`READABLE_COLUMN_WIDTH`), not a configuration
setting: presentation is not a data contract (§35.5), and a knob would only move the point at
which the output stops being readable.

## Consequences

- At 80 columns — the width redirected output is laid out for (§4.6) — every table the
  acceptance cases render is byte-identical to before: the process, file, service, mount,
  socket, user, interface, route, env, context and command tables were compared before and
  after the change. Redirected output therefore stays deterministic and unchanged.
- `crates/ono-render/tests/table_layout.rs` stays green and unedited: the 24-column path
  table still truncates, the 18-column process table still stacks, wide characters still line
  up.
- Tests: the render test named above, at the CLI on a 30-column PTY.

## Alternatives considered

- **Stack whenever any cell would be cut.** Rejected: a long `command` or `description`
  column would turn every wide table into stacked records at 80 columns, and a table with one
  shortened path is the right rendering for it.
- **Keep the table down to four cells (the previous rule).** Rejected by the test and by
  §13.2's own narrow example: `sy...` is not a rendering of `systemd`.
- **A `render.table.min_column` setting.** Rejected: a knob for the point where output stops
  being readable is not a decision the user should have to make.
