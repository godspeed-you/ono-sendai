# ADR-0716: The container engine answers its own lifecycle, live and in the past

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §3.3, §6.8, §10.5, §21.3, §21.4, §21.5, §21.6, §22.7, §35.3; v0.2 §18.2, §23,
  §31.57; ADR-0024, ADR-0112, ADR-0113, ADR-0710, ADR-0711
- Decided by: agent (autonomous)

## Context

v0.5 §22.7 asks container providers to "expose runtime-native lifecycle events when the runtime
supports them", and requires that "container event IDs and runtime identities MUST reconcile with
v0.4 container spatial identity". The Docker Engine API — which Podman serves compatibly — has
exactly such a stream at `GET /events`, and the same endpoint bounded by `since` and `until`
replays a past window and then closes. So one endpoint is both of §21's live source and historical
source, and this provider was opening neither: `watch container` fell through to the runtime's
poll loop, and `history` refused.

Two things stood in the way. The crate's HTTP client reads a whole response into a `Vec`, which
for a live event stream means answering nothing until the daemon stops. And the event envelope
`ono.container-event/1` declares `source: enum [subscription, poll]`, which has no word for an
event read out of the past.

## Decision

**The provider opens `GET /events` for both.** `Provider::subscribe` opens it with no upper bound
and relays what arrives; `Provider::history` opens it with `since` and `until` and streams the
replay. One decoder serves both, because the bytes are the same bytes.

**A historical event is an `ono.container/1` record dated by the engine's own instant.** Not an
`ono.container-event/1`: §22.7's requirement is about identity, and an ordinary container record
keyed on the engine's full container id *is* the v0.4 spatial identity, resolving to the same
`SpatialId` that `get container` produces. The record's provenance carries `timeNano` as the
observation instant, so a historical read is dated by the engine and never by when this shell got
round to asking (§3.3). Widening `ono.container-event/1`'s `source` enum was the alternative, and
it is a schema change in another package's contract for no gain: a consumer that wants the event
envelope already gets it from `watch`.

**What the event does not say stays null.** An event is not an inspection: it names the container,
its image and its name, and says nothing about `created`, `image_id` or `labels`. Those are
`Value::Null` rather than values copied out of a later listing (§35.3). What the event adds beyond
identity rides in two provider extensions, the way `systemd.*` does on `ono.service/1`:
`container.action` is the engine's verb verbatim, and `container.event_time_nano` is the
nanosecond instant that with the container id is the deduplication key of §6.8.

**`state` follows the engine's action where the action states one.** `create` leaves a container
`created`, `start`/`unpause`/`restart` leave it `running`, `pause` leaves it `paused`, and
`die`/`stop`/`kill`/`oom` leave it `exited`. The engine's action vocabulary is much larger than
its state vocabulary and most of it says nothing about the state that follows — `exec_start`,
`health_status`, `rename`, `update` all leave the container exactly as it was — so those are
`unknown`, which is the value `ono.container/1` already reserves for a state this shell does not
model (§10.5).

**An open upper end is closed at the instant the question is asked.** `GET /events` without
`until` never ends, and a question about the past that never returns is not an answer. This is the
one clock read in the provider's temporal path, and it bounds a question rather than dating an
observation.

**`historical_query: true`, `live_events: true`, `exhaustive_events: false`.** The last is the one
worth stating: the engine's event log is bounded by its own retention and by the daemon's
lifetime, neither of which this provider can read, so a container that came and went across a
daemon restart left nothing to find. `causal_tokens: false` too — the engine publishes no request
identifier on the event stream, so there is no transaction here to join a cause on (§21.6).

**`image` cannot be watched.** The engine's stream carries image events, and this provider's
subscription refuses them with `provider.unsupported` rather than reporting a container event as
an image one. `watch image` therefore keeps polling, which §18.2 makes explicit in `source`.

## Consequences

- `watch container` reports what the engine said happened, in the engine's own words and at the
  engine's own instants, rather than what two listings imply happened between them. A container
  that started and stopped inside one poll interval is now visible.
- `history container` is the second built-in historical source in the tree, beside the journal.
- The HTTP client grew `open_stream`, a body read a piece at a time with a bounded line length.
  The connect and head read are budgeted; the body is not, because a live event stream
  legitimately has nothing to say for hours.
- The provider holds no state across events, so a subscription is correct from the first byte and
  does not have to enumerate first. `ono_command::watch_events` still takes its own snapshot, and
  folds these in as the changes they are (ADR-0024).
- **A container event carries less than a container listing.** A consumer that needs `created` or
  `labels` for a container it learned about from the event stream has to ask for it.

Encoded by `crates/ono-provider-container/tests/events.rs`, against a Unix socket serving recorded
Engine API bytes — the same fake-of-the-outside-world the crate's HTTP tests already use.

## Alternatives considered

**Keep polling and claim nothing.** The status quo, and it is honest, which is why the matrix said
so. It also loses every container whose whole life fits inside a poll interval, and §22.7 asks for
better where the runtime offers it.

**Return `ono.container-event/1` from `history`.** It carries the action as a first-class field
rather than as an extension. It needs a third value in the `source` enum, which is a change to a
schema this package does not own, and it would make a historical container something a `where`
clause written against `ono.container/1` cannot follow (§28.2's concern, applied to containers).

**Claim `causal_tokens` on the engine's `Actor.ID`.** It is an identity, not a transaction: it
says which container an event is about, not which piece of work produced it. §21.6 is about the
latter, and a token that joins an object to itself would make `ono.provider-causal-token` fire on
every pair of events about one container.
