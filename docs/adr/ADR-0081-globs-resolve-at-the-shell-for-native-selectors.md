# ADR-0081: Globs resolve at the shell before a native command sees its selectors

- Status: accepted
- Date: 2026-08-27
- Spec refs: §9.1, §11.6, §16.5, §17.3; ADR-0009, ADR-0019
- Decided by: agent (autonomous)

## Context

Spec §17.3 says native commands "receive resolved objects where possible" and gives
`remove file *.tmp` as the case: the command "can know exact targets before mutation". ADR-0019
fixed glob expansion for programs — escape, tilde, variables, then globs, a pattern that matches
nothing is refused — but a native stage bound its arguments straight from the parser's words, so
`get file *.txt` handed the provider the literal `*.txt` and got `io.not_found` for a file that
was never meant.

## Decision

1. **A native stage's bare words are glob-expanded before binding**, with the same rules and
   the same `glob` as a program's argv (`crate::expand::expand_globs`): only an *unescaped*
   pattern character in a bare word expands; a quoted `"*.md"` is an expression the parser kept
   as text and stays literal, which is what `find file . --name "*.md"` needs. Each match takes
   the span of the word it replaced, so diagnostics still point at what was typed. A pattern
   matching nothing is refused with `io.not_found` before anything runs (ADR-0019), never passed
   on as a filename.
2. **A `path` selector that can name several objects is `repeatable`** in its contract, so the
   matches bind to one selector as a list (`docs/contracts/commands/file.yaml`: `get file`, and the
   mutations of ADR-0082). The provider walks every root a list names, and one root that is not
   there is that root's failure on the stream, not the walk's (spec §16.5).

## Consequences

- `get file *.txt | select name` returns one File record per match and nothing on stderr;
  `remove file *.txt` counts its targets before acting, which is what the bulk guard of
  spec §11.6 needs (ADR-0082).
- The exact same expansion serves programs and native commands; there is one glob in the shell.
- `explain` and the pre-flight `check` still see the unexpanded word — they describe the plan,
  not the matches — and a later increment may show the resolved count in a plan.
- Tests: `crates/ono-cli/tests/files_missing.rs::should_resolve_a_glob_to_exactly_the_matching_files_when_getting_files`.

## Alternatives considered

- **Let the provider match the pattern.** Every provider with a path selector would need its
  own globbing, and a remote provider would glob on the wrong machine. Rejected.
- **Expand every word (variables, tilde) for native stages too.** Native words already carry
  variables as expressions the evaluator runs; re-substituting them here would double-expand.
  Only the pattern words are touched.
