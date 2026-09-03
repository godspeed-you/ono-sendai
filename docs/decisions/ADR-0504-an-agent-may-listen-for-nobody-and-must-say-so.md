# ADR-0504: An agent may listen for nobody, and must say so

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.3, §9.2, §9.4, §11.1, §11.2, §11.3, §12.1, §12.4, §14.1, §54.1, §54.2,
  §55.1, §55.2; ADR-0010, ADR-0456, ADR-0461, ADR-0501
- Decided by: agent (autonomous)

## Context

§11.2 is two requirements in one paragraph. The first is a block of text:

> However, the process MUST print a clear startup summary including:
>
> ```text
> bound address
> server peer fingerprint
> authorization store path
> authorized client count
> maximum concurrent connections
> ```

The second is the sentence that decides what an agent nobody has authorized does, and it is written
with a deliberate `MAY`:

> If the authorization store contains zero clients, the agent **MAY** listen but MUST refuse all
> connections after cryptographic handshake. It MUST NOT infer authorization from network locality.

`docs/ACCEPTANCE.md` §4.8.4 read that as "an empty or absent store refuses to start rather than
listening permissively", and the argument for it is real: every client would be refused anyway, so
a listener with an empty store is a service that exists to say no.

## Decision

### 1. An agent with an empty store listens, and refuses every connection

The spec's `MAY` is a permission this ADR exercises, on three pieces of evidence.

**The spec says it in words.** §11.2 offers exactly two behaviours and requires the second of them
— "MUST refuse all connections after cryptographic handshake" — which is a sentence about a
listener that is listening. Issue #55's own exit test says the same: "an agent with an empty store
listens, completes TLS, and refuses".

**It is the workflow an operator has.** `docker/acceptance/cases/182-unknown-client-is-refused.case`
starts an agent, watches it refuse an unknown client, and *then* runs `add client-key` while the
agent is up — which is how a person actually sets one of these up, and is only possible because
authorization is read per accepted connection (ADR-0470). Refusing to start would make the first
`add client-key` require a restart, and would break four other cases that start an agent to prove
something about host keys rather than about authorization.

**Fail-closed is already satisfied.** §2.3 asks that a safety control which cannot be applied
prevents the operation. Nothing is admitted here: the store authorizes nobody and every peer is
refused with `remote.unauthorized` after the cryptographic handshake, which is §11.2's own
requirement and §11.3's refusal to read anything into a loopback address. What §2.3 forbids is a
permissive fallback, and there is none.

So the ACCEPTANCE box is corrected rather than implemented, and the test that ticks it is named for
what happens: `should_refuse_every_connection_when_the_authorization_store_is_empty_or_absent`.

### 2. What it does instead is say so, on the way up

The summary carries `authorized clients 0` and, immediately under it, a line stating that every
connection will be refused after the cryptographic handshake and what to do about it. §54.2 is
explicit that an important explanation must not need `RUST_LOG=debug`, and an operator who has just
exposed a port learns from one line rather than from a refused client an hour later.

### 3. The summary is §11.2's five fields, plus the other three ceilings

```text
ono: listening on 127.0.0.1:41337
ono: host key sha256:…
ono: authorization store /home/…/.config/ono/authorized_clients
ono: authorized clients 2
ono: maximum connections 32
ono: maximum connections per client 4
ono: maximum pending handshakes 16
ono: handshake timeout 10s
```

On stderr, because an agent carried over stdio owns stdout for the wire and never writes
diagnostics there (§21.4); a listening one keeps the same discipline so the two modes are not two
contracts. The bound address is the address actually bound, so a caller that asked for port 0
learns which port the system chose, and the fingerprint is printed where a first pin is worth
something — the host's own console.

The three extra ceilings are not in §11.2's list. §11.2 says "including", and an operator reading
what they have just exposed is better served by the whole of §12 than by a quarter of it.

### 4. `--listen` takes an optional address, and the default is loopback

`ono --agent --listen` binds `127.0.0.1:7734` — the loopback interface on the port every `link`
command already assumes. §11.1's requirement is that the socket is explicit, and it is: `--agent`
alone still opens none, and only this flag opens one.

The default is the narrowest exposure that is still a listening agent. It is **not** a trust
decision, which §11.3 forbids a loopback address from being: a peer on the loopback interface is
authenticated and authorized here exactly as a peer from another continent would be. What it is, is
a sensible answer to "expose this agent" from somebody who has not yet decided how far.

### 5. The ceilings the summary prints are the ceilings the agent enforces

`configured_limits()` starts from `Limits::default()` — Appendix A — and applies every
`limits.remote_*` value the environment layer sets, through the same catalogue
`Settings::assign` range-checks and `inspect limits` reports (ADR-0456, ADR-0461). So the figure a
user reads, the figure the summary prints and the figure the listener applies are one number, which
is §12.4 and §52.2 at the product rather than in a registry.

A value that fails to parse is reported and **not stored**, so the declared default stays in force
— §55.2's one MUST NOT ("a security-sensitive agent limit MUST NOT silently become unlimited
because a value failed to parse"), satisfied by the layer ADR-0456 built rather than by a check
here.

## Consequences

Easy: an operator sees what they exposed, in one block, before anything reaches it. A test asserts
the block field by field rather than pattern-matching a paragraph.

Hard: agent mode reads the **environment** layer and not `config.ono`. That is what agent mode does
for every other setting — `ono --agent` is a protocol endpoint and has never had a configuration
file execution surface — but it means an operator who set `limits.remote_connections` in their
configuration file and started an agent gets Appendix A. Reading the file layers in agent mode
needs `ono_cli::config::load` without a `Session`, which is a change in a module this increment
does not own; it is recorded for the board.

Also hard: the summary is eight lines on every start, and two existing acceptance cases wait for
`listening on` before they proceed. That line is kept, first, and in its original wording, so
nothing that parsed it has to change.

Encoded by `crates/ono-cli/tests/agent_startup.rs::should_print_the_bind_address_the_limits_and_the_authorized_client_count_when_listening_starts`,
`::should_refuse_every_connection_when_the_authorization_store_is_empty_or_absent`,
`::should_bind_the_documented_default_address_when_none_is_given`, and case
`188-listening-agent-stays-bounded`.

## Spec deviation

None. §11.2's `MAY` is exercised rather than departed from; what is corrected is
`docs/ACCEPTANCE.md` §4.8.4's paraphrase of it, which is a level-4 artifact and is fixed in the
same increment (AGENTS.md §5).

## Alternatives considered

**Refuse to start when the store is empty or absent.** The reading `docs/ACCEPTANCE.md` had, and it
is defensible: a listener nobody can use is a socket for no reason. It contradicts §11.2's own
sentence, breaks the only workflow that lets an operator authorize a client without a restart, and
would turn five existing acceptance cases red for a behaviour the spec offers as optional.

**Refuse to start only on a non-loopback bind with an empty store.** It keeps the workflow and
sounds careful. It is also a policy decision made from a network address, which is the shape §11.3
spends its whole section ruling out — even when the decision is conservative.

**Print the summary as JSON.** A machine could then read it. Nothing does, `ono` has structured
output everywhere it has a schema, and inventing a ninth schema for eight lines an operator reads
on a console would be the wrong kind of thorough. `inspect limits` is where a machine asks.
