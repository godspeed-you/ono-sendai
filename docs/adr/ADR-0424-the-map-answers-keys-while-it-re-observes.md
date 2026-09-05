# ADR-0424: The map answers keys while it re-observes

- Status: accepted
- Date: 2026-08-31
- Spec refs: v0.4 §34 (performance budgets), §34.1, §43.4 (resize preserves place and focus), §23.3
- Decided by: agent (autonomous)

## Context

`crates/ono-cli/tests/spatial_interactive_missing.rs::should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`
failed in roughly half of all `cargo test -p ono-cli` runs and passed every time the file ran
alone. It failed at its last assertion: after `Esc`, the view never left the alternate screen,
and the whole 45-second budget ran out.

The cause was measured rather than guessed. The hung `ono` was caught twice by sampling `/proc`:

- the main thread sat in `ep_poll` — tokio's reactor, not crossterm's `poll` on the terminal;
- its CPU time was **unchanged across 24 seconds** (`209` ticks at every sample);
- every worker thread was parked in `futex_do_wait`.

So the process was neither busy nor slow. It was parked in an `.await`, and the map's loop was
not reading the terminal at all while it was.

The loop's only awaits are the re-observations: the `Resize` arm and `redraw` both call
`spatial::map::projection`, which asks every provider that answers for the space. A provider is
allowed to be slow — `ono-provider-systemd`'s `CALL_BUDGET` alone gives each bus call ten
seconds, and one re-observation makes several. Under a full `-p ono-cli` run a dozen `ono`
processes query the same buses at once, which is why the whole package reproduces it and one file
does not.

Three explanations were ruled out by experiment rather than by reading:

- **CPU load** — the test passes with sixteen busy loops saturating the machine;
- **the number of processes the map draws** — it passes with 1 686 processes on the host;
- **crossterm's lone-`Esc` ambiguity** — `parse_event` holds a bare `\x1b` back only when
  `input_available` is set, and the unix source sets that from `read_count == TTY_BUFFER_SIZE`,
  which a one-byte read never is. `Esc` reaches the process; nothing was reading for it.

This is a product defect, not a test artefact. v0.4 §34 says the shell "MUST remain interactive
and progressively update rather than block unnecessarily", and gives navigation inside a rendered
map a 16 ms frame target. A user whose map was re-observing had a full-screen view that answered
no key at all — including the one key whose entire purpose is to get out — and a terminal they
could not take back until the providers happened to answer.

## Decision

**A re-observation never costs the view its ears.** Every await in the map loop now runs through
`while_answering`, which polls the terminal every `ANSWER_SLICE` (16 ms, from §34's frame target)
while the work is in flight.

**Keys typed during an observation are answered in the order they were typed.** They queue in
`waiting`, and the loop takes from that queue before asking the terminal for anything new. This is
not a detail: the first attempt at this fix *dropped* the keys it saw, and
`should_return_to_the_previous_place_when_back_is_used_at_the_prompt_and_in_the_map` — which types
`Down`, `Enter`, `Backspace`, `Esc` without pausing — then failed 4 runs out of 4, because the
`Backspace` was swallowed by the redraw the `Enter` had started.

**Leaving does not overtake an instruction the user gave first.** `Action::Close` short-circuits
only when nothing is queued ahead of it. The second attempt let `Esc` win unconditionally and the
same test still failed 4 out of 4, because closing beat the `Backspace` typed before it. The case
this ADR exists for — a user watching a view that has stopped answering — has an empty queue by
construction, so it is unaffected.

Draining stops at `KEY_BACKLOG` keys; the rest stay in the terminal's own buffer, which is where
unread input belongs.

## Consequences

- `Esc` closes a map whose providers are slow or stuck, at the cost of one 16 ms slice.
- `redraw` reports whether the user left, so each of its six call sites decides deliberately
  rather than drawing into a view nobody is watching. Its parameters moved into a `Ui` struct to
  stay under the argument limit.
- The map is still single-threaded and still applies one key at a time. Nothing here makes the
  observation itself faster or interruptible: a stuck provider still delays the *content*. What it
  no longer delays is the user's ability to leave.
- A slow provider stays worth fixing on its own. This decision makes it survivable, not
  invisible.

## Alternatives considered

- **Bound the re-observation with a timeout**, the way the systemd bus bounds a call. Rejected: it
  picks an arbitrary number, and any number large enough not to abandon a legitimately slow
  observation is also large enough to leave the view deaf for that long. The problem is the
  deafness, not the duration.
- **Observe on a background task and let the loop keep reading.** The honest long-term shape, and
  where §34.1 ("expensive discovery SHOULD occur asynchronously") points. Rejected for now: the
  observation borrows the session mutably, so moving it off the loop changes how spatial state is
  owned — a tranche, not a fix.
- **Drop keys that arrive during an observation.** Tried; it is what made the `back` test fail.
  Recorded because the failure is the argument.
