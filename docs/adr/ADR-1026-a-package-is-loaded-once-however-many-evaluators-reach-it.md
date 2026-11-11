# ADR-1026: A package is loaded once, however many evaluators reach it at the same moment

- Status: accepted
- Date: 2026-11-10
- Spec refs: spec §18.4, §31.8, §31.10, §31.37, §31.68, §31.72; ADR-0110, ADR-0952, ADR-0995;
  v0.6.5 review finding R-F1
- Decided by: agent (autonomous)

## Context

ADR-0995 made a background job run packages in the session's one KUANG/11 host. The lazy load of
spec §31.68 — a contributed command reached before its package runs — asked the host "is it
loaded?", and when it was not, loaded it and kept the instance with `add_instance`. Neither the
question nor the load was held against anyone: the job's evaluator and the shell's are two threads
over one host, so both could find the package unloaded and both could load it. `add_instance`
treats a second instance as a reload (spec §31.72, ADR-0110 §3): it kept the newer one and the
caller shut the older one down — under the invocation the other evaluator had just started in it.

The review reproduced it in two of three runs: `get echo-tick | take 200 | count | to json &`
beside `get echo-item | count | to json` ended one side with `Ono-Sendai-K11201 runtime.trap the
instance was shut down`, and the trail read `load, load, unload` for what should have been one
instance.

## Decision

**Loading a package is one turn at a time per package id, across every evaluator of the session,
and a lazy load that waited for the turn loads nothing if the package is loaded by then.**

1. The host keeps one turn per package id (`Host::load_turn`), in the tables the session and its
   jobs share. `load_plugin_with` takes it after the package id is resolved to its canonical form
   and holds it through validation, negotiation, spawn and `add_instance`.
2. A lazy load — the silent one behind a contributed command or a spatial lookup — checks, once it
   holds the turn, whether the package is loaded now. If it is, it returns and the caller runs in
   that instance. The losing evaluator never starts a process, so there is nothing to drop and no
   running instance is ever replaced by a race.
3. `load plugin <id>` typed by the operator takes the same turn and still reloads: replacing the
   instance is what the operator asked for (ADR-0110 §3), and the turn only stops it interleaving
   with a lazy load of the same package.
4. Waiting for the turn is bounded and interruptible. The holder's load is bounded by the
   supervisor's handshake deadline; the waiter polls the turn and gives up on the line's own
   interrupt (Ctrl-C in the foreground, `kill %N` in a job, a leaving shell), so a waiting job is
   never what keeps the shell from leaving. The turn is never held across an invocation, and no
   code that runs while it is held loads a package, so it cannot be taken twice by one thread.

## Consequences

One instance per package per session holds under concurrency, not only sequentially: `get plugin`
and the trail account for one load per instance however many evaluators raced for it. The turn is
per package, so loading two different packages in parallel still runs in parallel.

A lazy load that lost the race returns the moment the winner has kept its instance; it does not
report anything, because it loaded nothing.

Encoded by `crates/ono-cli/tests/plugin_instances.rs`
`::should_load_a_package_once_when_a_job_and_the_shell_both_load_it_lazily` — twenty rounds of a
job and a foreground line reaching one unloaded package, every answer arrives and the trail holds
exactly one load and one unload per round (it failed in about half the rounds before) — and by
acceptance case `174-a-job-and-the-shell-load-a-package-once`.

## Alternatives considered

**Let both load and drop the loser's new instance when keeping it.** The loser would still have
spawned a process, negotiated and recorded a load, and its own trail would have to be erased or
explained. The turn prevents the second spawn instead of undoing it.

**Hold the session's tables lock across the load.** The tables answer `get job`, `context.get`
and the consent prompt the load itself may reach; holding them across a spawn would stall every
evaluator and risk the load waiting on its own lock.

**Make the job load nothing and refuse until the shell has loaded the package.** ADR-0995 made a
job's lazy load the session's on purpose; refusing it would bring back the asymmetry it removed.
