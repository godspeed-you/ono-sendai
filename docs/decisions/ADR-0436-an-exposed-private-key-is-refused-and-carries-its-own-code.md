# ADR-0436: An exposed private key is refused, and the refusal carries its own code

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §8.3, §8.6, §2.3, §2.9, §59.6; spec §43, ADR-0006, ADR-0015 standing rule 4;
  AGENTS.md §5 level 2
- Decided by: agent (autonomous)

## Context

v0.4.1 §8.3 is two sentences and both of them say *refuse*:

> If an existing identity file is group/world writable, Ono MUST refuse to use it and report a
> security error.
>
> If it is group/world readable, Ono MUST refuse by default because the private key is exposed.

§59.6 turns the second into an acceptance scenario and adds what the diagnostic must and must not
contain: "the diagnostic identifies the path and required permissions without printing key
material." §2.9 forbids resolving it interactively.

Two things had to be decided: which error code a script sees, and what happens when the file with
the wrong mode is the *legacy* `host_key.pem` that §8.2's migration ladder is about to read.

## Decision

**A new stable code, `Ono-Sendai-E0604` / `remote.identity_permissions`, kind `safety`.** It is
registered in `docs/spec/errors.yaml` and in `crates/ono-core/src/error.rs` in the same increment
as the behaviour, as AGENTS.md §5 requires of a level-2 contract change, and `docs/reference/`
was regenerated from the registry rather than edited.

The `remote` family (E06xx) rather than the `safety` family (E07xx), because the number families
in this registry are grouped by *subject* and not by kind — E0603 `remote.host_key_changed` is
already a `safety` error sitting in the remote family, for exactly the same reason: it is a
refusal, and it is about a remote link's identity. The `kind` is what a script branches on
(`catch e if $e.kind == "safety"`), and it is `safety` here.

It is not `safety.policy_denied` (E0702). That code means a configured policy forbade the
operation; nothing here was configured, and a person who greps their logs for why a link stopped
working needs to land on the file permissions rather than on a policy they never set.

**The check is in `PeerIdentity::open_or_create`, before the file is read.** It refuses on
`mode & 0o044` (group or world readable) and on `mode & 0o022` (group or world writable), naming
which of the two it found, because the two have different consequences and "wrong permissions"
sends a person to `ls -l` to work out which one they have. Execute bits are not refused: §8.3
names readable and writable, and an executable PEM leaks nothing.

Putting it in the constructor rather than at each call site means every path that opens an
identity — the listening agent's `--host-key`, the migration ladder, `--print-peer-key`, the
client's own identity — is covered by the same check by construction. §2.3 asks that a claimed
safety control prevent the operation from starting, and a control that each caller has to
remember to invoke is not that control.

**An exposed *legacy* `host_key.pem` propagates the refusal instead of being stepped over.**
§8.2's ladder skips a legacy file that does not parse and generates instead. An exposed file is
not the same thing: stepping over it would generate the second unrelated identity §8.2 exists to
prevent, and it would do so *out of a security problem the operator was never shown*. So
`link_identity` distinguishes the two failures — `remote.identity_permissions` returns, anything
else falls through to rule 3.

**The diagnostic names the path, the mode it found, the mode it needs, and §8.6.** It says
nothing that came from inside the file, and the test greps for the fingerprint and for
`PRIVATE KEY` to prove it. The help ends by pointing at rotation, because `chmod 600` fixes the
permissions and does not un-disclose a key another account may already have copied — which is a
judgement only the operator can make, and one they will not make if nobody mentions it.

## Consequences

Easy: `chmod 0644 ~/.config/ono/link_identity.pem` now produces one code, one line and no link,
in a script exactly as at a prompt.

Hard: this refuses on files it did not create. A deployment that ships identities through a
configuration-management tool with a permissive umask will start failing at upgrade time, with an
error that names the fix. That is the intended compatibility change; §8.3 chose refusal over a
warning deliberately, and a warning about a key everyone can read is a warning nobody acts on.

Also hard: the mode is read with `stat`, so a file whose permissions change between the check and
the read is not covered. That race is not worth closing here — the attacker in it already has
write access to the directory, which §5.2's local attacker class does not assume, and the check
that matters is the one an operator's mistake trips.

Encoded by: `crates/ono-remote/tests/peer_identity.rs::should_refuse_an_identity_file_that_anyone_else_can_read`,
`::should_refuse_an_identity_file_that_anyone_else_can_write`,
`::should_use_an_identity_file_only_its_owner_can_reach`,
`crates/ono-cli/tests/peer_identity.rs::should_refuse_a_group_or_world_readable_identity_and_establish_no_link`,
`::should_refuse_an_exposed_legacy_host_key_rather_than_generate_a_second_identity`.

## Alternatives considered

**Reuse `safety.policy_denied` (E0702).** No registry change. Rejected: it is the wrong sentence
for a script to read, and E0702 already covers config-mode denials, so a `catch` on it would
conflate two unrelated causes.

**Warn and continue for the readable case, refuse only for writable.** §8.3 says "MUST refuse by
default" for readable, and "by default" is about configuration this release does not have, not
about severity.

**Fix the mode automatically with `chmod 600`.** Tempting and wrong. It hides that the key was
exposed, which is the fact the operator has to act on, and it is a shell silently changing
permissions on a file it did not create.

**Check the containing directory too.** §8.3 says the directory "SHOULD be owner-only where Ono
owns it", and Ono does not own `~/.config` — it owns `~/.config/ono`. A refusal on a directory the
user shares with every other tool would be wrong, and a refusal on `~/.config/ono` alone is a
narrower rule than the one §8.3 states. Left for H2, which adds `authorized_clients` to the same
directory and will have to decide it anyway.
