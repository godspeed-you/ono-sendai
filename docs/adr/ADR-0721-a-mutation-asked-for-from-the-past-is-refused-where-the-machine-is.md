# ADR-0721: A mutation asked for from the past is refused where the machine is

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §4.7, §30.6, §38.4; v0.4.1 §9.5, §10.2, §54.1, §59.1; ADR-0473
- Decided by: agent (autonomous)

## Context

v0.5 §4.7 makes historical context read-only, and §30.6 says remote temporal access "must respect
the same link authorization and capability model as current remote provider access". The obvious
reading is that the local guard is enough: the shell knows it is standing in the past, so it never
sends the mutation. That reading has the shape of every authorization bug v0.4.1 §10.2 was written
against — "negotiation filtering is not sufficient by itself" — and the agent already asks the same
question twice for exactly that reason (`crates/ono-remote/src/agent.rs` re-checks `require_observe`
that the protocol loop already checked).

The difficulty is that the agent cannot know. Whether the caller was standing at 11:50 is a fact
about the *caller's* session, and nothing on the wire carried it.

## Decision

**The caller declares the coordinate it asked from, and the agent refuses on it.**

`ActRequest` gains `attempted_from: Option<jiff::Timestamp>`, encoded as one nullable field.
`None` is the present, which is what every build before v0.5 meant, so an older peer's request
decodes unchanged and a caller in the present is untouched.

`PeerAuthorization::require_present(what, attempted_from)` is the refusal, called at the top of the
agent's `act` before any capability question. It answers `temporal.read_only` (E1304) — the
temporal family's own code rather than a capability denial, because the boundary that said no is
time and not a grant — and carries `denied_because: "historical_context"` beside the existing
`observe_not_allowed`, `capability_unknown` and `action_not_granted`. A script already matching on
that field learns which of the three boundaries closed.

The sentence names the peer and the instant, because §54.1 requires the refusal to name the
deciding boundary in the message and nothing on the default rendering path prints metadata.

**A caller that lies by omission is not the case this defends against.** A caller that omits the
coordinate has already been refused by its own local guard, which is where §4.7 puts the first
decision; what this adds is that a caller who tells the truth is refused twice, so the mutation
does not depend on one shell's bookkeeping being right. §38.4 extends the same rule to an
assistant, and `ono_model_broker::TurnStance::admits` is that rule on the model side.

## Consequences

`crates/ono-remote/tests/temporal.rs` proves both directions: a request carrying a past coordinate
is refused with `temporal.read_only` and the new discriminator, and a request carrying none
succeeds. `crates/ono-protocol/tests/messages.rs` proves the field round-trips and that its absence
is the old behaviour.

`ono-cli` is where a session's historical coordinate lives, so nothing sets `attempted_at` in the
product yet; the seam is `crates/ono-remote/src/client.rs`'s `Provider::act`, one line, in the
increment that gives the session a temporal context.

## Alternatives considered

**Refusing at the client only.** Rejected: it makes the safety of a remote machine depend on the
correctness of whatever is at the other end of the link, which is the assumption v0.4.1 §65.3
forbids.

**A new frame kind for a historical action.** Rejected: a frame kind an older peer cannot decode
turns a refusal into a link failure, and there is nothing to refuse differently — it is the same
action with one more fact about who asked.
