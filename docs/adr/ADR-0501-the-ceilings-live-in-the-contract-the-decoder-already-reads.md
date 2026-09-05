# ADR-0501: The ceilings live in the contract the decoder already reads

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.3, §12.1, §12.2, §12.3, §12.4, §52.1, §52.2, §52.3, §54.3, §55.1, §55.2,
  Appendix A; ADR-0015 T7, ADR-0453, ADR-0456, ADR-0461
- Decided by: agent (autonomous)

## Context

§12.4 asks for one thing and forbids another in the same breath:

> Their defaults MUST be centralized in one `Limits` contract and MUST be printed by a diagnostic
> command or test fixture.
>
> No code path may construct an effectively unlimited `Limits` instance for a network listener in
> production.

Ono arrived at H3 with three separate answers to the first half. `ono_protocol::Limits` held the
four wire bounds — frame payload, value depth, stream count, credit window — as a typed contract
with constants beside it. `docs/contracts/hardening/limits.yaml` held thirteen configuration keys
including four `limits.remote_*` rows, declared, range-checked and readable through
`inspect limits`, every one of them marked `enforced_by: pending` because nothing read them
(ADR-0456). `docs/contracts/hardening/remote_limits.yaml` — §52.1's second named registry — did not
exist. And the connection ceilings §12.1–§12.3 fix had no home in code at all.

The second half was untrue as written. `Limits::with_max_frame_payload` clamped to
`u32::MAX as usize`, so a caller could ask for a four-gibibyte frame and get it; the other three
setters had a floor of one and no ceiling. Nothing in production called them that way, which is
exactly the state ADR-0453 refused to accept for `Budget`: a guarantee that holds because nobody
has broken it yet is a guarantee a reviewer has to keep making.

## Decision

### 1. One `Limits`, widened from the wire to the listener

The four connection ceilings of §12.1–§12.3 become fields of `ono_protocol::Limits` beside the
four wire bounds: `max_connections`, `max_pending_handshakes`, `max_connections_per_client` and
`handshake_timeout`. Not a second `RemoteLimits` type. §12.4 says *one* contract, §52.2 says a
number must not be typed into five files, and a listener that had to assemble its bounds from two
structs would be one refactor away from having them disagree.

`Limits` keeps `Default`, and its default is Appendix A. ADR-0453 forbade `Default` on `Budget`
because a defaulted budget would be *unlimited*; a defaulted `Limits` is Appendix A's finite set,
which is the same distinction that ADR made for `MaterializationLimits`.

### 2. Every setter clamps, so the unlimited value cannot be written down

`with_max_connections(u32::MAX)` answers 65 536 — the maximum
`docs/contracts/hardening/limits.yaml` declares for `limits.remote_connections`. `with_max_connections(0)`
answers 1, because a ceiling of zero would turn the listener off silently and §2.3 wants a
boundary that refuses rather than one that disappears. The wire setters gained the same treatment:
a frame ceiling can now be lowered from `MAX_FRAME_PAYLOAD` and not raised past it.

This is the strongest available answer to "no code path may construct an effectively unlimited
`Limits`", and it is the answer ADR-0453 reached for `Budget`: the type has no representation for
the forbidden state, so no future caller reintroduces it and no reviewer has to check.
`crates/ono-remote/tests/limits.rs::should_offer_no_production_constructor_that_leaves_a_limit_unbounded`
reads the permitted maxima out of the registry and asserts each clamp against it, so raising a
range in the registry and not in the code — or the reverse — fails the gate.

**What could not be made unrepresentable.** A `NonZeroU32` would carry the floor in the type and
not the ceiling, and there is no standard type for "an integer in 1..=65536". A newtype per
ceiling would carry both, at the cost of four wrapper types whose only behaviour is the clamp the
setter already performs. The clamp is where every value passes, and the fields are private, so the
invariant holds for every instance that exists — one step weaker than `Budget`'s, and the step is
that a reader must look at the setter rather than at the field type.

