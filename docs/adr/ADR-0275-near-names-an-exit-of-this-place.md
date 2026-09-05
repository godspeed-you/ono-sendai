# ADR-0275: `near <relation>` names an exit of *this* place

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.4 §6.2, §35.2, §40, §42.4, §2.17; ADR-0128, ADR-0209, ADR-0271
- Decided by: agent (autonomous, `close-spat`)

## Context

Two further defects found while proving ADR-0271, both behind the same reported symptom —
`near <relation>` answering nothing.

**1. The relation filter ignored direction.** `keeps_group` kept a group when the named word
matched the relation's `canonical_label` *or* its `inverse_label`, whichever end the group was.
`process.parent_of` is `children`/`child` from the parent and `parent` from a child, so
`near parent` at pid 1 answered with pid 1's eight children — the opposite of what was asked, and
the rows said `relation: child` while the caller had asked for `parent`.
`relation::resolve_label` has always resolved by source type; the filter had a second, looser rule.

**2. A withheld exit answered as an empty one.** `enter process 1; near sockets` printed nothing
with status 0, because `/proc/1/fd` is unreadable for this user and a group whose state is
`permission_denied` carries no members. §42.4 forbids exactly that: denied information reported as
absence. `look` had always said `sockets  permission denied — /proc/1/fd: …` in the same situation;
`near` said nothing at all, which is why the dogfooding report read the symptom as "no such
neighbour".

## Decision

**1. A group accepts only the words that name its own end of the relation.** For a group labelled
with the relation's `canonical_group`, the accepted words are that group name and the
`canonical_label`; for one labelled with the `inverse_group`, the inverse pair. The relation id is
accepted from either end, because an id has no direction. `near socket` at a process still keeps
`sockets` (canonical), `near parent` at a process now keeps `parent` (inverse) and answers empty
where there is no parent.

**2. `near <relation>` refuses a named exit that could not be read**, with the §40 condition that
says why and the provider's own detail: `spatial.permission_denied` (E1008),
`spatial.unsupported` (E1009), `spatial.stale` (E1010). An exit that *was* read and holds nothing
is still an empty stream — the name was understood and the answer is "none" (§35.2's `empty`).

**3. `near` asks the query layer which exits a word keeps, rather than repeating its rule.**
ADR-0271's refusal compared the word to the group *label*, which was a second definition of a
question `keeps_group` already answers — and a wrong one, since a group accepts more than its
label. The refusal now fires when the filtered neighbourhood is empty, which is the query layer's
own answer to "no exit here is called this".

## Consequences

- `near parent`, `near child`, `near children` and `near sockets` each answer about the thing they
  name, and each of the three §35.2 states that is not an answer is now visible from `near` as it
  always was from `look`.
- ADR-0271's `near socket`-based test is replaced by `near owner` — the word a *socket* place uses
  for its process, which from the process end names no exit. The change is a correction of the
  test's premise, not of the rule it tests: `socket` is a word the `sockets` exit accepts and
  always was.
- Encoded by `ono-spatial-query/tests/neighborhood.rs::should_keep_the_exit_at_this_end_of_a_relation_rather_than_the_one_at_the_other`,
  `ono-cli/tests/spatial_navigation_missing.rs::should_keep_the_exit_at_this_end_of_a_relation_when_its_two_ends_have_different_words`,
  `::should_refuse_a_relation_the_place_does_not_offer_rather_than_answering_nothing`,
  `::should_answer_an_empty_stream_for_an_exit_that_exists_and_holds_nothing`, and acceptance case
  `102-spatial-look-near` `s4w`, `s4y`, `s4z`.

## Alternatives considered

- **Keeping the loose match and filtering the rows afterwards** — the group is the unit §3.6 ranks
  and §35.2 states; filtering rows out of a group that should not have been kept leaves the group's
  own state attached to the wrong question.
- **Answering a withheld exit with an empty stream and a note on stderr** — a note is not a status,
  and a script that branches on the exit code would still read denied as empty.
