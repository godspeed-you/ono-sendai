# ADR-0781: The full-screen timeline is one view behind two doors

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §19.1, §19.2, §19.3, §19.4, §19.5, §18.3, §11.3, §11.5, §11.8, §39.3, §55.7;
  v0.4 §29.1, §43.4, §49.8, §52.2; v0.2 §50
- Decided by: agent (autonomous)

## Context

v0.5 §19 specifies a full-screen timeline, and nothing in the shell opened it.

`ono_temporal_render::timeline_view` had been written and tested against records — §19.2's header,
rows, cursor, evidence line and legend, plus `RenderOptions::group_repeats` for §19.4 and
`RenderOptions::expanded` for §19.5 — and no caller in `ono-cli` used it. Two consequences were
visible from a terminal:

- `ono -c 'timeline --view'` answered `Ono-Sendai-E0202 type.unknown_field ``timeline`` has no
  option ``--view```. The option §19.1 names as the canonical invocation was not declared.
- `T` in the interactive map composed the string
  `` `timeline --view --at <t>` opens the timeline at the cursor `` and did nothing else — a hint
  naming a command that did not exist, in place of the action §18.3 makes normative.

So §19.2's information architecture and §19.3's keys were unreachable, and `docs/ACCEPTANCE.md`
§4.11.7's box "The full-screen timeline opens, navigates and exits cleanly" was the one box of its
subsection no proof could close.

Four things had to be decided: where the view lives, how it takes the terminal when the map
already holds it, what `--view` does where there is no terminal, and how much of §19.3 is built
now.

## Decision

**One view, one query, two doors.** `crate::temporal::views::timeline_view` holds the terminal
side of §19 and nothing else. It is opened by `timeline --view` and by `T` in the map, and both
doors lead to the same loop over the same record:

- the window is `ono_temporal_query::timeline::timeline` over an ordinary `TimelineRequest`, built
  the way the non-interactive command builds it — the horizon of §11.3, the `--since`/`--until`
  bounds, the `--kind` filter — and read through `read_window`, the module's one call into the
  query crate;
- the screen is `ono_temporal_render::timeline_view` over the record that call returned. Nothing
  in the view lays out a row, decides a group or resolves an instant. What it holds is a cursor,
  a viewport and the translation from a key press into a call somebody else implements (§39.3,
  §55.7).

Every key that changes what is shown changes the request or the coordinate and asks again. There
is no second query path and no cached second copy of the window.

**The terminal is taken once.** `open` takes `RawMode` and then `AlternateScreen` as guards, so
every exit path — `Esc`, `Ctrl-C`, an error, a panic unwinding — leaves the terminal cooked and
the shell's screen restored, exactly as `crate::spatial::interactive` does for the map. `drive` is
the same loop *without* the guards, for a caller that already owns the terminal. `T` calls `drive`
from inside the map's own loop: entering the one alternate buffer twice and leaving it once would
have left the map painting onto the shell's screen for the rest of its life.

**`--view` is a presentation, and it degrades rather than refuses.** `timeline_view::may_open`
asks the three questions v0.4 §29.1 asks — the evaluator says these values are shown rather than
consumed, the shell is interactive at a terminal, the terminal is not `dumb` — and where the
answer is no, `--view` falls through to the text timeline of §11.5 over exactly the same values.
`ono -c 'timeline --view'`, `timeline --view | to json` and a redirected stream therefore all
produce the same deterministic text and no escape sequence (v0.2 §50). `spatial.map.mode = "text"`
is deliberately *not* consulted: it is a preference about the map, and a user who asked for the
text map has said nothing about the timeline.

**`T` opens at the cursor, through the one selector engine.** The map's temporal cursor has an
instant; it is spelled as an RFC 3339 timestamp and resolved by `crate::temporal::coordinate::
resolve` — the function `at` and every `--at` already use — because §4.5 forbids a second
historical code path. §11.8 then centres the window on that coordinate rather than ending it at
now, which is what makes a timeline opened from a frozen cursor a different window from one opened
in the present. A cursor following the present has no instant of its own, and the session's
coordinate is what the window is read at.

**§19.4 grouping is on in this view and off in the text one.** §11.5's default rendering is a row
per event and stays one; §19.4 is written about the full-screen view, so `group_repeats` is true
here. The rows the cursor moves over are the planner's own `groups` — the judgement is made once,
in `ono-temporal-query`, and the renderer and the cursor both read it rather than re-deriving it.

**§19.3's keys, and the two that are not built.**

| key | what it does |
|---|---|
| `Up`/`k`, `Down`/`j` | select an event |
| `Enter` | inspect the selected event |
| `X` | expand a grouped row into the events it stands for (§19.5) |
| `W` | why the selected event, through `CausalEngine::builtin` |
| `A` | set the session's temporal context to the event (§4.2's `at`, resolved and committed) |
| `/` | search the window; Return jumps to the next match, Esc cancels |
| `G` | the coverage gaps inside the window, in full |
| `N` | read the window at now again |
| `?` | help |
| `Esc`, `Ctrl-C` | leave |

