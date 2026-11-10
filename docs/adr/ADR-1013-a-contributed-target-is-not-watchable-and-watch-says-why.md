# ADR-1013: A contributed target is not watchable, and `watch` says why

- Status: accepted
- Date: 2026-11-10
- Spec refs: §18.2, §18.3, §31.14, §31.23, §50; v0.9 (live view integration, "without a second
  live-data model"); `docs/contracts/kuang/contributions.v1.yaml` (`target.answer`); issue #224 (6);
  ADR-0024, ADR-0235, ADR-0588
- Decided by: agent (autonomous)

## Context

Issue #224 (6) also asked about `Provider::subscribe` for a contributed target. ADR-0588 declined
it: `watch <target>` needs an event vocabulary per target, and inventing one per contributed
target was a larger decision than that increment needed. The question now was whether a minimal,
contract-backed design exists.

What `watch` needs, from the current code:

1. **An event schema per target.** `WatchCommand` and `watch_events` resolve `ono.<target>-event/1`
   from `ono_value::builtin_schemas()`, a static set compiled into the binary. The watch runtime
   itself is generic — it subscribes where the provider can and otherwise polls `snapshot` and
   diffs by identity (ADR-0024, ADR-0235) — so a contributed target would not need `subscribe` at
   all to be watched by polling. What it lacks is the envelope schema.
2. **A `watch <target>` command contract.** Each watchable core target has one
   (`ono.process.watch` …) in `docs/contracts/commands/`; a package declares only the `get` of its
   targets, and `plugin_registry` synthesises nothing else.
3. **A bound implementation.** `ono-command`'s command table binds `WatchCommand` by core contract
   id, once per process, before a lazily loaded package has registered its providers.

So the minimal design is real but not small: a derived `<package>.<target>-event/1` schema
registered at the handshake (a runtime schema registry the watch path does not consult today), a
synthesised `watch` contract per target in the plugin registry, and binding it in a table that is
built before the package loads. Each part lives in a component this increment does not own
(`ono-command`'s watch and table, `plugin_registry`), and together they are a decision about the
event vocabulary, exactly as ADR-0588 said. A package-declared `events:` field on the target would
add a third way to describe a live answer beside `answer: unbounded` and the event-shaped records
`live::apply` already folds — the "second live-data model" v0.9 forbids.

Meanwhile the decline was not explicit. `PluginProvider` inherited the default `subscribe` ("cannot
watch for changes"), and at the shell `watch echo-item` had no entry, so the word went to the
`watch` program on `PATH`, which re-ran a command named `echo-item` every two seconds until Ctrl-C.

## Decision

**The decline stands. A contributed target is not watchable, and both doors say so with the same
structured refusal.**

- `PluginProvider::subscribe` returns `provider.unsupported`: "`<target>` cannot be watched:
  `<package>` contributes it, and a contributed target declares no event vocabulary to watch it
  with", with help naming `get <target>` and `answer: unbounded` (ADR-0588) — the one live read a
  package has.
- `watch <contributed target>` at the shell resolves to an entry that `plugins::contributed_command`
  builds for it and never registers — `help`, completion and `get command` do not offer a command
  that only refuses (spec §50) — and `invoke_contributed` answers it with that refusal before any
  package is loaded: the reason is a fact of the declaration, so nothing is started to say it.
- A target word that no package contributes is untouched: `watch <anything else>` resolves as it
  did.

## Consequences

The refusal is one function, `plugin_provider::unwatchable`, so the registry and the shell cannot
disagree about why. `contributions.v1.yaml` says, on `target.answer`, that an unbounded target is
a contributed target's only live read and that `watch` refuses.

Revisit when an event vocabulary for contributed targets is decided: the polling watch runtime
already exists, and the missing parts are the three listed in the context. That decision should
also say whether an unbounded target's event-shaped records and a watched target's events are the
same envelope, so that a package has one way to be live.

`watch <contributed target> &` in the background still reaches the external program, because the
contributed-stage dispatch in `eval/pipeline.rs` runs only in the foreground; the same is true of
every contributed entry and is not this decision's to change.

Encoded by `crates/ono-cli/tests/plugin_provider.rs::should_refuse_to_subscribe_to_a_contributed_target_and_say_why`,
`crates/ono-cli/tests/plugin_targets.rs::should_refuse_to_watch_a_contributed_target_and_say_why`
(it hung on the external program before), and acceptance case
`173-kuang-contributed-target-is-not-watchable`, which also shows the package was not started.

## Alternatives considered

**Implement the polling watch with a derived event schema.** Rejected for this increment: it
needs a runtime schema registry in the watch path and a contract per target bound after load, in
components another decision owns; see the context.

**Register `watch <target>` in the plugin registry so `help` shows it.** Rejected: a registered
command that can only refuse is what spec §50 forbids, and `get command` would list a capability
the shell does not have.

**Leave the fall-through to the external `watch`.** Rejected: it answers a different question
(about a program named after the target) and never ends, which is the opposite of a structured
refusal.
