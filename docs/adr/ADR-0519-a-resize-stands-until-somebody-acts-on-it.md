# ADR-0519: A resize stands until somebody acts on it

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4 §43.4 (a resize preserves the current place and focus), §39.3; v0.4.1 §38.1,
  §65.10 (skip-as-pass); AGENTS.md §11
- Issues: #6
- Decided by: agent (autonomous)

## Context

`should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open` resized a
pseudo-terminal and then waited for *new output naming the place*. The repaint the earlier `Down`
was still producing satisfied that, so the test passed whether or not any resize happened. Tracing
the map loop during a green run found sessions whose whole key history was `Down`, `Esc`, with no
resize event in them at all: §43.4 was held by nothing, and had not been for a release.

Issue #6 asks two things, and the second is only answerable once the first is: an assertion that
names something only a resize can produce, and why the `SIGWINCH` sometimes does not arrive.

Reading the map's frames off the wire answers the first immediately. The view repaints by homing
the cursor and addressing every row in turn — `ESC [ <row> ; 1 H` — so the set of rows a frame
touched *is* its height. A thirty-row terminal paints rows 1–30; a twenty-row one paints 1–20 and
stops. An earlier repaint cannot produce the second.

Written that way the test went red, immediately and every time, and printed the answer to the
second question with it: **after the resize the shell painted two more frames and both were thirty
rows high.** The signal never reached the view.

The path is `ready_key`:

```rust
fn ready_key() -> Option<Key> {
    match ono_editor::read_event_timeout(Duration::ZERO) {
        Ok(Some(TerminalEvent::Key(press))) => translate(press),
        _ => None,
    }
}
```

It is called every 5 ms while a projection runs, to keep a slow provider from swallowing the keys
somebody typed (ADR-0424). It reads *events*, and it wants keys — so a resize that arrived during
an observation was read out of the terminal and dropped on the floor. The map opens with a
projection that takes about a second on a busy host, which is precisely the window the test's
resize lands in. Waiting for the shell to go quiet before resizing made the test pass every time;
resizing while it was working made it fail every time.

## Decision

**A resize is reported for as long as it is unacknowledged, and only the code that acts on one may
acknowledge it.**

`ono_editor::read_event_timeout` no longer relies on the signal alone. Before polling, it compares
the terminal's current size against the last size somebody said they had drawn at, and reports a
`TerminalEvent::Resize` when they differ. Nothing is recorded there:
`ono_editor::remember_terminal_size(columns, rows)` is what ends the report, and the map loop calls
it after `view.resize(…)` has actually redrawn.

That inverts the failure. `ready_key` still drops the resize it cannot use — it is asked for a key
and a resize is not one — and the next read derives the same change again, because nothing said it
was handled. One `ioctl` per read is cheaper than a lost resize, and it makes the view's
correctness a property of what the terminal *is* rather than of what it managed to say.

`terminal_size()` records as it answers, so a view built at a size has established the baseline it
will be measured against. That also closes a second hole the same shape: `SIGWINCH` is delivered to
whatever handler is installed at the moment it arrives, and a full-screen view installs one when it
first polls, so a resize between opening the view and that first poll used to be gone for good.

**The gate holds the test shape too.** `scan::check_pty_resize_assertions` reports a test that
resizes a terminal and asserts nothing about the size it resized to — or about the resize itself,
as the signal it delivers or the size the terminal now reports, which is how
`should_deliver_sigwinch_to_the_child_when_the_window_changes` names it. What the scanner cannot
check is whether an assertion is a *good* one; what it can insist on is that the new size appears
in it at all, which is exactly what the old test did not do.

## Consequences

Easy: §43.4 is held by something. The rewritten test is red on a run where the resize does not
arrive, and `should_paint_no_frame_at_a_new_row_count_when_the_terminal_is_not_resized` is the
proof of that — the same key sequence without the resize, asserting that the frames stay at the
terminal's own thirty rows. That is the pair issue #6 asked for: an assertion only a resize can
satisfy, and a demonstration that it fails without one.

The test also went from 45 seconds to 2.5, because it no longer spends its whole budget waiting
for a frame that was never coming.

Hard: `read_event_timeout` now performs an `ioctl` on every call, including the zero-timeout calls
`ready_key` makes every 5 ms during an observation. That is about two hundred `TIOCGWINSZ` calls a
second on one file descriptor while a projection runs, which is nothing beside the projection, and
it stops when the projection does.

Also hard, and deliberate: a resize is now reported repeatedly until acknowledged, so a future
reader that consumes `TerminalEvent::Resize` and does nothing will see it again on the next read
rather than silently losing it. That is the right way round — a loop that ignores a resize spins
until it handles one, which is a bug that announces itself, where the old behaviour was a bug that
did not.

Encoded by: `crates/ono-cli/tests/spatial_interactive.rs::should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`,
`::should_paint_no_frame_at_a_new_row_count_when_the_terminal_is_not_resized`, and
`xtask/tests/scan.rs::should_report_a_pty_assertion_that_an_earlier_repaint_can_satisfy`,
`::should_accept_a_pty_assertion_that_names_the_frame_at_the_new_row_count`,
`::should_accept_a_resize_asserted_by_the_signal_it_delivers`,
`::should_report_this_repository_as_asserting_on_every_resize_it_makes`.

## Alternatives considered

**Make `ready_key` queue the resize beside the keys.** ADR-0424's `VecDeque<Key>` would have to
carry a second kind of thing, and every caller of `while_answering` would have to drain it. It
fixes the one call site that was found; the size comparison fixes the class, including the
installation race that no queue would have caught.

**Assert on the frame's width instead of its height.** The map truncates content at the terminal
width, so a narrower frame is also only a resize's doing — but a line's printable width depends on
what the map happened to draw, and a place with short labels would make the assertion pass at any
width. The row count is the frame's own geometry and does not depend on the content.

**Wait for the shell to go quiet before resizing, and leave the product alone.** It makes the test
green and it is what a person debugging this would try first. It also encodes the defect: the test
would then only ever exercise the path that works, and a user who resizes a window while a map is
loading — which is when a person resizes a window — would still get a map at the old size.
