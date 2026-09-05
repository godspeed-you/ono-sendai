# ADR-0354: An unknown host key is refused by default (F12 settled)

- Status: accepted
- Date: 2026-08-29
- Spec refs: §21.5, §49; ADR-0015 T5/T6, ADR-0274, ADR-0353
- Decided by: agent (autonomous, `close-remote`)

## Context

`docs/STATE.md`'s F12: `TrustPolicy`'s default was `Required` — trust on first use, where an
unknown key is recorded and the link proceeds — and ADR-0015 T5 states the test criterion "an
unknown key is refused, not prompted past". ADR-0274 explained why the contradiction could not be
resolved: with no transport that authenticates anything, both readings produced the same
behaviour (nothing was ever checked), so choosing between them would have been choosing a word.

ADR-0353 built the transport. The question is now real, and it has to be answered before anyone
links over it.

## Decision

**`TrustPolicy::Pinned` is the default, and it is what the `tcp` transport ships with.** An
unknown key is `safety.policy_denied` (E0702) carrying the fingerprint the peer presented, and
nothing is recorded. A host becomes trusted only by a deliberate act taken beforehand:
`add host-key <host> --fingerprint sha256:…` (ADR-0355), with the fingerprint read from the host's
own console, where `ono --agent --listen` prints it.

`TrustPolicy::Required` — trust on first use — stays in the protocol's vocabulary, because a
deployment whose first contact really is out of band (a machine just installed, on a cable the
operator controls) is a legitimate caller and the protocol should be able to express it. It is
never what a caller gets by saying nothing, and the shell does not offer it as a flag today: the
pinning command already covers the need, and a `--trust-on-first-use` option would be a safety
control added for a use nobody has yet asked for (AGENTS.md §4).

Three things this does **not** change:

- **ssh links stay `TrustPolicy::Unauthenticated`, by name.** ADR-0037 §4 stands: OpenSSH verified
  the host in its own `known_hosts` and will not tell this process the key, so Ono asserts nothing
  about it. That is a statement about ssh, not a default.
- **A changed key is refused under every policy.** The policy decides what happens to a peer that
  is *unknown*, never to one that contradicts what is known (ADR-0015 T6).
- **A refusal is never a prompt** (ADR-0015 standing rule 4). There is no "continue anyway"
  anywhere on this path; re-trusting is `set host-key`, typed on purpose.

## Consequences

Easy: the safe behaviour is what a caller gets by default, in the protocol crate and in the shell.
ADR-0015 T5's criterion is met by
`crates/ono-remote/tests/tls.rs::should_refuse_an_unknown_host_when_only_pinned_hosts_are_accepted`
and `crates/ono-cli/tests/authenticated_link.rs::should_refuse_a_host_whose_key_was_never_pinned`,
which also asserts that a refused link records nothing.

Hard: first contact costs a deliberate step, and a person who cannot reach the host's console
cannot link to it. That is the cost of the mitigation and it is the cost ADR-0015 chose; the
alternative accepts whoever answers first, which is the threat §49 lists. The error names the
fingerprint that was presented, so the step is "check this against the console" rather than
"guess".

Encoded by: the two tests above, and `crates/ono-protocol/src/trust.rs`'s `#[default]`.

## Alternatives considered

- **Keeping trust on first use as the default** — rejected: it silently makes the first machine to
  answer for a name *be* that name, which is exactly ADR-0015 T5's threat, and it contradicts a
  standing decision this ADR would otherwise have to overturn with better evidence than
  convenience.
- **Moving ADR-0015 T5 instead** — considered seriously, because SSH's own default is TOFU and it
  has served the world for decades. Rejected because Ono can afford the strict default that SSH
  could not: the fingerprint is printed by the agent itself at startup, so the out-of-band channel
  exists, and there is exactly one command to use it.
- **Prompting on first contact** — refused by ADR-0015 standing rule 4 and by spec §12: a prompt
  under a pipeline is a question a script eventually answers for the user.
