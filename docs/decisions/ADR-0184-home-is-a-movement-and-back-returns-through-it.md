# ADR-0184: `home` is a movement, and `back` returns through it

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.4, §6.6, §20.1, §20.3, §44.6, §44.7
- Decided by: agent (autonomous, phase S7), settling a conflict between two RED tests

## Context

Two v0.4 tests written before either was implemented disagree about one thing: whether `home`
pushes a place that a later `back` returns to.

- `spatial_identity_missing::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`
  runs `enter process P; look; home; kill P; back; look; trail` and requires the `back` to arrive
  at the process — so `home` must extend the history.
- `spatial_remote_missing::should_return_home_to_the_local_root_from_a_remote_place` runs
  `{LINK}; jump testbox; enter compute; home; look; back; back; look` and, as written, spends
  exactly two `back`s to get out of the remote scope — which only works if `home` does not.

ADR-0170 (phase S8) chose the second reading and set `Movement::Home.extends_history()` to
`false`, arguing from §6.6's grouping of `back`, `up` and `home` as returns: `home` ends an
excursion, so the place a later `back` should reach is the one before the excursion, and pushing
the place `home` left would make `back` bounce between the root and it.

## Decision

**`home` extends the navigation history, and `back` returns through it.** `Movement::extends_history`
is `!matches!(self, Movement::Back)` — `back` is the only movement that does not push, because it
is the undo.

Three things decide it:

1. **§20.1 writes a step for it.** `home` is one of the six values of the `movement` enum of
   `NavigationStep`. A movement the trail records and `back` cannot walk is a hole in the record
   that §6.7 makes readable.
2. **§2.4 is a numbered invariant**: "Every movement is reversible. `back` MUST return through
   the actual navigation trail where the previous location still exists." `home` is the movement
   that jumps furthest, and under ADR-0170's reading it would be the only irreversible one — a
   user who typed `home` by reflex in the middle of an investigation could not get back to where
   they were.
3. **§6.6's grouping is about meaning, not about mechanics.** It puts `back`, `up` and `home`
   together because all three take a user somewhere already known. What each one *records* is
   §20.1's question, and §20.1 answers it the same way for `home` as for `enter`.

The "bouncing" ADR-0170 warns about does not arise: `back` does not push (it is the undo), so
`home; back; back` reaches the place before the one `home` left, not the root again.

## Consequences

- `spatial_identity_missing::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`
  is green (ADR-0179 delivers the tombstone half).
- `spatial_remote_missing::should_return_home_to_the_local_root_from_a_remote_place` needs one
  more `back`: its walk is three movements deep — jump, enter, home — rather than two. **Its
  assertions are unchanged**: `home` after a jump still lands on the *remote* root, and walking
  the history back still leaves the remote scope with the crossing visible. Only the number of
  steps the script spends to walk its own history changes, which is the assumption the test made
  about a contract that had not been decided yet.
- ADR-0170 is superseded **on this point only**. Everything else it decides about remote
  navigation stands.
- `crates/ono-spatial-core/tests/trail.rs` and the acceptance case
  `docker/acceptance/cases/104-spatial-back-up-home-trail.case` encode the rule.

## Alternatives considered

- **ADR-0170's reading — `home` is a return, so it does not push.** Rejected against §2.4: it
  makes one movement irreversible, and it is the movement a user is most likely to type by
  reflex. It also leaves §20.1's step for `home` recording something `back` cannot act on.
- **Making `home` push only when it crosses a scope.** Rejected: a rule that depends on where the
  user happens to be is not one anybody can predict, and §29.1 wants spatial semantics scripts can
  rely on.
