# ADR-0355: A pinned host key is an object, and the ordinary verbs act on it

- Status: accepted
- Date: 2026-08-29
- Spec refs: §8.4, §9.1, §21.5, §49; ADR-0015 T5/T6, ADR-0090, ADR-0103/0104, ADR-0353, ADR-0354
- Decided by: agent (autonomous, `close-remote`)

## Context

ADR-0274 refused to give the trust store a command surface while nothing wrote to it on any
production path: "a safety control that does nothing is the defect class this tranche has been
removing, and a UX over an unused store is exactly that." ADR-0353 removed that condition — the
store now decides every `tcp` link — and ADR-0354 made the default `Pinned`, which means a person
*must* be able to record a fingerprint before they can link at all. The UX is now load-bearing.

## Decision

1. **`host-key` is a target, and the pins are `ono.host-key/1` records.** Spec §8.4's design rule
   is that a target is a noun; a pinned key is one, with an identity (`host`) and fields a person
   needs (`algorithm`, `fingerprint`, and the `path` of the file it is kept in). It is answered by
   `ono.shell` beside `link`, `job` and `host`, because a pin is not something a provider found on
   a machine — it is a decision this shell recorded (ADR-0090, ADR-0103).

2. **The verbs are the ones the vocabulary already has**, and their meanings are the trust store's:

   | command | effect |
   |---|---|
   | `get host-key` | what is pinned, and where the file is |
   | `add host-key <host> --fingerprint <fp>` | pin, refusing E0603 if a *different* key is pinned |
   | `set host-key <host> --fingerprint <fp>` | the deliberate re-trust of ADR-0015 T6 |
   | `remove host-key <host>` | forget, so the host must be trusted again deliberately |

   `add` refusing to overwrite is what makes `set` mean something: re-trusting a host can never be
   something that merely happened. No verb offers a "continue anyway", because ADR-0015 standing
   rule 4 forbids one. `docs/contracts/verbs.yaml` needed nothing new, which is the check that this is
   the right shape.

3. **A fingerprint, not a key, is what a person pins with.** What an operator reads off a host's
   console is the fingerprint the agent prints, never the key material, so `TrustStore` grew
   `pin_fingerprint`/`repin_fingerprint`/`forget` beside the key-taking forms. Nothing is weakened
   by it: the store compares fingerprints and a fingerprint is what it records.

4. **The store is `<config>/trusted_hosts`**, beside the shell's own host file, under the
   configuration directory of ADR-0010. A session with no configuration directory gets a store
   that lives only for the process and `path` is `null`, which is honest: nothing is written to a
   place nobody will look for it, and the table says so.

5. **`ono --agent --listen` prints its fingerprint on stderr at startup**, and
   `ono --agent --print-host-key` prints it on stdout and exits. Without that, ADR-0354's default
   would be a wall: the out-of-band channel a strict default needs is the host's own console, so
   the host has to say the fingerprint out loud.

## Consequences

Easy: `get host-key | to json` is data like everything else; the pins are one readable file a
person can also edit by hand; the whole trust surface is four commands and no new verb.

Hard: `host-key` is a hyphenated target, which is a first for this vocabulary — `docs/contracts/targets.yaml`
had none — and the parser already carried it, but it is a shape later targets will follow.
`ono.host-key/1` is a fifth session-fact schema, so `ono.shell` grows again; the alternative was a
provider that reads the shell's own configuration, which is the thing §14.4 says a link frame must
not be able to swap.

Encoded by: `crates/ono-cli/tests/authenticated_link.rs::should_show_replace_and_forget_a_pinned_key`
and `::should_keep_a_pin_in_a_file_a_person_can_read`, acceptance case
`171-authenticated-link-refuses-a-changed-key`.

## Alternatives considered

- **Fields on `ono.host/1` instead of a target of its own** — rejected: a host is something a
  source lists, reachable or not; a pin is a decision, and folding them would make `get host` a
  table where half the rows describe knowledge and half describe policy.
- **`trust host` / `forget host` as new verbs** — rejected: `docs/contracts/verbs.yaml` is a closed
  vocabulary that spec §8.4 wants kept small, and `add`/`set`/`remove` already mean exactly this.
- **Reading and writing `~/.ssh/known_hosts`** — refused by ADR-0274 and ADR-0037 §4: it would
  record a verification OpenSSH performed as if Ono had performed it.
