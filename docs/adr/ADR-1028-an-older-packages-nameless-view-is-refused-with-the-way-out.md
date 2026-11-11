# ADR-1028: A nameless view among several invocations is refused, and the refusal names the way out

- Status: accepted
- Date: 2026-11-10
- Spec refs: spec §31.27, §31.28; `docs/contracts/kuang/protocol.v1.yaml` → `views.open`;
  ADR-0991, ADR-0995, ADR-1027; v0.6.5 review finding R-F6
- Decided by: agent (autonomous)

## Context

ADR-0991 made `views.open` name the invocation a view belongs to. It accepts no name while
exactly one invocation is open and refuses it, `view.protocol_error`, while several are. Only the
0.6.5 SDK sends the name, so every package built with an earlier SDK opens its views nameless.

That was compatible as long as several open invocations were rare. They no longer are: a job runs
in the session's instance (ADR-0995), and an invocation beyond the ceiling now waits rather than
failing (ADR-1027). An older package that opens a view while a job reads from it was refused with
a message about handles that told its operator nothing they could act on.

The review offered two ways: refuse with a message naming the SDK upgrade, or attribute the view
to "the single invocation that has a pending host call / the most recent one", but only if that
is unambiguous and safe.

## Decision

**The host still refuses a nameless `views.open` while several invocations are open, and the
refusal says what makes the package work: the 0.6.5 SDK, or a manifest ceiling of one.**

- The refusal stays `view.protocol_error`; its help names `ono-kuang-sdk 0.6.5` (which names a
  view's invocation) and `runtime.max_concurrent_invocations: 1`.
- A ceiling of one in the manifest is a complete remedy without a rebuild: since ADR-1027 the host
  does not send an invocation beyond the negotiated ceiling, so it never has a second invocation
  open beside the one that opens the view, and a nameless view always has exactly one owner — while
  further invocations wait for the slot.
- No inference. Neither guess the review allowed is a fact the host has. The `views.open` request
  is itself the only host call the host can see in flight, so "the one with a pending call" names
  nobody; and "the most recent" is wrong whenever the older invocation opens the view, which for a
  job running beside the shell is the common case. A wrong guess hands one invocation's terminal to
  another and closes it when the wrong one ends — the defect ADR-0991 exists to prevent.

## Consequences

An older package with the SDK's default concurrency of one keeps working whenever its manifest
says so; one without the declaration is refused only when two invocations are really open in it,
and the operator reads which two lines fix it. Nothing changes for a package built with the 0.6.5
SDK.

Encoded by `crates/ono-kuang-sdk/tests/conformance.rs`:
`should_name_the_sdk_upgrade_when_a_view_names_no_invocation_while_several_are_open` and
`should_give_an_unnamed_view_its_invocation_when_another_waits_at_a_ceiling_of_one`.
`protocol.v1.yaml` (`views.open`) says so.

## Alternatives considered

**Attribute the view to the most recently started invocation, or to the only one with a pending
host call.** Rejected above: neither is unambiguous, and the cost of a wrong guess is another
invocation's terminal.

**Accept the nameless view and close it only when the last open invocation ends.** ADR-0991
rejected it: the view outlives its own invocation while a sibling runs.

**Learn a package's real ceiling from its first refusal and gate on it.** It would narrow the
window for a package whose manifest is silent, but it is a heuristic on top of a rule the manifest
can already state exactly.
