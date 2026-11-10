# ADR-0990: A spawn that fails before its first control names the spawn failure

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4.1 §2.3, §2.6, §16.3, §16.5, §54.1; base spec §31.79; ADR-0443, ADR-0445
- Decided by: agent (autonomous)

## Context

ADR-0445 builds the confinement report from a page the forked child writes as it installs each
control, and its *Consequences* name the case it left open: a child that dies between `fork` and
its first store leaves every row it owns at `not_attempted`, `unmet()` then picks the first
mandatory one, and the refusal says `an earlier mandatory control failed first` — about a control
nobody refused, with no earlier one to point at.

The case is not hypothetical. The standard library changes into the working directory, resets the
signal mask and runs every `pre_exec` closure registered before the supervisor's in that window.
A working directory that exists but cannot be entered (a mode-000 `work/` under the package's
private root) is enough, unprivileged, and the operator was told about `fd_hygiene`. The reason
was in hand all along: `Command::spawn` returns the child's `errno` as an `io::Error`, and
`sandbox::spawn` used it only when the report was confined, which in this case it is not (issue
#197).

## Decision

**When a spawn fails and no row of its report reads `failed`, the refusal's detail is the error
`spawn` returned.**

- The code stays the one the first unmet mandatory control maps to (§16.3's family:
  `plugin.confinement_failed`, `plugin.resource_limit_failed`, `plugin.no_new_privs_failed`), and
  the metadata keep `control`, `execution_tier` and `result: not_attempted`. A script that branches
  on "the launch was refused for confinement" still does; §2.6 still holds, because the control is
  still not claimed as applied.
- The message says what happened in order: `<package> was not started because the spawn failed
  before <control> could be installed: <spawn error>`, and `platform_detail` is the spawn error's
  text — the operating system's words, the same kind of content the row's own detail carries
  (§16.5's "MUST not expose secrets").
- When a row does read `failed`, that row is the cause and nothing changes: its own `errno` is the
  detail, and the spawn error is only its echo. Case `189-kuang-confinement-fail-closed` is that
  path, untouched.

`ConfinementReport::refusal(package)` keeps its signature; the spawn path calls a crate-private
`refusal_after(package, Some(&error))`.

## Consequences

The operator reads the reason (`Permission denied (os error 13)`) instead of a control that never
ran. Nothing that refuses today starts to succeed, and nothing that succeeds starts to refuse: the
change is to the words of an error that was already returned.

Encoded by `crates/ono-kuang-supervisor/tests/confinement.rs::should_name_the_spawn_failure_when_the_child_dies_before_its_first_control`
(a `pre_exec` registered before the supervisor's fails the child before the first control) and
acceptance case `145-kuang-spawn-failure-named` (a working directory the plugin cannot enter, from
outside, unprivileged).

## Alternatives considered

**`load.runtime_unavailable` for this case.** That is the code for an artifact that could not be
executed after the controls were in force. Here nothing is known to be in force, so the honest
family is the confinement one; switching codes on the cause would make a script's branch depend
on *where* in the child the failure fell.

**Name no control at all.** The report's invariant is about required controls; dropping the
control from the metadata would leave a `plugin.*_failed` code without the field a script reads to
learn which, for this one case only.
