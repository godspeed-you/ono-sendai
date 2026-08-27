# ADR-0068: The mutation seams — registry-dispatched `set`/`remove`, the ActionResult contract, and capability-driven verb binding

- Status: accepted
- Date: 2026-08-27
- Spec refs: §7.1, §11.5, §11.6, §16.1, §16.5, §27.1, §27.2, §28.8, §30, §43, §50;
  ADR-0006, ADR-0010, ADR-0012, ADR-0020 §9, ADR-0028, ADR-0029, ADR-0036
- Decided by: agent (autonomous)

## Context

Every family that mutates the system — files, services, processes, mounts, identities, routes —
drives on three shared seams, and each of them was subtly wrong in the same way: it answered
from the wrong layer.

1. **`set` and `remove` were claimed whole by the shell.** `crates/ono-cli/src/resolve.rs`
   listed both as builtins, so `set file x --mode 0755` answered `E0102 set has no target file`
   from `builtin.rs`, and `… | remove file | to json` was refused as "runs in the shell itself
   and cannot be a pipeline stage". The registry declares `ono.file.set`, `ono.file.remove`,
   `ono.service.set`, `ono.dir.set` and more (`docs/spec/commands/`); none of them could ever be
   reached, whatever a provider implemented.
2. **The ActionResult row did not match its own schema.** `docs/spec/schemas/action-result.v1.yaml`
   says `operation` is "the command id of the operation, e.g. `ono.service.restart`" and
   `error` is an `ono.error/1` — whose `code` is `Ono-Sendai-E0301` and whose `name` is
   `io.not_found` (`docs/spec/schemas/error.v1.yaml`). `stop process 1 | to json` wrote
   `"operation":"stop"` and `"error":{"error":{"code":"io.permission_denied",…}}`: the verb
   instead of the id, the error one level too deep, the dotted name in the `code` slot, and no
   `name` or `kind` at all. A target that did not exist produced `[]` — no row — and every
   pipeline exited 0 whatever its rows said, although spec §16.5 and ADR-0006 derive the
   aggregate status from the rows.
3. **Only four verbs could reach a provider.** `impls/mod.rs` bound `start|stop|restart|kill`
   to `ProviderMutation` by name; every other mutating verb — `set`, `remove`, `write`, `copy`,
   `move`, `mount`, `unmount`, `add`, `send` — returned `None` even when a provider advertised the
   capability its contract names. A family delivering `file.set` in its provider would still
   have had to edit the command crate.

## Decision

### 1. `set` and `remove` are builtins only for the shell's own state

`resolve::builtin_for(name, first_argument)` is the one predicate. `set env`, `set config` and
`remove env` are the shell's — they change the session, which no other layer can (ADR-0010,
ADR-0020 §9). Every other `set <target>` / `remove <target>` resolves like any native command:
the registry contract (`registry.resolve`), then the bound implementation, or the honest
`E0101 <id> is declared but this build implements nothing for it` (exit 127), or
`E0102`/target-not-found where nothing is declared. Pipeline position works
(`get mount / | unmount filesystem`, `… | remove file | to json`). The evaluator, the native
runner and the external fallback all consult `builtin_for`, so the three never disagree on who
owns a stage.

A configuration file may run neither kind. The refusal of ADR-0010 now stands in front of the
native path too: `set file` from `config.ono` is refused as "a configuration file may not run
this command", exactly as `touch` is. (Native producers in a config file were let through
before this change; that was a gap, not a promise.)

### 2. The ActionResult contract, as the schema writes it

- **A `failed` row fails the run.** The native runner inspects the outcomes of every mutation
  stage; if any is `failed`, the pipeline's exit status is 1 — after the rows are written, so
  `97 succeeded, 3 failed` is still three named rows *and* a non-zero status (spec §16.5,
  ADR-0006). `skipped` is not a failure.
- **A target that does not exist is a failed row.** When a selector resolves to nothing,
  `ProviderMutation` emits one `failed` row whose target names what was asked for
  (`ono.process/1[4000000]`) and whose error is `io.not_found` (E0301). An empty stream is the
  answer to "nothing matched a filter", not to "act on this thing that is not there".
- **`operation` is the command id.** The provider is still asked in its own vocabulary — the
  `Action` carries the verb (`kill`, `stop`, `set`), which is what every `act` matches on — and
  the row the shell writes carries `ono.process.kill`. The stamping happens where the outcome
  becomes a record, from the contract that ran.
- **`error` is an `ono.error/1` value, flat.** `to json` writes an error value as the fields
  `error.v1.yaml` declares: `code` (`Ono-Sendai-E0302`), `name` (`io.permission_denied`), `kind`
  (`permission`), `message`, `target`, `source`, `help`, `retryable`, `span`, `metadata`. No
  wrapping object, and never the dotted name in `code`. This is the data form of spec §33.5
  wherever an error sits — in a record's field, in an ActionResult, at the top level.
- **Across a link, the same row.** The remote provider's `resolve` treats an `io.not_found`
  failure with no objects as "nothing resolved", as the local provider does, so the shell
  writes the same E0301 row for a missing pid on either side (ADR-0036: nothing above the
  registry can tell).

### 3. A mutating verb binds when a provider advertises its capability

`builtin_commands_for(registry, providers)` binds a contract to `ProviderMutation` when its
verb is `mutating` in `docs/spec/verbs.yaml`, it names a `provider_capability`, its phase is
delivered, and a provider registered for its target advertises that capability
(`Provider::capabilities()`, vocabulary in `docs/spec/capabilities.yaml`). `builtin_commands`
without providers keeps binding only the verbs every `act` already speaks, for hosts that have
no provider set. The shell builds its table from its local providers.

`ProviderMutation` is the single implementation: one row per target, options and selectors or
piped objects carried in the `Action`, aggregate status derived by the shell from the rows. It
checks the capability again against the provider that will act, so a remote that lacks it
answers E0101 before anything is attempted. A verb no provider implements stays unbound and
answers E0101 — never a stub that fails halfway.

## Consequences

- Families deliver a mutation by implementing `act` and advertising the capability; nothing in
  the command crate changes. `set service`, `set file`, `unmount filesystem` are reachable the
  moment their providers say so.
- Scripts can rely on `$?` after a mutation, and on `.error.code` / `.error.name` in a row.
- `to json` of any error value changes shape (flat, with `name` and `kind`). The one test that
  encoded the wrapped form is corrected in a `test:` commit citing `error.v1.yaml`.
- `crates/ono-cli/tests/builtins.rs` proves the dispatch; `processes_missing.rs` and
  `remote_missing.rs` prove the row contract; `crates/ono-command/tests/mutations.rs` proves the
  generic binding reaches `act` with verb and options.

## Alternatives considered

- **Keep `set`/`remove` as builtins that forward to the registry.** Two dispatchers for one
  name, and the pipeline-position refusal would have stayed. Rejected.
- **Put the command id into the `Action`.** Every provider's `act` matches on the verb, and
  the remote protocol carries it; changing the vocabulary for a label the shell already knows
  is churn without a gain. Rejected.
- **Register every mutating contract and let `act` refuse.** `help` and
  `unbound_stable_commands` would then claim implementations that do not exist (spec §50).
  Rejected.
