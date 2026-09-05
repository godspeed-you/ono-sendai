# ADR-0453: A budget cannot say unlimited

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.4, §16.1, §21.1, §21.3, §21.4, §22.2, §53.1, §53.2, §53.3, §65.6;
  ADR-0006, ADR-0125; AGENTS.md §4, §7
- Decided by: agent (autonomous)

## Context

§21.1 gives the shape of the shared budget and, in the same breath, the way such a type is
usually defeated:

> A limit of `None` is permitted only for internal/test contexts where unboundedness is explicit
> in the type or constructor name. Production interactive paths MUST NOT accidentally obtain an
> unlimited budget through a default constructor.

The logical model it prints has `Option<u64>` ceilings. An `Option` that is allowed to be `None`
in some contexts is an `Option` that will be `None` in one context nobody looked at, and the
sentence after it says which context that is: the one reached through `Default`.

§21.3 adds a second trap, in the *response* rather than the type. A budget may stop with a
structured error, or a documented cache may evict — *"The two behaviors MUST not be mixed
implicitly."* A single type that raised an error would force history to catch and discard it; a
single type that truncated would make `sort` silently wrong.

## Decision

### 1. `Budget` has no `Option` ceilings, no `Default`, and no unlimited constructor

`ono_pipeline::Budget` carries `max_items: u64` and `max_bytes: u64`. Not `Option<u64>`.

The strongest available guarantee that no production path obtains an unlimited budget is that the
type cannot hold one — no reviewer has to check, and no future caller can reintroduce it without
changing the type. §21.1's `None` is a permission, not a requirement, and H5 has no internal or
test context that needs it: every test in this tranche states the two figures it is testing
against, and stating them is what makes those tests readable.

A ceiling of zero means "no values permitted", which is §22.2's own rule. Zero is not a way back
to unlimited, and `u64::MAX` is not either — `should_offer_no_default_that_leaves_a_budget_unlimited`
asserts that no constructor answers it.

There are exactly three constructors, and each names what it is for:

| Constructor | Ceilings | For |
| --- | --- | --- |
| `Budget::of(stage, items, bytes)` | as given | a caller that knows its own figures |
| `Budget::materialization(stage)` | 100 000 / 128 MiB | §22.2, Appendix A |
| `Budget::command_captures()` | 100 000 / 256 MiB | §23.4, Appendix A |

The absence of `Default` is asserted at compile time and read at run time. An inherent associated
constant outranks a trait's, so `Probe<T>` answers `true` only when `T: Default` is satisfied; the
test checks the probe against `u64` first, so a probe that had stopped working could not pass by
answering `false` to everything.

`MaterializationLimits` — the configured pair a pipeline carries — *does* implement `Default`, and
that is not the hazard §21.1 names: its default is Appendix A's finite pair, and the type has no
representation for an unlimited one either.

### 2. A budget refuses; it never evicts and never warns

`Budget::charge` returns `Result<(), Exceeded>`. On the error path nothing is admitted — the
consumed counters do not move — so a caller cannot accidentally implement "keep collecting while
warning", which is the behaviour §21.3 forbids.

`Exceeded` is deliberately **not** an `ErrorValue`. It reports which ceiling was reached, what it
was configured as, what consumption crossed it, and which stage was enforcing it. Turning that
into a refusal is `Exceeded::into_error`, and taking §21.3's other branch is not calling it:
result history reads the same figures and evicts under its own documented policy (ADR-0458). The
two behaviours are two call sites, which is what "not mixed implicitly" means in code.

### 3. Three error codes, one new kind

§21.4 and §53.1 require `resource.item_limit`, `resource.byte_limit` and
`resource.materialization_limit`. They are allocated in `docs/contracts/errors.yaml` and
`ono_core::ErrorCode` together, as ADR-0125 requires of every new family, in the block after
spatial that ADR-0125 left free:

| Code | Selector | Raised when |
| --- | --- | --- |
| `Ono-Sendai-E1101` | `resource.item_limit` | a value ceiling was reached |
| `Ono-Sendai-E1102` | `resource.byte_limit` | a byte ceiling was reached |
| `Ono-Sendai-E1103` | `resource.materialization_limit` | the input as a whole may not be materialized |

They carry a new `ErrorKind::Resource`. §16.1's kinds are what a script branches on before it
looks at a code, and none of the existing twelve fits: this is not a timeout, not a cancellation,
not a stream-shape refusal, and calling it `safety` would put a memory ceiling in the same bucket
as a changed host key. ADR-0006 already extended §16.1's list with `safety` and `stream` for the
same reason, so the mechanism and its precedent both exist.

