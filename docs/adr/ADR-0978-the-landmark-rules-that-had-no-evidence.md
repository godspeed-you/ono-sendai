# ADR-0978: The landmark rules that had no evidence

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §2.16, §2.17, §3.7, §25.1, §26.2, §26.3, §35.3; ADR-0128, ADR-0163, ADR-0964; issue #209
- Decided by: agent (autonomous)

## Context

ADR-0163 left out every rule of §26.2 whose evidence no provider served, "rather than
approximated". Two of them had a reason in §3.7's vocabulary, a threshold in
`docs/contracts/spatial/landmarks.yaml` and a setting, and could never fire (#209):

- `high_memory` — "high memory relative to host/cgroup budget". A process record states its
  resident memory; nothing stated a budget, and §2.16 keeps the spatial layer from reading
  `/proc/meminfo` itself.
- `restarting` — "restart loop". `ono.service/1` carried no restart count.

Four network rules of §26.2 — interface down, route change, unusually high traffic, new remote
peer — have no reason of their own in §3.7, and acceptance case 105 holds the built-in
landmarks to that list.

## Decision

**The host's memory is a fact of the kernel record.** `ono.kernel/1` gains `memory_total`,
`MemTotal` of `/proc/meminfo` in bytes, null where `/proc` publishes none. The session asks the
`kernel` target once per host when it first observes processes there, and the rule computes
`memory / memory_total`. At or above `spatial.landmarks.high_memory` (25 percent) the object is
a `high_memory` landmark whose evidence names the bytes, the share and the budget. A host whose
provider states no budget gets no such landmark.

**The restart count is a fact of the service record.** `ono.service/1` gains `restarts`,
systemd's `NRestarts`, null where the provider keeps no count and for a unit that is not a
service. A unit with at least `spatial.landmarks.restart_loop` (3) restarts **whose state
changed inside the change window** is `restarting`, in place of `recently_changed`: the count
alone is history — systemd keeps it for as long as the unit is loaded — and the recent change
is what says the unit is still at it. The evidence names both.

**Three network rules are changes of state and carry the reasons §3.7 has for a change**, as
`landmarks.yaml` already reads the storage transitions: an interface whose state moved is
`recently_changed`, a route or a remote peer that was not there is `new_object`, one that has
gone is `removed_object`. They come from the change engine of a live or repeated view, which
is where a change can be observed at all.

**"Unusually high traffic" has no rule.** An interface record carries byte counters since the
last reset; a rate needs two readings and a notion of usual, and no provider states either.

## Consequences

Easy: the two thresholds that were inspectable and inert now decide something; `get kernel`
answers how much memory the host has; `get service | where restarts > 0` is a pipeline.

Hard: the budget is the host's. A process confined by a cgroup limit far below the host's
memory is measured against the host; the cgroup budget — `memory.max` of the process's control
group — is a field `ono.cgroup/1` does not carry yet. `restarts` counts since the unit was
loaded, so a unit restarted often last week that changes state once today reads as a loop for
one change window.

The session's first observation of processes on a host costs one read of the kernel record.

Encoded by `crates/ono-provider-linux/tests/kernel.rs::should_state_the_memory_the_kernel_manages_and_leave_it_unknown_where_it_is_not_published`,
`crates/ono-provider-systemd/tests/service.rs::should_report_how_often_systemd_restarted_a_service_and_null_where_it_keeps_no_count`,
`crates/ono-spatial-query/tests/landmarks.rs::should_promote_a_process_holding_a_large_share_of_the_hosts_memory`,
`::should_not_call_memory_high_when_nobody_stated_a_budget_to_measure_it_against`,
`::should_call_a_service_restarting_when_it_was_restarted_often_and_changed_state_just_now`,
`crates/ono-spatial-events/tests/snapshot_comparison.rs::should_report_the_network_changes_of_the_landmark_rules_with_the_reasons_the_vocabulary_has`,
`crates/ono-cli/tests/spatial_relationships.rs::should_measure_a_process_against_the_memory_the_host_states`
and acceptance case `138-high-memory-landmark`.

## Alternatives considered

**A `memory` target of its own.** v0.2 §8.1 lists none, and one figure does not make a
collection; the kernel record is where v0.4 §7.1's other host facts already come from.

**A `memory_share` field on the process record.** The provider would divide by a total it read
itself, per process, and the rule's threshold would compare a number whose budget nobody can
inspect.

**New reasons for the network rules** (`interface_down`, `route_changed`, …). §3.7 says the
built-in reasons "MUST include" its fourteen, which admits more; ADR-0163 and case 105 treat
the list as what a core landmark may carry, and the three rules are changes the vocabulary
already names.

**Call an interface that is down `failed`.** A bridge with no port is down by design, and
`failed` asks for attention (§26.3).
