# ADR-0944: A completion set says whether it is whole

- Status: accepted
- Date: 2026-10-07
- Spec refs: v0.4.1 §36.2, §61.4; §15.1; ADR-0252, ADR-0550
- Decided by: agent (autonomous)

## Context

v0.4.1 §36.2: "At the soft budget, completion MAY return a partial set marked incomplete. At the
hard budget it MUST stop additional discovery work and return what it has." The provider-backed
completer of ADR-0252 does stop at both budgets, and bounds what it reads (`CEILING`, 500 objects)
and offers (`OFFERED`, 50 candidates) — but `ValueCompleter::complete` and `ono_command::complete`
returned a bare `Vec<Candidate>`, so a set cut short by any of the four could not be told from a
whole one (issue #178). A user at the prompt saw an empty listing that meant "nothing yet" exactly
as one that meant "nothing at all".

## Decision

1. **`ono_command::Completions`** carries the candidates and `complete: bool`. `Completions::new`
   is a whole set, `Completions::partial` one cut short. It dereferences to `[Candidate]`, so a
   caller that only lists candidates reads it as before. `ValueCompleter::complete` returns it,
   and `ono_command::complete` returns it too, partial exactly when the value hook said any answer
   it gave was partial — what the registry answers alone is always whole.
2. **The shell's completer marks a set partial** when the soft budget answered before the
   providers did, when the hard budget stopped the provider walk, when a provider filled the
   query's `CEILING` (it may hold more), or when more candidates matched than `OFFERED` admits.
   The cache of ADR-0550 still keeps only whole walks, and remembers whether a cached read was
   capped, so a cached answer is marked the same as the read that filled it.
3. **The editor shows it.** `ono_editor::Completion` gains `incomplete`. A listing of a partial
   set ends with a line of its own, `…`, painted dim, never a candidate and never inserted; an
   empty partial set shows the marker alone ("nothing yet" is not "nothing"); a lone candidate of
   a partial set is listed with the marker rather than inserted as if it were the only answer.
4. The non-interactive completion surface (ADR-0945) reports the same flag as `complete`.

## Consequences

- §36.2's MAY is taken: the marker exists at the library, the shell and the editor.
- A `ValueCompleter` implementation outside the shell must now say whether its answer is whole;
  `Vec<Candidate>::into()` makes the common case a whole set.
- Tests: `crates/ono-command/tests/completion.rs`
  (`should_mark_a_completion_incomplete_when_the_value_hook_stopped_at_its_budget`,
  `should_mark_a_completion_complete_when_every_source_finished`);
  `crates/ono-editor/tests/completion.rs` (marker line, no marker for a whole set, lone candidate,
  empty partial set); `crates/ono-cli/tests/completion.rs`
  (`should_mark_the_answer_incomplete_when_the_provider_is_slower_than_the_soft_budget`,
  `should_mark_the_answer_complete_when_the_provider_answered_within_the_budget`, and the PTY
  test `should_end_the_listing_with_the_incomplete_marker_when_the_set_is_cut_short`);
  acceptance case `382-incomplete-completion-is-marked.case`.

## Alternatives considered

- **A sentinel candidate (`…`) inside the list.** A caller that inserts candidates would insert
  it; the flag is data, the marker presentation.
- **Wait for the hard budget before answering.** §36.2 makes the soft budget the keystroke's, and
  ADR-0252 already decided a keystroke never waits for a provider.
