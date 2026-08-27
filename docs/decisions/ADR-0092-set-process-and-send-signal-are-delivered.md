# ADR-0092: `set process --priority` and `send signal` are validated and delivered

- Status: accepted
- Date: 2026-08-27
- Spec refs: §7.1, §9.1, §11.5, §16.5, §52; ADR-0015 T13, ADR-0068
- Decided by: agent (autonomous)

## Context

`docs/spec/commands/process.yaml` carried two `planned` entries with `validation_required:
true`: `ono.process.set` (spec §52 marks the process/`set` cell `M?` — "the semantic usefulness
must be validated rather than mechanically implemented for symmetry") and `ono.signal.send`
(spec §7.1 lists `signal` among `send`'s typical targets; §9.1 defines no such command, and the
registry note says it "overlaps `ono.process.kill --signal` entirely" and that "one of the two
should be withdrawn before either is stable").

`crates/ono-cli/tests/processes_missing.rs` asserts both as user-visible behaviour, with the
contract's own examples: `set process <pid> --priority 10` and `get process <pid> | send signal
SIGTERM`.

## Decision

### 1. `set process --priority` is validated: it is the one mutable attribute `ps` cannot set

Niceness is the one process attribute a user changes routinely (`renice`), the change is
reversible, and it is a mutation of an object the shell already resolves by identity — exactly
what `set <target>` means everywhere else. The cell is validated. `ono.process.set` becomes
`stable`, phase C, with `--priority` documented as a niceness from -20 to 19.

The procfs provider advertises `process.set` and answers `set` in its `act` (the mutation road
of ADR-0068 §3): the identity `(pid, started)` is confirmed first, as for a signal, so a
recycled pid is never reniced (ADR-0015 T13); an already-equal niceness is `skipped`, a change
is `success` with `changed: true`; a kernel refusal — raising priority without `CAP_SYS_NICE` —
is the target's `failed` row carrying `io.permission_denied` (E0302), and the pipeline exits 1
(ADR-0006). `set process <pid>` with no attribute is a `type.mismatch` row, because a mutation
that changes nothing by construction should say so rather than succeed.

`getpriority(2)`/`setpriority(2)` come from `rustix` (`process` feature), which the workspace
already builds transitively; `nix` 0.31 has no binding for them and the provider crate forbids
`unsafe`. Both calls sit behind the `Priorities` trait so a fixture can prove the refusal
paths without renicing anything.

### 2. `send signal` is kept, and it is the pipeline spelling of a signal

The two spellings are not the same operation seen from the pipeline: `kill process` names a
*process* and takes the signal as an option; `send signal` names a *signal* and takes the
processes from the pipeline. `get process | where name == "nginx" | send signal SIGHUP` reads as
what it does, and `send` is the verb spec §7.1 gives the act of emitting a signal. Withdrawing
it would leave `signal` a target nothing serves. `ono.signal.send` becomes `stable`, phase C;
the registry note that asked for one of the two to be withdrawn is superseded by this ADR.

Delivery is generic: the procfs provider claims the `signal` target beside `process` and treats
the operation `send` as it treats `kill`, reading the signal from the action's `signal`
argument. For that to be there, `ProviderMutation` now carries the bound **selectors** of a
command into the `Action` as arguments alongside the options whenever the targets arrived
through the pipeline — a selector that did not select is the operation's payload, and a
provider is the one to say what it means. Without piped input, `send signal SIGTERM` resolves
`signal SIGTERM` against the `signal` target, which names nothing: the E0301 failed row of
ADR-0068 §2.

## Consequences

- `crates/ono-cli/tests/processes_missing.rs`: `should_set_the_niceness_of_a_process`,
  `should_report_a_denied_priority_raise_as_a_failed_result`,
  `should_deliver_a_signal_to_the_process_arriving_through_the_pipeline`.
- `docs/spec/providers/linux-procfs.yaml` declares `process.set` and the `signal` target.
- Other attributes of a process (`--oom-score`, an affinity mask) are further options on the
  same command, each its own increment; none is promised here.

## Spec deviation

- Section: spec §52 (the `?` cells) and the registry note on `ono.signal.send`
- Text: "the semantic usefulness must be validated rather than mechanically implemented for
  symmetry" / "one of the two should be withdrawn before either is stable"
- Instead: both cells are validated by the argument above and delivered; neither spelling is
  withdrawn.
- Why: the spec asks for a validation, not a refusal; this ADR is it, and the tests are the
  evidence that the two commands answer different questions.

## Alternatives considered

- **Withdraw `send signal`, keep `kill --signal`.** Leaves `send` without its §7.1 target and
  the pipeline form without a spelling that names the signal first. Rejected.
- **A dedicated `SetPriority` implementation in the command crate.** ADR-0068 §3 exists so a
  provider delivers a mutation by advertising it; a per-command implementation would be the
  road it replaced. Rejected.
- **`libc::setpriority` behind `unsafe`.** The provider crate forbids `unsafe` and the workspace
  denies it; `rustix` is safe, already in the build, and reads the errno the same way. Rejected.
