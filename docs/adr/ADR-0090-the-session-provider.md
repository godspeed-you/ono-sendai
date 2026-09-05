# ADR-0090: The session provider — `get job` answers from tables the shell publishes

- Status: accepted
- Date: 2026-08-27
- Spec refs: §18.1, §18.4, §21, §27.1, §31.23; ADR-0024, ADR-0068, ADR-0071 §4
- Decided by: agent (autonomous)

## Context

Spec §18.4 says "`get job` returns structured job objects", and `docs/contracts/commands/process.yaml`
declares `ono.job.get` as a stable producer over the `job` target with the `job.list`
capability. Every producer in this shell is `ProviderProducer` over a registered provider
(ADR-0012, ADR-0021), and no provider could claim `job`: the job table is not a system
interface. Half of it is the executor's process groups (`ono_process::Executor::jobs`, spec
§18.1) and half is the session's detached native pipelines (`Session::native_jobs`, ADR-0024),
both owned by `ono-cli`, neither visible from a provider crate. `get job` therefore answered
`E0102 no provider answers job`.

The remote family faces the same shape: the links a session holds (`SessionLink`) and the hosts
they reach are session state too, and `remote_missing.rs` asks for them as data.

## Decision

### 1. The session publishes; a provider answers

`crates/ono-cli/src/session_provider.rs` holds `SessionTables` — plain rows, one type per
table — behind an `Arc<Mutex<_>>` the session owns. `SessionProvider` (`id: ono.shell`) is an
ordinary `ono_provider_api::Provider` over that handle: it is registered in the session's
provider set like every other provider (`providers::registry_with_tables`), declared in
`docs/contracts/providers/linux-procfs.yaml`, and checked by the conformance test with the rest. It
serves `job` now with `job.list` and `ono.job/1`.

The session refreshes the tables in `Session::pipeline_context()`, the one call every native
pipeline goes through, after reaping the executor's jobs. What `get job` answers is therefore
what was true when the pipeline started — the same instant `jobs` reports from — and the
provider never reaches back into the session.

### 2. What a job row says

- `kind`: `external` for an executor job (a process group), `native` for a detached pipeline.
- `state`: `running`, `stopped`; `done` once an external job exited or a native task finished;
  `failed` when a stage of an external job could not be started at all. `cancelled` is reserved
  for a native job stopped by `kill %N`; today that removes the job from the table (ADR-0071 §4),
  so no row shows it.
- `exit_status`: null while running; the code once done. An external job ended by a signal
  reports null (job.v1: never a fabricated `128 + n`). A finished native job reports 0, or 1
  when its stream carried failures.
- `process_group`/`pids`: the executor's, with never-started stages (pid 0) omitted; null for a
  native job.
- `started`: the instant the job was detached. The executor's table does not record it, so the
  session stamps it — at `&` for an external pipeline (`eval.rs`), at detachment for a native
  one (`NativeJob::started`); a job that reached the table by being stopped is stamped at its
  first publication.
- `current`: the job an unqualified reference means — the highest number in the table, which is
  what `fg` without an argument resolves to.

### 3. The seam is meant to grow

`link` and `host` join by adding a row type to `SessionTables`, a publish step where the session
changes that state, and the target, capability and schema to `SessionProvider`. Nothing in the
command crate changes, and the remote family owns those additions.

## Consequences

- `get job`, `get job N`, `get job | where state == running` work as the contract examples show;
  `crates/ono-cli/tests/processes_missing.rs` (`should_list_a_backgrounded_external_pipeline_as_a_job`,
  `should_list_a_detached_live_view_as_a_native_job`, `should_resolve_one_job_by_its_number`,
  `should_report_a_finished_job_as_done_with_its_exit_status`) prove it.
- The conformance test's `built_registry()` builds the provider over empty tables: the
  declaration is checked without a session.
- A provider crate could not own this; `ono.shell` is the one provider that lives in `ono-cli`.

## Alternatives considered

- **A builtin `get job` in `builtin.rs`.** It would bypass the registry, the type checker,
  `explain` and every transform's contract; `jobs` already exists for the terminal form.
  Rejected.
- **Give the provider a handle to the session.** A provider is `Send + Sync` and outlives the
  borrow a pipeline holds on the session; a back-reference would need the session behind a lock
  and would make the provider answer from state a running pipeline is mutating. Rejected.
- **Read `/proc` for the job's start time.** True for the group leader while it lives, gone
  once it exited — exactly when `done` rows need it. Rejected.
