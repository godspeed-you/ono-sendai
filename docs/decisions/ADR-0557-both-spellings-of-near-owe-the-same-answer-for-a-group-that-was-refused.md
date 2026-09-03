# ADR-0557: Both spellings of `near` owe the same answer for a group that was refused

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4 §2.17, §6.2, §35.2, §40, §42.4; ADR-0271, ADR-0275
- Decided by: agent (autonomous)

## Context

v0.4 §42.4 forbids a false empty collection: a group this user may not read, or that nothing in
this build serves, is not an empty group. ADR-0271 made `near <relation>` say so — `near sockets`
on a process whose descriptors the kernel refuses answers
`Ono-Sendai-E1008 spatial.permission_denied` rather than printing nothing at status 0.

The guard it wrote is `if named_relation.is_some()`. `near` narrows on two spellings, though:
`near sockets` names the exit by its relation, and `near --type socket` names it by the type of
what is behind it. The second one fell straight through to the empty stream, so §42.4's false
empty survived on one of the two spellings — verified at HEAD: `enter process 1; near sockets`
refuses, `enter process 1; near --type socket` prints nothing and exits 0.

## Decision

**Whether `near` narrowed is the question, not which word narrowed it.** `near` computes
`narrowed = named_relation.is_some() || --type was written`, and when it narrowed and every group
in the answer is empty, the first group that is withheld supplies the refusal.

Two details follow from `--type` being able to keep more than one group where a relation keeps
at most one:

- the emptiness test is over *every* group in the narrowed answer, not over the first one. One
  group with members is an answer, and an answer is not a refusal;
- the refusal is the first withheld group's, which is the one ADR-0271 already produced for the
  relation spelling — `withheld_exit` is unchanged and still the only place that decides what a
  withheld group says.

A `near` that did not narrow is untouched: the whole horizon of a place legitimately contains
groups that are withheld beside groups that answer, and refusing the whole command because one
exit is unreadable would lose the exits that are readable. `look` is the command that shows the
per-group state, and it already does.

## Consequences

- `near --type socket` on a place whose socket exit is refused answers
  `Ono-Sendai-E1008 spatial.permission_denied` at a non-zero status, exactly as `near sockets`
  does. `crates/ono-cli/tests/spatial_navigation.rs::
  should_refuse_a_withheld_group_when_it_was_asked_for_by_type_rather_than_by_relation` holds it,
  taking the relation spelling's refusal as its premise so the test asserts the two spellings
  agree rather than asserting a property of the host.
- `near --type X` where the place genuinely holds nothing of that type still answers an empty
  stream at status 0. That is an answer, and §2.17 keeps it apart from a refusal.
- The unknown-exit branch below it is still relation-only, and correctly: `--type` names a type
  from a closed vocabulary that `spatial_type` already refuses when it is not one, so there is no
  "this place has no such type" list to offer.

## Alternatives considered

- **Refuse whenever any group is withheld, narrowed or not.** Rejected: `near` on a place with
  twelve exits, one of them unreadable, would answer nothing at all instead of the eleven it can
  read — a worse loss than the one being fixed, and `look` is where the per-group state belongs.
- **Report the refusal as a failed row in the stream rather than as a command refusal.** It is
  the shape §16.5 uses for partial failure, and it is the right shape for the un-narrowed case.
  Rejected here for consistency: ADR-0271 chose a refusal for the narrowed case, and two
  spellings of one question must not answer in two shapes.
