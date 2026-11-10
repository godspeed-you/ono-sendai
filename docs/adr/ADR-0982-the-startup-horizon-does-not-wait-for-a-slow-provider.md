# ADR-0982: The startup horizon does not wait for a slow provider

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §5, §25.2, §34, §35.2, §39.4, §43.7; ADR-0176, ADR-0186, ADR-0970; issue #210
- Decided by: agent (autonomous)

## Context

§5: "The startup horizon MUST NOT block startup on expensive global scans. Providers SHOULD
populate expensive counts asynchronously and update the horizon when available." The horizon is
`look` at the root (§49.5), and `look` waited for every provider it asked. ADR-0970 made the
wait the slowest provider's rather than the sum; a container engine that takes its full thirty
seconds to refuse still held the first prompt for thirty seconds (#210).

The same issue records that `spatial.reduced_motion` "switches nothing off".

## Decision

**The startup horizon waits 150 ms for its providers** — §34's "interactive startup to usable
prompt" target — **and is drawn with what answered.** Every target is asked on its own task.
One that has not answered inside the patience is shown as `unknown — still being read`, never
as a zero and never as `unsupported`, and its task keeps running: when the answer arrives it is
registered in the session exactly as a waited-for answer is, so the next `look`, `near` or
`map` reads it from the index (§33.1). An answer that arrives after the session moved to
another host is dropped; it belongs to the host it was asked on (§43.7).

Only the startup horizon has a patience. A `look` the user typed waits for its providers, as
does every script: an answer drawn incomplete because of the clock is acceptable where nobody
asked a question, and nowhere else.

The screen is not redrawn when the late answer arrives. The horizon was printed above the
prompt the user is typing at, and rewriting lines there would move text under their cursor;
§5's "update the horizon when available" is met in the session's state, which the next view
shows.

**`spatial.reduced_motion` keeps having nothing to switch off, and says so.** §39.4 asks it to
"disable transition animation while preserving live updates". The views draw no transition —
ADR-0176: a frame is repainted only when it differs, which is §25.2's rule — so there is no
motion to remove, and adding one for the setting to remove would be the decoration §25.2
forbids. The setting's description, which `get config` prints, states that; a test holds live
updates to continue with it on.

## Consequences

Easy: a host whose container engine, service manager or remote link is slow starts as fast as
one where they are absent, and the horizon says which count is outstanding.

Hard: on a loaded machine an ordinary provider can miss 150 ms, and the horizon then shows
`still being read` for a count that arrives a moment later. The domains are named either way,
which is what §5 requires of the horizon; the counts are "compact counts or summaries".

The acceptance container has no provider that can be made slow without a helper the image does
not carry, so the proof through the real binary is the pseudo-terminal test with a held engine
socket.

Encoded by `crates/ono-cli/tests/spatial_interactive.rs::should_reach_the_prompt_before_a_slow_provider_answers_and_say_what_is_still_being_read`
and `crates/ono-cli/tests/spatial_contracts.rs::should_keep_live_updates_and_say_what_reduced_motion_switches_off`.

## Alternatives considered

**Draw nothing until asked** — `spatial.startup_horizon = false` already does that, and §5
makes the horizon the default.

**Redraw the horizon in place when the answer arrives.** It needs the cursor arithmetic of a
full-screen view at a line-oriented prompt, and it changes text the user may be reading.

**A patience for every `look`.** A typed `look` that answers `still being read` about half the
system on a busy host would have to be typed twice; the user asked, and waits.

**Give `reduced_motion` an effect by slowing the live refresh.** That removes live updates,
which is the one thing §39.4 says the setting must preserve.
