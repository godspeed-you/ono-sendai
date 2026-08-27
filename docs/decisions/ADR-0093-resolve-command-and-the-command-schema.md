# ADR-0093: `resolve command` is the shell's answer, and `ono.command/1` names the stage

- Status: accepted
- Date: 2026-08-27
- Spec refs: §6.5, §7.1, §15.4, §27; ADR-0011, ADR-0012, ADR-0070
- Decided by: agent (autonomous)

## Context

`docs/spec/commands/meta.yaml` declares `ono.command.resolve` — "report what a head word
resolves to, without running it" — with output `ono.command/1`, a schema that was deferred since
Phase D and existed only as a hand-built table inside `ono-command`'s `get command`. Spec §6.5
requires the resolution order to be "explicit and inspectable"; ADR-0011 fixed the order —
keyword, function, alias, native, external, not found — and ADR-0070 made functions and aliases
real. The registry cannot answer `resolve command hi`: a function or an alias lives in the
session, and `PATH` is the session's environment. The header of `impls/mod.rs` already says so
and leaves the command unbound.

Two things had to be decided: where the command runs, and what its record looks like.

## Decision

1. **`resolve command` is answered by the shell.** `crates/ono-cli/src/meta.rs` claims the stage
   by its head word and target (`resolve command`, or `ono:resolve command`), builds the one
   record from `crate::resolve::describe`, and seeds the rest of the pipeline with it exactly as
   a plugin's answer or a `$value` head seeds one. The contract in `meta.yaml` still supplies
   help, completion and typing; the evaluator supplies the value. The same seam serves
   `get config` and `set config` (ADR-0094).
2. **`describe` walks the evaluator's own order.** Keywords are the statement keywords of
   `docs/spec/language.yaml`; functions and aliases are the session's definitions; natives are
   the registry's verbs and the shell's builtins; externals are found on `PATH` with the
   executable-bit rule. A forced namespace (`ono:`, `fn:`, `exec:`) answers from its stage alone
   and is never retried elsewhere; a miss is `resolve.command_not_found` (E0101), with
   edit-distance suggestions in the unqualified case (spec §15.4).
3. **`ono.command/1` is written down** in `docs/spec/schemas/command.v1.yaml` and removed from
   `deferred.yaml`. One schema serves both `get command` and `resolve command`: a `kind` field
   (`keyword | function | alias | native | external`) names the resolution stage, `path` carries
   the absolute path of an external hit, and the registry fields — `id`, `verb`, `target`,
   `stability`, … — are nullable, null for anything that is not a registry entry. Identity is
   `spelling`. `get command` sets `kind = native` on every entry. The hand-built schema in
   `ono-command` is gone; the crate reads the same contract everyone else does.

## Consequences

Easy: `resolve command ls` prints `external` and `/usr/bin/ls`; `resolve command hi` after
`fn hi() { … }` prints `function`; `resolve command ono:ls` refuses honestly. The order is
reported by the code that performs it, which is what §6.5 asks for and what `explain` already
does for whole pipelines.

Hard: a builtin (`cd`, `set env`) reports as `native` with `verb = cd` and no id, because the
shell's own commands have no registry entry; the summary says so. A KUANG/11 package namespace
(`pkg:cmd`) is not described yet — it resolves as not found — and lands with the plugin family.

Encoded by: the `resolve command` cases of `crates/ono-cli/tests/meta_config_missing.rs` and
acceptance case `041-config-and-resolve`.

## Alternatives considered

- **Binding `ono.command.resolve` in the registry** — rejected: it would report a resolution
  the shell does not perform, missing every function, alias and `PATH` entry.
- **A separate `ono.resolution/1` schema** — rejected: the contract already names
  `ono.command/1` as the output, and a resolution of a native command *is* the command record.
- **Keeping `id` required and inventing `ono.shell.cd` ids** — rejected: an id that is in no
  registry is a claim `help` cannot honour.
