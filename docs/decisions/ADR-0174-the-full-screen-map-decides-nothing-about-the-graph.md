# ADR-0174: The full-screen map decides nothing about the graph

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §8.3, §23.3, §23.4, §39.1, §43.4, §45.4, §49.5, §49.8, §52.2, §53
- Decided by: agent S6 (autonomous)

## Context

§45.4 ends the description of `ono-spatial-render` with "It MUST NOT invent semantic
nodes/edges", and §49.5 names renderer-owned truth as an anti-pattern. §23.4 and §53 add the rule
that makes an interactive map safe: "Moving focus inside a map MUST NOT change the shell's current
place. Only `Enter` or explicit navigation action changes place." §8.3 draws the same line for
clusters: "Expansion is a view action. `enter` is navigation."

The risk in a full-screen view is that it grows a second answer to what the system looks like:
its own ranking, its own bound, its own idea of which nodes exist. Two answers is one too many.

## Decision

**The seam is the input of the text map, not its output.** `ono_spatial_render::map_lines(record,
width, charset)` draws one `ono.spatial-map/1` — already ranked, bounded and clustered by
`ono-spatial-query` — and returns each line together with the node or edge it drew. `spatial_map`
is that function with the identities dropped. The full-screen view is `MapView`, which adds a
viewport, a cursor and key semantics to those lines and nothing else. It never re-selects and
never re-ranks; when the place, the zoom or a live tick changes, the *shell* asks
`crate::spatial::map::projection` for a new record and hands it over.

**Focus is a question, not a movement.** `MapView::apply(key)` answers with an `Effect`. Only
`Enter`, `Follow`, `Back`, `Up` and `Home` produce an effect the shell acts on; every other key
changes the view alone. Enter on a *cluster* is `--expand`, not a movement, because a cluster is
not a place (§8.3).

**Semantic actions are normative; keys are configuration.** `Action` is §23.3's table — twenty-one
actions — and `Keymap` maps keys onto it. `Keymap::default_bindings()` is §23.3 key for key, and
`spatial.map.keys` applies `<action>=<key…>` overrides on top of it. An override rebinds the key
and leaves the action's other keys alone, so a partial configuration can never make an action
unreachable. The `?` overlay is generated from the bindings in force, so the help is true after a
rebinding.

§23.3's own table binds `h` twice — once inside "Arrow keys / hjkl move focus" and once as
"h or Home command → home (key binding may differ from vi-h)" — and flags the collision in the
same line. The table is followed literally: `h` is `home`, and focus moves with the arrow keys,
`j`, `k`, `l`, Tab and Shift-Tab. Horizontal movement has no meaning in a ranked tree anyway, so
`h` as vi-left would have been the binding that did nothing. `spatial.map.keys` is how a user who
disagrees writes `home=g, focus-previous=h`, which is the escape hatch the same section provides.

**Nothing depends on colour.** §39.1 lists six distinctions colour may not be needed for; the
focused item is one. The cursor is a `>` in the left margin of the focused line, and the view is
drawn two columns narrower to make room for it.

**The screen is borrowed and given back.** `ono_editor::AlternateScreen` and
`ono_editor::RawMode` are guards; whichever way the loop ends the terminal is cooked and the
shell's screen is back (§49.8, §52.2, §44.10). `AlternateScreen::enter` *queues* the switch rather
than flushing it, so the first painted frame leaves with it in one write and nobody ever sees an
empty alternate screen.

**Movement has one implementation.** `back`, `up` and `home` inside the view call the same
`go_back`, `go_up` and `go_home` the commands call. A refusal from one of them — `back` at the
start of the trail — is shown in the view's footer, because it is an answer to a key press and not
a reason to take the screen away.

## Consequences

- The text map and the full-screen map cannot disagree: they are the same lines.
- `MapView` is testable without a terminal, and `crates/ono-spatial-render/tests/view.rs` tests it
  that way — focus that asks for nothing, a rebound key reaching the same action, the cursor
  legible in monochrome, no line past the right edge, focus surviving a resize.
- `ono-spatial-render` gained a `Key` enum of its own rather than depending on `ono-editor`; the
  three-line translation lives in the driver. The renderer stays free of any terminal crate.
- Adding a semantic action means adding an `Action` variant and a default binding; the config
  syntax and the help table follow for free.

## Alternatives considered

- **A view that queries the index itself.** Rejected by §45.4 and §49.5: it is exactly the second
  answer this decision exists to prevent.
- **Reusing `ono_editor::KeyPress` in the renderer.** Rejected: it would make the pure view model
  depend on the crate that owns `crossterm`.
- **Enter closes the view after navigating.** Rejected: §23.3 binds `b`/Backspace, `u` and `h`
  *inside* the view, which only means something if the view is still open after an Enter.
