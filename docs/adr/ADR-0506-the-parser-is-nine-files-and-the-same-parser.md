# ADR-0506: The parser is nine files and the same parser

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §29.1–§29.4, §56.8, §65.12, §66.6, Appendix I.1; spec §24.4, §26, §35.6;
  ADR-0009 (the recoverable recursive-descent parser), ADR-0260 (words mode re-reads the source)
- Decided by: agent (autonomous)

## Context

§29.1 states the problem without ambiguity: the parser "has reached a module size where local
changes carry excessive cognitive scope". `crates/ono-parser/src/parser.rs` held 2473 lines — the
lexer's caller, the statement grammar, the precedence ladder, the stage grammar, both argument
modes, string interpolation, the recursion guard and every diagnostic the parser can raise, in one
file whose only internal structure was five `// --- section ---` comments inside four `impl`
blocks.

§29.2 names seven responsibilities that MUST become separately navigable and offers a reference
layout. §29.3 forbids reading that as licence for a rewrite: "the recursive-descent strategy,
recovery behavior, incomplete-input semantics, recursion-depth guard and AST contracts MUST
remain". §29.4 asks for "moves/extractions with minimal behavioral edits", and §65.12 explains
why: combining decomposition with redesign "destroys the ability of tests to prove behavior
preservation".

The question this ADR answers is therefore not *whether* to split, but **where the cuts go, and
what "no behavioural edit" is allowed to cost in visibility.**

## Decision

**`parser.rs` becomes `parser/`, and every function moves without a character of its body
changing.**

### 1. The nine modules

`§29.2`'s seven responsibilities, each in the file its name says, plus `literals` for the leaves
of the expression grammar and `mod.rs` for the crate's public surface:

```text
crates/ono-parser/src/parser/
    mod.rs          Parsed, parse, tokens, words_arguments — the four public items
    state.rs        MAX_DEPTH, struct Parser, peek/peek_after/bump/eat/at_keyword, skipping
    diagnostics.rs  report, report_unexpected, describe, expect, expect_ident, close
    recovery.rs     is_possibly_unfinished, finish_statement, recover_to_statement_end,
                    skip_balanced_block
    statements.rs   parse_program and the statement forms that are not control constructs
    blocks.rs       fn/if/for/while/match/try, patterns, types, params, blocks
    pipelines.rs    pipelines, stages, redirections, words-mode arguments, ends_stage
    expressions.rs  the precedence ladder, postfix, primary, parenthesised forms, binary
    literals.rs     numbers and units, lists, records, strings and interpolation, escapes
```

`literals` is an eighth file for a responsibility §29.2 does not enumerate, and it earns its place
the way the other eight do: the leaves of the grammar have their own decoding — an escape
sequence, a unit suffix, a `@-1` selector — which is text work rather than grammar work and reads
badly beside a precedence ladder.

Two cuts are worth naming because a reader could reasonably have made them elsewhere:

- **`close` is a diagnostic, not an expression.** It is called only from the bracketed forms, but
  what it *does* is decide whether a missing `)` is `parse.incomplete` or `parse.syntax` — the
  distinction ADR-0009 exists for. It belongs with the other place that decision is made.
- **`skip_balanced_block` is recovery, not blocks.** It runs only when a block cannot be parsed,
  and its job is to get back to a statement boundary. Grouping it with `parse_block` would put the
  success path and the failure path in one file and hide that the failure path is shared.

### 2. Visibility is the only edit

Every extracted item is now `pub(super)` — methods, free functions, `MAX_DEPTH`, `struct Parser`
and each of its seven fields. Nothing became `pub` or `pub(crate)`: the `parser` module is private
in `lib.rs`, so `pub(super)` is exactly "visible to the other eight files and to nobody else", and
the crate's public surface is the same four items `lib.rs` re-exported before.

That, the module declarations, the per-file `use` lists and the five section comments the module
boundaries replace are the **entire** diff. A normalised multiset comparison of every non-import,
non-blank line before and after is empty in both directions: no statement, no match arm, no
message string and no doc comment changed. The recursion guard is the same constant read at the
same four sites, the recovery points are the same, `Parsed` and the AST are untouched.

### 3. `ono-parser` keeps everything it owned

§56.8 is explicit that "crate ownership remains unchanged", and nothing crossed a crate boundary:
no item moved into `ono-core`, and none moved up into `ono-cli`. §30.4's prohibition on
architecture inversion is a statement about the evaluator, but the parser had the same opportunity
to take it and did not.

## Consequences

Easy: a change to precedence is a change to one 511-line file; a change to how a missing delimiter
is reported is a change to one 77-line file. The largest remaining file is `pipelines.rs` at 549
lines, down from 2473, and every one of §29.2's seven names is a filename.

Hard, or at least newly visible:

- **`pub(super)` is a wider door than `fn`.** Before, a helper was private to a 2473-line file;
  now it is private to a nine-file module. That is the price of the split and there is no cheaper
  one — Rust has no "visible to my siblings only" narrower than the parent module.
- **The section comments are gone.** They said what the file boundaries now say. A reader looking
  for `// --- expressions ---` finds `expressions.rs`.
- **`docs/STATE.md` still cites `crates/ono-parser/src/parser.rs`** for where `MAX_DEPTH` lives
  (the deferred entry about deep nesting under Miri). The constant is now in
  `parser/state.rs`. `docs/STATE.md` is written centrally, so the correction is reported rather
  than made here.

Encoded by: the parser suite unchanged and green —
`crates/ono-parser/tests/{diagnostics,lexer,parse_commands,parse_expressions,parse_statements,partial_input,robustness}.rs`,
121 tests plus 8 doc tests before the split and 121 plus 8 after, with `robustness.rs` replaying
Appendix I.1's fuzz seeds (`should_survive_a_pseudo_random_corpus_without_panicking`,
`should_not_overflow_the_stack_when_the_input_nests_deeply`). Not one test file was edited.

## Alternatives considered

**Keep `parser.rs` and add `parser/` beside it.** Rust 2018 allows it, and it would have left the
`site:`-keyed scans and `docs/STATE.md`'s path reference intact. Rejected: §29.2's reference
layout names `mod.rs`, and a file that exists only to hold `mod` statements next to a directory of
the same name is the shape the 2018 module system was changed to stop producing.

**Split by grammar production rather than by responsibility** — one file per statement form, one
per expression level. Rejected: it produces thirty files of forty lines and answers a question
nobody asks. §29.2 enumerates responsibilities, not productions.

**Fold `literals` into `expressions`.** It would match §29.2's list exactly. Rejected: it puts a
511-line precedence ladder and a 494-line decoder in one 1000-line file, which is the problem
§29.1 describes, one level down.

**Take the opportunity to make the four `impl Parser<'_>` blocks one trait.** Rejected on sight:
§65.12. A trait is a redesign, the tests could not tell the difference between a good one and a
bad one, and nothing asked for it.
