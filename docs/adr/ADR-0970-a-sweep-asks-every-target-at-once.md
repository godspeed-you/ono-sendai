# ADR-0970: A sweep asks every target at once

- Status: accepted
- Date: 2026-11-09
- Spec refs: v0.4 §29.3, §33.1, §34.2, §34.4; v0.4.1 §36.1; ADR-0497, ADR-0576
- Decided by: agent (autonomous)

## Context

Every spatial observation — a `look` at a collection, a `near`, the sweep a selector miss runs
through §34.2's acquisition classes — asked its targets one after another: snapshot the first,
collect it, absorb it, then the second. The providers read different parts of the system and
none of them waits for another, so the sweep cost the sum of their latencies where it could have
cost the slowest. At Profile M a selector miss cost 943 ms, most of it the systemd enumeration
at the end of the queue (#179, #187).

## Decision

**An observation asks all of its targets at once and absorbs their answers in order.** The
`observe` of the spatial view collects the questions first — which targets to ask, with what
bound — starts every snapshot together, waits for all of them, and only then folds each answer
into the index and the view, in the targets' order.

Two things do not change. The absorption stays sequential, because the index is one structure
and the order the targets fold in is the order the exits are built in, which §29.3 wants the
same twice. And the class-by-class order of the miss sweep (ADR-0497) stays: within a class
the targets are asked together, and the sweep still stops at the first class that resolves the
selector, so the expensive classes are not started for a selector a cheap one answers.

## Consequences

Easy: a sweep over N targets costs about its slowest provider; the first `look` of a session
and a miss both get the same gain.

Hard: N providers read the system at the same moment, which on a loaded host is N readers of
`/proc` at once rather than one; every provider here is a reader and none holds a lock the others
need. A provider that is slow because the system is slow is still slow; this takes the waiting
out, not the reading.

Encoded by `crates/ono-cli/src/spatial/view.rs::tests::should_ask_every_target_at_once_so_a_sweep_costs_its_slowest_provider`,
which registers four providers of 300 ms each and bounds the sweep well under their sum.

## Alternatives considered

**Ask every class at once as well**, not only every target within a class — rejected: a
selector most often resolves in the cheapest class, and starting the systemd enumeration for a
pid that `/proc` answers in a millisecond is the global candidate set §36.1 forbids.

**Absorb as answers arrive**, in arrival order — rejected: the exits would be built in a
different order on two runs of the same command, which §29.3 forbids a script to see.

**Bound the sweep** — rejected by ADR-0576 already: an `enter` that stopped early would say
`not_found` about something that is there.
