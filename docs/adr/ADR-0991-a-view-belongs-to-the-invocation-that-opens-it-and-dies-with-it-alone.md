# ADR-0991: A view belongs to the invocation that opens it, and dies with it alone

- Status: accepted
- Date: 2026-11-10
- Spec refs: §31.27, §31.28, §31.73, §31.74; `docs/contracts/kuang/protocol.v1.yaml` → `handles`,
  `views.open`; ADR-0572, ADR-0586
- Decided by: agent (autonomous)

## Context

Spec §31.28 says a view outlives no invocation, and `protocol.v1.yaml` says the same of every
handle: "scoped to the invocation that created it and dies with it", "Every other handle belongs
to exactly one of these". The supervisor honoured the first half and not the second: when *any*
invocation answered, `handle_envelope` called `close_all_views(false)`. With one invocation per
instance that was the same thing. Since ADR-0586 an instance answers several at once, and the
first to finish tore down every other invocation's view — the terminal was taken back from a
handler that was still waiting for keys, and it was not even told (issue #157).

The host could not do better without being told whose view it is. `views.open` carried the view
id and an input stream; host calls are not otherwise attributed to an invocation, and the
supervisor's own `invocation_label` reads "one running invocation labels the trail precisely;
anything else is session-scoped". The SDK knew — its worker runs exactly one invocation — and
already kept its own view-to-invocation map for routing events.

## Decision

**`views.open` names its invocation, and the host closes a view when, and only when, that
invocation ends.**

- `ViewOpenParams` gains `invocation` (optional on the wire; additive, so `kuang-host/11.1` is
  unchanged). The SDK fills it from the invocation handle `command.invoke` / `provider.query`
  delivered, for every `Ctx::open_view`.
- The supervisor records the owner beside the view's handle. When an invocation's result arrives
  — completed, failed or cancelled — it closes that invocation's views and no others, with the
  terminal restored by the view host exactly as before.
- **A named invocation that is not open is refused** with `view.protocol_error`: a view whose
  owner has already ended would outlive it from its first moment.
- **No name is accepted while exactly one invocation is open**, which then owns the view. Every
  package built before this change opens views only from its single invocation, so it keeps
  working unchanged.
- **No name while several are open is refused** with `view.protocol_error`. The host cannot tell
  whose terminal it would be handing out, and both guesses are wrong for someone: the first-ending
  invocation would close it under the other, the last-ending would let it outlive its own.

`close_all_views` is gone with its only caller.

## Consequences

Two concurrent view-opening invocations on one instance each keep their own view to the end of
their own invocation, and a view a handler forgets to close still comes back to the operator when
that handler ends, however it ends. Encoded by `crates/ono-kuang-sdk/tests/conformance.rs`:
`should_keep_a_view_open_when_another_invocation_on_the_instance_completes`, `…_fails`,
`…_is_cancelled` (two views at once; the first invocation ends leaving its view open; the host
closes it, the second survives, still answers keys, and closes with its own invocation), and
`should_refuse_a_view_whose_named_invocation_is_not_open`,
`should_give_a_view_that_names_no_invocation_to_the_only_one_open`,
`should_refuse_a_view_that_names_no_invocation_while_several_are_open` for the ownership rule.
The test host's `RecordingViews` gained `is_open(index)` and `press_on(index, key)` so a test can
address one view of several; the example package's `browse` gained `end` (leave the view open
and end as told) and `invocation` (open the view with a raw call naming an owner, or none).

A package that runs concurrent invocations and speaks the protocol without this SDK must now name
the invocation when it opens a view with others open. That is a stricter reading of a rule the
contract already stated, applied where the alternative is closing someone else's view.

Not changed here: an instance that crashes answers its open invocations from `close_everything`,
not through a result frame, so their views are not closed through `close_view` there — neither
before this change nor after it.

## Alternatives considered

**Infer the owner from timing** (the invocation most recently started, or the only one with a
pending host call). Neither is a fact the host has; both are wrong under exactly the concurrency
that made the bug reachable.

**Close every view when the last open invocation ends.** Removes the crossover but lets a view
outlive its own invocation whenever a sibling is still running — the other half of §31.28.

**Key the owner by the output stream handle**, which the SDK already uses internally. The
contract's vocabulary for "whose handle" is the invocation handle, which `InvokeParams` documents
as "The invocation every handle the plugin opens will belong to"; the wire uses that.
