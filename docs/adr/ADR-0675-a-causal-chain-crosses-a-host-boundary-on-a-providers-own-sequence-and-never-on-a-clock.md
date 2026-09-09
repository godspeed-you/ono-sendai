# ADR-0675: A causal chain crosses a host boundary on a provider's own sequence and never on a clock

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §21.6, §24.3, §25.5, §26.1, §26.3, §26.4, §55.6; ADR-0627
- Decided by: agent (autonomous)

## Context

v0.5 §26.4 permits a causal explanation to cross hosts *"only when the evidence chain actually
crosses the boundary"*, and gives the example of a request identity carried through a connection.
§26.1 lists what supports happens-before: *"same sequence stream; action -> provider transaction;
request -> response; process creation parent event; explicit message/connection transaction
identity"*. §55.6 forbids the alternative outright: sorting remote events by timestamp and
treating that order as causal truth.

`ono_temporal_core::happens_before` implements two of §26.1's sources — a monotonic reading inside
one `ClockDomain`, and a source sequence inside one domain from one provider — and answers
`Concurrent` otherwise. Its own documentation says why the rest are missing: *"a transaction
token, an action chain, a parent process, a request and its response — are relations between
events rather than facts on one, so they arrive as `CausalLink`s and are read from the ledger
rather than derived here."*

Two clock domains never satisfy either implemented source, so a rule that required
`happens_before(cause, effect) == Before` could never emit a cross-host link, and §26.4's own
example would be unimplementable.

## Decision

`ono.provider-causal-token` asks `happens_before` first and takes its answer whenever it has one.
`Before` accepts the pair, `After` rejects it. Only on `Concurrent` does the rule supply the
ordering evidence that `ono-temporal-core` deliberately left to the causal engine: the two events'
`source_sequence` numbers, compared **within one already-established transaction from one evidence
source**. That is §26.1's "same sequence stream" and "explicit message/connection transaction
identity" taken together, and both preconditions are facts the provider published.

Three constraints keep it from becoming a clock in disguise:

- the rule reaches this point only after joining the two events on an equal transaction token from
  one `EvidenceSource` that advertises `causal_tokens` (§21.6) — the identity join happens first;
- a pair where either event carries no `source_sequence` stays unordered and produces no link;
- no comparison of `presentation_instant`, `observed_at` or `source_time` occurs anywhere in the
  rule. `crates/ono-temporal-query/tests/cross_host.rs` includes a pair whose far-host wall clock
  reads *earlier* than the near one and asserts that the sequence decides.

`happens_before` is unchanged and is not worked around: this rule adds evidence it is documented
not to hold, under a precondition it cannot see.

Everything else about a host boundary stays as §26.4 leaves it. Two events on different hosts with
no shared token are `Concurrent`, produce no `preceded_by` entry in an explanation, and reach at
most `ono.remote-endpoint-retry-spike`, which emits `correlated_with` and is capped at the
`correlated` strength (§7.2). Where the far side is a KUANG/11 package, §37.4's ceiling applies
and the link's strength is capped at `asserted`.

## Consequences

- §26.4's worked example is implementable, and the test file that proves it also proves the
  negative case beside it: the same two events without the token produce nothing.
- A remote provider that numbers its stream gains cross-host causality; one that does not gains
  correlation. Neither gains anything from its clock, which is what §24.3 and §55.6 ask for.
- If `ono-temporal-core` later grows the transaction and request/response arms of §26.1 —
  necessarily as a function over links rather than over two events — this rule should delegate to
  it and this ADR should be superseded.

## Alternatives considered

**Require `happens_before == Before` unconditionally.** Rejected: it makes §26.4 dead text and
`ono.provider-causal-token` a same-host rule, which its registry row does not say it is.

**Order by wall clock when the domains differ, adjusted by `clock_uncertainty`.** Rejected by
§55.6 and §24.3 in as many words.

**Emit an undirected causal link when the order is unknown.** Rejected: §15.1's causal classes are
directional, and a `caused_by` whose direction is a guess is a guess.
