# ADR-0247: The §27.2 binding check runs against the real registry

- Status: accepted
- Date: 2026-08-29
- Spec refs: §27.2 ("CI verifies every stable command is bound to an implementation"), §36.5,
  §37, §50; ADR-0068 §3, ADR-0075, ADR-0083, ADR-0092, ADR-0010, ADR-0020 §9, ADR-0104,
  ADR-0141; v0.4 §45.6
- Decided by: agent (autonomous)

## Context

`ono_command::unbound_stable_commands` was written in phase D to answer spec §27.2's question,
and it was never asked. A repository-wide grep found two callers, both tests: one builds a
synthetic table holding a single command, the other filters the answer down to phases A–D. No
call existed in `crates/ono-cli/`, in `xtask/` or in `scripts/`, so the check that decides
whether the product surface is real ran against nothing the product actually uses, and a command
of phase E through J or S could lose its implementation without a single test noticing.

The obstacle is that "bound" has three different meanings in this shell, and only one of them is
visible to a library:

* `ono_command::builtin_commands` binds the transforms, the producers, `watch`, `trace`,
  `inspect` and the meta commands — everything a library can implement without a system around it;
* a *provider* binds every mutating verb and every content verb, and only where it advertises the
  capability the contract names (ADR-0068 §3, ADR-0083) — so a table built without providers binds
  none of them, deliberately, because a stub that always fails is worse than an honest absence
  (spec §50);
* `ono-cli` binds the rest: the context stack of §14.1 (ADR-0075), configuration (ADR-0010), the
  session's own scope (ADR-0020 §9), the KUANG/11 lifecycle, the link table (ADR-0104) and the
  fourteen spatial verbs, which v0.4 §45.6 keeps in the shell because a place belongs to a host
  and a boot no library crate knows (ADR-0141).

Fifty-two of the 106 stable commands of a delivered phase fall into the second and third groups.
A check that simply demanded they all be in the library table would be wrong; a check that
excused whole categories — "any mutating verb may be unbound" — would excuse the very drift it
exists to catch.

## Decision

**`cargo xtask spec-check` runs the §27.2 check against the embedded registry and the library
table, with an enumerated list of the commands something else binds.**

`xtask::bindings::check_bindings(registry, is_bound)` reports two things, and both are failures:

1. a stable command of a delivered phase that neither the table binds nor `BOUND_ELSEWHERE` names;
2. a `BOUND_ELSEWHERE` entry that the table binds after all — an excuse that has expired.

`BOUND_ELSEWHERE` is a flat list of fifty-two ids, each with the one thing that does bind it, in
four groups: a provider advertising a named capability; the session's context stack; the
evaluator's configuration and scope; and `ono-cli` itself. It is a list rather than a rule,
because every rule that would generate it also generalises it — and "any mutating verb may be
missing" is not a statement anyone should be able to hide behind.

`is_bound` is a closure rather than a `CommandTable`, so the check can be driven with a table
that has lost a binding. That is the only way to know it would notice, and
`xtask/tests/bindings.rs::should_report_a_stable_command_that_lost_its_implementation` is that
proof.

`xtask` gains a dependency on `ono-command` for this. That is the point: the check has to see the
real registry and the real table, and a copy of either inside `xtask` would be a second source of
truth that drifts on its own.

## Consequences

Losing the implementation of any of the fifty-four library-bound stable commands — every
transform, every producer, every `watch`, `trace`, `inspect` and meta command — now turns the gate
red naming the command. That is new: nothing checked it before.

The fifty-two commands on the list stay out of this check's reach, and their bindings are proven
where they are implemented, by the suites that own them: `files_missing.rs`, `storage_missing.rs`,
`containers_packages_missing.rs` and `processes_missing.rs` for the provider-bound verbs,
`context.rs` for `enter`/`leave`, `meta_config_missing.rs` for configuration, `plugins.rs` and
`plugins_missing.rs` for KUANG/11, `remote_missing.rs` for the link table, and the `spatial_*`
suites for the fourteen spatial verbs. The list's own honesty is checked in the other direction:
the day one of them becomes library-bound, the entry is reported as stale.

The maintenance cost is one line per new stable command that the library does not bind.

## Alternatives considered

- **Building the shell's real table in `xtask`.** It needs a `Session`, a provider registry and a
  tokio runtime, one of whose providers connects to D-Bus. A contract check that opens a socket is
  a contract check that fails on a machine without systemd.
- **Deriving the excuses from the contract data** — "a mutating verb with a `provider_capability`
  may be unbound". Rejected above: it would excuse `ono.service.stop` losing its binding, which is
  exactly the drift §27.2 names.
- **Running every stable command against the real binary and asserting the answer is not
  `resolve.command_not_found`.** Attractive, and rejected for now: argument binding happens before
  the table lookup, so each of the fifty-two needs a hand-written, argument-valid, side-effect-free
  invocation, and several of them (`enter group`, `enter service`) resolve differently depending on
  what exists on the machine. A machine-dependent smoke test would be a worse referee than the
  suites that already cover those commands with fixtures.
