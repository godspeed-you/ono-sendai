# ADR-0159: A referee check with an empty vocabulary reports, it does not pass

- Status: accepted
- Date: 2026-08-28
- Spec refs: §36.5, ADR-0009, ADR-0138
- Decided by: agent (autonomous)

## Context

`xtask::contracts::check_commands` cross-checks the `argument_mode` every command declares
against the heads ADR-0009 parses as expressions. It read those heads through
`expression_heads()`, which looked for `argument_modes.expression_heads` — a mapping with one
key. `docs/contracts/language.yaml` has never written that shape: it writes `argument_modes` as a
sequence of named modes, each carrying its own `heads`. The reader therefore found nothing
against this repository's own registry, and the check guarded itself with
`!expression_heads.is_empty()`, so it waved all 171 commands through in silence. Only the test
fixture, which had been typed in the mapping shape, ever exercised it.

Two things were wrong, and only one of them is a typo. The reader read a shape no file writes.
And the check treated "I know nothing" as "everything is fine" — the failure mode that makes a
referee worthless, because it is indistinguishable from a green result.

## Decision

1. **The registry's shape is the contract; readers conform to it.** `argument_modes` is a
   sequence of modes, each with a `name`, and the expression mode names its `heads`
   (`docs/contracts/language.yaml`). `expression_heads()` reads exactly that, and nothing else reads
   the mapping shape any more. The test fixture is written in the registry's shape, so a fixture
   can no longer certify a reader the real file would defeat.
2. **An empty vocabulary is reported, not tolerated.** `expression_heads()` distinguishes "there
   is no `language.yaml`" (`None` — registries arrive with the phase that needs them, AGENTS.md
   §14) from "`language.yaml` declares no expression heads" (`Some(∅)`), and `spec-check` reports
   the second as a problem against `docs/contracts/language.yaml`. A check that cannot fail is a check
   that is not running.
3. **Where a check compares two sets, prefer symmetric drift.** `contracts::drift` already
   reports in both directions, which is why the ten spatial vocabularies could not have gone
   blind the same way: an empty declaration there produces a problem per implemented name rather
   than silence. New cross-checks are written that way when they can be.

## Consequences

- The argument-mode check is armed against this repository's own registry, and
  `should_reject_an_argument_mode_that_disagrees_with_the_grammar_this_repository_declares`
  loads `docs/contracts/language.yaml` verbatim so it stays armed. The 171 committed commands pass it.
- `language.yaml` may not silently lose its expression-mode heads: `spec-check` turns red.
- A future reader of `argument_modes` has one shape to support. The mapping spelling is gone.

## Alternatives considered

- **Teach the reader both shapes** (what `check_expression_options` did). Rejected: a mapping and
  a sequence cannot both be the contract, and supporting both is what let the fixture and the
  registry disagree for as long as they did without either being wrong.
- **Rewrite `docs/contracts/language.yaml` into the mapping shape.** Rejected: the file is the public
  contract and is read by the generated reference; the reader is the cheaper and more honest
  thing to change.
- **Keep the `is_empty()` guard.** Rejected: it is the defect. A referee that abstains when it has
  no evidence reports the same thing as a referee that checked and found nothing wrong.
