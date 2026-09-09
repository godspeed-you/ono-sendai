# ADR-0746: Section 49's fixture ledger is a profile of its own beside Appendix F's

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §49, §31.4, §31.5, §10.4; v0.4.1 §32.2, §52.2, Appendix F; ADR-0431, ADR-0488
- Decided by: agent (autonomous)

## Context

`docs/contracts/hardening/performance_profiles.yaml` already declares three reference profiles,
and `crates/ono-spatial-query/tests/profiles.rs` compares them against `ono_testkit::profile` in
both directions — a profile the registry declares and the testkit cannot build is a failure, and so
is a profile the testkit knows and the registry omits. That test also asserts the registry declares
**exactly** Appendix F's three, which is what stops the list from growing a home for whatever did
not fit.

v0.5 §49 needs a fourth number set, and it is not a fourth profile:

```text
1,000,000 events
100,000 objects/lifetimes
500,000 relation changes
10,000 action records
```

Appendix F sizes a *host* — how many processes exist, how many sockets are open. §49 sizes a
*history*. They are independent axes of the same question, and conflating them would be wrong in
both directions: a timeline over a million events is slow because of the rows in the store, not
because of the processes on the machine, and Profile M's thousand processes say nothing about how
long a recorder ran.

## Decision

**A second section of the same registry, `temporal_profiles`, with one row, `T`.** It carries
§49's four counts, a `built_by`, and one field Appendix F's rows have no use for: a **seed**.

The seed is part of the declaration because "deterministic" is a claim about a *specific* stream. A
seed chosen at the call site would let two runs build two different histories under one profile
name, and two figures measured against two histories are not comparable — which is the whole
purpose §32.4 gives a baseline. `TEMPORAL_PROFILE_T.seed` is `0x0005_0049`, which is 327 753 in the
registry.

`built_by: benchmark`. §49's ledger is minutes of writing and hundreds of megabytes on disk, which
is more than every gate run should pay and well inside what `cargo xtask perf` and a developer
machine can afford. The vocabulary is the one ADR-0488 already established, so a reader needs no
second concept to know where a fixture of this size is built.

`ono_testkit::temporal::TEMPORAL_PROFILE_T` carries the same numbers and
`ono_testkit::temporal::declared_temporal_profiles` reads them back;
`xtask/tests/perf.rs::should_declare_the_fixture_ledger_in_the_registry_and_in_the_testkit_alike`
compares the two in both directions, with §49's figures typed out from the specification so the
check has something to disagree with. That is the property `profiles.rs` keeps for Appendix F,
kept here for §49, and Appendix F's own exactly-three assertion is untouched.

### What the numbers mean, once, so nobody re-derives them

- The million events **include** the half-million relation changes. §49 lists four properties of
  one fixture, not four independent populations.
- The hundred thousand objects are the distinct identities those events are about, and every one of
  them appears exactly once — the fixture holds exactly the declared number of lifetimes rather
  than however many a random draw happened to touch.
- The ten thousand action records are `record_action` rows, which are not events.
- The events are spaced 86.4 ms apart, so a million of them span exactly the twenty-four hours
  §10.4 makes the default retention window. §32.3 states its budgets "on a ledger within default
  retention", and this is the largest history that sentence can be about.

## Consequences

A record may name profile `T`, and `perf::check_registries` accepts it because `declared_profiles`
now reads both sections. `cargo xtask perf` builds the fixture once under `target/perf/` and
records a manifest beside it — seed, cardinality, and a SHA-256 over every `EventId` in write
order — so a later run reuses a store it can prove is the same history rather than rebuilding for
minutes.

The one thing this does not do is make the fixture cheap. It is a real SQLite database written
through `LedgerWrite`, with the production schema, the production CBOR payloads and the production
indexes, because §32.2's rule that "provider/planner code exercised by the benchmark MUST match
production logic" bites hardest where the thing under measurement *is* the index.

## Alternatives considered

- **A fourth row in `profiles`.** Rejected: it would break `profiles.rs`'s exactly-three assertion,
  and it would put a history's size in a table of host sizes, where `processes` and `sockets` have
  no honest value.
- **A separate registry file.** Rejected: `perf::check_registries` and `ono_testkit` would each
  need a second reader for one row, and §52.2's argument for one home per number is an argument
  against a second file as much as against a second copy.
- **No seed in the declaration; seed at the call site.** Rejected: see above. A fixture whose
  content depends on the caller is not the deterministic fixture §49 asks for.
