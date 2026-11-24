# ADR-1045: This shell's own keys are a target, and a diverging pair is said out loud

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.4.1 §7.2, §8.1, §8.2, §8.5, §11.2, §14.3; spec §7, §8, §21.5; ADR-0355,
  ADR-0435, ADR-0468, ADR-0504
- Issues: #201
- Decided by: agent (autonomous)

## Context

v0.4.1 §8.2 rule 5: "both files MUST NOT diverge silently if both are explicitly configured for the
same process role". ADR-0435 met the rule per role — the listening agent presents the file
`--host-key` names, or the link identity when the flag is absent — and recorded that the copy it
makes of a legacy `host_key.pem` can later be edited apart. What it left open is the *silently*:
an operator can run `ono --agent --listen --host-key <path>` on one key while every direct link
from the same machine presents `link_identity.pem`, and a peer that pinned the one refuses the
other. Nothing showed it. `--print-peer-key` prints one fingerprint, `--agent --host-key <path>
--print-host-key` prints the other, and putting the two side by side meant knowing to ask both.

The vocabulary already has the shape. `host-key` is the keys of *other* hosts this shell pinned
(ADR-0355), `client-key` the keys of other clients this machine authorizes (ADR-0468), and the
flag that prints this shell's own key is `--print-peer-key` (§8.5).

## Decision

**1. `peer-key` is a target: the identities this shell keeps in its configuration directory, by
fingerprint.** `get peer-key` streams `ono.peer-key/1` records answered by `ono.shell`, as its two
siblings are:

| field | |
|---|---|
| `role` | `link_identity` — `link_identity.pem`, what every direct link and a listening agent without `--host-key` present (§8.1); `legacy_host_key` — `host_key.pem`, kept by §8.2 rule 4, the file `--agent --host-key` was historically pointed at |
| `path` | the file |
| `algorithm`, `fingerprint` | as `ono.host-key/1` spells them |
| `same_as_link_identity` | whether this file's fingerprint is the link identity's; null when there is no readable link identity to compare with |

Only `get`: there is nothing to add, set or remove that is not a file operation the operator
should make deliberately (§8.6). `get` observes — it never generates an identity, never runs the
§8.2 migration, and reads a file only after the same owner-only check `open_or_create` applies; a
file that fails it is reported on the stream's failure channel with §8.3's refusal and the other
file is still listed. A file whose contents are not an identity is likewise a per-file failure.
No field carries key material; the record holds the fingerprint, as §7.2 requires of anything that
leaves the identity object.

**2. A listening agent started with `--host-key` names the link identity when the two differ.**
Right after the `host key` line of the §11.2 summary (ADR-0504):

```text
ono: host key sha256:aaaa…
ono: link identity sha256:bbbb… (/home/…/.config/ono/link_identity.pem) differs from the host key this agent presents; a peer that pinned one is refused by the other
```

Printed only when `--host-key` was given, the link identity exists, and the fingerprints differ.
The agent does not create a link identity to compare against.

**3. A file named by `--host-key` outside the configuration directory is not listed by `get
peer-key`.** The shell does not know which paths agents were started with, and inventing a
registry of them would be a second source of truth. The agent's summary is where that pair is
visible; `get peer-key` is where the two files the configuration directory holds are.

## Consequences

The diverging pair is visible without reading the filesystem by hand, in both of the places an
operator looks: the console of the agent that presents the key, and `get peer-key` on the machine.

New public surface — target `peer-key`, command `ono.peer-key.get`, schema `ono.peer-key/1` — with
help, completion, the generated reference, the provider declaration of `ono.shell` and a
`may_be_empty` entry in the command-example register (a fresh directory holds no identity, and
`get` never makes one). Additive;
nothing existing changes shape. The startup summary gains a line only in the diverging case, after
`host key`, so the summary's existing lines and order are unchanged.

Encoded by `crates/ono-cli/tests/peer_keys.rs`:
`should_list_the_link_identity_by_its_fingerprint`,
`should_show_a_legacy_host_key_that_differs_from_the_link_identity`,
`should_create_no_identity_when_asked_to_list_them`,
`should_name_the_link_identity_when_a_listening_agent_presents_a_different_key`,
`should_say_nothing_more_when_the_host_key_file_is_the_link_identity`,
`should_report_an_exposed_identity_beside_the_others_rather_than_read_it`; acceptance case
`402-a-diverging-peer-key-pair-is-visible`.

## Alternatives considered

**`get identity`.** The issue's working name. `identity` already means the runtime identity of
v0.4.1 §7.3 — the user and uid a peer *reports* — which grants nothing; naming the key a peer
*proves* the same way is the confusion §7.3 spends a section keeping apart. `peer-key` matches the
flag that prints it and the two key targets beside it.

**A warning when `--host-key` is given at all.** The flag is legitimate (ADR-0435 kept it so a
listening agent and a linking client can have separate identities); what is worth saying is that
the two differ, with both fingerprints.

**Refuse to start an agent whose `--host-key` differs from the link identity.** §8.2 rule 5 asks
for visibility, not sameness, and the separate-identity setup is one ADR-0435 deliberately left
possible.
