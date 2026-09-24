# ADR-0920: The terminal is read with a level-triggered poll

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4 §23.3, §34, §43.4; v0.5 §18.2; ADR-0424
- Issues: none filed — found at the v0.6.2 baseline, where it blocked `scripts/release-check.sh`
- Decided by: agent (autonomous)

## Context

`spatial_interactive.rs::should_show_the_paused_marker_and_keep_its_instant_when_the_terminal_is_resized`
failed intermittently on a loaded machine (1 in 10 at load 10–15, 2 in 8 with extra busy loops,
and once in the release check's gate), always the same way: after the resized, paused map frame
was on the screen, `Esc` went unanswered for the test's whole budget. The byte was in the
terminal; the view never read it.

The cause is below `ono`, in the terminal event reader `crossterm` 0.29 uses by default on Unix
(`event/source/unix/mio.rs`). It waits on two descriptors — the terminal and the pipe its
`SIGWINCH` handler writes to — through `mio`, which registers them **edge-triggered**. When one
wait reports both, the reader walks the readiness list and *returns* on the first entry it can
answer. If the resize comes first, it returns `Resize`, and the terminal's readiness in the same
list is discarded unread. Edge-triggered readiness is reported once per arrival, so nothing
reports those bytes again until the user types something else: the key is stranded, not lost
from the terminal, but a view waiting for it waits forever.

Both being ready in one wait is exactly what a loaded machine produces. The map notices a resize
by comparing the terminal's size on every read (issue #6), which is independent of whether the
signal has been collected yet; so the view can redraw at the new size, the user (or the test) can
see that frame and type `Esc`, and the reader's next wait then finds the uncollected resize
notice *ahead of* the key. The same holds for anyone who resizes a window while typing, at the
prompt as much as in a view: the line editor reads through the same reader.

`crates/ono-editor/tests/terminal_events.rs` reproduces it every time rather than by chance: the
re-run raises `SIGWINCH` on its own thread (so the notice is pending before the key), has the
parent type `Esc`, waits — without reading — until the key is in the terminal, and then asks.
With the edge-triggered reader it gets `Resize` and then nothing for five seconds.

## Decision

`crossterm` is built with its `use-dev-tty` feature, which replaces the `mio` reader with its
`poll(2)` reader (`event/source/unix/tty.rs`). `poll(2)` is level-triggered: a descriptor that is
still readable is reported on every wait, so a key that arrived beside a resize is simply found
on the next read. That reader also answers the terminal before the resize pipe, so the key comes
first and the resize — still pending, and still re-checked by size — right after it.

That reader treats a zero timeout as "return without looking". `read_event_timeout` promises the
opposite — a zero patience means "do not wait", and the map relies on it to hand over a key that
is already there while a provider is slow (ADR-0424) — so it never asks for less than one
millisecond (`SHORTEST_LOOK`), the finest wait `poll(2)` knows.

## Consequences

- A key typed together with a resize is answered. Tests:
  `ono-editor` `terminal_events::should_answer_a_key_that_reaches_the_terminal_together_with_a_resize`
  (red before this change, `ESC-STRANDED`; it also checks the resize is still reported) and
  `terminal_events::should_hand_over_a_key_already_waiting_when_asked_without_patience` (red with
  the new reader until `SHORTEST_LOOK`, `Ok(None)`).
- A read with no patience costs up to a millisecond when nothing is waiting. The map asks that
  way once per 16 ms slice while a provider works (ADR-0424); the view stays responsive.
- `filedescriptor` 0.8 (MIT) and, through it, a second `thiserror` major (1.x) enter the graph.
  `deny.toml` warns on duplicate versions and does not refuse them.
- The `poll(2)` reader reads the terminal again, blocking, when a read yielded bytes but no
  complete event — a split escape sequence, which a terminal writes in one piece. It is the reader
  `crossterm` ships for this platform, not a local patch, so upgrading `crossterm` keeps it.
- If `crossterm` fixes its `mio` reader upstream (process every ready entry, or keep the
  terminal's readiness when it returns early), this feature can be dropped again; the two
  `terminal_events` tests are what say whether the replacement holds.

## Alternatives considered

**Patch the `mio` reader (a vendored `crossterm` under `[patch.crates-io]`).** The fix is a few
lines, but it means carrying and re-basing a copy of an 800 KB crate, excluding it from the
workspace lints, and owning its licence notice — for behaviour the crate already offers as a
feature.

**Collect the resize signal promptly so it never meets a key.** It narrows the window without
closing it: a resize and a keystroke can always land in one wait, and a loaded machine is when
they do.

**Recover a stranded key in `ono-editor`** by checking with `poll(2)` after every `Resize` whether
the terminal still holds bytes. Detecting them is easy; handing them to `crossterm` is not — its
reader only reads on a new edge, and its escape-sequence parser is private, so `ono` would have to
parse terminal input itself.

**Raise the test's budget or re-send `Esc`.** The key is stranded, not slow: no budget is long
enough, and a test that types twice would hide a product that loses a keystroke (AGENTS.md §7).
