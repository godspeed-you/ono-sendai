# ADR-0444: A pre-exec failure is a launch failure with a name

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.3, §2.6, §16.3, §16.5, §18.1, §53.1, §53.2, §53.3, §54.1, §54.2, §59.7,
  §59.8, §65.4, §67.3, Appendix D; base spec §31.79; AGENTS.md §7; ADR-0022, ADR-0430, ADR-0442,
  ADR-0443, ADR-0445
- Decided by: agent (autonomous)

## Context

v0.4.1 §16.3 requires two things of a failure inside the pre-exec child setup context:

> Failures that occur in a pre-exec child setup context MUST be propagated to the parent in a way
> that prevents `exec` of the plugin. The caller MUST receive a structured error identifying which
> control could not be installed.

and names the error family — `plugin.confinement_failed`, `plugin.resource_limit_failed`,
`plugin.no_new_privs_failed` — while §67.3 shows the rendered shape, down to the two lines
`required control: no_new_privs` and `execution tier: native-confined`.

The first half is free once ADR-0443's driver returns `Err`: the standard library abandons the
spawn on a `pre_exec` failure, so nothing execs. The second half is not, and the reason is
specific. What `std` carries back from that child is one integer — the `errno` — over a
close-on-exec pipe. An `errno` cannot say *which* control produced it: `EPERM` from `setsid` and
`EPERM` from `setrlimit` are the same integer. It also says nothing about the controls that
succeeded, or about a best-effort one that failed without stopping the spawn, both of which
§16.5's report needs.

Three questions followed.

## Decision

**1. The child writes its outcomes into a page it shares with the parent.** ADR-0445 records the
mechanism. What matters here is that the structured error is built from *observation* rather than
inference: the parent reads which control the child recorded as `failed`, and names that one.

**2. `plugin.*` is a new error family at `K118`, not three more `load.*` codes.** Base spec
§31.79's families are `001 package … 701 remote`; v0.4.1 §16.3 adds three codes with a prefix
none of them has. Folding them into `load.*` was tempting — a launch failure is a load failure —
and was rejected because §18.1 gives the new family a consequence the load family does not carry:
*"A plugin whose required confinement cannot be installed MUST not enter quarantine, because it
never safely started."* A distinct family is what makes that rule checkable rather than
remembered. `801` is the next free block, and the taxonomy stays closed and additive (ADR-0006):
nothing was renumbered or re-pointed. The codes are registered in all four places a code lives —
`docs/spec/errors.yaml`, `docs/spec/kuang/errors.v1.yaml`, `ono_core::ErrorCode` and
`ono_kuang_protocol::KuangErrorCode` — because a code missing from the first two is a code the
shell flattens into `provider.unsupported` on its way to the user, which is what it did before
this increment.

**3. Which of the three codes a failure gets is a property of the control, not of the call site.**
`Control::failure_code()` maps the privilege-transition control to `plugin.no_new_privs_failed`,
the seven resource ceilings to `plugin.resource_limit_failed`, and everything else to
`plugin.confinement_failed`. The distinction is the one an operator acts on: a refused `setrlimit`
is usually a hard ceiling below the requested soft one, which is a question about the shell's own
limits; a refused `PR_SET_NO_NEW_PRIVS` is a question about the kernel.

**4. The refusal names the *refused* control, never its consequences.** A mandatory failure stops
the sequence, so every control after it reads `not_attempted`. `ConfinementReport::unmet` prefers
a row that reads `failed` over one that reads `not_attempted`, whatever order they are stored in.
Without that rule the operator is told about a consequence and left to find the cause — §54.1
asks for the boundary that decided, and the boundary that decided is the one that was refused.

**5. The error carries its detail in metadata, and the shell forwards it.** `control`,
`execution_tier`, `result` and `platform_detail` are machine-readable fields, because §53.2
forbids string matching for policy. `crates/ono-cli/src/plugins.rs::error_value` now copies a
`KuangError`'s metadata onto the `ErrorValue` the shell raises; it dropped it before, which left
the sentence as the only place the control's name appeared. §54.2: none of this needs
`RUST_LOG=debug`.

## Consequences

Easy: `try { load plugin … } catch e { $e | to json }` answers with the code, the dotted name, the
control, the tier and the platform's own reason — the shape §67.3 draws. `docker/acceptance/cases/189-kuang-confinement-fail-closed.case`
arranges the failure with a hard `RLIMIT_NOFILE` below the tier's 256, which needs no test hook at
all: an unprivileged process cannot raise a hard limit, so the child's `setrlimit` is refused by
the kernel and §59.8's scenario runs against the shipped binary.

Hard: the K-family now lives in five files that must agree, and only `spec-check` makes them.
That was already true of the other twenty-seven codes.

Also: `sandbox::apply` is no longer public. ADR-0430 exported it so the H0 proof had a boundary to
assert at and anticipated this — *"the H4 fix will very likely change its signature … the proof
asserts on the marker file and not on the signature, so it survives either shape."* It did:
`spawn` replaced it, the proof's assertion is the one it was written with, and its `#[ignore]` is
gone.

Encoded by: `crates/ono-kuang-supervisor/tests/confinement.rs::should_never_exec_the_plugin_when_a_mandatory_control_cannot_be_installed`,
`::should_leave_the_plugins_startup_marker_absent_after_a_failed_confinement_setup`,
`::should_name_the_control_that_could_not_be_installed_in_the_structured_error`,
`::should_not_exec_the_plugin_when_a_mandatory_confinement_control_fails`,
`crates/ono-kuang-sdk/tests/failure_classes.rs::should_distinguish_a_launch_failure_from_a_quarantine_a_resource_kill_and_a_crash`,
case `189-kuang-confinement-fail-closed`.

## Alternatives considered

**Encode the control in the `errno` the child returns.** The only channel `std` gives without new
machinery. There is no spare errno space, and a made-up one would surface to the user as whatever
`strerror` says about it.

**Report `load.runtime_unavailable` and put the control in the message.** What the code did before
the K-family existed, and what the shell did until this increment. It fails §53.2 outright: a
script that wants to know whether a load failed for confinement reasons would have to match a
sentence.

**Let the parent re-derive which control failed by trying them itself.** Racy, and wrong in
principle: the parent's limits are not the child's, and §2.6 forbids reporting a guess as an
observation.

**Quarantine the package on a launch failure, so it is not retried in a loop.** §18.1 forbids it
in as many words, and it would be the wrong answer anyway: a hard `ulimit` the operator set is a
fact about the machine, not about the package, and the package must load normally once it is
lifted.
