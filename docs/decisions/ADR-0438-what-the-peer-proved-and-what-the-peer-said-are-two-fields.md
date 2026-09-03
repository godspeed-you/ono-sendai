# ADR-0438: What the peer proved and what the peer said are two fields, and stay two fields

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.1, §2.6, §4.1, §4.3, §7.1, §7.3, §7.4, §14.3; spec §21.2; ADR-0037 §4,
  ADR-0090, ADR-0274, ADR-0353, ADR-0434, ADR-0437
- Decided by: agent (autonomous)

## Context

ADR-0437 made the listening side demand a client certificate. The client half of §7.1's symmetry
came with it — `connect` presents a `PeerIdentity` and keeps verifying the server's certificate as
a pinned host key — so what was left of issue #36 is §7.3, which is not about the handshake at all
but about how the result is *described*:

> The authenticated transport identity and the runtime `Identity { user, uid, elevated }` MUST
> remain separate fields. […] The runtime identity is useful context but MUST NOT grant authority.

§7.3 then sketches a `Peer` view with `transport_fingerprint`, `transport_trust`, `runtime_user`,
`runtime_uid`, `runtime_elevated`, `agent`, `os`, `arch`.

The protocol crate already carried all of it: `Negotiated` has held `trust`, `fingerprint` and a
`PeerInfo` with the runtime `Identity` since ADR-0353. What it had was nowhere to be seen. The
`ono.link/1` record — the thing `get link` answers with, and the only place a person inspects a
link — had `name`, `host`, `transport`, `mode`, `state`, `targets`, `protocol` and `providers`,
and not one field about who is on the other end.

## Decision

**The `Peer` view of §7.3 is `ono.link/1`, extended additively, and not a new object.** Five
nullable fields join the schema: `transport_fingerprint`, `transport_trust` (`pinned` ·
`newly_pinned` · `unauthenticated`), `runtime_user`, `runtime_uid`, `runtime_elevated`. §4.1
permits exactly this — "existing typed schemas MUST remain compatible unless a security-relevant
field needs an additive extension" — and the alternative, a second record for the peer, would give
a person two objects to correlate for one connection they made with one command.

A link is session state, not something a provider observed (ADR-0090), so the fields are filled
from `LinkConnection` where the rest of the row is, and `default_view.columns` is unchanged: the
existing six-column `get link` keeps its shape, and the identities are asked for by name.

**`agent`, `os` and `arch` are not added.** They are in §7.3's sketch, and they are neither of the
two identities the section is about — they are what the far side is, already reachable through
`test host` and `far_end_name`. Adding them here would widen a security-relevant schema change
with three fields nothing in this tranche needs.

**`transport_fingerprint` is null over `ssh` and `local`, and that is the answer, not a gap.**
§4.3 spells out what an ssh-carried agent must report — "peer key visible to ono: no" — and
`SubprocessTransport::peer_key` has answered `None` truthfully since ADR-0037 §4 for the reason
ADR-0274 documented: OpenSSH authenticates the host in its own `known_hosts` and offers the parent
no way to learn which key it accepted. §2.6 forbids inventing certainty to make a field look full.
`transport_trust` says `unauthenticated` there, in the word §7.4 asks for, so the row reads as the
statement it is rather than as missing data.

**The runtime identity is proven to grant nothing by a guard, not by a red test.** `decide` takes
`(policy, store, host, key)` and has never been able to see a runtime identity, so there was no
failure to reproduce. The two tests added to `ono-remote/tests/trust.rs` hold the key fixed and
vary the claim: a peer calling itself `root`, uid 0, elevated is still refused when its key is not
pinned, and a peer calling itself `nobody` is still accepted when its key is. They passed the
moment they compiled, which is what a guard on an invariant that already holds does; what they buy
is that the day someone adds an `if peer.is_elevated()` to a trust path, one of them goes red. §7.3
is an invariant to keep rather than a behaviour to build, and this is the shape of test that keeps
one.

## Consequences

Easy: `get link | select transport_fingerprint transport_trust runtime_user` answers §14.3's
"user-facing inspection" requirement, and a person can see at a glance whether a link is
authenticated to *this* process or merely carried by something else that authenticated it.
Phase H2's authorization commands have a fingerprint to name a client by that a user has already
seen in a link row.

Hard: five nullable fields on a schema that is now security-relevant means five places a future
change could make a claim that is not true. The rule that keeps them honest is one sentence — a
`transport_*` field is written only from something this process verified — and it is stated in
the schema's own documentation, where the next person to add a field will read it.

Also hard: `transport_trust` names `newly_pinned`, a decision `TrustPolicy::Required` produces and
which the CLI's `tcp` path never asks for (it uses `Pinned`, ADR-0354). The value is in the enum
because the protocol can produce it and a row that could not spell it would be a row that lies
about a link somebody made with a different policy.

Encoded by: `crates/ono-cli/tests/authenticated_link.rs::should_show_the_proved_identity_and_the_reported_one_as_separate_fields`,
`::should_report_no_proved_key_over_a_transport_that_proves_nothing`,
`crates/ono-remote/tests/trust.rs::should_refuse_an_unpinned_peer_however_privileged_it_says_it_is`,
`::should_carry_what_the_peer_says_about_itself_beside_the_decision_about_its_key`,
`crates/ono-remote/tests/client_authentication.rs::should_report_the_key_the_accepted_client_proved_it_holds`.

## Alternatives considered

**A new `ono.peer/1` schema, one record per connected link.** Closest to §7.3's literal `Peer`
block. Rejected: a link already is the object a person holds, and splitting its identity into a
second record means `get link | join get peer` to answer "who am I talking to", which is the
question the row exists for.

**Merge the two identities into one `identity` field with a `proved: bool` beside it.** Compact.
Rejected outright: §7.3 says *separate fields*, and a boolean qualifier on a merged value is
precisely how a self-reported user name ends up being read as an authenticated one — §65.1's
failure mode, in a schema.

**Fill `transport_fingerprint` for `ssh` from `~/.ssh/known_hosts`.** Ruled out already by
ADR-0274 and §2.17: it would record a verification OpenSSH performed as if Ono had performed it.
Restated here because the field's existence makes the temptation new.
