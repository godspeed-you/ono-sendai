# ADR-0452: A value is measured once, and approximately

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.4, §21.2, §56.6, §65.6; v0.2 §10.2, §10.5, §25.1, §25.2; AGENTS.md §7, §11
- Decided by: agent (autonomous)

## Context

v0.4.1 §2.4 makes a byte bound mandatory beside every item bound, wherever a collection is
retained, queued or materialized and its elements can be any size. §65.6 names the alternative
as a defect rather than a limit: *"Allowing `N` values while each may contain arbitrarily large
payloads, with no byte budget, is forbidden for retained/materialized collections."* §0.5.6
records that this is what Ono ships today — `RETAINED_VALUES = 10_000` in
`crates/ono-cli/src/session.rs` counts values and knows nothing about what is inside them.

A byte bound needs a number, and `Value` has no cheap exact one. Every compound variant sits
behind an `Arc` (v0.2 §10.2, so that a pipeline stage costs a refcount bump rather than a copy),
which means two values may retain the same bytes; the allocator's real footprint is neither
observable from safe Rust nor stable across builds; and the same value must answer the same
number twice, because a limit that moves refuses different inputs on different days.

§21.2 states the requirement rather than the algorithm. This ADR states the algorithm, and — more
importantly — states what it deliberately does not see, because an estimator whose exclusions are
undocumented is a limit nobody can reason about.

## Decision

**`ono_value::estimated_size(&Value) -> u64` is the one figure every byte budget in the shell is
spent against, and it is deterministic first and accurate second.**

### 1. It lives in `ono-value`, as a free function

§56.6 permits *"a closely associated utility crate if architectural layering makes it
inappropriate directly in `ono-value`"*. Nothing makes it inappropriate: the traversal reads the
variants of `Value` and the public accessors of `RecordValue`, `ErrorValue` and `Provenance`, all
of which are `ono-value`'s own, and every crate that will spend a budget — `ono-pipeline`,
`ono-cli`, `ono-history` — already depends on `ono-value`. A second crate would exist only to
hold one function.

It is a free function rather than a `Value` method because a budget calls it on values it does not
own and because §21.2's own wording — "directly or through a utility" — treats the two as
equivalent. `Value` gains no new inherent surface.

### 2. What it counts

- the slot every value occupies wherever it is stored — `size_of::<Value>()` — so that a million
  nulls is not free, which is the count half of §2.4 expressed in the same unit as the byte half;
- the payload of every string, byte string, path and regex pattern reached, at its exact length;
- list elements, map keys and entries, record fields and extras, error messages, help text,
  metadata and cause chains, and record provenance, recursively;
- one `Arc` header — two `usize` — per distinct heap allocation reached;
- **each shared allocation once.** A list holding a hundred clones of one string costs one string,
  because one string is what it retains. This is §21.2's "avoid double-counting shared `Arc` data
  within one estimation traversal", and it is the difference between a budget that measures memory
  and a budget that measures fan-out.

### 3. What it deliberately does not count

- **Allocator overhead, alignment padding and spare `Vec` capacity.** None of the three is
  observable from safe Rust, and a figure that changed with the allocator, the target or the
  build profile would not be deterministic. This is the largest source of the gap between the
  estimate and RSS, and §21.2 accepts it in as many words: *"The result need not equal allocator
  RSS."*
- **The compiled automaton behind a `RegexValue`.** Its size is an implementation detail of the
  `regex` crate, and it changes when that crate changes. The pattern text stands in for it. A
  regex is a value a user typed, so the payload that matters is bounded by the line they typed it
  on.
- **The `Schema` a record is bound to.** A schema is provider metadata shared by every record that
  provider produces, not the record's payload. Charging each of a million process records for one
  process schema would make the estimate say more about the provider than about the data, and it
  would make the same hundred records cost a hundred times more when measured one at a time than
  when measured as a list — which is precisely how a budget stops being a budget. The `SchemaId`
  inside a record's provenance is left out for the same reason.
