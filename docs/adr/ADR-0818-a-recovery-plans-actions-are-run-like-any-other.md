# ADR-0818: A recovery plan's actions are run like any other

- Status: accepted — amended by ADR-0822
- Date: 2026-09-10
- Spec refs: v0.6 §5.8, §12.2, §12.3, §24.2, §25.1, §62.9, Appendix C.1
- Decided by: agent (autonomous)

## Context

§5.8 splits recovery in two: `recover` builds a `RecoveryPlan` and changes nothing, and applying
that plan is what performs the restore. §24.2 makes its RECOVER actions ordinary plan actions, and
the first-party providers build them as `Execution::RecoveryOperation { provider, capability,
arguments }` — §12.3's shape, naming the provider rather than a command line.

Nothing ran them. `ono-cli`'s executor bridge answered every `RecoveryOperation` with
`change.action_not_plannable` and a comment claiming that recovery operations are carried out "at
PREPARE and RESTORE, never as a plan action of its own". PREPARE was true; RESTORE had no other
route, so applying a recovery plan failed at its first action, every time, on every provider.

A second defect sat behind it. The file provider's recovery verification read

> the SHA-256 of `/srv/app/config` is the digest the archive recorded

which is a sentence, not a condition the shell can observe. §62.9 names verifying from an exit
code as a failure mode; a contract nobody can answer is the same failure with an extra step, and
§25.3 forbids acting on a recovery verification that cannot be scoped.

## Decision

**`world::execute_with` routes a `RecoveryOperation` to the recovery provider that named itself in
it**, through `RecoveryProvider::restore`. The action carries which provider, which capability and
which asset; the asset is read back out of the plan store and handed to that provider and no
other. A provider that is not registered here is a refusal rather than a substitution, because
§11.4's validation was made against *that* provider's asset and another one's answer is a
different claim.

Only `recovery.restore` is carried out this way. `recovery.prepare` is run by §4.5 and
`recovery.cleanup` by §37, each holding the asset it is about, and an action asking for one of
them here is asking the wrong thing to run it.

**A recovery verification is written in the form the shell observes.** The file provider's
contract is now `file <path>` / `sha256 == <digest>`, and `observe` answers it by reading the file
and hashing it. A digest is not a property a provider carries — it is an answer to a question
about the bytes — so it is taken at the moment the question is asked. A file that cannot be read
is `UNKNOWN` and not a failure, because §2.4 forbids reading an unestablished fact either way.

`ono-change-recovery`'s generic fallback contract follows the same form, and it recognises a
provider's own contract by the identity inside the subject rather than by the whole string:
`file /srv/app/config` and `/srv/app/config` are one object, and comparing them verbatim added a
second, unanswerable contract beside the answerable one.

## Consequences

- The recovery round trip runs end to end: `plan` → `apply` → `recover` → `apply <recovery>`
  restores the object and its verification passes on the digest the archive recorded.
- `execute` keeps its old signature and forwards to `execute_with` with no session. A caller
  without the change session gets a structured refusal naming what is missing, rather than a
  restore against whatever registry happened to be nearest.
- Without a captured digest the fallback contract asks `exists == true`. That is less than a
  byte-consistent restore claims, and it is what can be established from nothing; §25.2's
  `RESTORED` is not asserted from it.
- The `sha256` field is observed for `file` and `dir` targets by path. It is not a schema field
  and does not become one: adding it to `ono.file/1` would make every `get file` hash every file.

## Alternatives considered

- **Give the recovery provider its own verifier.** It would keep the digest inside the crate that
  captured it. It also gives recovery a second observation path beside `verify`'s, and §25.1's
  point is that recovery verification is verification.
- **Leave the prose contract and let it read `UNKNOWN`.** Honest about not knowing, and it makes
  every recovery `DEGRADED` — a verification that never passes is one nobody reads, which is how
  §62.9's failure mode establishes itself.
