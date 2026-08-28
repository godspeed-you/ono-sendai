# ADR-0210: A search refuses a question it cannot ask

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.17, §6.8, §29.3, §32.1; v0.2 §11.3, §15.4, §43
- Decided by: agent (autonomous, `S11c`), on a decision the user fixed in the task brief

## Context

`docs/dogfood/v0.4-2026-08-28.md` finding 3. `find place --where` answered `0` for two questions
it had never managed to ask:

```text
find place --type process --where nosuchfield == 1 | count   ->  0
get process | where nosuchfield == 1 | count                 ->  Ono-Sendai-E0202
find place --type process --where memory > 1 | count         ->  0
get process | where memory > 1 | count                       ->  Ono-Sendai-E0203 per row
```

A typo in a script was therefore indistinguishable from an empty system, which invariant §2.17
forbids ("uncertainty MUST not be rendered as absence") and §29.3 forbids again for scripts.

This was not an oversight. `docker/acceptance/cases/101-spatial-find-place.case` assertion `s3i`
pinned the behaviour under a header citing §2.17 — it asserted the opposite of what its own
header claimed — and `TargetPlan::is_empty` defended it in prose: "a predicate over a field no
schema declares cannot match anything, and saying so costs nothing". Reversing it is a contract
decision, so it is an ADR rather than a hardening commit.

## Decision

`find place` answers a search. Where it cannot put the question, it refuses instead of answering
about the part of the system it managed to ask.

1. **A predicate field no candidate kind of place declares is a typo, and is refused** with
   `Ono-Sendai-E0202 type.unknown_field`, naming the field and offering the nearest declared one
   (v0.2 §11.3, §15.4). "Candidate" is every provider target a provider serves that also serves
   the kind of place `--type` asked for; cost does not enter, because whether a target is
   *enumerated* is a different question from whether the word means anything there. The
   suggestion pool is the fields those candidates' schemas declare, which is the same pool the
   check consulted, so the answer and the suggestion cannot disagree.

   §6.8 gives the search two sources, so the check has two: a field is declared if a candidate
   target's schema declares it **or** a record this session's index already holds does. A place
   only a v0.3 adapter observed — `ip addr` on a host whose address provider is absent
   (ADR-0193, ADR-0201) — is in the index and in no provider target, and refusing
   `--where family == "inet"` there would have made the search unable to see what it can already
   answer about. `crates/ono-cli/tests/spatial_contracts_missing.rs::should_find_a_place_by_its_properties_when_the_index_holds_it_and_no_provider_serves_it`
   is the test that caught the first draft doing exactly that.
2. **A field some candidate kinds declare filters normally.** A cross-type search is what
   `find place` is for, and a mount having no `cpu` is not an error: a target whose schema does
   not declare the field is skipped as `Skipped::MissingField`, exactly as before.
3. **An evaluation error on a record surfaces.** Every record reaching the predicate comes from
   a target whose schema declares every field it reads, so a failure to evaluate is a failure of
   the question rather than a row that did not match: `memory > 1` is
   `Ono-Sendai-E0203 cannot compare bytesize and int`, and the search reports it.

`find place` refuses on the first such error rather than emitting one error per row the way the
v0.2 `where` does. The two are different stages: `where` filters a stream the user assembled and
§9's partial-failure semantics let the good rows through, while `find place` *chooses* its
candidates from the predicate's own fields — so a comparison that is ill-typed for a candidate
was ill-typed for the question, and a filtered stream would understate the system without saying
so.

A null still never errors: v0.2's comparisons are three-valued, so `cpu > 5` against a process
whose `cpu` is null is `null`, which is not true, and the row does not match. Nothing about
finding 8 (`cpu` is null in a one-shot run) changes here.

## Consequences

- `TargetPlan` gains `candidates()` and `unknown_fields()`; `is_empty()` keeps its meaning for
  the case it was right about — a `--type container` search on a host with no container runtime
  still answers an empty stream — and its documentation no longer claims the case this ADR
  reverses.
- `ono_command::closest` becomes public: the near-miss search of §15.4 is now needed outside
  `ono-command`, and a second copy of it would be a second answer to the same question.
- `docker/acceptance/cases/101-spatial-find-place.case` `s3i` is corrected in the same commit,
  and `s3j`/`s3k` are added for rules 2 and 3.
- The tests that encode it, all in `crates/ono-cli/tests/spatial_navigation_missing.rs`:
  `::should_refuse_a_predicate_over_a_field_no_kind_of_place_declares`,
  `::should_still_search_across_kinds_when_only_some_of_them_declare_the_field`,
  `::should_surface_an_evaluation_error_rather_than_answering_an_empty_search`.

## Alternatives considered

- **Refuse whenever any candidate lacks the field.** Rejected: it would make every cross-type
  search a refusal, and destroy the thing `find place` exists for.
- **Emit the evaluation error into the stream, one per row, as `where` does.** Rejected for the
  reason above — and because `find place | count` would then count refusals as results, which is
  the worst of both answers.
- **Leave it and document the limit.** Rejected: it is the shape §2.17 names, and the case that
  pinned it contradicted its own header.

## Spec deviation

None. This ADR supersedes an earlier *implementation* reading (the prose on `TargetPlan::is_empty`
and case 101's `s3i`), not any sentence of a specification.
