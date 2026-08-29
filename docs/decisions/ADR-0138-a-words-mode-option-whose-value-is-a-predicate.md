# ADR-0138: A words-mode option whose value is a predicate

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.8 (`find`), §9.3, §43.3; v0.2 §6.5 (resolution order), ADR-0009 (argument
  modes), ADR-0124 (`find place`)
- Decided by: agent (autonomous)

## Context

v0.4 §6.8 spells the spatial search `find --where <expression>`, and ADR-0124 makes it
`find place --where <expression>`. The predicate is an ordinary v0.2 predicate: the RED suites
write `--where state == "running"`, `--where pid > 1`, `--where local.port == 8080`, and v0.4
§6.8's own example is `--where cpu > 50`.

ADR-0009 decides how a line is lexed from its **head word alone**, so the editor can classify a
line at keystroke time without a command registry. `find` is a words-mode head, and it must stay
one: bare `find` reaches findutils through the v0.3 adapter (ADR-0124), where `find . -name '*.c'
> out` redirects and `/var/log` is a path rather than a division.

In words mode a predicate cannot be read:

- `--where state == "running"` binds `state` as the option's value and leaves `==` and `"running"`
  as two further positional arguments, which is a `type.mismatch`;
- `--where pid > 1` opens a **redirection** and writes the result stream into a file called `1`.

The second one is decisive. There is no spelling of the option's value that words mode reads as
the expression §6.8 says it is.

## Decision

The parser carries a second static table beside ADR-0009's `EXPRESSION_HEADS`: the
`(head, option)` pairs whose bare option takes an **expression** as its value, read in expression
mode, even on a words-mode line.

Today the table holds exactly one pair: `("find", "where")`.

Rules:

1. The pair is `(head, option)`, never an option name on its own. `grep --where state` is
   unchanged, and so is every external program's `--where`.
2. It applies only to the bare spelling `--where <expression>`. `--where=<text>` keeps ADR-0009's
   meaning, and an option at the end of a stage keeps no value at all, so the command reports the
   missing value with the type it wanted.
3. The expression ends where the stage ends: `find place --where pid > 1 | take 1` is two stages.
4. The table is declared in `docs/spec/language.yaml` under
   `argument_modes[].option_values`, and `cargo run -p xtask -- spec-check` fails when the
   parser and the declaration disagree in either direction. The language a user is told about and
   the language the parser implements are the same language.
5. A new pair is added only where the option's value genuinely is an expression of the object
   model, and only for a head that cannot become an expression head — otherwise the head moves
   into `EXPRESSION_HEADS` instead, which is the smaller change.

## Consequences

- `find place --where state == "running"`, `--where pid > 1` and `--where local.port == 8080`
  parse as one predicate each, and reach the command as an unevaluated `Expr` through
  `Binding::Expressions` — the same shape `where` itself receives.
- Redirection is not lost for `find`: only the value of `--where` is read in expression mode, and
  `find place --where state == "running" > out.txt` still redirects, because the redirection is
  outside the predicate.
- An external command invoked as `somecmd --where foo` would have its argv rendered
  `["somecmd", "--where="]` by the literal-argv path, which is a degradation for a spelling no
  such program has. It is bounded by rule 1: only a head named in the table is affected, and the
  only head in the table is `find`, which no external program accepts `--where` for.
- Tests encoding it: `crates/ono-parser/tests/parse_commands.rs`
  (`should_read_the_predicate_as_an_expression_when_a_words_mode_find_is_given_where`,
  `should_compare_rather_than_redirect_when_a_predicate_option_contains_a_greater_than`,
  `should_leave_an_unrelated_option_a_bare_flag_when_its_head_declares_no_predicate`) and the
  drift check `xtask::contracts::check_expression_options`.

## Alternatives considered

- **Make `find` an expression head.** Rejected: it breaks bare `find` for findutils — paths become
  divisions, globs become multiplications, `>` stops redirecting — which is the collision ADR-0124
  exists to avoid.
- **Require `--where=(state == "running")`.** Rejected: the parenthesised escape of ADR-0009 works,
  but making it mandatory would give v0.4's primary discovery verb a spelling no other predicate in
  the shell needs, and the specification writes the bare form four times.
- **Rebuild the predicate from the words after `--where`.** Rejected: `>` has already become a
  redirection by then, so the words that reach the command are not the words the user typed.
- **Let the command registry decide, rather than a static table.** Rejected: ADR-0009's reason
  stands — the editor classifies a line before any registry is available, and a parse that depended
  on the registry would give the same text two meanings in two contexts.
