# ADR-0869: A profile the whole suite holds no case of fails

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §38 (a missing check is not a pass); ADR-0851, ADR-0913
- Issues: #127
- Decided by: agent (autonomous)

## Context

ADR-0913 gave `scripts/acceptance.sh` a `--profile core`, and the script's own rule, in the
comment above it, was that "a selection with nothing left in this profile is not a failure":
`--group` asks for a range of numbers, and a range may hold only cases of the other profile. An independent review found the rule applied to the
unnarrowed selection too. `scripts/acceptance.sh --profile core`, the command CI runs for #127's
core claim, printed "none of the N selected cases runs in the core profile" and exited 0 when no
case declared `profile: core` — the claim would pass with zero cases, which is the fail-open shape
§38 forbids for skipped tests.

## Decision

A selection is narrowed when it names a case (a name fragment) or a group. A narrowed selection
with no case in the profile still says so and exits 0, as before, because CI's group
jobs ask for ranges that may hold only the other profile's cases. **An unnarrowed selection — the
whole suite — with no case in the profile fails**, naming the profile: a profile nothing tests
proves nothing about its build.

## Consequences

- Deleting or re-profiling the last core case turns CI's core job red instead of green.
- `--build-only` is unaffected: it builds and runs nothing.
- Test: `xtask/tests/harness.rs` —
  `should_fail_a_profile_that_holds_no_case_unless_the_selection_narrowed_it`, against the
  stand-in runtime.

## Alternatives considered

- **Fail every empty selection.** A group or name selection may legitimately hold only the
  other profile's cases, and would fail for that.
- **Require a minimum count of core cases.** A number chosen today; one case is already the
  difference between a claim and no claim.
