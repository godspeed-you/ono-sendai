# ADR-0148: What a place view says about the object behind it

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §3.1, §3.3, §10.1, §10.2, §11.3, §14.3, §14.4, §24.1, §24.2, §25.3, §28,
  §29.4, §33.4, §35.2, §40; v0.2 §43
- Decided by: agent (autonomous, phase S4b)

## Context

§3.1's `SpatialObject` names eight fields; ADR-0140 turned them into `ono.spatial-place/1`. Using
that record to *navigate* showed what it was still missing: the place of a socket did not say it
was listening, the place of a process did not carry the pid a pipeline filters by, and the exits
were a ranked list with no way to ask for one by name.

## Decision

`ono.spatial-place/1` gains four fields and loses one:

- **`type`** — the §3.3 place kind with the families it belongs to, most specific first:
  `process`, `directory file`, `connection socket`. §3.3 lists `Socket` among the kinds and
  §14.3/§14.4 then split it; both readings are true of one place, so the chain is written out
  rather than one of them chosen.
- **`canonical_ref`** — §3.1's own field, at last present: the provider's schema and the values
  of that schema's identity fields. It is the handle an action revalidates through (§33.2) and
  what §37.1's identity merge compares.
- **`lifetime`** — §3.1's `lifetime`: the tier, first and last observation, the provider's own
  start time and the end where there is one (§10.1, §10.2, §10.3).
- **`state`** and **`summary`** — the object's own state and the columns its provider's default
  view names. §12 and §13 print exactly these (`state running`, `since`, `listeners :80, :443`),
  and §24.1 keeps the exhaustive property list for `inspect`.
- **`parent` is removed**; `canonical_parent` carries the same answer under the name §33.1 and
  §11.3 give it. Two fields answering one question is one answer too many.

The object's **identity fields also travel at the top level** of the place record, as record
extensions: `look --json | from json | where pid == 1842` is an ordinary v0.2 pipeline, and a
place that hid its pid inside a nested reference would need a second vocabulary to be filtered
by (§28, §29.4). A name the place contract already declares keeps its own meaning.

`ono.place-view/1` gains:

- **`exits`** — the same groups keyed by the word `look` prints (§24.2's "groups as exits"), so a
  reader can ask for one exit by name instead of scanning a list; `groups` keeps the rank order.
- **`freshness`** — §25.3's vocabulary for how the data behind the place is kept current
  (§33.4). This build has no subscriptions: every place is read when it is looked at, so the
  honest word is `polled`, never `event_driven`. Past the index TTL it is `stale`; where an exit
  could not be read it is `partial`.

`ono.neighborhood-group/1`'s **`count` becomes nullable**. §2.17 and §42.4: a count nobody could
take is not zero, and `files  permission denied for 14 process FDs` must never reach a reader as
`files  0`.

`ono.spatial-neighbor/1` gains the eight fields §11.4 requires of an inspectable relationship —
`source`, `target`, `direction`, `provider`, `provenance`, `confidence`, `observed_at`, plus the
provider's own relation word — and the neighbour's `canonical_ref`, `type` and `identity`. All of
the edge fields are **null for a member reached by hierarchy**: §2.6 keeps containment and
relationship apart, and a containment dressed as an explainable edge would be exactly the
confusion it forbids.

Finally, **a refusal prints its dotted name beside its code**: `ono: Ono-Sendai-E1004
spatial.no_relation …`. v0.2 §43 gives every error both, §40 names its fourteen conditions in the
dotted vocabulary, and a condition a user is expected to act on has to be readable where the
refusal appears.

## Consequences

- A place view is bigger. §24.1 budgets what a *renderer* shows, not what the structured value
  carries; `ono-spatial-render` decides what reaches a terminal.
- The identity extensions are unnamespaced, where §10.4 namespaces a provider's own extensions.
  They are the spatial layer's composition of fields the provider already declared, not new
  facts, and the names are the object schema's own.
- Tests: `spatial_identity_missing::{should_carry_a_lifetime_descriptor_…,
  should_expose_how_fresh_the_data_behind_a_place_is,
  should_name_one_of_the_defined_permission_states_for_every_neighborhood_group,
  should_report_permission_denied_rather_than_zero_files_for_another_users_process}`,
  `spatial_relationships_missing::should_refuse_to_follow_a_canonical_child_…`.
