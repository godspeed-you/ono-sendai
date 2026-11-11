# ADR-0995: A job runs packages in the session's instances, and the session unloads them

- Status: superseded by ADR-1023 (in part: that a just-in-time request a job makes is answered by the consent source of the evaluator that loaded the instance)
- Date: 2026-11-10
- Spec refs: spec §18.4, §31.8, §31.10, §31.12, §31.33, §31.37, §31.38; v0.4.1 §17; ADR-0090,
  ADR-0107, ADR-0567, ADR-0952, ADR-0958, ADR-0959; issue #304
- Decided by: agent (autonomous)

## Context

A background job has an evaluator of its own, built from a `JobSnapshot` (ADR-0952). The snapshot
shared the session's providers — the `ono.shell` provider among them, which answers `get job`,
`get plugin` and `get audit` from the session's tables — but built the job's own `SessionTables`
fresh, and those hold the KUANG/11 host (ADR-0107). So a job asked one host what was loaded and
ran packages in another:

- a package command in a job found no instance in the job's empty host and loaded a **second
  instance**, under the job's own audit trail;
- `get plugin` — the session's, even inside the job — never listed it;
- when the job's session dropped, the instance was torn down implicitly (`kill_on_drop` for a
  process, an abort for a component): no `lifecycle.shutdown`, no unload in the trail;
- a namespaced spelling (`echo:emit`) in a job was refused outright when the session had loaded
  the package, because the job's host had not.

The session itself never recorded an unload either: an instance it held at exit went the same
implicit way, and an instance taken out by `unload plugin` or replaced by a reload left no end in
the trail and dropped what it had recorded since the last flush.

Spec §31.8 makes the instance states visible through `get plugin`, and §31.37 makes the trail the
answer to "what did that package do". Both assume the host knows every instance it runs.

## Decision

**One host per session knows every instance, and a job uses it.** There is no second lifecycle
model: a job neither owns instances nor has a host of its own.

1. **A job shares the session's tables.** `fork_for_job` carries the session's `SessionTables`
   into the job, as it already carried the providers that answer from them. A package command in
   a job resolves, lazily loads and invokes through the session's host; an instance a job loads
   is the session's instance, listed by `get plugin`, its load and its trail in the session's
   audit; an instance the session loaded is the one the job runs.
2. **A job publishes nothing into them.** The job table, the link table, the context a package
   reads (`context.get`, ADR-0567) and the host's configuration are the session's, published before
   each of its pipelines. A job's pipeline only appends what was recorded to the trail on disk.
   What `get job` answers inside a job is therefore what it already answered — the session's table,
   through the shared provider — and a package's `context.get` answers where the session stands.
3. **The host records an instance's end where it lets the instance go.** Taking an instance out of
   the host — `unload plugin`, `disable plugin`, `remove plugin`, a reload that replaces it, the
   session's end — keeps what the instance recorded and records `lifecycle.unload` (capability
   `plugin.unload`, invocation `host`) beside the `lifecycle.load` that started it. One instance
   that ran is one load and one unload in the trail.
4. **The session unloads its instances when it ends, after its jobs.** `Session::drop` first ends
   the jobs (ADR-0959's escalation, one bounded grace period), then sends every instance
   `lifecycle.shutdown` with reason `host-exit` — all at once, within one more grace period
   (`shutdown::GRACE`, 2 s) — and then writes the trail. An invocation a job still had running in
   the instance ends with the shutdown, so a job blocked on a package cannot hold the shell. A job's
   session never unloads anything. A shell ended by a signal skips this, as it skips the trail's
   last write (ADR-0959).

Concurrent invocations of one package from the foreground and from a job go to the one instance,
whose supervisor already serves concurrent invocations up to the negotiated
`max_concurrent_invocations` and keeps each invocation's stream its own.

## Consequences

Easy: `get plugin` and `get audit` account for every instance a session ran, whichever evaluator
ran it; a package keeps one state per session; a backgrounded package command no longer doubles
its process, its memory and its state; the namespaced spelling works in a job.

Hard:

- A shell with loaded packages takes up to one more grace period to leave when an instance does
  not acknowledge its shutdown; leaving is bounded by ADR-0959's period plus this one.
- A package's `context.get` inside a job answers the session's context — the directory the shell
  stands in, not the one the job was started in (ADR-0957). One instance has one context source;
  giving each invocation the context of the evaluator that runs it is a supervisor change.
- `get plugin`'s `jobs` column is still a constant zero (`ono.plugin/1` claims the host always
  knows it), and now a job can hold an invocation open while the shell reads the table: the
  instance shows `active` with `jobs: 0`. Counting invocations needs an accessor the supervisor does
  not have; recorded as a finding of this run.
- A just-in-time capability request an instance makes while a job runs it is answered by the
  consent source of the evaluator that loaded the instance, as before.

Encoded by `crates/ono-cli/tests/plugin_instances.rs`:
`::should_list_the_instance_a_background_job_loaded_in_get_plugin`,
`::should_record_one_load_and_one_unload_when_a_job_and_the_shell_use_one_package`,
`::should_unload_the_instance_in_order_when_the_shell_leaves_with_a_job_still_running_it`;
acceptance case `155-a-jobs-package-runs-in-the-sessions-instance`.

## Alternatives considered

**The job owns its instances, listed, audited and unloaded in order.** A second lifecycle model
beside the session's: two instances of one package with two states, `get plugin` rows that are
per-evaluator, a reload in the shell that does not reach the job's copy, and a load (and a
process, and its memory) per backgrounded line. The issue allows it; the simpler model is one host.

**Share only the host, keeping the job's own tables for jobs, links and context.** It would need
the host behind a lock of its own inside the tables, changing every caller that reaches the host
through the tables' lock, to keep apart three tables nobody reads in a job: the shared `ono.shell`
provider already answers the job's `get job` from the session's tables.

**Unload the instances when the host is dropped.** A drop cannot wait on the runtime that serves
the shutdown, and the order matters: the jobs end first, then the instances they were using.
