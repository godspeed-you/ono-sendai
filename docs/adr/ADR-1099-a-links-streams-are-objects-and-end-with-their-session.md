# ADR-1099: A link's streams are objects, end with their session, and the plan carries its context

- Status: accepted
- Date: 2026-11-24
- Spec refs: spec §16.5, §18.5, §21.4, §22.4, §33.4, §42.2; v0.4.1 §12.5, §12.6; ADR-0105,
  ADR-0106, ADR-0942, ADR-1057
- Issues: #219 (b), #219 (c)
- Decided by: agent (autonomous)

## Context

Issue #219 carries two remote gaps besides the probing `watch host` (ADR-1096):

- **(b) the multiplexed streams of `trace link`.** `ono.link.trace`'s summary promises "a link's
  transport, agent, negotiated providers and multiplexed streams"; ADR-0105 drew the first three
  and left the streams out because "the protocol does not expose its open streams as objects, and
  drawing a count would be a number without an identity". The workstream brief adds the
  semantics behind them: stream identity, cancellation and errors per stream; concurrent streams
  over one link must not interleave or lose each other's errors; a cancelled stream beside a live
  one; a peer that dies mid-stream.
- **(c) the execution context in the `ono.execution-plan/1` value.** ADR-0106 printed `explain`'s
  EXECUTION CONTEXT block and deferred the value form; the issue's evidence was
  `deferred.yaml:18`.

## Decision

### (b) Streams are objects, and a session's streams end with it

**Objects.** `Link::open_streams` (and the weak `Prober::open_streams`) lists the streams a link
carries: the id both ends route the stream's frames by, what opened it (`query`, `subscribe`,
`act`, `adapt`, `probe`, `links`), the target it asked about, and when. `ono.link/1` gains
`streams`, read when the link table is read — a list for an agent link (empty when it carries
nothing), null where nothing multiplexes (a definition, the agentless fallback). `trace link`
draws each as an `ono.link-stream/1` node (identity: link and id) that the link `carries`,
`exact`, from the new `LinkStreams` relationship provider. A stream is in the table exactly while
it is open, so a cancelled one leaves it and a live one beside it stays.

**Semantics, verified rather than assumed.** Per-stream failures stay on their stream, side by
side (four concurrent streams: two flaky, one unanswerable, one clean); a cancelled stream does
not disturb a live one; and a peer that dies mid-stream must end every open stream with
`remote.unreachable` after the values that arrived. The last one **failed**: when the agent's
serving loop ended — the caller hung up, the transport failed, or the session was dropped from
outside, as a revocation does (§12.5) — its producers kept waiting for credit, each holding a
clone of the frame sink, so the writer task and the socket stayed open and the caller's streams
never ended. `serve` now holds a drop guard that, however the loop ends, first hangs the wire up
and then cancels every open stream; the hang-up is queued first, so a cancelled producer's
closing `end` is never written and the caller cannot read a dead session's stream as complete.

This bounds the writer task's life by its session's on every exit path. It deliberately does not
add a write deadline to an established session's writer (the remote-protocol workstream's
finding about `connection.rs::spawn_writer`): a reader that is slow on a live session is
legitimate back-pressure, and that decision is not this one.

### (c) The plan value carries the execution context — already, and now pinned

ADR-0942 (#173) made `explain` one producer of one `ono.execution-plan/1` record and folded the
session's execution context into it: `context` holds `host`, `link`, `transport`, `mode`,
`answers` and `identity` inside a link and is null on the local machine;
`docs/contracts/schemas/execution-plan.v1.yaml` exists and the schema left `deferred.yaml` in
that change (it no longer lists it). What #219 (c) asked for is therefore the public contract
already; what was missing was a test of the *value* form inside a link — the existing ones read
the rendered block. `crates/ono-cli/tests/explain_context.rs` pins it: the context of a remote
mutation's plan, a reduced link's `answers`, and null locally.

## Consequences

- Tests: `crates/ono-protocol/tests/multiplexing.rs` (three cases; the dying peer reproduced the
  hang), `crates/ono-cli/tests/link_streams.rs` (a stream open while the link is traced is drawn
  by id, kind and target; an idle agent link lists `[]`, an agentless one `null`),
  `crates/ono-cli/tests/explain_context.rs`. Acceptance case
  `459-trace-link-draws-the-streams-it-carries.case`.
- New schema `ono.link-stream/1`; `ono.link/1` gains `streams`.
- A listening agent no longer keeps a socket and its tasks alive after a session with open
  streams ends.

## Alternatives considered

- **Draw a stream count on the link node.** ADR-0105's own objection: a number without an
  identity.
- **Send `end` on every stream when a session ends.** A session that ends is not an answer that
  completed; the caller must see the link end (§16.5: a failure is not an empty result).
- **A write deadline on the session writer.** Would end slow but healthy readers; out of this
  decision's scope.

## Amendment (v0.6.6 remediation): a stream opened as its link fails

`LinkInner::open` checked the link alive and then registered the stream under the streams lock;
`fail` marks the link dead and then drains the streams under that lock. A stream opened in
between — checked alive, then the drain, then the registration — sat on a dead link that nothing
would ever end, and its reader waited for ever. The liveness check is now repeated inside the
lock, so a stream is either registered before the drain (and told) or refused. Present since
before v0.6.6 (`42cf37d6`'s `open` has the same shape); found as an intermittent 20 s hang of
`crates/ono-remote/tests/limits.rs::should_refuse_a_never_authorized_client_with_a_refusal_even_when_a_revocation_sweep_runs_first`,
whose `still_serves` query races the revoked session's close. Encoded by
`crates/ono-protocol/tests/streams.rs::should_end_every_stream_opened_while_the_link_is_failing`
(a thousand links failed while streams are being opened; without the fix it failed four runs
in five at three hundred).