The details carry the limit, the observed consumption and the enforcing stage — and never the
retained values. §53.3 keeps secrets out of error details; §21.4 keeps the payload out for a
second reason, which is that a resource error printing what it was holding is a second resource
problem.

### 4. `resource.materialization_limit` is not a synonym for `stream.unbounded_operation`

§67.5's illustration labels an unbounded-input refusal `resource.materialization_limit`. Ono
already refuses that condition, by name, with `stream.unbounded_operation` (E0801), before
anything runs, and has since v0.2 — see the `Spec deviation` heading below. The three resource
codes divide as:

- `resource.item_limit` — a counter reached its ceiling;
- `resource.byte_limit` — the other counter reached its ceiling;
- `resource.materialization_limit` — the operation may not materialize this input *at all*,
  independent of any counter.

ADR-0125's rule applies: a condition that turns out to be an existing one reuses the existing
code rather than duplicating it under a new name.

## Spec deviation

- Section: v0.4.1 §67.5
- Text: "`local://~ > unbounded-source | sort timestamp` / `error resource.materialization_limit:`
  / `sort requires finite input, but the upstream stream is unbounded`"
- Instead: that refusal keeps `stream.unbounded_operation` (`Ono-Sendai-E0801`), which Ono has
  raised for it since v0.2 §11.1 and which `crates/ono-pipeline/tests/boundedness.rs` already
  pins. `resource.materialization_limit` exists, in the registry and in the taxonomy, for a
  materialization that is refused as a whole for a reason that is not an unbounded upstream.
- Why: §53.1 permits exactly this — *"Exact names may be reconciled with existing naming
  conventions, but the failure classes MUST remain distinct."* The class is one class, and it
  already has a stable code that scripts match on. Adding a second name for it would break
  §53.2's premise that a code identifies a condition, and ADR-0125 settled that a condition which
  turns out to be an existing one reuses the existing code. The message is what changes, to
  §54.1's shape (ADR-0455).

## Consequences

Easy: no caller can obtain an unlimited budget, so no review of a new capture path has to ask
whether it did. Every refusal in H5 carries the same four details, so `explain` (#69) and
`inspect limits` (#120) read one shape.

Hard: a caller that genuinely wants no ceiling has to say a number. That is the point, and it is
the reason `MATERIALIZE_MAX_ITEMS` and `MATERIALIZE_MAX_BYTES` are public constants — a caller
that wants "the default" names the default rather than omitting it.

Also hard: `ErrorKind::Resource` is a thirteenth kind, and `ErrorKind` is not `#[non_exhaustive]`.
Any exhaustive match on it outside `ono-core` had to be checked; there were none, because every
consumer matches on `ErrorCode` and uses `kind()` as a predicate. Adding a fourteenth kind should
be as cheap, and if it ever is not, the enum should become `#[non_exhaustive]` before the kind is
added rather than after.

Constrains H6: a streaming `each` (#75) must not take a `Budget` merely because it forwards
values. §23.1's rule is about *capture*, and a stage that retains nothing has nothing to charge —
charging it would be a limit on throughput wearing a memory limit's name.

Encoded by `crates/ono-pipeline/tests/budget.rs`:
`should_require_both_an_item_and_a_byte_ceiling_when_a_budget_is_constructed`,
`should_offer_no_default_that_leaves_a_budget_unlimited`,
`should_refuse_rather_than_truncate_when_a_budget_is_exceeded`,
`should_charge_a_byte_ceiling_from_the_estimated_size_of_what_it_admits` and
`should_charge_a_nested_budget_against_the_budget_it_was_taken_from`.

## Alternatives considered

**`Option<u64>` ceilings, as §21.1 prints them, with a constructor named `unlimited_for_tests`.**
It satisfies the letter of §21.1 and leaves the hole open: the constructor exists, it is public
because integration tests live outside the crate, and nothing but a reviewer stops a production
path calling it. The type that cannot represent the state needs no reviewer.

**`Budget` raising `ErrorValue` directly.** Shorter at the materialization call site and wrong at
the history call site, which would then be catching an error to discard it — literally mixing the
two responses of §21.3, with the mixing hidden inside a `let _ =`.

**One `resource.limit` code with a `ceiling` field.** §21.4 names three families and §53.2 makes
the code the thing automation matches on. A script that wants to retry a byte limit with a
narrower query and give up on an item limit would have to read a detail field to tell them apart,
which is the string matching §53.2 forbids wearing a different hat.

**Reusing `ErrorKind::Stream` for the three codes.** History and captures are not streams, and a
`stream`-kinded error raised while retaining a command's output would be a lie about where the
refusal came from.
