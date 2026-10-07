# ADR-0937: An evaluated argument is held to its declared type

- Status: accepted
- Date: 2026-10-07
- Spec refs: §2.6 (a filter that silently did not apply), §6.1 and §26.2 (selectors as
  alternatives), §11.3 (typing before execution), §43; ADR-0009, ADR-0014, ADR-0085 §2,
  ADR-0095, ADR-0556
- Decided by: agent (autonomous)

## Context

ADR-0009 has the binder reinterpret a words-mode argument against the command's declared type,
so `get process 4419` binds an `int` and `--every 5s` a `duration`, and a word that fits no
declaration is refused by name. An expression — `--verb ("get")`, `get user $who`,
`--since (now() - 1h)` — has no value at bind time and stays an expression until
`BoundArguments::evaluated` (or, for a provider query, the producer) evaluates it. Nothing then
held the value to the declaration. `get command --verb ["get"]` handed a one-element list to an
option `meta.yaml` declares `string`; the command's `as_str()` failed, the filter was skipped,
and the reader who asked a narrower question got all 222 commands (issue #168). `let who =
"root"; get user $who` bound `"root"` to `get user`'s first selector, `uid: int`, and answered
every account on the machine.

## Decision

1. The check lives in the binding layer, beside the declared type: `BoundArguments` keeps the
   command's declared selectors and options, and `evaluated` holds every value an expression
   evaluates to against its parameter's `DeclaredType`. The provider producer builds its query
   from `evaluated` instead of evaluating the expressions itself, so a provider query and a
   command that reads values are checked by the same code.
2. A value is admitted when:
   - it is already of the declared type (`record` admits a record or a map; `value` admits
     anything; `ref<…>` admits any non-list, as identity text or the object itself);
   - it is a list, and the parameter is `list<T>` or repeatable, and every item is admitted as `T`;
   - it is a scalar whose canonical text the declared type's word coercion accepts — `"4419"` for
     an `int`, `443` for a `port`, `"5s"` for a `duration` — and then it binds as that coerced
     value, so an expression is never held to a stricter rule than the word it stands for;
   - it is `null`: an unknown is not a mistyped value (ADR-0014), and the command treats it as
     it treated it before.
3. A selector whose value its own declaration does not admit binds to the first other declared
   selector that is still free and does admit it — the rule a word already follows (ADR-0095).
   `get user $who` with `$who = "root"` is `get user root`; with `$who = 0` it is `get user 0`.
4. Anything else is refused with `type.mismatch` (E0201): "`--verb` of `get command` is declared
   `string`, and its value is a `list`", with the metadata `parameter`, `declared` and `received`.

## Consequences

- `crates/ono-cli/tests/evaluated_arguments.rs` proves the refusal by name for a list and for a
  record literal, that `--verb ("get")` and `--verb $verb` answer what `--verb get` answers, that
  an evaluated selector binds to the selector its value fits, and that a repeatable selector
  still takes a list.
- A command implementation that reads `arguments().option(…)` after `evaluated` can rely on the
  value's type and no longer needs a per-command check.
- An expression-mode command (`where`, `each`, `sort`) is untouched: its expressions are
  evaluated per row, against the row, and never pass through `evaluated`.
- Out of scope and unchanged: `collapse` drops the word values of a parameter written both as a
  word and as an expression (`--x a --x (b)` keeps only `b`).

## Alternatives considered

- Check in each command (`as_str()` or refuse) — rejected: the issue is that every command would
  have to remember, and the one that forgot is the defect; the declaration is in one place.
- Refuse a selector value that does not fit its positional selector instead of rebinding —
  rejected: `get user $who` would refuse where `get user root` works, holding the expression to a
  stricter rule than the word.
- Refuse `null` as a mismatch — rejected here: an unbound variable is `null` everywhere in the
  shell, and refusing it is a separate, wider decision than the type check this issue asks for.
