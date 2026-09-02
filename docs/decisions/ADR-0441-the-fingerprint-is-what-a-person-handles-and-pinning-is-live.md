# ADR-0441: The fingerprint is what a person handles, and pinning is finally live

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §7.2, §8.1, §8.2, §8.5, §8.6, §59.6; spec §21.5, §49;
  ADR-0015 T5/T6 and F12, ADR-0037 §4, ADR-0274 (superseded by this ADR), ADR-0353, ADR-0354,
  ADR-0355, ADR-0435, ADR-0437, ADR-0439
- Decided by: agent (autonomous)

## Context

Two things end here, and they are the same thing seen from either end of a link.

**§8.5** asks for a non-secret way to print the local peer fingerprint, canonically
`ono --print-peer-key`, and requires the existing `ono --agent --print-host-key` to keep working
and to print the same fingerprint on the default path. The fingerprint is what §7.2 calls the
public contract of an identity, and what ADR-0355 made an object a person can pin, replace and
forget; without a way to read it off a machine's own console, ADR-0354's default — an unknown key
is refused — leaves a person with nothing to pin.

**Issue #18 / B-remote-2** has been open since the ADR-0015 security checklist: the trust store was
complete and proven at unit level, and never consulted in production, because both production
transports ran behind `ssh(1)` and `SubprocessTransport::peer_key` truthfully answered `None`.
ADR-0274 wrote down why the exit test could not be written and what would have to exist first: "a
transport that certifies its peer to this process — an Ono-native authenticated transport over
TCP, where the peer's certificate or static public key *is* the `HostKey`".

## Decision

**`ono --print-peer-key` is a global invocation, not an agent one.** It resolves the identity
through the same ladder every other path uses (§8.1, §8.2, ADR-0435) and prints the fingerprint on
stdout, so it can be piped into `add host-key` on the machine that will link here.
`--agent --print-host-key` is unchanged and, because both now go through one default path, prints
the same string by construction rather than by two implementations agreeing. `--help` names the
new spelling; the old one stays accepted and undocumented in the help, which is what §8.5 asks for
("the help text SHOULD direct new users to `--print-peer-key`").

Printing is *using* the identity, so an exposed identity file refuses here too (§8.3, ADR-0436) and
nothing reaches stdout on the way to the refusal. A fingerprint read out of a world-readable key
file names a key that is no longer only yours, and printing it would invite somebody to pin it.

**B-remote-2 is closed, and ADR-0274 is superseded.** Every condition it named is met:

- `ono-remote`'s TLS transport certifies its peer to this process (ADR-0353), and since ADR-0437
  it does so in *both* directions, so `Transport::peer_key` answers `Some` on either end of a
  direct link;
- the exit test ADR-0274 could not write exists twice over — at the shell in
  `crates/ono-cli/tests/authenticated_link.rs::should_refuse_a_changed_host_key_with_the_stable_safety_code`,
  and in the container as
  `docker/acceptance/cases/171-authenticated-link-refuses-a-changed-key.case`, which stands two
  `ono` processes on the loopback interface with different identities and asserts
  `Ono-Sendai-E0603` when the second answers for the first;
- **F12 is settled.** ADR-0274 said the choice between trust-on-first-use and pinned-only "depends
  on whether first contact can be verified out of band, and that question has no answer while no
  transport authenticates at all". It has one now, and the answer is yes: an operator reads the
  fingerprint off the host's own console with `--print-peer-key`, which is exactly the out-of-band
  channel that makes a first pin worth anything. `TrustPolicy::Pinned` is therefore the default
  (ADR-0354) and the `tcp` path names it explicitly.

**It closes because the TCP transport certifies its peer, not because ssh started to.**
`SubprocessTransport::peer_key` is still `None` for `ssh` and `local`, ADR-0037 §4 stands
unchanged, and §4.3 keeps that trust model on purpose. What changed is that it is no longer the
*only* production transport, so the store is consulted on a path a user can actually take, and
`ono.link/1` now says which of the two a given link is (ADR-0438). Both of ADR-0274's standing
prohibitions also stand: `known_hosts` is not copied into the store, and there is no trust-store
command surface beyond the four verbs ADR-0355 already justified by something writing to the store.

## Consequences

Easy: `ono --print-peer-key` on the agent, `add host-key <host> --fingerprint <it>` on the client,
`link host <address> --transport tcp` — the whole loop is three commands and none of them can be
answered by a prompt. E0603 is reachable by a person rather than only by a test.

Hard: the fingerprint is now printed by a path that generates an identity when none exists, so
`ono --print-peer-key` on a fresh machine creates `~/.config/ono/link_identity.pem` as a
side-effect of asking a question. That is deliberate — there is no answer to give otherwise, and
§8.4 makes generation the normal case — but it means the command is not read-only, and §8.6's
rotation warning applies to whatever it created.

Also hard: what is *not* delivered here is an acceptance case for `--print-peer-key` itself.
`docker/` is outside this tranche's file scope, and case 171 already exercises
`--agent --print-host-key`. The case that should be added is small and is written out in the
report to the orchestrator: add `--print-peer-key` beside the existing `--print-host-key` in case
171 and assert the two strings are equal.

Encoded by: `crates/ono-cli/tests/peer_identity.rs::should_print_the_same_fingerprint_however_it_is_asked_for`,
`::should_print_the_identity_a_machine_already_had`,
`::should_refuse_to_print_from_an_identity_file_anyone_can_read`,
`::should_point_new_users_at_the_canonical_spelling_in_the_help`;
for B-remote-2, `crates/ono-cli/tests/authenticated_link.rs::should_refuse_a_changed_host_key_with_the_stable_safety_code`
and `docker/acceptance/cases/171-authenticated-link-refuses-a-changed-key.case`.

## Alternatives considered

**Make `--print-peer-key` a sub-flag of `--agent`, beside `--print-host-key`.** Smallest change.
Rejected: §8.5 calls it a "canonical global invocation", and the point of the rename is that a
client which never listens still has an identity and still needs to print it. Requiring `--agent`
to ask would keep the host-only mental model the whole tranche is removing.

**Print the certificate as well as the fingerprint.** Would let a peer be pinned by material
rather than by hash. Rejected: the trust store compares fingerprints, `pin_fingerprint` already
exists for a person holding one, and a command whose output is sometimes a key and sometimes a
hash is a command whose output nobody can pipe.

**Keep B-remote-2 open until an acceptance case asserts E0603 over a *mutually* authenticated
link.** Tempting, and wrong about what the issue asks: its exit test is "a changed key refuses with
E0603, over a transport that authenticates", and case 171 has been that since ADR-0353. Mutual
authentication makes the transport stricter, not differently authenticated; holding the issue open
for it would be moving the goalposts after the goal.
