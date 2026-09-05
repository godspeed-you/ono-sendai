# ADR-0037: The SSH fallback is a subprocess transport around `ono --agent`

- Status: accepted
- Date: 2026-08-26
- Spec refs: §21.3, §21.4, §21.5, §37 Phase H, §12.5
- Decided by: agent (autonomous)

## Context

Spec §37 Phase H lists "SSH fallback" among its deliverables, and spec §21.5 requires that
agent mode "use authenticated encryption, explicit host trust and least privilege".
`ono-protocol` deliberately implements no cryptography: a `Transport` is a byte stream that
already authenticated and encrypted itself. Phase H had to decide what the first production
transport is, and how to prove it without a network, real ssh, or key material in tests
(AGENTS.md §11).

## Decision

1. **The first transport is ssh itself.** `SubprocessTransport::spawn(command)` runs a child
   process and speaks the link protocol over its piped stdin/stdout; `ssh_command(&SshTarget)`
   builds the one command that makes it the SSH fallback: `ssh -o BatchMode=yes -T [-p PORT]
   [-l USER] -- <host> ono --agent`. Authenticated encryption and host verification are
   OpenSSH's — a stack the spec explicitly does not want re-invented — and the far end is the
   agent loop of ADR-0036 reading the same pipe. Stderr is left alone, so ssh's own
   diagnostics reach the user's terminal unchanged (spec §12.5).

2. **The ssh spelling lives in exactly one function.** Everything else — every test — passes
   its own `Command` to `SubprocessTransport::spawn`. The suites spawn the crate's
   `ono-remote-fixture-agent` binary (the same `agent_main` loop over the same fixture registry
   the in-process suites use, shared source via `#[path]`), which proves the transport, the
   handshake, the streams and the hang-up across a real process boundary with no network. The
   transport cannot tell a local child from `ssh`; that indistinguishability is the property
   that makes the offline proof valid.

3. **`BatchMode=yes`, unconditionally.** ADR-0015 standing rule 4: a refusal is never a
   prompt. An untrusted or unreachable host fails visibly instead of ssh asking a question
   underneath a shell that may be mid-pipeline. First-contact key acceptance is an interactive
   CLI concern, decided when `link host` is wired, not something the transport smuggles in.

4. **The transport reports no peer key.** `Transport::peer_key` must be what the transport
   *verified*. This process verifies nothing — OpenSSH verified the host against its
   `known_hosts` before the first frame crossed — so the transport truthfully answers `None`,
   and connecting over ssh uses `TrustPolicy::Unauthenticated`, named in code as the protocol
   requires. Ono's own pinned-key trust store (ADR-0015 T5/T6) remains what it always was: the
   trust layer for transports that *do* certify a peer key (TLS/Noise, later), and its E0603
   refusal is proven at the `RemoteLink` level in `tests/trust.rs`. Double-pinning ssh-verified
   hosts in Ono's store would assert a verification this process did not perform. A caller
   with an out-of-band verified key can still declare it via `with_peer_key`.

5. **Hang-up is closing stdin, and the child is reaped, not killed.** A pipe signals
   end-of-input only when its descriptor closes, so the transport's `poll_shutdown` actually
   drops the stdin handle (a flush alone would leave the agent waiting forever). The child is
   waited on in a background task rather than `kill_on_drop`: killing ssh at hang-up would race
   the agent's own clean exit. `SubprocessTransport::exited()` hands out a future of the exit
   status that outlives the transport, which is also how the suite asserts the clean end.

6. **This is not spec §21.3 agentless mode.** Agentless mode (a limited provider set over
   plain ssh command execution, visibly reduced) is a different deliverable, owned by the CLI
   wiring that can fall back when `ono --agent` is not installed remotely. The subprocess
   transport is the *agent mode over ssh* of spec §21.4; nothing here precludes agentless mode
   later.

## Consequences

Easy: `link host prod-db` becomes `RemoteLink::connect(SubprocessTransport::spawn(ssh_command(
&target))?, config)`; testing anything transport-shaped stays offline; a future TLS/Noise
transport slots in with full pinned-key trust and changes nothing else.

Hard: over ssh, Ono's own trust store is not consulted (deliberately, point 4), so `get link`
should show the transport so a user can see which trust regime a link is under; the fixture
agent binary ships in the crate (documented as test scaffolding, not product).

Encoded by: `crates/ono-remote/tests/subprocess.rs` (child round trip, clean exit on hang-up,
the ssh spelling).

## Alternatives considered

- **A native TLS/Noise transport first** — rejected for Phase H: it adds a cryptography and
  key-management surface the phase does not need to prove its deliverables; ssh is already on
  every target machine and already trusted by its operators.
- **`kill_on_drop(true)`** — rejected: it races the agent's clean exit and turns every normal
  hang-up into a SIGKILL; closing stdin is the protocol's own goodbye.
- **Pinning ssh hosts into Ono's trust store from the transport** — rejected: the transport
  would be asserting a verification it never performed; trust entries must mean "this process
  verified this key".
- **Letting tests stub `ssh` via `PATH`** — rejected: substituting the whole command is
  simpler, exercises the identical code, and keeps tests independent of the machine's `PATH`.
