# ADR-0446: Quarantine is a judgement about conduct, so a plugin that never ran cannot earn one

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §18.1, §18.2, §18.3, §18.4, §53.2, §53.3; base spec §31.8, §31.34;
  AGENTS.md §11; ADR-0041, ADR-0442, ADR-0444
- Decided by: agent (autonomous)

## Context

v0.4.1 §18 splits one word — "the plugin failed" — into four outcomes, and the shell had a single
path for three of them and no name for the fourth:

- **§18.1 pre-exec failure.** "A plugin whose required confinement cannot be installed MUST not
  enter quarantine, because it never safely started. It receives a launch failure."
- **§18.2 protocol violation.** Malformed, oversized or credit-violating frames MAY quarantine.
- **§18.3 resource-limit termination.** "Ono MUST classify the exit distinctly from a protocol
  crash where the platform permits determination. The error SHOULD identify the enforced resource
  class, not merely 'plugin exited'."
- **§18.4 crash containment.** Plugin failure "MUST not corrupt the shell's provider registry or
  leave partially registered capabilities visible as healthy."

Most of this was already true. `Actor::quarantine` and `Actor::fail_instance` were two paths
before v0.4.1 (ADR-0041), and `Actor::death` already told a `SIGXCPU` from a `SIGXFSZ` from a
memory ceiling. What was missing was one machine-readable field, one fixture, and the *rule* that
says why §18.1 is not a fifth reason to quarantine.

## Decision

**The four outcomes are distinguished by the code a caller receives, and quarantine is reserved
for conduct.**

**§18.1 is a launch failure, and its family is `plugin.*` (ADR-0444).** Nothing is quarantined
because no `LoadedPlugin` exists to quarantine: `Supervisor::load` returns `Err` before the
handshake. The consequence that matters — and the one the test asserts — is that the *next* load
of the same package succeeds. Quarantine is a standing judgement about how a package behaved, and
a package that never executed an instruction has not behaved.

**§18.3 carries `resource_class` in the error metadata**: `memory`, `cpu` or `file_size`. §53.2
forbids string matching for policy, and "identify the enforced resource class" is exactly the kind
of thing a script branches on. A crash carries no `resource_class`, which is what makes the two
tellable apart without reading a sentence.

**§18.4 needed a fixture, because three of the four already had one.** The example plugin gains
`--misbehave=die`: it completes the handshake honestly and then exits mid-invocation, breaking no
protocol rule at all. That is the outcome §18.4 is about — a package that does nothing wrong on
the wire and simply stops being there — and without it the crash path was only ever exercised as a
side effect of some other failure.

**The suite is one test per §18 clause, driven through the deterministic test host** against the
real example plugin binary, so what is asserted is the outcome a shell sees rather than which
internal path ran (AGENTS.md §11). It lives in `crates/ono-kuang-sdk/tests/failure_classes.rs`
rather than in the supervisor's own `tests/`, because three of the four cases need a process that
speaks KUANG/11 and `CARGO_BIN_EXE_kuang-example-plugin` exists only for the crate that defines
that binary.

## Consequences

Easy: `runtime.memory_limit` with `resource_class: memory` is a limit; `runtime.trap` with no
`resource_class` is a defect; `runtime.protocol_violation` with `state == quarantined` is
misconduct; `plugin.*_failed` with no instance at all is a machine that will not let the package
start. Four answers, four codes, no prose.

Hard: §18.3's classification is still evidence rather than certainty, and deliberately so.
`RLIMIT_DATA` makes an allocation *fail* rather than raising a signal, so the host reports
`runtime.memory_limit` when it observed the instance at its ceiling and otherwise names the signal
with the ceiling and the high-water mark beside it. A package that aborts for its own reasons
while near its ceiling will be reported as having reached it. The alternative is to report nothing,
which §54.1 rules out.

Also: the acceptance case asserts `{"state":"installed"}` after a refused launch, which is the
state a package that has never been loaded is in. That is the observable form of §18.1, and it
would fail loudly if a future increment decided to quarantine on launch failure.

Encoded by: `crates/ono-kuang-sdk/tests/failure_classes.rs::should_distinguish_a_launch_failure_from_a_quarantine_a_resource_kill_and_a_crash`,
`::should_keep_the_shell_and_the_other_plugins_running_when_one_plugin_crashes`,
case `189-kuang-confinement-fail-closed`.

## Alternatives considered

**Quarantine on a launch failure, to stop a retry loop.** Forbidden by §18.1 in as many words, and
wrong on the merits: a hard `ulimit` an operator set is a fact about the machine, and the package
must load normally the moment it is lifted.

**A fifth `PluginState` for "launched but not confined".** There is no such state to be in: the
process does not exist. Spec §31.8's six states are about a package's standing, and a failed
launch does not change it.

**Derive the resource class in the CLI from the error code.** `runtime.memory_limit` implies
`memory` today, and would stop implying it the moment a second memory-shaped code existed. The
supervisor observed the signal and the ceiling; it is the one that knows.

**Add a `crash` mode to the SDK rather than to the example plugin's `--misbehave` family.** The
misbehave family already bypasses the SDK on purpose and speaks the wire directly, which is what
this fixture needs: an honest handshake followed by a process that is simply gone.