### 3. `docs/contracts/hardening/remote_limits.yaml` exists and holds no numbers

§52.1 names `remote_limits` as one of its seven registries and ADR-0456 folded its *numbers* into
`limits.yaml`, leaving the question of the file itself open with an explicit hand-off:

> `docs/contracts/hardening/remote_limits.yaml` — if §52.1's separate registry is still wanted — should
> describe the *semantics* of the enforcement and reference these keys rather than restate the
> numbers.

That is what it now does. One row per ceiling, carrying `limit_key` (the `limits.*` key that holds
the number), the accessor that answers it, the component that enforces it, the point in a
connection's life at which it applies, the stable error a refused peer receives, and the §14.1
audit class the decision is recorded under. Plus two prose sections that are policy rather than
number: what revocation does to an established connection (§12.5) and what one failing connection
may cost the others (§12.6).

The test asserts the absence: a row carrying a `default` or a `min` fails, and a `limit_key` that
`limits.yaml` does not declare fails. So the two files cannot drift, because there is nothing in
both of them.

### 4. The four `limits.remote_*` rows become `enforced_by: ono-remote`

They said `pending` because nothing read them. `ono_remote::ListeningAgent` now does, so the
registry says so. The change lands with the increment that makes it true rather than with this
one — #53, by which point all four are enforced — and is recorded here so the sequence is
findable.

## Consequences

Easy: a listener's bounds are one value, passed where the wire bounds were already passed. Adding
a ninth bound is a field, a clamped setter, a row in `limits.yaml` and a row in
`remote_limits.yaml` — and the test fails until all four exist.

Hard: `Limits` now means two things — what this end enforces on the peer it is talking to, and
what the agent enforces across all its peers. A connection-scoped copy carries three ceilings it
personally has nothing to do with. The alternative was two types and one more chance for them to
disagree, and §52.2 is explicit about which risk it would rather run.

Also hard: `ono_protocol::MAX_CONNECTIONS` and `limits.remote_connections` are the same number in
two files, held together by a test rather than by construction. Genuinely one file would mean
`ono-protocol` parsing YAML at runtime, and a registry that failed to parse would have to answer
either "unlimited" or "panic" — a security boundary whose value depends on a file read succeeding
is the shape §55.2 spends its one MUST NOT on. The catalogue in `ono_cli::settings` is held to the
registry the same way (ADR-0456), so this is the repository's existing discipline rather than a
new one.

Owed elsewhere: the four `SettingSpec` descriptions in `crates/ono-cli/src/settings.rs` still say
"Declared and validated; enforcement is phase H3's", which stops being true with this milestone.
The strings are user-visible through `get config` and `inspect limits`, and correcting them is a
one-line edit in a file this agent does not own; it is recorded for the board.

Encoded by `crates/ono-remote/tests/limits.rs::should_read_every_connection_ceiling_from_the_one_limits_contract`
and `::should_offer_no_production_constructor_that_leaves_a_limit_unbounded`.

## Alternatives considered

**A separate `RemoteLimits` type in `ono-remote`.** It puts each ceiling next to the code that
enforces it and gives the listener a type that carries nothing irrelevant. It also gives the
product two contracts where §12.4 asks for one, and makes "which limits does this agent enforce?"
a question with two answers.

**`ono-protocol` parsing `limits.yaml` at runtime, so there is literally one copy.** The most
faithful reading of §52.2, and it makes a boundary's value depend on a file read. Every failure
mode is bad: fall back to a constant and the constant is the second copy again; refuse to start
and a malformed comment in a documentation file stops the agent; answer unlimited and §55.2's one
MUST NOT is broken by design.

**Leaving the wire setters unclamped, since no production path widens them.** That is the sentence
ADR-0453 wrote about `unlimited_for_tests`: the hole exists, it is public because integration tests
live outside the crate, and nothing but a reviewer stops a production path finding it.

**Numbers in `remote_limits.yaml` as well, for a reader who wants one file.** §52.2's own worked
example is `max_connections = 32` appearing twice. A reader who wants the number follows one key.
