# ADR-0948: A hierarchy of records is drawn as a tree for a person reading it

- Status: accepted
- Date: 2026-10-07
- Spec refs: §4.6, §10.4, §13.1, §13.6, §22.4; ADR-0015, ADR-0091 §3
- Decided by: agent (autonomous)

## Context

`get process --tree` nests descendants under the extension `children`
(`crates/ono-provider-linux/src/process.rs::nest`, ADR-0091 §3), but the terminal drew the table of
the roots, so the hierarchy the user asked for was invisible (issue #223, part 3). `view tree`
draws a record as its schema id with a child per field, which is a view of one value, not of a
process tree. The shell has no per-schema render-hint field; presentation is decided by the value's
shape, the schema's `default_view` and the destination's `Presentation` (spec §4.6's progressive
enhancement).

## Decision

1. **The renderer decides, from the value's shape.** `ono_render::Renderer::hierarchy` recognises
   a stream of records of one schema in which records nest records of the *same* schema under
   `children`, and lays them out as rows depth first: the schema's default-view columns, the
   first non-numeric column indented with spec §22.4's ASCII guides (`+-- `, `|   `), the same
   cells — and so the same sanitising — as the table. Nothing in the evaluator or the provider
   knows about it; any provider that nests the same way is drawn the same way.
2. **Only for a person.** The table view uses it when the presentation is `Terminal` or `Plain`
   (someone is reading). `Pipe`, `Redirect` and `Script` keep the table of the stream's records,
   which is deterministic and what a reader of redirected output expects (§4.6). Serialisation
   (`| to json`) and every pipeline stage are untouched: the stream still carries
   `ono.process/1` records with `children`.
3. Width is the layout's, as for any table: a deep row is shortened, never wrapped.

## Consequences

- `get process --tree` at a terminal shows the forest; `ono -c 'get process --tree' > file`
  is unchanged.
- Tests: `crates/ono-render/tests/value_hierarchy.rs` (snapshot of the guides, roots-only for a
  pipe/file/script, width, sanitising a nested name); `crates/ono-cli/tests/process_tree.rs` (PTY
  draws guides, redirected output does not, `to json` carries `children`); acceptance case 040
  extended (redirected stays a table) and case `386-process-tree-at-a-terminal.case`.

## Alternatives considered

- **A `render: tree` hint on `ono.process/1`.** The schema describes every process stream, most
  of which are flat; the shape of the value is what makes it a tree.
- **Have `--tree` emit a pre-rendered view.** Providers never format (spec §5), and it would
  break `| to json`.
- **Box-drawing guides.** §22.4 draws trees in ASCII so they survive any terminal, and
  `render_tree` already does.
