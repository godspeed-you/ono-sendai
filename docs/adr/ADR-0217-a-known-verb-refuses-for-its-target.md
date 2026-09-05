# ADR-0217: A verb the registry knows refuses for its target, not for itself

- Status: accepted
- Date: 2026-08-29
- Spec refs: §6.5, §15.4, §17.1, §43
- Decided by: agent (autonomous, `close-data`)

## Context

ADR-0011 puts the command registry at step 4 of resolution and `PATH` at step 5. `CommandRegistry::resolve`
already distinguishes the two ways it can miss (spec §15.4): `resolve.command_not_found` when
nothing answers to the head word at all, and `resolve.target_not_found` — "`trace` has no target
`group`", with the nearest target it does have — when the verb is known and only the target word
is wrong.

The evaluator discarded that distinction. `native_contract` reduced the registry's answer to
`Option` with `.ok()?`, resolution fell through to `PATH`, and the shell reported

```text
Ono-Sendai-E0101 resolve.command_not_found command not found: trace
  did you mean: strace, tac, true
```

for a verb it implements. `stop zzz foo` answered the same about `stop`. The user is told the
wrong word is missing and offered three unrelated programs.

## Decision

**When resolution has failed everywhere and the registry's refusal was
`resolve.target_not_found`, that refusal is the answer.** The evaluator asks the registry once
more at the point where `PATH` came back empty, and reports `Ono-Sendai-E0102` naming the target
and its near misses instead of `E0101` naming the verb.

The condition is exactly `resolve.target_not_found`: an unknown head word — `frobnicate group` —
still answers `resolve.command_not_found` with `PATH` suggestions, and a head word that *is* on
`PATH` never reaches this at all, so `find . -type f` stays findutils (acceptance case 087).

`explain` reports the same thing, as a note on the stage: spec §17.1 makes the plan what the line
would do, and what this line would do is refuse.

## Consequences

- `trace group root`, `stop zzz foo` and every other known verb with an unknown target name the
  target, at `Ono-Sendai-E0102`, with `help <verb> lists its targets` or the nearest target.
- The registry is consulted twice on the failing path only. Nothing changes for a line that runs.
- A program on `PATH` whose name collides with a verb keeps winning where the registry has no
  command for the written target — the search order of ADR-0011 is unchanged.

## Alternatives considered

- **Carry the registry's error out of `native_contract`.** Rejected: that function answers "is
  this stage native?", and turning it into a fallible one would make every caller decide what to
  do with an error that is only interesting when everything else has already missed.
- **Report `resolve.target_not_found` before searching `PATH`.** Rejected: it inverts ADR-0011's
  order and would break `find`, `look`, `sort` and every other verb that shares a name with a
  program.
