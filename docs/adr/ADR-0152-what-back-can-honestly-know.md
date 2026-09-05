# ADR-0152: What `back` can honestly know about a place before tombstones exist

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.4, §6.6, §20.3, §40, §10.3
- Decided by: agent (autonomous, S4c)

## Context

§2.4 makes every movement reversible "where the previous location still exists", and §20.3 says
what happens where it does not: resolve a tombstone if available; otherwise skip to the nearest
valid previous place, but only after informing the user; and retain the original trail record
either way. §40 adds two conditions `back` may raise — `spatial.history_empty` when there is no
previous place at all, and `spatial.destination_gone` when there is one and it is gone.

Tombstones are §10.3 and belong to phase S7. Until they exist, this shell has no liveness check
behind a place: an exited process stays in the index exactly as it was last observed, because the
index is a cache of what providers said and nothing re-reads `/proc` to ask whether it is still
true (§33.2).

## Decision

**`back` answers what the session can actually know, and the whole of §20.3's machinery is
implemented behind that answer.** "Still exists" means "this session can still say what the place
is": the index holds it, or it is a canonical space, which is declared geography and therefore
always there (§4.1). Every outcome §20.3 describes is handled:

- the previous place answers — `back` returns to it;
- some places on the way back do not — the shell prints, on stderr, how many and which, and then
  returns to the nearest one that does. The movement succeeded, so this is a notice and not a
  failure: a script must not die because a process it visited has since exited;
- none of them does — `spatial.destination_gone`, and the session stays where it is;
- there was never a previous place — `spatial.history_empty`.

In every branch the trail keeps its records, including the `back` itself: `NavigationTrail` appends
the return as a step rather than unwinding what it walked (§20.3's third clause).

**`spatial.history_empty` and `spatial.no_parent` are two conditions and stay two.** An empty trail
is a fact about history; a place with no canonical parent is a fact about hierarchy. `back` raises
only the first and `up` only the second, each with its own `Ono-Sendai-E` code and its own help
text pointing at the other verb.

## Consequences

- With no liveness check, the skip and `destination_gone` branches are reachable only when a place
  leaves the index — which today happens when a session is asked about something it never observed.
  They are implemented, tested through `NavigationTrail`'s own unit tests, and will start
  answering for real the moment S7 makes a place answer `alive`/`dead`.
- S7 changes one function, `still_a_place` in `crates/ono-cli/src/spatial/movement.rs`: a tombstoned
  place is not a place `back` returns to unchanged, it is a place `back` returns to *as its
  tombstone*. `spatial_identity_missing::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`
  is the test waiting for that and stays `#[ignore]`d until then.
- What is already true and tested: `spatial_navigation_missing::should_answer_history_empty_when_back_runs_with_no_previous_place`,
  `…::should_answer_no_parent_when_up_runs_at_the_system_root`, and
  `spatial_contracts_missing::should_refuse_to_go_back_or_up_from_the_root_with_a_named_spatial_error`.

## Alternatives considered

- **Verify liveness inside `back` by re-reading the provider.** That is a second source of system
  truth in the navigation layer, which §2.16 forbids, and it would answer "gone" for every place
  whose provider is merely unreachable — turning `spatial.remote_unavailable` into
  `spatial.destination_gone`.
- **Refuse `back` until tombstones exist.** §2.4 is an invariant, not a phase.
- **Treat a skipped place as a failure.** §20.3 asks for the user to be informed, not for the
  movement to be abandoned.
