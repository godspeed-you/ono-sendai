# ADR-0351: The agentless fallback is a strategy table over first-party adapter packs

- Status: accepted
- Date: 2026-08-29
- Spec refs: §21.2, §21.3, §35.3, §50; v0.3 §1.7, §1.8, §1.10; ADR-0037 §6, ADR-0057
- Decided by: agent (autonomous, `close-remote`)

## Context

Spec §21.3: "If no Ono-Sendai agent exists remotely, the link MAY fall back to SSH and a limited
provider set implemented through standard commands/procfs reads. Fallback MUST be visible because
semantics and performance may differ."

ADR-0037 §6 deferred this deliberately, and until now `--agentless` was a flag that changed one
sentence of output while the agent still answered every query. Three things had to be decided to
make it real: what the reduced set reads, how it turns text into records without violating §50,
and what "visible" means in a way a test can hold.

## Decision

1. **The reduced set reads through the v0.3 adaptation layer, never through a parser of its own.**
   `crates/ono-remote/src/agentless.rs` holds a table of strategies, each naming an adapter pack,
   an adapter and one of its *declared invocations*: the plan's argv and environment are what runs
   on the far side, and `ono_adapter::decode` is what reads the bytes back. §50 forbids parsing
   unstable human-readable output; an adapter contract is the documented exception, and reusing it
   means the agentless reading of `ps` is the same reading, with the same fixtures and the same
   conformance suite, as the local one.

   Today the table is two rows — `process` from `org.ono.compat.procps/ps` and `filesystem` from
   `org.ono.compat.coreutils/df`. That is the whole claim, and the table is the place a later row
   is added; a target may only enter it when some adapter pack already stands behind its command.

2. **Visibility is structural.** An agentless link mounts one provider per target the shell would
   have been able to ask an *agent* — the caller's own target vocabulary — and the ones without a
   strategy answer `Availability::Unavailable(reason)`. `ProviderRegistry::provider_for` then
   turns `get service` on a reduced link into `provider.unavailable` naming the mode, rather than
   an empty table. Spec §35.3's rule that absence and ignorance must never be confused is the
   whole point of §21.3's "MUST be visible", and a printed sentence would not have satisfied it.

3. **`uname -s -m` is the agentless handshake.** Opening a reduced link runs exactly one command,
   and it is the only item of §21.2's handshake list a machine without an agent can be asked:
   remote OS and arch. Nothing else is negotiated, because there is nobody to negotiate with.
   A far side that cannot answer it is `remote.unreachable`.

4. **`FarSide` is the only thing that differs between ssh and this machine.** `SshFarSide` wraps
   each command in `ssh -o BatchMode=yes -T -- <host> <command line>`; `LocalFarSide` runs it as a
   child. Everything above — strategy, decoder, provenance, refusals — is one code path, which is
   what makes the fallback provable with no network, exactly as ADR-0037 §2 argued for the agent
   transport. The words after the host are single-quoted once, here, because ssh concatenates them
   and hands them to an account's login shell that nobody in this process chose.

5. **The provider id is `remote.agentless`, one id for the whole reduced set.** Which command
   produced a record is not hidden by that: the adaptation layer already writes the executable, its
   invocation and its exactness into the record's provenance (v0.3 §1.8), and every record is
   re-tagged `Remote(host)` on the way in (§25.2), agent or not.

6. **A tool's version is `null`, not guessed.** An adapter's version probe is a second round trip
   per query, and an agentless link is already the expensive path; the honest answer to "which ps
   is that" is that this link did not ask (§35.3).

## Consequences

Easy: adding a target to a reduced link is a table row. `get process` and `get filesystem` work
across a link to a machine that has never heard of Ono. Nothing above the registry changes.

Hard: the reduced set answers snapshots only — it cannot `watch` and cannot `act`, and both say so
through the ordinary provider refusals rather than by pretending. Each query is one command and one
process on the far side, which is the performance difference §21.3 warns about; it is visible in
`get link`'s `mode` and in the provenance of every record.

Encoded by: `crates/ono-remote/tests/agentless.rs` (nine tests, including
`should_refuse_a_target_the_reduced_set_cannot_answer_rather_than_answer_nothing` and
`should_name_every_target_the_agent_would_have_served`).

## Alternatives considered

- **A hand-written `/proc` reader over `cat`** — rejected: it is a text parser in a provider by
  another name (§50), and it would duplicate what `linux.procfs` and the adapter packs already own.
- **Running the whole reduced set through one ssh multiplexed session** — rejected for now: it is a
  performance decision that changes no semantics, and §21.3 explicitly allows the performance to
  differ. It can be added under `FarSide` without touching a strategy.
- **Marking unanswerable targets by simply not mounting them** — rejected: `get service` would then
  fail with `resolve.target_not_found` ("no provider answers that"), which is a different and false
  statement. The target exists; this link cannot reach it.