- **Anything nested deeper than `MAX_ESTIMATE_DEPTH`**, which is `MAX_YAML_DEPTH`, 128. §21.2 asks
  the estimator to *"cap recursion using the same or stricter depth rules used for
  serialization"*; this is the same rule. A value nested deeper than the serializer will emit
  cannot cross a boundary where its size is the question.

Under-counting at the depth cap is the one place the estimator knowingly answers low, and it is
bounded by the same rule that stops the value being serializable at all. `Value` cannot hold a
cycle — `Arc<[Value]>` and `Arc<MapValue>` are immutable once built — so the cap is a guard
against a pathological input rather than against non-termination.

### 4. Determinism, precisely

Nothing in the traversal reads a clock, a hash seed or an address *as a quantity*. Pointer
identity decides only whether an allocation has already been charged inside this traversal, and
the running total is a sum, so the order in which allocations are met cannot change it. An empty
payload is never deduplicated, because two distinct empty allocations may share an address and
treating them as one would leave the second's header uncharged.

Determinism is therefore per-value, not per-shape: two structurally equal values built from
different allocations legitimately answer differently, because they retain different amounts of
memory. That is the property a budget wants.

### 5. The documented tolerance

For a value whose payload dominates its structure — a large string, a byte blob, a list of them —
the estimate stays **within a factor of two** of the bytes that payload really occupies, and is
never below it. That is the figure
`should_stay_within_the_documented_tolerance_of_the_measured_retained_size` pins, measured against
bytes the test itself allocated, which is the only retained size a test can know exactly.

## Consequences

Easy: every later increment of H5 — the shared `Budget` (#66), the materialization helper (#67),
capture accounting (#70), history byte ceilings (#72) — has one number to spend and one place to
change if it is ever wrong. `explain` (#69) and `inspect limits` (#120) show figures in the same
unit the enforcement uses.

Hard: the estimate is not RSS and must never be described as memory used. A user comparing
`limits.materialize_bytes` against `ps` will see a difference, and the documentation has to say
which one is which. The exclusion list above is the contract, not an implementation note.

Also hard: charging shared allocations once means a budget accumulated **value by value** will
over-count what a budget accumulated **over the whole collection** charges, whenever the values
share payload. H5's callers accumulate per value, because that is the only way to refuse at the
moment the ceiling is crossed rather than after everything is already in memory (§21.3). The
over-count is in the safe direction and is the price of stopping early.

Constrains H6: a streaming `each` (#75) that wants to charge what it forwards will pay per value
and therefore over-count shared payload. If that becomes visible, the fix is a longer-lived
`Estimator` across one operation's values, not a change to this function.

Encoded by `crates/ono-value/tests/size_estimate.rs`:
`should_answer_the_same_estimate_for_the_same_value_on_every_run`,
`should_define_an_estimate_for_every_value_variant`,
`should_stay_within_the_documented_tolerance_of_the_measured_retained_size` and
`should_count_shared_payload_once_within_one_estimate`.

## Alternatives considered

**`std::mem::size_of_val` plus a heap-size trait.** `size_of_val` sees the slot and nothing behind
the `Arc`s, which is the whole payload. A `HeapSize` trait implemented per type would be the same
traversal wearing a trait, with the shared-allocation set threaded through every implementation as
a parameter — the abstraction without the second use case AGENTS.md §4 rejects.

**An allocator hook measuring real bytes.** Exact, and not deterministic: it would answer
differently under a different allocator, a different profile and a different fragmentation state,
and it cannot answer at all for a value it did not watch being built. §21.2 asks for predictable
logical payload limits, not for RSS.

**Structural hashing instead of pointer identity, so that two equal values dedup.** It would make
the estimate a function of the value's *shape*, which reads as more principled and is wrong: two
equal strings in two allocations retain twice the memory, and a budget that says otherwise permits
twice the memory. It would also cost a full hash of every payload on every estimate.

**Counting the schema.** Rejected in §3 above. It was tried first, and made a list of a hundred
records cost less per record than the same records measured individually — an estimate whose
answer depends on how it was asked cannot bound anything.

**No depth cap, relying on `Value` being acyclic.** True today and not a property the type system
enforces; a future variant holding a `Weak` or an interior-mutable container would turn a limit
check into a stack overflow. The cap is free and §21.2 asks for it.
