# ADR-0271: A place says what it offers, and refuses what it does not

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.4 §6.2, §11.1, §38.2, §40, §2.17
- Decided by: agent (autonomous, `close-spat`)

## Context

Three findings from one dogfooding session, all of them the same shape — the shell knew the answer
and did not say it:

1. `near --relation process` answered `E0202 'near' has no option '--relation'` and listed the
   four options it takes, never mentioning that a relation is the *positional* selector
   `near process`, which is the spelling that works.
2. `enter process 1; near socket` printed nothing at all with status 0. A process has `sockets`,
   not `socket`, so "there is no such exit" and "this exit is empty" were the same answer.
3. `help here` did not exist (`E0101 … did you mean 'where'?`), although §38.2 asks for it and
   §38.1's `help spatial` had already been delivered.

## Decision

**1. An unknown option that names a declared selector says so.** `CommandContract::unknown_option`
checks the selectors before it lists the options, and answers "`relation` is a positional selector:
write `near <relation>`". It is registry-driven, so every command gets it: the check is that the
word *is* something the command takes, written the other way round.

**2. `near <relation>` refuses a relation the current place does not offer.** `spatial.no_relation`
(E1004), naming the exits this place actually has. The exits come from the place's own
neighbourhood rather than from the global relation vocabulary — `sockets` is an exit of a process
and `processes` is an exit of COMPUTE — and they are read only on the way to the refusal, so the
answer path costs nothing. `follow` has always made this distinction; `near` did not.

**A relation the place declares and has no neighbour in is still an empty stream**, not a refusal:
the name was understood, which is the `find` precedent of ADR-0210 and §40's own line between
`spatial.no_relation` and an empty answer.

**3. `help here` is a page about the current place.** §38.2: "At any place: `help here` … SHOULD
show spatial operations supported by that place." It cannot be a registry topic like `help spatial`,
because the answer is a fact about where the session is standing and the command registry knows
nothing about that — so it is answered in the CLI, from the live neighbourhood, and it lists:

- every exit with what is behind it, spelled the way that place is traversed — `near`/`follow` for
  a relationship, `enter` for a canonical child, because §11.1 makes hierarchy not the graph and
  telling a reader to `follow COMPUTE` would be telling them to do something the shell refuses;
- a permission or support state where the provider gave one, rather than a count it does not have
  (§2.17, §35.2);
- `up` only where there is a parent and `back` only where the trail has one.

**4. Help topics complete.** `help <tab>` had no answer at all, because a topic is a vocabulary of
`help` alone and no contract carries it. `ono_command::help::topics()` is now the one enumeration
of the browsing pages — `builtin_topic`'s match cannot be listed — and completion offers those plus
every verb and every command spelling.

## Consequences

- `help here` is answered in `ono-cli` rather than in `ono-command`, which is the layering §45.6
  already fixes: the CLI owns the session state and the command layer must not reach into it.
- The positional-selector hint changes the help line of every command with selectors, not only
  `near`. That is the point; nothing else was true about them before.
- Encoded by `ono-cli/tests/spatial_navigation_missing.rs::should_name_the_positional_spelling_when_a_selector_is_written_as_an_option`,
  `::should_refuse_a_relation_the_place_does_not_offer_rather_than_answering_nothing`,
  `::should_answer_an_empty_stream_for_an_exit_that_exists_and_holds_nothing`;
  `ono-cli/tests/spatial_help.rs::should_name_the_relations_of_the_current_place_when_help_here_runs`,
  `::should_say_what_the_root_place_offers_when_help_here_runs_there`,
  `::should_offer_here_among_the_topics_help_lists`;
  `ono-command/tests/completion.rs::should_offer_the_help_topics_when_a_topic_is_being_typed`,
  `::should_narrow_the_help_topics_to_the_prefix_typed`; and acceptance case
  `102-spatial-look-near` cases `s4v`, `s4w`, `s4x`.

## Alternatives considered

- **Printing "no neighbours" to stdout for an empty exit** — would put prose into a pipeline that
  a `| count` reads as data.
- **Making `help here` a `builtin_topic` arm** — `builtin_topic` takes only the command registry,
  and giving it the spatial session would make the command layer depend on the shell's state.
- **Listing every relation the vocabulary knows in the refusal** — the answer to "what can I do
  here" is not "what can anyone do anywhere".