`X` is an addition: §19.5 requires a grouped row to be expandable and fixes no key for it, and
§19.3's legend has room for its own ten. `?` is where it is discoverable, together with the two
keys this view does not answer:

- **`M` — map at event time — is not built.** The map view needs the spatial session, and the
  spatial session is already locked by the map whenever `T` opened this view; opening a map from
  the timeline therefore needs a terminal- and lock-ownership hand-off between two full-screen
  views, which is what v0.8's Deck workspace specifies and which this increment is not. `A`
  followed by `map` reaches the same place through commands that exist.
- **`C` — show/hide correlations — is not built.** `ono.temporal-timeline/1` carries no
  correlations and `RenderOptions` has no switch for them, so building it means changing
  `ono-temporal-render`, which is outside this increment. `W` shows the selected event's
  correlations with the evidence behind them, which is where §16.5 puts them.

Both keys answer by saying they are not built and naming what reaches the same information. That
is not the defect this ADR fixes: the defect was a key that described the command that would do
its job, in place of doing it.

**`N` moves the view, not the session.** §19.3 lists `N` as "jump to events near now" and §4.3
makes `now` a command the user types. A view that quietly returned the session to the present
would take back a coordinate the user had asked for, so `N` resets the window and leaves the
session where it stands. `A` is the only key here that moves the session, and it says so.

**The module is declared from `views.rs`.** `crate::temporal::views::timeline_view` lives in
`crates/ono-cli/src/temporal/timeline_view.rs` and is declared with `#[path]` from `views.rs`, so
the view and the command that opens it are one unit and `temporal/mod.rs` — which several
increments are editing at once — is untouched.

## Consequences

- §19.1, §19.2, §19.3 (bar `M` and `C`), §19.4 and §19.5 are reachable from a terminal.
  `docs/ACCEPTANCE.md` §4.11.7's box has a proof for the first time.
- `crates/ono-cli/tests/timeline_view.rs` holds ten PTY tests over the real binary: the view
  opens with §19.2's window, rows, evidence line and legend; the arrow keys move the selection;
  `Enter`, `W`, `?` and `X` answer; `A` leaves the session standing in the past after the view has
  closed; `T` in a paused map opens a window centred on the cursor rather than ending at now, and
  the map is still there afterwards; `Esc` and `Ctrl-C` both restore a cooked terminal that an
  external `stty -a` then reports as `icanon`; and `timeline --view` without a terminal answers
  the text timeline with no escape sequence in it.
- §19.4's grouping and §19.5's expansion of a *grouped* row are not proved from a terminal, and
  cannot be until something in the shell writes an `object.observed` or `object.changed` event.
  Nothing calls `crate::temporal::events::ingest_changes` or `ingest_provider_events` today, so
  the only events a session's ledger holds are action lifecycles, which §19.4 never folds. The
  grouping and the expansion are proved over records by
  `crates/ono-temporal-render/tests/grouping.rs` and the planner's own tests; what the terminal
  proves is that the view asks for grouping and answers honestly on a row that stands for one
  event. This is a gap in the *fixture*, not in the code, and it closes itself the moment the
  spatial observation bridge appends its events.
- A second full-screen view now exists beside the map, and the two share no host. v0.8 §Deck is
  where that consolidation belongs; until then each view owns its own guards, and `drive` is the
  seam that keeps one screen under one owner.
- Reversing any of this is local: the view is one module, the option is one line of
  `docs/contracts/commands/temporal.yaml`, and the command falls back to the text timeline
  whenever the view declines to open.

## Alternatives considered

- **Refusing `timeline --view` without a terminal**, as `map --live` refuses. Rejected: `--live`
  has no non-view meaning and `--view` has one — the window itself. A refusal would also have
  needed an error code, and §34's temporal family has none for "no terminal"; borrowing
  `spatial.unsupported` would have put a spatial code on a temporal command.
- **Letting `T` open the timeline with guards of its own.** Rejected: there is one alternate
  screen buffer per terminal. Entering it twice and leaving it once leaves the map drawing over
  the shell's own screen, which is the failure v0.4 §49.8 exists to prevent.
- **Building the rows in the view from the events.** Rejected by §39.3 and §55.7, and by §19.4:
  the grouping is a judgement about events, it is made in `ono-temporal-query`, and a second
  derivation in the shell would be a second answer to the same question.
- **`Enter` expanding a grouped row**, as `Enter` expands a cluster in the map. Rejected: §19.3
  fixes `Enter` as "inspect event" and the legend the renderer draws says so, so overloading it
  would have made the legend wrong. A grouped row's representative is an event like any other and
  is inspected the same way.
- **Implementing `M` by taking the spatial lock with `try_lock`** and refusing when the map holds
  it. Rejected as the worse half of a feature: a key that works from one door and not from the
  other is harder to explain than a key that is honestly not built yet.
