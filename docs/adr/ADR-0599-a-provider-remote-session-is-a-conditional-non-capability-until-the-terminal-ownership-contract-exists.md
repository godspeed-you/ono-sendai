# ADR-0599: A provider remote session is a conditional non-capability until the terminal-ownership contract exists

- Status: accepted
- Date: 2026-09-07
- Spec refs: v0.2 §31.10, §31.12, §31.14, §31.16; v0.8 §0.1, §14 (Generic Terminal Ownership
  Contract), §16 (Foreground External Processes), §17 (Job Control, Suspend and Resume); v0.9;
  `docs/architecture/external-system-provider.md` §2.3, §21.1, §27.4; the Kubernetes provider
  specification §42.3–§42.6, §51.1, §51.4 (by reference); ADR-0573, ADR-0586
- Decided by: agent (autonomous)

## Context

The Kubernetes provider specification conditionally requires three operations that no other
provider surface does — exec into a container, attach to one, and port-forward to it (§42.3–§42.5,
each introduced with "if supported"). Each is a bidirectional byte session, not a request/response:
exec and attach begin with `101 Switching Protocols` and then multiplex stdin, stdout, stderr, a
resize channel and an error channel over the upgraded connection; port-forward needs a local
listening socket whose lifecycle is a job. The specification is explicit about what such an
operation must integrate with: a "dedicated remote-execution capability integrated with Ono
terminal/job-control" (§42.3), remote-session infrastructure shared with attach (§42.4), and a
job/session with clear local and remote endpoints (§42.5).

The Kubernetes provider recorded, in its own ADR-0018, that it implements all three as structured
refusals that name what is missing, and that the missing pieces are core's: no protocol-upgrade
path on the brokered connection, no channel codec, **no terminal or job-control contract to
integrate with**, no brokered listening socket, and no granted capability. This ADR is core's
answer to the generic half of that: is there a clean, provider-generic remote-session contract to
build, and if not, exactly what is missing.

The assessment was made seriously rather than by pattern-matching "hard":

1. **The transport is buildable and is not the blocker.** `network.connect` already yields a
   bidirectional byte stream (ADR-0573), and `ADR-0586` gave the runtime concurrent invocations,
   so a long-lived session worker is expressible. A WebSocket or SPDY channel codec is ordinary
   code. If transport were the only gap, this would be a `feat`, not an ADR.

2. **The blocker is that the contract a remote session must integrate with is specified and not
   yet implemented.** v0.8 §14 defines the Generic Terminal Ownership Contract — "exactly one
   owner may control interactive terminal presentation at a time", with `RichTtyShell`,
   `FullScreenOnoHost`, `ForegroundExternalProcess` and a suspended state as the owners, and a
   lease over termios, alternate-screen, cursor and redraw. v0.8 §16 defines how a foreground
   process is *handed* the terminal, and v0.8 §17 how job control cooperates with ownership.
   An interactive exec is precisely a `ForegroundExternalProcess`-shaped owner whose far end is a
   container rather than a local pid, and a port-forward is precisely v0.8 §17's job with a local
   endpoint. **v0.8 is not implemented** (it is a level-0 enhancement specification layered on the
   base; `docs/STATE.md` schedules it behind v0.7, and the released substrate is v0.4.1). The
   contract a provider remote session must plug into therefore exists as normative text and as no
   code.

3. **Building the capability now means one of two things, and both are forbidden.** Either invent
   a terminal/job-session contract inside the provider boundary or as an ad-hoc core addition — which
   §0.4's "no core exception by convenience" forbids, and which would almost certainly disagree
   with v0.8 §14–§17 when those are implemented, leaving two contracts for one terminal — or
   build a remote session that bypasses terminal ownership entirely, which v0.8 §14.1's "terminal
   state cannot be a Deck-specific implementation detail" forbids for the same reason it forbids a
   view owning the terminal. The generic provider contract's §2.3 also makes "arbitrary remote
   code execution" a non-goal *for the host-access model*, which is a further reason the capability
   must be a deliberate, terminal-owning, job-controlled thing rather than a byte pipe a provider
   opens.

## Decision

**A provider remote-session capability is a deliberate conditional non-capability, reserved until
the v0.8 terminal-ownership and job-control contract is implemented. Core does not add it now, and
the Kubernetes provider's exec/attach/port-forward remain the explicit refusals of its ADR-0018.**

The exact missing generic contract, so that a later tranche builds against a named gap rather than
rediscovering it:

- **A remote-session owner in the terminal-ownership contract (v0.8 §14).** A fourth conceptual
  owner — a provider-backed foreground session — or a demonstration that
  `ForegroundExternalProcess` covers it, with the lease responsibilities of §14.2 met for a far
  end that is not a local process group.
- **A job/session representation (v0.8 §17) for a provider session**, so port-forward's lifecycle
  and endpoints are a job the operator lists, suspends and cancels, exactly as §42.5 requires.
- **A capability family for opening one.** Distinct from `provider.mutate` (ADR-0594): mutation
  changes durable state through the API and returns; a remote session is `observe`-or-stronger,
  holds a channel open, and — for exec and attach — runs code or writes stdin, which §51.4 of the
  Kubernetes specification and the threat model treat as a higher grant than either reading or
  mutating. It is also gated on the terminal-ownership lease, so it cannot be granted to a
  non-interactive context that has no terminal to lend (v0.2 §31.9's non-interactive rule).
- **A host-brokered channel primitive** on top of `network.connect`: the upgrade and the
  multiplexed channels, or a host call that returns the raw upgraded stream to the package with
  whatever was already buffered past the `101`.

When those exist, the capability is implementable and the Kubernetes provider's refusals become an
implementation; `logs`, which needs none of them, is already retrieved (ADR-0018).

## Consequences

The three operations stay refusals that name what is missing, and the refusal is now backed by a
core decision rather than only by the provider's reading of its own limits. This is a
**conditional non-capability**, in the specification's own "if supported" terms — not unfinished
code and not a MUST left undone. The Kubernetes provider's coverage records it against this ADR.

The cost is real and is the intended one: an operator cannot exec into a container through Ono
today, and no configuration changes that. The alternative — a byte pipe that ignores terminal
ownership — is the one v0.8 exists to prevent, and shipping it before v0.8 would make v0.8's
implementation a migration rather than a feature.

## Alternatives considered

**Implement exec now over a WebSocket channel, owning the terminal directly.** Rejected: it
builds the terminal-ownership contract inside the provider path, which §0.4 forbids and v0.8 §14
supersedes, and it is a migration waiting to happen.

**Implement port-forward only, since it needs no terminal — just a local listener and a job.**
Rejected as the smaller version of the same error: v0.8 §17 owns the job/suspend/resume contract a
forward's lifecycle must use, and `network.listen` brokering a local socket for a provider is a
capability decision that belongs with the session family rather than taken piecemeal. It is the
most reasonable thing to build *first* when the tranche comes, and it is recorded here as such.

**Declare the whole area out of scope permanently.** Rejected: §42 says "if supported", the
capability is genuinely wanted, and the only thing standing between the refusal and an
implementation is a contract the project has already specified. "Reserved until v0.8" is the true
statement; "never" is not.
