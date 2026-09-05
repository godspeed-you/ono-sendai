# ADR-0036: A remote machine is mounted as ordinary providers

- Status: accepted
- Date: 2026-08-26
- Spec refs: §21.2, §21.4, §25.2, §17.1, §35.3, §37 Phase H
- Decided by: agent (autonomous)

## Context

Spec §21.4 sketches the agent side of a link ("a small remote agent can expose native provider
calls and typed streams") but says nothing about how the local side surfaces what was
negotiated. The shell already has exactly one place where sources of objects live — the
`ono_provider_api::ProviderRegistry` — and every layer above it (evaluator, pipeline, renderer,
`watch`, `trace`) is written against the `Provider` trait. Phase H had to decide where remote
targets plug in, who serves them on the far end, and what happens to provenance and capability
metadata on the way across.

## Decision

`ono-remote` implements both ends of spec §21.4 as thin projections of the provider model over
`ono-protocol`:

1. **The agent is a `RemoteService` over a real `ProviderRegistry`.** `serve_registry` answers
   queries with `registry.snapshot`, subscriptions with `registry.subscribe`, actions with
   `registry.act`, and derives the negotiation material of spec §21.2 from the registry itself:
   one `ProviderDescriptor` per registered provider — id, targets, capabilities, availability
   with the provider's own reason (spec §35.3) — plus every schema any provider produces on top
   of the built-in ones. Nothing about the offer is written down twice. The agent additionally
   enforces the query's `limit` itself, because a provider is free to ignore it and an endless
   remote target must still end. `agent_main(stdin, stdout, config)` is the same loop over a
   process's standard streams and returns an `ExitCode` per ADR-0008 (`0` for a session that
   ended, `1` for an agent that failed); it is what the `ono --agent` flag will call.

2. **The client mounts one `RemoteProvider` per negotiated target.** `RemoteLink::connect`
   performs the handshake and trust decision (unchanged from `ono-protocol`, ADR-0015 T5/T6),
   then builds a `Provider` implementation per `(descriptor, target)` pair, ready for
   `ProviderRegistry::register`. `get process` against a linked machine is therefore the same
   code path as locally; nothing above the registry can tell. Unavailable remote providers are
   mounted too, with `Availability::Unavailable(reason)`, so "the remote has no systemd" stays
   distinguishable from "there are no services" (spec §21.3, §35.3).

3. **Provider identity is preserved; the machine is the provenance link.** A mounted provider
   keeps the *remote* provider's id (`linux.procfs` stays `linux.procfs`), because that is who
   produced the records. Which machine they came from is said where spec §25.2 says it: every
   arriving value is re-tagged `Link::Local → Link::Remote(host)` — host as the user named it —
   recursively through records, lists and maps, preserving provider, observation time, source
   and confidence. A record that already carries `Remote(other-host)` is left alone: on a
   chained link the origin is the machine that observed the record, not the last hop.

