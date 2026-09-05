# ADR-0199: One directory, however the path spells it — and a hierarchy walk that terminates

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §11.3, §15.1, §27.2, §42.1; ADR-0187
- Decided by: agent (autonomous), delivering v0.4 §50 Phase S11

## Context

§42.1 requires one object to be one place, and §15.1 identifies a filesystem place by the object —
device and inode — rather than by the text that reached it. The shell observed whatever path it
was given: `enter /srv/app/..` registered the place of `/srv` and then filed it under the path
parent `/srv/app`, because `Path::parent` is textual. `/srv/app` was already filed under `/srv`.

The result was a two-step cycle in the path hierarchy, and `place_path` walks that hierarchy by
recursion with no guard. The next `look` overflowed the stack and aborted the process:

```text
$ ono -c 'enter /srv/app/..; look --json'
thread 'main' has overflowed its stack
fatal runtime error: stack overflow, aborting
```

## Decision

1. **A path is resolved before it is observed.** `storage::absolute` canonicalises the path it
   produces, so `/srv/app/..` is `/srv` before anything reads it and the path tree stays a tree.
   A path that cannot be resolved — it is not there, or a component of it is not this user's to
   walk — keeps the text the user typed, so the provider stays the one that decides which of
   those two it is (§35.2, ADR-0198).
2. **The walk up the canonical hierarchy terminates whatever the index holds.** `place_path`
   carries the places already on the way up and stops at one it has seen. §11.3 makes the
   canonical parent deterministic; it does not make the index incapable of holding a cycle, and a
   walk that assumes so is a crashed shell rather than a wrong answer.

## Consequences

- Symlinked spellings reach the same place as their targets, which is what device and inode
  already made true of the identity; the place path now agrees with the identity.
- `place_path` allocates one small set per call. It is called once per rendered place.
- Encoded by `spatial_storage_missing::should_stand_in_the_directory_a_path_names_however_the_path_spells_it`
  and `ono-spatial-query` `resolution::should_answer_a_place_path_rather_than_looping_when_the_hierarchy_holds_a_cycle`.

## Alternatives considered

- **Normalise `..` textually instead of canonicalising.** Rejected: it would leave a symlinked
  spelling filed under a parent that does not hold the object, which is the same cycle by another
  route.
- **Guard the walk only, and keep observing the path as typed.** Rejected: the guard stops the
  crash and leaves `/srv` sitting inside `/srv/app`, which is a wrong answer to §27.2's question.
