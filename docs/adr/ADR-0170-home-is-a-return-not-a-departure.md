# ADR-0170: `home` is a return, so it does not extend the history `back` walks

- Status: accepted
- Date: 2026-08-28
- Spec refs: §6.6, §2.4, §20.1, §20.3, §19.2
- Decided by: agent (autonomous, phase S8)

## Context

§6.6 groups `back`, `up` and `home`, and §2.4 requires every movement to be reversible along the
actual trail. `NavigationTrail` implements that with two structures: an append-only list of steps
(what `trail` shows, retained even for movements that were undone, §20.3) and a stack of places a
`back` can return through. `Movement::extends_history` decides which movements push onto the
stack, and `back` already did not — "going back and then back again reaches the place before, not
the one just left, which is what makes `back` an undo rather than a toggle".

S8's `should_return_home_to_the_local_root_from_a_remote_place` runs

```text
link host testbox --transport local; jump testbox; enter compute; home; look --json; back; back; look --json
```

and requires the view after `home` to be the *remote* root (§6.6's "the root SYSTEM place for the
current host", read as the host the session is standing on) and the view after two `back`s to be
local again. With `home` pushing the place it left, the stack is `[local root, testbox root,
testbox compute]` and two `back`s reach `testbox compute` and then `testbox root` — both remote.
The test is only satisfiable if `home` does not push.

## Decision

`Movement::Home.extends_history()` is `false`, exactly as `Movement::Back`'s is.

`home` is a return, not a departure: it ends an excursion by going back to the root the excursion
started from. What a later `back` returns to is therefore the place *before* that excursion. The
step is still recorded in the trail — §20.1's movement list includes `home`, and `trail` shows it
— so nothing about the history is hidden; only the returnable stack is left alone.

This is the same argument the code already made for `back`. Had `home` pushed, `home` followed by
`back` would bounce between the root and the place just left, which is the toggle `back` is
defined not to be.

## Consequences

- After `home`, the top of the returnable stack is the place the excursion departed from, so
  `back` unwinds the excursion rather than re-entering it.
- A `back` immediately after a `home` that returned to a place already on the stack is a movement
  to where the session already stands. It is recorded as a `back` step, and it is honest: the
  trail says the session came from that root.
- **This contradicts a RED test of another phase.** `spatial_identity_missing::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`
  (still `#[ignore]`d, assigned to S7) runs `enter process <pid>; look; home; kill; back; look` and
  requires the `back` to return to the dead process. Under this ADR that `back` returns to the
  root instead. The two tests are structurally identical — `L → P → home(L)` against
  `L → T → C → home(T)` — so no rule about `home` alone satisfies both: the first demands that
  `back` after `home` reach the pre-`home` place, the second demands that two `back`s after
  `home` cross a link that is two entries down the stack. S8 implements the S8 reading and records
  the collision here; S7 owns the tombstone test and this ADR is the first thing to read when it
  is un-ignored.

## Alternatives considered

- **`home` pops the stack down to the root.** Satisfies S8 (in one `back` instead of two) and
  breaks the S7 test harder — `back` then answers `spatial.history_empty`.
- **`home` keeps pushing.** Satisfies S7, fails S8's assertion that two `back`s leave the remote
  scope. Rejected because S8's test is the one this phase delivers, and because the toggle
  argument already settled the same question for `back`.
