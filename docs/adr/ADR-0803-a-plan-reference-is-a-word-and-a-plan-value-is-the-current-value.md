# ADR-0803: A plan reference is a word, and `@plan` is spelled `@`

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §5, §5.8, §36.4, §64; v0.2 §19.4, §12.3; ADR-0071
- Decided by: agent (autonomous)

## Context

v0.6 writes its examples as `impact @plan`, `apply @plan`, `recovery = recover @plan`, and §5 adds
that "the exact implementation MAY expose result references such as `@plan`". §36.4 separately
requires `get plan a82f`, `apply plan/a82f` and `recover plan/a82f` to work.

The shell already has `@`, and it means something precise: v0.2 §19.4's current pipeline value,
with `@-1` for the previous result and `@3` for an item of the current one. It is lexed in
`ono-parser` as `TokenKind::CurrentValue`, and `@` followed by an identifier is not a reference —
in words mode it does not lex as one at all.

So `@plan` is either a new lexical form, or it is the specification writing a named variable in a
shell that spells named variables `$plan`.

## Decision

Both of §36.4's spellings work, and `@plan` is not added to the lexer.

- **A plan reference is a word**: `a82f`, or `plan/a82f`, or any unambiguous prefix.
  `impact a82f`, `apply plan/a82f` and `recover plan/a82f` are the canonical forms, and §36.4's
  three examples work verbatim.
- **A plan value is the current value**: `plan restart service nginx` emits an
  `ono.change-plan/1` record, so `apply @` applies what was just planned, and `apply @-1` applies
  the one before. Every command that takes a plan declares its selector as
  `ref<ono.change-plan/1>`, which accepts both the reference text and the record itself.
- **A named plan is a variable**: `let recovery = recover a82f` then `apply $recovery`, which is
  §5.8's four-line sequence in the spelling this shell already has.

## Spec deviation

- Section: v0.6 §5, §5.8, §64
- Text: "`impact @plan`", "`recovery = recover @plan`", "`apply @a82f`"
- Instead: `impact a82f` / `impact plan/a82f` for a stored plan, `apply @` for the plan just
  produced, and `let recovery = recover a82f` / `apply $recovery` for a named one.
- Why: `@` is v0.2 §19.4's current-value reference and `@name` does not lex. Adding it would give
  the shell two sigils for two kinds of reference that a reader would have to learn to tell apart,
  and §5 itself makes the spelling implementation-defined. §36.4's spellings, which the
  specification states normatively, all work exactly as written.

A second, smaller one in the same family:

- Section: v0.6 §64
- Text: "`rebase plan/a82f`"
- Instead: `rebase plan a82f`.
- Why: `apply` and `recover` take a plan without naming a target, so `plan/a82f` is read there as
  a reference and works verbatim. `rebase`'s target *is* `plan` (`docs/contracts/commands/change.yaml`),
  so the word after the verb is the target word, and `plan/a82f` is neither a target nor a
  reference in that position. Making it one would mean teaching the shell's target resolver that
  `<target>/<reference>` is a target — a change to every command's grammar for one example, where
  the spelling the grammar already has is one character different. §36.4, which is where the
  specification states the reference forms normatively, does not include this one.

## Consequences

§64's end-to-end interaction reads with `@a82f` replaced by `a82f`, and `map --plan @a82f` by
`map --plan a82f`. Everything else in it is unchanged, including the shape of every view.

The pipeline form is better than the specification's: `plan restart service nginx | apply` would
be the natural next thing to want, and it falls out of a plan being an ordinary value rather than
needing a second reference syntax.

A reference is resolved against the store by unambiguous prefix, and a prefix matching two plans
refuses with `change.plan_reference_ambiguous` listing the candidates and the width that tells them
apart — the same rule ADR-0783 fixed for event references.

## Alternatives considered

**Add `@name` to the lexer.** Rejected: it touches `ono-parser`'s lexer, its grammar, the AST, the
expression evaluator and the session's binding table, to add a second spelling for something `$name`
already does.

**Make `plan` a target word, so `apply plan a82f` parses as verb + target + selector.** Rejected:
`apply plan/a82f` is what §36.4 writes, and `plan/a82f` is one word.
