# ADR-0706: An explanation is a record and it states the instant it explains

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §15.5, §15.6, §15.7, §16.4, §16.5, §16.6, §35.5, §39.3; ADR-0670, ADR-0672
- Decided by: agent (autonomous)

## Context

§16.4 fixes what `why` returns and §35.5 fixes the one property the shape must have:

> Fields correspond to section 16.4 and MUST preserve causal vs correlated associations as
> separate arrays.

`ono.causal-explanation/1` was written to that shape, and `CausalExplanation` in
`ono-temporal-query` was built to it, and nothing joined the two: the type had no `to_record`, so
the schema had no producer and `ono_temporal_render::causal_explanation` had no input it could ever
be handed. `why` could not render.

Two of §16.5's and §16.6's lines need an instant the type did not carry:

```text
failed at 14:03:17.004
```

```text
14:03:06  /etc/nginx/nginx.conf changed
            11s before failure
```

§39.3 forbids the renderer resolving an id to find one, so a renderer holding only
`explained_event: EventId` can print neither line. The render crate had been reading `at` as an
untyped extension of v0.2 §10.4 while waiting for the field.

## Decision

**1. `CausalExplanation::to_record` produces `ono.causal-explanation/1`, with §16.4's ten members
and nothing else.** `subject`, `explained_event`, `state_or_change`, `cause`, `causal_chain`,
`correlations`, `preceding`, `gaps`, `coverage`, `provenance`.

**2. Causal, correlated and preceding stay three arrays, and nothing moves between them.** §35.5
requires it and §16.6 spells the failure it prevents: "The renderer MUST NOT move the config change
into the `known cause` section because it appears plausible." There is no ranked list for a
correlation to be promoted inside, because there is no ranked list.

**3. Every nested piece goes through `ono_temporal_core::value`.** A chain step's `link` is
`value::causal_link_record`, a gap is `value::gap_record`, the coverage is
`value::coverage_summary`. This crate spells `depth`, `summary` and `at` — the words that wrap a
link into a step — and re-spells nothing the core module already spells.

**4. `ono.causal-explanation/1` declares a nullable `at`, and the producer fills it.** It is the
presentation instant of the explained event. It is null exactly when `explained_event` is null,
which is §15.7's answer for a window that recorded nothing notable — a complete answer, and one
whose heading has no instant to carry.

**5. Every node states its own instant too.** `CausalNode`, `CausalStep` and `TemporalAssociation`
each carry `at: Option<Timestamp>`, read out of the candidate set where the answer was computed.
§16.5 draws a clock beside every chain node; §39.3 means the producer is the only place that clock
can come from. A node the producer could not date renders undated rather than guessed.

**6. An id in an explanation is written bare — `e4f3…`, never `@e4f3…`.** The renderer adds the
`@` when it shortens the digest to a reference (ADR-0704), and an id that already carried one would
render `@e@e4f3…`.

## Consequences

- `why` renders. The CLI needs `explanation.to_record()` and `ono_temporal_render::causal_explanation`,
  and nothing between them.
- `CausalNode`, `CausalStep` and `TemporalAssociation` gained a field. All three are constructed
  only inside `CausalEngine::explain`.
- `is_causal` and the relation's inverse label travel on every node, so a renderer never re-derives
  from a class name whether an edge asserts causation (§15.6).
- The record validates against the embedded contract, so `cargo xtask spec-check` compares one
  producer against one schema.

## Alternatives considered

- **Let the renderer resolve `explained_event` against a ledger.** Rejected outright: §39.3.
- **Derive the heading instant from the first chain step.** Rejected: the chain is what caused the
  event, so its instants are earlier, and an explanation with no chain would have no heading at all.
- **One ranked `associations` list with a `class` discriminator.** Rejected: §35.5 says separate
  arrays, and a discriminator is one careless `sort_by` away from promoting a correlation.
- **Embed each endpoint's whole `ono.temporal-event/1` in every step.** Rejected: an explanation
  would carry the same event several times, and `summary` plus `at` is what §16.5 draws.
