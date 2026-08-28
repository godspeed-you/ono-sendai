# ADR-0141: Where a spatial command is implemented

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §10.2, §33.1, §33.2, §45.2, §45.3, §45.6, §46, §46.1; v0.2 §27.2 (native
  implementations registered against a command id); ADR-0124, ADR-0139
- Decided by: agent (autonomous)

## Context

§45.6 says `ono-cli` "should parse/dispatch spatial commands and own session current-place state,
but SHOULD NOT implement graph selection, identity reconciliation or map layout directly". It does
not say where the *command implementation* lives, and the shell has two plausible homes: the
registry-driven implementations of `ono-command` (`impls/`), where every other native command
lives, or the shell itself.

One fact decides it. A spatial object's identity needs the scope it was observed in, and §10.2
makes the **boot identity of the host** part of a process's identity — "PID alone MUST NOT be
treated as a persistent spatial identity". No `ono.process/1` record carries a boot id, and no
library crate may read one: `ono-spatial-core` is a data model, `ono-spatial-index` "MUST treat
providers as truth" (§45.2), and `ono-spatial-query` plans rather than observes (ADR-0139).

The host and boot an observation belongs to are session facts, and §46 puts session state in the
shell.

## Decision

**The spatial commands are implemented in `ono-cli` and registered into the command table beside
the built-in implementations**, against the ids `docs/spec/commands/spatial.yaml` declares.
`crates/ono-cli/src/native.rs` adds them to the table `ono_command::builtin_commands_for` builds.

What that implementation is allowed to do is exactly §45.6's list:

- read the arguments the registry bound;
- know which host and which boot the session belongs to (`spatial::local_scope`, read from
  `/proc/sys/kernel/hostname` and `/proc/sys/kernel/random/boot_id`, with an
  `unknown_boot` identity where either cannot be read — §2.17);
- ask the providers for the objects the query plan named;
- hand everything else on.

What it may not do, and does not: decide which record is which place (that is
`ono_spatial_index::ProviderBridge`, §45.2), decide which places answer a query and in which order
(that is `ono-spatial-query`, §45.3), or render anything (that is `ono-spatial-render`, §45.4).

The contract stays in `docs/spec/commands/`, so `help`, completion, `explain` and `spec-check` see
`find place` exactly as they see `get process` — the registry is the public surface whoever
implements it (v0.2 §27).

`phase: S` marks the v0.4 tranche, whose phases §50 numbers S1–S11 rather than lettering them like
v0.2 §37.

## Consequences

- `explain find place nginx` names `ono.place.find`, `help find place` prints the contract, and
  completion offers `--type`, `--where`, `--near`, `--limit` and `--all`. None of it is written
  twice.
- `ono_command::builtin_commands` — the table a library embedder gets — does not contain the
  spatial commands, and `unbound_stable_commands` reports them there. That is honest: an embedder
  without a session has no host, no boot and no current place, and a spatial command that invented
  them would be inventing identity.
- The index each spatial command uses is built for that command's own query and discarded
  (ADR-0139). The session-held index of §33.1 and the `SpatialSessionState` of §46 arrive with the
  navigation phase, which is the phase that has a current place to hold.
- `ono-cli` gains `ono-spatial-core`, `ono-spatial-index` and `ono-spatial-query` as
  dependencies; `ono-command` gains none.

## Alternatives considered

- **Implement in `ono-command/src/impls/`, beside `get` and `trace`.** Rejected: the host and boot
  identity would have to be read there, which puts a `/proc` read into the crate that is meant to
  be the registry and its bindings, and it would still be the wrong layer once §46's session state
  exists — the current place is the shell's.
- **A `place` provider in the provider registry, answered by `ProviderProducer`.** Rejected: the
  provider would need the provider registry to build its index, which is circular, and §2.16
  forbids the spatial layer from presenting itself as a source of system truth. A provider is a
  source of facts; the spatial layer composes them.
- **Read the boot identity in `ono-spatial-index`.** Rejected: §45.2 makes that crate a cache over
  what providers said, and a crate that reads `/proc` behind the providers' back is exactly the
  undocumented source of truth §2.16 forbids.