4. **Capabilities cross the link typed.** The handshake's `ProviderDescriptor` carries
   `CapabilityDescriptor { id, risk, elevation }` rather than bare names, so a mounted remote
   provider reports `process.signal` as `Mutate`+elevation exactly as the remote declared it —
   spec §17.1 computes risk displays from this, and a remote mutation must never look like a
   read. A risk name a build does not recognise decodes as `Destructive`: an ununderstood claim
   is over-stated, never under-stated. A bare name (the wire's default) decodes as `Read`.

5. **`Provider::targets()` returns interned names.** The trait returns borrowed `&[&str]`, but
   a remote target name exists only at negotiation time. Names are interned into a process-wide
   table that leaks one copy per *distinct* name — bounded by the target vocabulary
   (`docs/contracts/targets.yaml` plus plugin contributions), not by how often links are opened.

6. **Actions are not forwarded through the mounted provider — deliberately.**
   `ono_provider_api::Action` and `Query` expose their arguments/options only by name
   (`argument(name)`, `option_value(name)`), never for enumeration. A forwarding provider
   therefore cannot build a faithful `ActRequest`: it would silently drop, say, the signal of a
   `stop`, and a silently dropped argument on a mutation is the kind of lie ADR-0015 exists to
   prevent. `RemoteProvider::act` refuses with `provider.unsupported` and points at
   `RemoteLink::act`, which takes an explicit `ActRequest` carrying every argument. The same
   gap makes `RemoteQuery::from_query` drop provider options (already documented in
   `ono-protocol`). **Follow-up:** add `Action::arguments()` and `Query::options()` accessors
   to `ono-provider-api` (additive, a few lines) and then forward both faithfully; that crate
   was outside this increment's file scope.

7. **`resolve` is a query.** The protocol has no resolve message; `RemoteProvider::resolve`
   sends the selector as a query for its target and builds `ObjectRef`s from the re-tagged
   records. A failure beside resolved objects leaves the objects standing; a failure with no
   objects is returned as the error.

8. **Dropping a `Link` hangs up.** The protocol's reader task shares the link's frame sink, so
   the sink could never close by scope alone and a dropped client previously left the agent
   waiting forever. `Link` now sends an explicit hang-up on drop: queued control frames flush,
   the transport shuts down, and the agent's `serve` returns `Ok(())` — a caller going away is
   a successful end of session. (`fix` in `ono-protocol`, proven by
   `should_end_the_serving_side_cleanly_when_the_link_is_dropped`.)

## Consequences

Easy: the CLI wiring for `link host` is registry plumbing — connect, `register_into`, done;
`watch` over a link reuses `Provider::subscribe`; KUANG/11's remote projection (spec §31.39/40)
gets providers it can already speak to.

Hard: remote actions have two paths until the accessor follow-up lands; `Provider::subscribe`
over a link cannot surface a remote "cannot watch" refusal eagerly (the event envelope has no
error channel — spec §31.14), it surfaces as an immediately ending stream; provider options do
not cross the link yet, which matters for `get file --recursive` the moment the file provider
is linked.

Encoded by: `crates/ono-remote/tests/{agent,provider,trust,subprocess}.rs`,
`crates/ono-protocol/tests/handshake.rs`
(`should_carry_a_provider_capability_with_its_risk_across_the_link`) and
`crates/ono-protocol/tests/streams.rs`.

## Alternatives considered

- **A dedicated remote command family talking to the link directly** — rejected: it would fork
  every value-producing code path into a local and a remote variant, which is exactly what
  spec §21's "object-aware remote execution" exists to avoid.
- **Renaming mounted providers (`remhost:linux.procfs`)** — rejected: provenance already has a
  field whose meaning is "which machine", and a synthetic id would break provider-id equality
  with `docs/contracts/providers/*.yaml`.
- **Forwarding actions with the arguments reachable by known names** — rejected: guessing
  argument names per operation is a hidden contract; refusing loudly is honest and cheap until
  the accessors exist.
- **Capabilities as bare names on the wire** — rejected: a remote `process.signal` would mount
  as `Read`, under-stating a mutation (ADR-0015).

## Amendment (2026-08-26): actions and options now forward

Point 6's follow-up landed the same day: `ono-provider-api` gained `Query::options()` and
`Action::arguments()`, which removes the only reason the mounted provider refused actions.
The rules of point 6 are superseded as follows; everything else in this ADR stands.

- `RemoteQuery::from_query` carries **every option** (and the agent replays them into the
  `Query` it runs), so `get file --recursive` over a link asks the remote the same question it
  would ask locally.
- `ActRequest::from_action` converts a local `Action` losslessly — operation, target identity,
  all arguments, the dry-run flag — and `RemoteProvider::act` forwards through
  `RemoteLink::act`. The structural refusal is gone; `RemoteLink::act` remains as the explicit
  form for callers that already hold an `ActRequest`.
- Spec §16.5 holds across the wire: an action that was attempted and failed comes back as a
  `Failed` `ActionOutcome` with its structured error (code and message) intact, never collapsed
  into a link error; a dry run comes back `Skipped` with the remote's own message.
- No new protocol message was needed: `RemoteQuery` and `ActRequest` always had the fields;
  only the conversions were lossy.

Encoded by: `crates/ono-protocol/tests/messages.rs`
(`should_carry_every_option_when_a_local_query_becomes_a_remote_one`,
`should_carry_every_argument_when_a_local_action_becomes_a_remote_request`) and
`crates/ono-remote/tests/provider.rs` (`should_carry_a_provider_option_across_the_link`,
`should_forward_an_action_with_its_arguments_through_the_mounted_provider`,
`should_report_a_failed_remote_action_as_an_outcome_not_an_error`,
`should_carry_a_dry_run_and_its_argument_to_the_remote`).
