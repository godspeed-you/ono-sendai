# ADR-0475: Four words, four fields, and a link that says which one it means

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.1, §2.6, §4.1, §4.3, §7.3, §14.3, §19.1, §65.1, §65.2; ADR-0438
- Decided by: agent (autonomous)

## Context

§14.3: "`get link` / equivalent link inspection SHOULD show the authenticated fingerprint and
authorization state for direct connections. The words `authenticated`, `authorized`, `pinned` and
`self-reported identity` MUST not be conflated." §19.1 fixes what each means:

| term | meaning |
|---|---|
| authenticated | cryptographic peer proof was verified |
| authorized | authenticated principal is permitted by policy |
| pinned | fingerprint matches a recorded trust decision |

ADR-0438 already separated two of the four on `ono.link/1`: `transport_fingerprint` and
`transport_trust` for what the peer proved and what was decided about it, `runtime_user`,
`runtime_uid` and `runtime_elevated` for what the peer said. What it could not add is the pair H2
brought into existence — whether *this process* verified anything, and whether the far side's
policy admitted the connection.

## Decision

**Two more nullable fields on `ono.link/1`, additively, as §4.1 permits for a security-relevant
extension.**

- `authenticated` — whether this process verified the far side's key on this link. True exactly
  where `transport_fingerprint` names a key; **false** over `ssh` and `local`, where something else
  did the authenticating and §4.3 requires the link to say so rather than borrow it. §65.1's
  mistake is calling a session authenticated because it is encrypted, and a boolean that is false
  over ssh is the sentence that refuses to make it.
- `authorized` — whether the far side's own policy admitted this connection. True for an
  established direct link, because the agent resolved its `authorized_clients` store before it
  negotiated anything (§9.4, §10.1). **Null** over `ssh` and `local`, where no policy this process
  can see decided, and null for a definition that was never established. §2.6 keeps unknown
  unknown; a `false` there would be a claim about a decision nobody made.

Four fields, four values, one row: `authenticated` (proof), `authorized` (policy),
`transport_trust: pinned` (the recorded decision), `runtime_user` (the claim). A reader can tell a
proved identity from a claimed one without knowing which of the four the implementer had in mind.

**Both are derived from the row the session already publishes**, not from a new field the session
has to fill. `authenticated` is `transport_fingerprint.is_some()`; `authorized` is "connected over
`tcp`". Deriving rather than storing means there is no way for the two to disagree with the
fingerprint beside them, and the rule ADR-0438 wrote for `transport_*` — a field is written only
from something this process verified — extends to both without a second place to remember it.

**A client the agent refuses never becomes a row, and that is not a gap.** The "authenticated but
unauthorized" state is a refusal, not a link: `remote.unauthorized` (E1202), kind `safety`, on a
connection whose host key was pinned and whose handshake completed. That is where an operator meets
it, so that is where the test asserts it — and it asserts that the code is not
`remote.peer_unauthenticated`, not `remote.host_key_changed` and not `remote.unreachable`, because
the whole point of §14.3 is that those are four different things.

**`default_view.columns` is unchanged.** Six columns, as before; the identities are asked for by
name. A person who wants them types `get link | select authenticated authorized transport_trust
runtime_user`, which is also the shape §14.3's sentence takes when it is a command.

## Consequences

Easy: `get link | where not authorized` is a question a person can now ask, and `get link | select
authenticated transport_trust` distinguishes "I verified this key" from "I had already recorded
it".

Hard: `ono.link/1` now carries seven nullable security-relevant fields, and each is a place a
future change could make a claim that is not true. The rule that keeps them honest is the one
sentence ADR-0438 wrote into the schema's own documentation, where the next person to add a field
reads it.

Encoded by: `crates/ono-cli/tests/authenticated_link.rs::should_distinguish_authenticated_authorized_pinned_and_self_reported_on_a_link`,
`::should_report_an_authenticated_but_unauthorized_link_as_exactly_that`,
`::should_report_no_proved_key_over_a_transport_that_proves_nothing`, case `186`.

## Alternatives considered

**One `trust` enum with a value per combination.** Fewer fields. Rejected: it is exactly the
conflation §14.3 forbids, and the combinations multiply — a peer can be authenticated and
unpinned, pinned and unauthorized, or authorized over a carrier that authenticated nobody.

**Record a refused link as a `closed` row with `authorized: false`.** Would put the state on the
table §14.3 talks about. Rejected: a link that was refused is not a link this session holds, and a
failing command that leaves a row behind is a worse surprise than a refusal that carries the whole
story in its code and metadata.
