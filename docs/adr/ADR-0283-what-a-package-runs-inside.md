# ADR-0283: What a package runs inside

- Status: accepted
- Date: 2026-08-29
- Spec refs: §31.10, §31.15, §31.16, §31.33, §31.34, §31.67, §31.80
- Decided by: agent (autonomous, C4-kuang)

## Context

Spec §31.10 puts a native out-of-process host at tier T1 — "a separate process, native executable
protocol" — and spec §31.15 requires "per-plugin memory ceilings", "CPU/fuel/budget controls
appropriate to runtime" and bounded resources generally.

The supervisor spawned a bare `Command` with piped stdio. A separate process is where isolation
*starts*: by default the child inherited the shell's whole environment, the user's current
working directory, the terminal's process group and every ceiling the login shell was given. The
manifest's `memory_max` and `cpu_budget` were parsed, carried into the negotiated contract, shown
by `inspect plugin` — and enforced by nothing (C-4(a)). Spec §31.33's health block
(`memory/current/limit`, `cpu time`) was a column of nulls, and spec §31.34's "resource limit"
failure class could not be reached because no limit existed to reach.

## Decision

### 1. The confinement, applied between fork and exec

`crates/ono-kuang-supervisor/src/sandbox.rs` builds a `Sandbox` from the negotiated contract and
applies it to the `Command` before the artifact runs, so no package instruction ever executes
outside it:

| What | How | Why this and not something else |
|---|---|---|
| memory | `RLIMIT_DATA` at the negotiated `memory_max` | `RLIMIT_DATA` bounds the brk and private anonymous mappings — the memory a package *allocates*. `RLIMIT_AS` bounds reserved address space, most of which in any threaded program is stack guard pages nobody touches; it would refuse a package that allocated nothing. |
| scheduling | `setpriority` from `cpu_budget` | §31.15 calls `cpu_budget` a scheduling *class*, and §31.67 requires that a plugin never block terminal input. That is a priority statement, not a CPU-seconds quota, so `interactive` keeps the default priority and `batch`/`background` step down. |
| descriptors | `RLIMIT_NOFILE` | A descriptor leak in a package stays the package's problem. |
| file size | `RLIMIT_FSIZE` | A package cannot fill a disk unnoticed. |
| core dumps | `RLIMIT_CORE` at zero | A dump writes a package's address space to a file nobody asked for (§31.20). |
| session | `setsid` | The package cannot signal the shell's process group, and a Ctrl-C meant for the pipeline does not reach it (§31.34). |
| privileges | `PR_SET_NO_NEW_PRIVS` | A setuid binary the package execs gains it nothing (§31.80). |
| working directory | the package's own `work/` under `~/.local/state/ono/kuang/<id>/` | §31.31's location for what is a package's own. Anything is better than the user's current directory, which is what an unconfigured child inherits. |
| environment | cleared, then `PATH`, `HOME`, `LC_ALL=C`, `TZ=UTC` | The shell's variables carry tokens, paths and session identity (§31.80). `LC_ALL` and `TZ` are fixed rather than inherited because §31.69 wants a plugin's behaviour reproducible. |

**`RLIMIT_NPROC` is deliberately not set.** It counts every process the *real user* owns, so
setting it for one package refuses a fork because of processes that package never created, on a
machine where the user is merely busy. Bounding what one package spawns needs a cgroup
`pids.max`.

### 2. `unsafe` for `pre_exec`, and nowhere else

Resource limits can only be set in the child between `fork` and `execve`, and the standard
library offers exactly one way to run code there. `sandbox.rs` therefore carries two
`#[allow(unsafe_code)]` sites — `pre_exec` and the `setrlimit`/`sysconf` wrappers — each with a
`// SAFETY:` note. Every call inside the closure is a direct syscall wrapper that is
async-signal-safe (`setrlimit`, `setpriority`, `setsid`, `prctl`), and every value it reads was
computed before the fork, so the closure neither allocates nor takes a lock.

### 3. The host measures, and says what it measured

The actor samples `/proc/<pid>/status` `VmData` — the figure `RLIMIT_DATA` bounds — and
`/proc/<pid>/stat`'s user+system time, on a 100 ms tick and on every frame the instance sends
(free: the actor is awake then anyway). That fills spec §31.33's `memory_current`, `memory_limit`
and `cpu_time`, which were nulls, and gives §31.34 something to reason from. An unsampled figure
stays null; spec §35.3 forbids reporting an unknown as a zero.

### 4. How a death is classified, and where the classification stops

