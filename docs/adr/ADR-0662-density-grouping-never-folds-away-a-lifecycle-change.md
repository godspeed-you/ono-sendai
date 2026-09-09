# ADR-0662: Density grouping never folds away a lifecycle change

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §19.4, §19.5, §6.3, §6.4, §11.7
- Decided by: agent (autonomous)

## Context

§19.4 requires a large event set to be "semantically grouped rather than rendered as an unreadable
firehose", and lists four allowed dimensions: same object and field in a short interval, repeated
connection churn within an aggregation window, repeated provider samples that change no canonical
state, and cluster events a provider already aggregated. §19.5 requires a group to expand to the
individuals.

Two of those dimensions collide with what a timeline is for. An object appearing and an object
disappearing are the events a reader is looking for (§6.3), and a relation coming and going is the
topology change (§6.4); a grouping that hides one of those makes the timeline lie in the same way
§11.7 forbids a hidden gap from lying. But §19.4 names connection churn explicitly, and connection
churn *is* object lifecycle.

## Decision

**Grouping folds nothing away: a group enumerates every member, and lifecycle grouping is
restricted to the one case §19.4 names.**

1. `EventGroup::members` holds the `EventId` of every member, not only the hidden ones. §19.5's
   expansion is therefore a property of the value rather than a second query, and a group can be
   audited without asking the ledger anything.
2. `hidden` is the count that is not the representative, and `from`/`until` are the span — §19.4's
   "grouping MUST preserve hidden counts and time span".
3. `object.appeared` and `object.disappeared` are grouped **only** where the subject is a
   `Connection` or a `Socket`, which is §19.4's "connection churn" and nothing wider.
   `relation.added` and `relation.removed` are never grouped: the specification names no relation
   grouping dimension, and a topology change is the thing a reader came for.
4. Everything else follows the list: one subject and one field inside the window is `same_field`;
   a repeated `object.observed` that changed no canonical state is `unchanged_sample`; an event
   whose payload carries an `aggregated_count` a provider already folded is `provider_cluster` and
   keeps that count.
5. A run of one is not a group: its `reason` is `None`, so a renderer draws it as an event.

## Consequences

- A dense window of connection churn reads as one row and still names every connection that came
  and went, so nothing is lost and nothing has to be re-queried to find it.
- A process appearing is always its own row, however many appear at once. That is the case §19.4's
  firehose language is about and the case a reader most needs to see.
- Grouping is a pure function of the events and the aggregation window, so a renderer and a test
  see the same groups.
- Encoded by `crates/ono-temporal-query/tests/timeline.rs`.

## Alternatives considered

- **Grouping every lifecycle event by subject type.** It is what §19.4's first sentence suggests
  and it hides the disappearance a reader is hunting for.
- **Storing only the hidden ids and re-querying to expand.** It makes §19.5 a round trip and lets
  a group outlive the events it claims to stand for.
