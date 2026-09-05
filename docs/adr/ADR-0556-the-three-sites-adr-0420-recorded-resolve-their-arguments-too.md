# ADR-0556: The three sites ADR-0420 recorded resolve their arguments too

- Status: accepted
- Date: 2026-09-03
- Spec refs: §14.5, §43; ADR-0009, ADR-0085 §2, ADR-0219, ADR-0420
- Decided by: agent (autonomous)

## Context

ADR-0420 fixed `trace --relations ["child"]`, which restricted nothing and said nothing about it,
and recorded the same trap open in three more places: `impls/meta.rs` (`help`'s subject, `type`'s
subject, `get command --verb/--target/--stability`, `find command`'s query), `impls/convert.rs`
(`to text --field`, `format table --columns`, `--max-rows`) and `ono-cli/src/spatial/commands.rs`
(`look --changes`, `near --type/--limit/--changed`, `follow`'s relation).

All three reproduced at HEAD:

| Written | Answered |
|---|---|
| `get command --verb ("ge" + "t")` | 193 commands — the whole registry, as if no verb was written |
| `get process \| take 2 \| format table --columns ["pid"]` | all five columns |
| `near --limit (1 + 1)` | 35 neighbours |

The cause is the one ADR-0420 states: a bracketed list and a parenthesised expression are
*expressions*, ADR-0009 keeps them unevaluated until something knows what they mean, and
`BoundArguments::option` answers for value bindings only — by design and as documented. A command
that reads `option()` without evaluating takes the absence for "not written".

## Decision

Each of the three sites evaluates its bound arguments against the invocation's scope before it
reads them, exactly as `mutate` (ADR-0219) and `trace` (ADR-0420) do:

```rust
let arguments = ctx.arguments().evaluated(ctx.scope())?;
```

In `meta.rs` the evaluation happens once at the top of `invoke`, and the four helpers take the
resolved `&BoundArguments` instead of reaching back into the invocation — which also removes the
last reason for those helpers to hold an `Invocation` at all.

`enter`'s selector is left as it is. It already evaluates its expression explicitly, with a
comment saying why, and rewriting it would be a refactor riding along a fix (AGENTS.md §4).

**The general form is still not taken.** ADR-0420 named it — have the dispatcher evaluate for
every `ArgumentMode::Words` command — and it is unsafe as a blanket rule: `find --where {…}`
(`ono-cli/src/spatial/find.rs`) and `enter`'s selector read `option_expression`/
`selector_expression` deliberately, and eager evaluation would turn those bindings into values
before the command that wants the expression could see them. Closing that properly means the
*contract* saying which parameters take an expression, which is a tranche of its own and not a
rider on this one.

## Consequences

- The four sites that read values — `trace`, `mutate`, and now these three — all resolve at the
  same point and in the same way, so the rule is one rule rather than a habit.
- A spelling that cannot be honoured is now refused rather than ignored: `near --limit ($n)` with
  no `$n` raises the undefined-variable refusal of §43 where it used to answer, unrestricted, in
  silence.
- **One test was itself a victim.** `ono-command/tests/conversions.rs::
  should_render_only_the_columns_that_were_asked_for` asked for `--columns [name]` and asserted
  that `OWNER` was absent. The option never reached `format`, and `OWNER` is not in the fixture
  schema's default view either, so the test passed while proving nothing. It now asks with
  `--columns ["name"]` and asserts that a column of the default view is absent, which only a
  restriction that arrived can satisfy; a second test pins ADR-0420's third row at this site —
  `[name]` is refused rather than ignored.
- **Found and not fixed here:** an option whose evaluated value does not fit its declared type is
  still dropped rather than refused. `get command --verb ["get"]` evaluates to a one-element list
  where the contract declares `string`, `as_str()` fails, and the filter is skipped — the same
  silence, one layer down, now that the value arrives. Recorded for the board; it is a check in
  the binding layer, not in these three commands.

## Alternatives considered

- **One increment per site**, as issue #24 suggests. The three fixes are one line each, share one
  cause, one ADR and one acceptance case, and three commits would have said the same thing three
  times. The three reproductions are kept separate, which is what the advice was protecting.
- **Evaluate inside `BoundArguments::option`.** Rejected for ADR-0420's reason: it has no scope,
  and giving it one puts a runtime inside the binding layer.
