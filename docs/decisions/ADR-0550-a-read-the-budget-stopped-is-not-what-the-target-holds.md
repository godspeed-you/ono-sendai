# ADR-0550: A read the budget stopped is not what the target holds

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.2 §15.1 (completion is semantic and provider-aware), §34 (the completion budget);
  v0.4.1 §36.2 ("at the hard budget it MUST stop additional discovery work and return what it
  has"), §2.7 (a test reports execution truth); ADR-0252, ADR-0517
- Decided by: agent (autonomous)

## Context

`crates/ono-cli/tests/completion.rs::should_stop_discovery_at_the_hard_budget_and_answer_what_it_has`
failed the GitHub runner at its second assertion and one workspace run in twelve on a loaded
development machine, while passing every run in isolation. ADR-0517 had already scaled the window
that assertion waits in, because the window is a watchdog on a cache filling rather than the
budget under test. Scaling was not enough, and it was not enough for a reason that has nothing to
do with how long the window is.

The test asks the same question twice: once with a budget of one nanosecond, which must admit no
discovery, and then with the budgets the catalogue declares, which must answer with this machine's
accounts. ADR-0252's cache sits between the two, and it is process-wide.

Both reads run on detached threads and both write to that cache when they finish. The nanosecond
read finds nothing — its deadline is already past when the thread starts — and writes **that**
into the cache under the target `user`, where `FRESH` keeps it for five seconds. Whichever of the
two threads writes last wins. The patient read is slower, so it normally wins; on a busy machine
the impatient one can be scheduled late enough to win instead, and then every completion for
`user` reads an empty answer from the cache for the next five seconds — longer than any window
the second assertion could reasonably wait in, scaled or not.

So the flake was not a window that was too short. It was **the product answering a keystroke with
the truncation a previous keystroke's budget caused**, and a longer wait would have hidden it
without changing it.

The user-visible form needs no test harness: press Tab on an expensive target, let the first read
run out of budget, and the shell offers nothing for five seconds — including to the keystrokes
that had all the budget in the world. ADR-0252's own doc comment says the opposite is intended:
"The first Tab for a target … may run out of budget before the answer lands — it is then in the
cache, and the next keystroke has it."

## Decision

**What a budget stopped is how far one keystroke got. What a completed read found is what the
target holds. Only the second is remembered.**

`read` returns its values together with whether it asked every provider registered for the target.
The flag is false in exactly two places: the hard deadline stopped the walk with providers left to
ask, and there was no runtime to ask them on. `objects` writes to the cache only when the flag is
true; the values still go back to the caller either way, because §36.2 requires the keystroke to
be answered with what discovery had.

Nothing about §36.2 changes: discovery still stops at the hard budget, and the answer for *this*
keystroke is still whatever had arrived. What stops is a fragment outliving the keystroke that
produced it.

## Consequences

Easy: a completer that ran out of budget costs its own keystroke and nothing after it. The
nanosecond-budget probe the suite uses is no longer able to silence the shell for five seconds,
and neither is a real Tab on a machine that was busy for 150 ms.

Hard: on a host where the hard budget genuinely cannot cover the providers of a target, nothing is
ever cached for that target and every keystroke pays the full read again, up to the soft budget.
That is the honest behaviour — a cache of fragments answers quickly and wrongly — and it is
bounded by the same two budgets as before. A target whose read cannot finish inside 150 ms is a
performance problem to measure (§33.2) rather than one to paper over with a stale fragment.

Also worth stating: this was the second flake in this tranche whose cause was in the product
rather than in the test (ADR-0549 was the first). ADR-0517 correctly identified the window as a
watchdog and correctly scaled it; scaling a watchdog cannot fix a race that is not about time, and
the residual failure was the evidence that something else was left.

Encoded by `crates/ono-cli/tests/completion.rs::should_not_answer_the_next_completion_with_what_a_budget_stopped`,
which lets the impatient read finish before the patient one starts, so the outcome is decided by
the rule rather than by which thread was scheduled first. It failed deterministically before this
change and passes 30 runs in 30 beside a load average of 15 on 8 processors after it.

## Alternatives considered

**Widen the window the second assertion waits in.** The poisoned entry lives five seconds and the
race is a race, so the window would have to exceed `FRESH` to be reliable — a test that waits for
a defect to expire, asserting nothing about the product. AGENTS.md §14.

**Remember only non-empty answers.** It would fix this test and leave the general case wrong: a
read the budget stopped after two of five providers is not empty and is not the answer either.
The distinction the cache needs is "did discovery finish", not "did discovery find anything" — a
target that legitimately holds nothing must be able to cache that.

**Give the cache per-entry provenance and let a complete read replace a truncated one.** More
mechanism for the same outcome. The truncated entry has no reader that wants it: the keystroke
that produced it was already answered directly.

**Shorten `FRESH`.** It would narrow the window in which the defect is visible without removing
it, and five seconds is ADR-0252's deliberate trade between a burst of Tabs and an account created
a moment ago.
