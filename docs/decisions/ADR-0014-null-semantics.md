# ADR-0014: Null in predicates, comparisons and arithmetic

- Status: accepted
- Date: 2026-08-26
- Spec refs: §10.5, §11.3, §11.4, §16.5, §48
- Decided by: agent (autonomous)

## Context

Spec §48 Step 3 raises the question and then requires an answer: "comparing nullable `cpu`
requires defined null behavior; default predicate treats null as non-match or requires explicit
coalescing — **this policy must be frozen**." Spec §11.4 gives a partial default for a *missing*
field, and §10.5 requires that three different things never be conflated: a field absent from the
schema, a field whose value is unknown, and a field whose read failed.

`where cpu > 20` over a process whose CPU could not be read is the single most common encounter a
user will have with this, so the answer has to be defensible rather than convenient.

## Decision

### The three absences stay three things

| Situation | Result |
|---|---|
| the field is not in the schema | `type.unknown_field` (E0202) at **plan** time — nothing runs |
| the field is in the schema, its value is unknown | `null` |
| the field could not be read (permission, race) | an `Error` value |

`?.` opts a field access into a runtime lookup that yields `null` for an unknown field instead of
failing the plan. That is the "safe field lookup" of spec §11.4, and it is the only way to get
schema-absence to behave like value-absence — which is what makes heterogeneous streams workable
without making homogeneous ones sloppy.

### Comparisons are three-valued

An ordering or relational comparison with `null` on either side yields `null`, not `false` and
not an error:

```text
null > 20      -> null
null == 20     -> null
null ~= /x/    -> null
null in [1, 2] -> null
```

`and`, `or` and `not` follow Kleene's three-valued logic, so `null and false` is `false` and
`null or true` is `true` — a predicate that is decided by one operand is not made unknown by
another.

### `null` as a literal is an identity test

`x == null` is `true` exactly when `x` is null, and `x != null` is its negation. Both are total;
neither yields `null`.

This is a deliberate exception to three-valued comparison, and it is worth the exception: without
it every user needs a separate `is null` operator for the most ordinary question there is, and
`where cpu == null` — which everyone will write — would silently match nothing. The rule is
stated in one sentence and there is no second case to remember.

### `where` admits only `true`

`where` keeps a value when its predicate evaluates to exactly `true`. `false` and `null` both
exclude it. So `get process | where cpu > 20` does not report processes whose CPU is unknown, and
`get process | where cpu == null` reports exactly those.

The exclusions are not silent: `where` counts the values it excluded because the predicate was
unknown, and that count is available on the pipeline's diagnostics and shown by `explain`. A user
who is surprised by a row count has somewhere to look that is not the source code.

### An error is not a null

If evaluating a predicate produces an `Error` value — the third case of §10.5 — the value is
excluded **and** the error is emitted on the pipeline's error channel with the object's identity.
It is a partial failure in the sense of spec §16.5, never a quiet non-match. Conflating "I am not
allowed to see this process's memory" with "this process is using less than 1 GiB" is exactly the
ambiguity Ono exists to remove.

### Arithmetic and aggregation

`null` in arithmetic yields `null`. `measure` and `count` skip nulls and report how many they
skipped alongside the result, so an average is never quietly computed over a different population
than the user thinks.

`sort` places nulls last under ascending order and first under descending, so they are never
mistaken for the smallest value.

## Consequences

Easy: the rule is short — comparisons with unknown are unknown, `where` keeps only `true`,
`== null` asks whether it is null — and it matches what anyone who has written SQL already
expects. A permission failure can never be silently read as a non-match.

Hard: `where cpu > 20` and `where not (cpu > 20)` do not partition the stream, because a row with
unknown CPU is in neither. That is the correct behaviour and it will surprise someone; the
excluded-unknown count exists so the surprise resolves in seconds.

Encoded by: `crates/ono-value/tests/null_semantics.rs`, the `where` tests in the transform suite,
and the acceptance cases for `where` over a field the container cannot read.

## Alternatives considered

- **null compares as false** — rejected: it makes `where x > 20` and `where x <= 20` both exclude
  an unknown row while looking like a partition, and it gives `not` no coherent meaning.
- **Comparing null is an error** — rejected: on a real system most nullable fields are null for
  some rows, so every ordinary filter would fail on a machine with one unreadable process.
- **Requiring explicit coalescing (`cpu ?? 0 > 20`)** — rejected as the *default*: it fabricates
  a value, which spec §10.5 and §35.3 forbid ("unknown data is `null`, never fabricated and never
  silently zero"). The operator remains available for a user who genuinely wants a default.
- **Three-valued `== null` as well** — rejected: see above; it would make the commonest question
  unanswerable without a fourth construct.
