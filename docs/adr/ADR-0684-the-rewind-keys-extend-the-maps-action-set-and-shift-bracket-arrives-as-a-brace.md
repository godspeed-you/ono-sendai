# ADR-0684: The rewind keys extend the map's action set, and Shift-bracket arrives as a brace

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §18.2, §18.3, §18.4, §18.7; v0.4 §23.3, §39.1
- Decided by: agent (autonomous)

## Context

§18.3 prints eight bindings for the map temporal view and says they are normative defaults that
"MUST be configurable through the existing key-binding mechanism". v0.4 §23.3 already has that
mechanism: `ono_spatial_render::view::{Key, Action, Keymap, Effect}`, where `Action` is the
normative half and `Keymap` the configurable one, fed from `spatial.map.keys`.

Two of §18.3's rows name keys a terminal does not deliver. `Shift-[` and `Shift-]` arrive as the
characters `{` and `}`: a terminal reports the character the shift produced, not the shift.

## Decision

**§18.3's eight rows become eight new `Action` variants on the existing set, and the bindings are
written in the spelling the terminal actually delivers.**

| §18.3 | action | default key |
|---|---|---|
| `Space` | `pause` | `Space` |
| `[` | `step-previous` | `[` |
| `]` | `step-next` | `]` |
| `Shift-[` | `nudge-back` | `{` |
| `Shift-]` | `nudge-forward` | `}` |
| `N` | `now` | `N` |
| `T` | `timeline` | `T` |
| `D` | `changes` | `D` |

They are ordinary members of `Action::ALL`, so `Keymap::apply_overrides`, `Keymap::describe` and
the `?` overlay pick them up with no second mechanism, and the existing test that every normative
action has a key covers them. `Effect` gains one variant per action; `Effect::Nudge(i64)` carries
the seconds so the thirty of §18.3 is a constant rather than two variants.

Two pieces of view state come with them, both set by the shell:

- `MapView::set_temporal(Option<String>)` draws §18.2's `PAUSED @14:03:12.410` in the header. The
  words are composed by `ono-cli` from `ono_temporal_render::paused_marker`, because
  `ono-spatial-render` depends on `ono-value` alone and holds no clock and no formatter for one.
- `MapView::set_gap(Option<Vec<String>>)` implements ADR-0685.

## Consequences

- One key system, one configuration key, one help table. A user who rebound `close=q` rebinds
  `pause=p` the same way.
- `Space` stops being an unbound key in the map view. It was unbound before, so nothing is
  displaced.
- `N`, `T` and `D` are capitals, and the lower-case letters keep their v0.4 meanings (`h` home,
  `p` pin, and so on). Case is the whole of the difference, and §39.1's rule about colour does not
  apply to it: the help table prints both.

## Spec deviation

- Section: v0.5 §18.3
- Text: "`Shift-[     -30s`" and "`Shift-]     +30s`"
- Instead: the defaults are bound to `{` and `}`.
- Why: a terminal delivers the shifted character rather than a modifier for a printable key, so a
  binding on `Shift-[` would be unreachable. `{` and `}` are the characters `Shift-[` and `Shift-]`
  produce on the keyboard layouts the shell targets, so the physical gesture §18.3 describes is the
  one that works. A user on a layout where they are not may rebind `nudge-back` and
  `nudge-forward`, which §18.3 explicitly allows.

## Alternatives considered

- **A second keymap for temporal actions.** Rejected outright by §18.3's "the existing key-binding
  mechanism", and it would give the `?` overlay two tables to reconcile.
- **Extending `Key` with a `Shift(char)` variant.** Rejected: the terminal cannot report it for a
  printable key, so the variant would exist to hold a value nothing produces.
