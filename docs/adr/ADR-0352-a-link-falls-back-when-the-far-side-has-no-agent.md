# ADR-0352: A link falls back when the far side has no agent, and says so everywhere

- Status: accepted
- Date: 2026-08-29
- Spec refs: §12.5, §21.2, §21.3, §21.4, §35.3; ADR-0037 §3/§6, ADR-0104, ADR-0106, ADR-0351
- Decided by: agent (autonomous, `close-remote`)

## Context

ADR-0351 built the reduced provider set. This ADR wires it into the shell: when the fallback is
taken, how a link that took it is described, and what stops the fallback from becoming a way to
answer for a machine nobody reached.

## Decision

1. **The probe is the far side's exit status, and the rule is exit 127.** `ssh <host> ono --agent`
   gives a shell exactly one honest way to tell "this machine has no Ono on it" from "this machine
   was never reached": a POSIX shell that cannot find a command exits **127**, ssh reserves **255**
   for its own failures, and everything else passes through from the far side.
   `ono_remote::far_side_lacks_agent` is that rule, and it is the only thing that turns a refused
   agent connection into a reduced link. A status nobody observed is not evidence and does not
   fall back.

   No extra round trip is spent asking `command -v ono` first: the answer is already in the
   attempt, and a second probe would double the latency of every link that has an agent — the
   common case — to make the rare one prettier.

2. **`--agentless` skips the agent entirely.** Asked for by name, the reduced set is what is
   opened; the flag is a statement about the far side, not a preference.

3. **A link reports the mode it is in, never the mode it was asked for.** `SessionLink::row` reads
   `mode` off the established connection, so a link that fell back says `agentless` in `get link`
   although nobody typed the flag, and the fallback is announced on **stderr** when it is taken —
   stdout carries the data a script asked for and nothing else (§12.5).

4. **`targets` on a reduced link is what it can answer.** For an agent link the field is what the
   handshake negotiated (§21.2). A reduced link negotiated nothing, so the honest content of
   `ono.link/1`'s "what its context can answer" is the strategy table's targets — which makes the
   reduction visible in the table itself, next to a full link, without connecting to anything.
   The targets it *cannot* answer stay visible where it matters more: asking for one is
   `provider.unavailable` naming the mode, never an empty stream (§35.3).

5. **`protocol_version` is null and the far side names itself `agentless (<uname -s -m>)`.**
   `ono.probe-result/1` already anticipated this — "the agentless fallback of §21.3 names itself
   too, so the fallback is visible" — and a version nobody negotiated is not a number to invent.
   `providers` is `["remote.agentless"]`, which is who really answers.

6. **A reduced link cannot adapt an external command across the link, and says which link it is.**
   v0.3 §1.54's remote adaptation is an agent call; on a reduced link `run_remote_adapted` reports
   `this link is agentless: there is no agent over there to negotiate adapters` instead of the
   generic "the remote agent cannot negotiate adapters", so the reason a familiar command behaves
   differently is the mode, spelled.

7. **`LinkConnection` holds a `FarEnd`, not a `RemoteLink`.** The two kinds of far end differ only
   where the shell *describes* a link — `get link`, `test host`, hang-up, remote adaptation —
   never where it uses one, because both mount ordinary providers into an ordinary registry. Every
   place that reached through `connection.link` now asks the question it actually has
   (`protocol_version`, `far_end_name`, `provider_ids`, `agent_link`), which is what makes the
   agentless case impossible to forget at a call site.

## Consequences

Easy: `link host`, `connect host` and `test host` all fall back through one path; a definition that
asked for the reduced set is probed in the reduced set; nothing above the registry changed.

Hard: the automatic fallback branch is proven at the process boundary
(`should_tell_a_far_side_without_ono_from_one_that_cannot_be_reached`, a child exiting 127 through
the real `SubprocessTransport`) rather than over real ssh, because the acceptance container has
neither ssh nor a network — the same limit ADR-0037 §2 works within. The end-to-end shell path is
proven through `--agentless`, which enters `connect_agentless` at the identical point.

Encoded by: `crates/ono-cli/tests/agentless_link.rs` (6 tests),
`crates/ono-remote/tests/agentless.rs::should_tell_a_far_side_without_ono_from_one_that_cannot_be_reached`,
acceptance case `170-agentless-link-is-visibly-reduced`.

## Alternatives considered

- **Falling back on any failed handshake** — rejected: a host that is down, or whose key ssh
  refused, would silently become a reduced link, and `get process` would answer for a machine that
  was never reached. That is the worst failure this whole tranche exists to remove.
- **Probing with `ssh <host> command -v ono` first** — rejected as point 1 says: it taxes every
  link with an agent to simplify the one without.
- **Keeping `--agentless` as a preference the agent may override** — rejected: it is what the
  current code did, and it made a flag that changed one sentence of output and nothing else.
