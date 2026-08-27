# ADR-0070: User functions and aliases — the calling convention and the expansion rule

- Status: accepted
- Date: 2026-08-27
- Spec refs: §6.5, §15.3, §19.3, §19.5, §30; ADR-0009, ADR-0010, ADR-0011, ADR-0019, ADR-0069
- Decided by: agent (autonomous)

## Context

ADR-0011 fixed the resolution order — keyword, function, alias, native, external — and the
grammar of ADR-0009 parses `fn name(params) -> Type { … }`, but the evaluator bound a function
to its own source text and never called it, and nothing spelled an alias at all: neither
`docs/spec/grammar.ebnf` nor `docs/spec/language.yaml` had a production for one, although both
list step 3 of the order. Spec §19.3 shows the declaration and §6.5 requires that a function or
alias resolve before a native command; neither says how an argument reaches a parameter or what
"expanded exactly once" means for a self-referential alias.

## Decision

### Functions (resolution step 2)

1. **A function is a scoped definition.** `fn` records the declaration in the innermost scope,
   beside the `let` bindings; a call finds "the innermost scope that defines one" (ADR-0011),
   and `fn:name` forces this step. A function named like an external program shadows it, and
   `explain` says so.
2. **Calling convention: positional, words mode.** `name a b` binds `a` and `b` to the
   declared parameters in order. A bare word is a string; when the parameter declares a type
   — `n: Int`, `Float`, `Bool`, `String`, `Path` — the word is converted to it, and a word
   that cannot be is `type.mismatch` (E0201). A value argument — `(…)`, `$x`, `"…"`, `[…]` —
   binds as the value it is, regardless of the declared type. A parameter without an argument
   takes its default, evaluated in the callee's scope at call time; without a default it is
   `null`. More arguments than parameters is `type.mismatch`.
3. **A function's body runs in the caller's output context.** When the call has a consumer —
   stages after it, `hot-processes | select pid` — the body runs in a fresh scope with its
   results captured as ADR-0069 captures a bound pipeline: every value a statement would have
   shown, and the value of `return`, become the stream the consumer reads, in order. When
   nothing follows the call, the body's statements show their results exactly as they would at
   the prompt, and a `return` value is shown as a producer's result would be; when the call
   itself is being bound (`let x = f`), the enclosing capture receives them. So `fn f() { echo
   hi }; f` prints `hi`, and `f | count` counts one string.
4. **Annotations are recorded, not enforced.** A declared return type is kept for `explain` and
   for the strict mode of spec §19.7; nothing is coerced on the way out.

### Aliases (resolution step 3)

5. **Syntax.** `alias name = pipeline` — an `alias_stmt` in `grammar.ebnf`, with `alias` a
   statement keyword in `language.yaml`. The right-hand side is any pipeline; it is parsed when
   declared, so a broken alias fails where it is written, not where it is used. An alias is
   declarative and is allowed in configuration files (ADR-0010).
6. **Expansion is textual and happens exactly once.** When a stage's head names an alias, the
   alias's source text replaces the head word, the rest of the stage — its arguments and
   redirections — and the rest of the pipeline are appended verbatim, and the result is parsed
   and run again. During that run the alias just expanded is not an alias for the head that
   came from it, so re-resolution starts from step 1 and `alias echo = echo prefixed` reaches
   the real `echo`; a cycle of aliases ends the same way. Other aliases inside an expansion
   expand normally.
7. **`explain` names both.** `explain hi` for `alias hi = echo hello` reports that `hi` is an
   alias for `echo hello` (step 3) and then explains the expansion; a function head is reported
   as step 2.

## Consequences

Easy: the specification's `fn hot-processes(limit: Float = 20) -> Stream<Process> { … }` is
callable; `alias ll = ls -la` and `alias gs = get service | where state == failed` both work
and both explain themselves; an alias can shadow a native command, which ADR-0011 wants and
`explain` makes visible.

Hard: an untyped parameter receives a string even when a number was typed, so `fn twice(n) {
($n * 2) }` needs `n: Int` — the price of never guessing at a word's type, which ADR-0019 pays
everywhere else. Textual alias expansion means an alias's arguments are re-parsed, exactly as
in every other shell; a value that must not be re-read is passed through a function instead.

Encoded by: the function and alias cases of `crates/ono-cli/tests/language_missing.rs`, the
parser cases in `crates/ono-parser/tests/parse_statements.rs`, and the acceptance case
`035-scripting-language`.

## Alternatives considered

- **Infer a word's type from its spelling** — rejected: `f 007` and `f 1e3` would silently
  become numbers; a declared type is one token away.
- **Always capturing the body, then rendering the values** — rejected: a bare `f` whose body
  runs `echo hi` would print a one-column table headed `VALUE` where the prompt prints `hi`; a
  body must look the same whether it was written inline or behind a name.
- **Structural alias expansion (splicing parsed stages)** — rejected: spans and source text
  would then belong to two documents, and every error, `explain` and history entry quotes the
  source. Re-parsing the expanded text keeps one document per run.
- **Aliases as functions in disguise** — rejected: an alias must be able to carry a partial
  command (`alias gp = get process`) that the user completes with arguments; a function has a
  fixed parameter list.