`SIGXCPU` and `SIGXFSZ` name their limit exactly and become `runtime.timeout` and
`runtime.trap` with the limit in the message.

Memory has no signal of its own: `RLIMIT_DATA` makes an over-large allocation *fail*, and what
the package does then is the package's business — a Rust artifact aborts, a C one may carry on.
So the host reasons from what it observed. The kernel refuses the allocation that would cross the
ceiling, so an exhausted instance's last observation sits *just below* it and never at or above
it; the host reads "at its ceiling" as **within a sixteenth of it**, and reports
`runtime.memory_limit`. Measured on the fixture that allocates until it cannot: 66 568 192 bytes
observed against a 67 108 864 byte ceiling — 0.8 % short.

This is an inference from an observation and is stated as one rather than hidden: **every**
abnormal-death message carries the ceiling that was in force and the figure the host observed, so
an operator sees what the host saw instead of being told a story about it.

### 5. What the sandbox does not claim

`Sandbox::filesystem` and `Sandbox::network` are `Confinement::Broker`, and the
`ono.plugin-runtime/1` record prints the word. A `native-process` package can still open any file
its user can; the broker refuses a host call outside a granted scope, and a package that opens a
file itself is not asking the host at all. Spec §31.16: "A scope that cannot be enforced reliably
MUST NOT be offered as if it were a security boundary." Saying `broker` where it is the broker is
the whole point of the field, and `Confinement::Kernel` exists for the day a kernel sandbox is
applied.

## Consequences

- `memory_max` and `cpu_budget` now mean something; a package that exceeds its declared memory
  ends, and the shell answers the next stage (spec §31.34).
- `inspect plugin`'s health block reports measurements instead of nulls.
- `ono.plugin-runtime/1` gains a required `sandbox` record: what was applied, and how far it
  reaches.
- `HostLimits` gains `memory_max` (512 MiB by default) and `negotiate` takes the smaller of it
  and the manifest's, so the manifest's "Host policy caps it" is true.
- Encoded by `ono-kuang-supervisor/src/sandbox.rs`'s unit tests,
  `supervisor.rs::should_read_an_instance_just_under_its_ceiling_as_having_reached_it`,
  `ono-cli/tests/plugins.rs::should_end_the_instance_and_not_the_shell_when_a_package_exceeds_its_memory_ceiling`,
  `::should_start_a_package_with_an_environment_it_did_not_inherit`,
  `::should_run_a_package_in_a_private_directory_rather_than_the_users`,
  `::should_report_what_a_running_instance_has_allocated_and_used`
  and acceptance case `127-kuang-runtime-isolation`.

## What this does not close, and what would

- **A filesystem or network scope the kernel enforces.** Landlock (ABI 1 for paths, ABI 4 for TCP
  ports) would turn `Confinement::Broker` into `Confinement::Kernel` for a `filesystem.read` or
  `network.connect` scope. It needs a new dependency and a kernel probe, and — because §31.16
  forbids presenting an unenforced scope as a boundary — it must report which level it actually
  reached rather than assuming one. Left undelivered rather than half-built.
- **An exact `runtime.memory_limit` for every allocation-denied death.** The kernel would have to
  report the refusal, which on Linux means a cgroup v2 `memory.events` `oom_kill` counter, which
  needs a delegated cgroup the shell does not have as an unprivileged user. Until then the
  inference of §4 above is the honest answer, and it names its own basis.
- **A bound on the processes one package spawns.** Same reason: cgroup `pids.max`.

## Alternatives considered

- **`RLIMIT_AS` for the memory ceiling.** Rejected: it bounds reserved address space, so a
  threaded artifact that allocates nothing can still be refused, and the number the operator
  declared would not be the number the package could use.
- **`RLIMIT_CPU` for `cpu_budget`.** Rejected: `cpu_budget` names a scheduling class
  (`interactive`, `batch`, `background`), not a number of seconds. Turning a class into a
  seconds quota would invent a limit the manifest never declared.
- **A wrapper binary that sets the limits and execs the artifact.** Rejected: it puts a second
  program on the trusted path for no gain over `pre_exec`, and it would have to be installed and
  found before any package could load.
- **Namespaces (`unshare`) for the filesystem.** Rejected here: unprivileged user namespaces are
  available on some hosts and disabled on others, so the boundary would exist on one machine and
  not the next — exactly the situation §31.16 forbids presenting as a boundary. A probe-and-report
  design belongs with the Landlock work above, not bolted on silently.
