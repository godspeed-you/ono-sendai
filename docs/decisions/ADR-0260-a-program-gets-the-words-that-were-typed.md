# ADR-0260: A program gets the words that were typed

- Status: accepted
- Date: 2026-08-29
- Spec refs: §6.2, §12.3; ADR-0009, ADR-0028, ADR-0057
- Decided by: agent (autonomous, `close-data`)

> ADR numbering: this batch was given 0215–0229 and used all of it. 0230–0244 and 0245–0259 are
> in use by the provider and harness batches running beside it, so the remaining decisions of
> this batch continue at 0260.

## Context

```text
$ printf "b\na\nc\n" | ono -c 'sort -r'
Ono-Sendai-E0201 type.mismatch cannot subtract int and null
```

ADR-0028 settled which `sort` runs: reached by objects it is the transform, reached by bytes it
is the coreutils program, and the executor gets that right — `sort` alone at a byte boundary is
coreutils. What it did not get right is the program's *arguments*.

`ArgMode::for_head` fixes a stage's argument mode at parse time from its head word (ADR-0009),
before anything knows whether the head resolves to the native command or to a program of the same
name. `sort` is a native head, so `-r` parsed as an expression — the negation of a field named
`r` — and the evaluator, having resolved the stage to `/usr/bin/sort`, evaluated it to build the
argv. Worse than the error: `diff -u /tmp/a /tmp/b` parses as *one* arithmetic term — a negation,
four divisions and two subtractions — and the program received it as a single argument
containing spaces.

The board asked whether a demand-driven native `sort` should carry GNU's flag vocabulary instead.
It should not: `-r`, `-k2,3`, `-t,` and `--field-separator` belong to a program that already
implements them, and a native transform growing a second, partial copy of them is how two tools
with one name become one tool that is wrong twice.

## Decision

**A stage the parser read in expression mode, and resolution handed to a program, has its
argument region re-read in words mode.** `ono_parser::words_arguments(text)` reads a fragment as
a words-mode stage's arguments — the same lexer, the same quoting, the same option syntax — and
the evaluator uses it for exactly this case, evaluating any option value against the fragment
the arguments came from.

Nothing else changes. A words-mode stage was already read as words. A native stage keeps its
expressions: `get process | sort pid desc` and `where cpu > 20` are unaffected, because they are
never handed to a program.

`sort -r` is therefore coreutils `sort` with the flag `-r`; `diff -u a b` is a flag and two
paths; `get process | sort cpu desc` is the transform.

## Consequences

- Every head that is both a native command and a program — `sort`, `diff`, `join`, `tail`,
  `uniq` — takes that program's flags when it is the program, without the registry knowing one
  of them.
- A quoted argument survives, because the words-mode lexer reads it: `sort -t"," -k2` reaches the
  program as three arguments.
- The parse must still succeed before the words are recovered. `sort -k1,2` is a parse error in
  expression mode — a bare `,` is not an expression — and stays one. Writing it as `sort '-k1,2'`
  or `exec:sort -k1,2` works. That is the residue of deciding the argument mode before
  resolution, and it is written here rather than left to be rediscovered.

## Alternatives considered

- **Teach the native `sort` GNU's flags.** Rejected, as above: two partial implementations of one
  vocabulary, and the flags would then also have to mean something for a stream of objects.
- **Decide the argument mode after resolution.** Rejected as a much larger change: resolution
  needs the parsed stage, and the parse needs the mode. Re-reading one region afterwards buys the
  same outcome for the case that is wrong.
- **Read the argument region's source text back as one string.** Tried and rejected: it recovers
  `-r`, but `-u /tmp/a /tmp/b` parses as a single expression, so its span is the whole run and the
  program still receives one argument with spaces in it.
