# ADR-0420: `trace` resolves its arguments before it walks

- Status: accepted
- Date: 2026-08-31
- Spec refs: §22.3, §43; ADR-0009, ADR-0085 §2, ADR-0219
- Decided by: agent (autonomous)

## Context

At `72aea1e`, `trace process 1 --depth 2 --relations ["child"]` answered `child` **and** `parent`
edges — the same answer as no option at all — and `trace process 1 --relations [child]` did the
same. Recorded on the board as "`trace --relations` restricts nothing", found by `readme-demo`.

The filter itself was never at fault. `TraceOptions::wants` and `offers_wanted`
(`ono-graph/src/trace.rs`) are correct, and `--relations child`, written as a word, restricts the
walk exactly as declared. What differed was how the argument reached the command.

`docs/contracts/commands/process.yaml` declares `relations` as `list<string>`, and the language spells
a list `["child"]`. A bracketed list is an *expression*, so ADR-0009 has the binder keep it
unevaluated: `Binding::Expressions`. `BoundArguments::option` answers only for
`Binding::Value` — by design, and its doc comment says so — and `TraceOptions::from_query` reads
the provider query, which `CommandContract::query` builds from value bindings alone, for the same
documented reason.

So the option was read from two places that both, correctly, had nothing to give, and `trace`
took the absence for "not written". Nothing was reported: the walk simply ran unrestricted. The
one thing worse than refusing a question is answering a wider one silently.

`mutate.rs` already had the answer. ADR-0219 puts the resolution point in the command:
`BoundArguments::evaluated(scope)` turns every expression binding into a value, once, before the
command acts — which is exactly what a words-mode command that reads values needs.

## Decision

**`trace` evaluates its bound arguments against the invocation's scope before it does anything
else**, and reads its subject, `--depth`, `--relations` and `--users` from the evaluated
arguments. The provider query is built from them too, so `trace process $p` narrows at the
provider instead of enumerating the target and filtering afterwards.

Three spellings, three honest outcomes:

| Written | Before | Now |
|---|---|---|
| `--relations child` | restricts | restricts (unchanged) |
| `--relations ["child"]` | ignored, unrestricted answer | restricts |
| `--relations [child]` | ignored, unrestricted answer | refused: `child` names no variable |

The third is the point as much as the second. `[child]` is a list holding a bare name, and a bare
name is a variable this shell was never given; evaluating it produces the structured refusal
spec §43 asks for, where before the reader asked a narrower question and got a wider answer with
nothing said about it.

## Consequences

- `trace` gains one evaluation pass over its own arguments per invocation. It is bounded by the
  number of arguments written, and it happens before the walk, which reads procfs for every
  process it reaches.
- `trace process $p` and `each { trace process @.pid }` now narrow at the provider. That is a
  strict improvement and is why the query moved too, rather than only the options.
- **The same trap is open in three other places, and is now on the board rather than fixed here**
  (AGENTS.md §4 — one fix per commit): `impls/meta.rs` (`help`, `get command`'s `--verb`,
  `--target`, `--stability`), `impls/convert.rs` (`to text --field`, `format table --columns`,
  `--max-rows`) and `ono-cli/src/spatial/commands.rs` (`near --type`, `--limit`, `--changed`,
  `follow`'s relation) all read `ctx.arguments().option(...)` without evaluating, so each ignores
  an argument written as an expression. Each needs its own reproduction and its own increment.
- The general form — have the dispatcher evaluate for every words-mode command — is deliberately
  not taken here; see below.

## Alternatives considered

- **Evaluate in the dispatcher for every `ArgumentMode::Words` command.** It would close all four
  sites at once, and it is the shape to reach for eventually. Rejected for this increment: it
  changes when *every* command's arguments are evaluated, including commands that pass blocks
  through, and `evaluated` is documented as forbidden for expression-mode commands — a
  cross-cutting change of that size needs its own increment and its own tests, not a ride along a
  one-command fix (AGENTS.md §4).
- **Make `BoundArguments::option` evaluate lazily.** Rejected: it has no scope, and giving it one
  would put a runtime inside the binding layer that ADR-0009 keeps out of it on purpose.
- **Have the binder fold a literal list into a `Binding::Value` at bind time.** Tempting, since
  `["child"]` holds only literals — rejected because it puts a second, partial evaluator in the
  binder, and it would still refuse to answer for `[$a, $b]`, leaving the same silent drop for the
  next reader to find.
- **Leave the bracket spelling unsupported and document `--relations child`.** Rejected: the
  contract says `list<string>`, the language spells a list with brackets, and a shell that ignores
  its own list syntax teaches a reader not to trust the syntax.
