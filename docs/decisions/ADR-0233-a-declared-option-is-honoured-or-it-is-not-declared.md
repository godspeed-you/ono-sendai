# ADR-0233: A declared option is honoured, or it is not declared

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §27 and §36 (the registries are the public contract), §36.5 (`spec-check` fails
  on contract drift), §50 (help and completion metadata are part of a delivered capability),
  §31.18 and §31.81 (a grant is made to one package; removal), §31.24 (the closed severity set),
  §31.48 (autonomy levels — "Ono controls policy")
- Decided by: agent (autonomous)

## Context

`docs/spec/commands/*.yaml` is where an option becomes real: help prints it, completion offers it,
`docs/reference/commands.md` documents it, and `spec-check` verifies that a command's verb, target
and provider capability all exist. Nothing verified the options. A command could advertise
`--keep-grants` in all four places while no code in the repository ever looked at the word, and a
user found out by the answer being wrong rather than by being refused. That is the failure mode
`get process --user root` and `get socket --listening` once had; both were fixed one at a time, and
the family stayed open because nothing stopped the next one.

Three options were in that state when this was written:

- **`remove plugin --keep-grants`** — "Retain the capability grants made to it." Nothing revoked
  them either way, so the flag changed nothing and the grants of a removed package stood.
- **`ask assistant --autonomy`** — the level was never looked at. `ask assistant` refuses because
  no assistant package is loaded (ADR-0111 §3), so the option was unreachable behind that refusal.
- **`get finding --severity`** — "Minimum severity", over a stream that is empty until an analysis
  runs, and no filter anywhere.

## Decision

**An option a command declares must be named by the shell's own sources, and `spec-check` fails
when it is not.** `contracts::check_declared_options` reads every non-planned command in
`docs/spec/commands/` and requires each of its option names to appear as a string literal —
`"tree"` as a provider query reads it, or `"--problems"` as a builtin reads its words — in
`crates/*/src` or `xtask/src`. Test sources do not count: a test naming an option proves the test
knows about it, not the shell.

This is a necessary condition, not a sufficient one; a static check cannot prove that a name that
appears is a name that is *obeyed*. It is exactly the guarantee `check_commands` already gives for
verbs, targets and capabilities, in the direction that was missing, and it closes the one failure
mode that recurred: an option nobody ever wrote code for.

**And the three options are honoured.**

- `remove plugin` revokes every grant standing for the package unless `--keep-grants` is given.
  A grant is made to one package (§31.18); a package that is gone takes its permissions with it,
  and one that comes back asks again. The grants are retained as revoked rather than deleted, so
  the audit trail still shows what the package was once allowed to do.
- `ask assistant --autonomy` is checked against the five levels of §31.48 — `L0 explain-only`,
  `L1 observe`, `L2 propose`, `L3 act-confirmed`, `L4 delegated-scope` — before anything else,
  and a word outside them is refused. §31.48 says a package declares what it supports but "Ono
  controls policy", and it rules out an unrestricted level: a turn must never run under a policy
  nothing can enforce, so the vocabulary is closed and it is the shell's.
- `get finding --severity` is a minimum over §31.24's closed set, and a sixth word is refused.
  A filter nobody applied answers with everything, which is the worst answer a filter can give.

## Consequences

- The next option added to a registry without an implementation turns the gate red in the same
  commit that adds it, rather than shipping as help text for behaviour that does not exist.
- Removing an option from a command is now as deliberate as adding one: the contract and the code
  move together or `spec-check` says so.
- The check is textual, and its two known limits are stated rather than hidden. It cannot see an
  option read through a computed name, and it cannot tell a name that is read from a name that
  merely appears. Both would need the option to be *observed at runtime*, which means running every
  command — including the mutating ones — inside the gate. That trade is not worth making for a
  guarantee that would still be partial.
- Encoded by `should_report_a_declared_option_no_implementation_names`,
  `should_accept_a_declared_option_an_implementation_names`,
  `should_not_accept_an_option_named_only_by_a_test` and
  `should_report_this_repositorys_own_registries_as_consistent_when_checked` in
  `xtask/tests/contracts.rs`; by
  `should_revoke_the_grants_of_a_removed_package_unless_asked_to_keep_them`,
  `should_refuse_an_autonomy_level_the_shell_does_not_define` and
  `should_refuse_a_severity_the_finding_schema_does_not_carry` in
  `crates/ono-cli/tests/plugins_missing.rs`.

## Alternatives considered

- **Record option reads on the `Query` and assert after each provider call that none went
  unread.** The strongest form, and it misreports: `ProcessProvider` reads `--sample` before it
  spawns and would pass, while an option read lazily inside a stream task would look unread until
  the stream had been drained. Turning that into a user-visible error would make a correct command
  fail; turning it into a debug assertion would put a panic in a library path (AGENTS.md §16).
- **Run every command with each option set and check the answer changes.** It cannot be done for
  the mutating half of the command table, and a check that covers only the read half is a check
  that the write half will drift past.
- **Require the option name next to a known reader call** (`option_value(`, `flag(`, `argument(`).
  Sharper in principle and wrong in practice: eleven options in this repository are read through
  helpers that take the name from a caller, and the check would have reported all of them.
