# ADR-0863: The binary is measured, budgeted, and reduced in three separate steps

- Status: accepted
- Date: 2026-09-04
- Spec refs: v0.2 §31, §31.36, §34, §37; v0.4.1 §44.2, §50.1, Appendix H; ADR-0010, ADR-0121,
  ADR-0450
- Issues: #124, #125, #126, #127 (milestone *Binary size — measured, budgeted, reduced*)
- Decided by: user (direction) and agent (measurements, profile choice)
- Origin: written on branch `binary-size` as ADR-0578 and renumbered on integration, because
  this tree had already given ADR-0578 to an unrelated record.

## Context

The v0.4.0 packages of 2026-09-01 ship a 20.0 MB `ono`. Two days later the KUANG/11 component
tier brought `wasmtime` and its compiler into the shell (fcd8ce7), and the stripped release binary
at 75beb43 is **35.4 MB**. Nothing said so: the gate counts crates, tests, cases, ADRs and
generated documents (§50.1), the release manifest names every tool version (§44.2, ADR-0450), and
neither has a column for the size of the artifact they exist to vouch for.

The user asked whether the binary can be made substantially smaller, and then how small the object
shell alone could be. This record holds the measurements, because each one costs a full release
build and the next person to ask should start from numbers rather than from guesses.

### What the 35.4 MB are

`cargo bloat --release -p ono-cli --crates` on the unstripped build (26.1 MiB of `.text`):

| share of `.text` | what |
|---|---|
| ~9 MiB | `wasmtime`, `cranelift_codegen`, `cranelift_assembler_x64`, `regalloc2`, `wasmparser`, `pulley_interpreter`, `wasmtime_environ` |
| 4.7 MiB | `std`, largely monomorphisations, a good part of them from the wasm stack |
| 1.9 MiB | `ono_cli` itself, including the evaluator |
| 1.5 MiB | `zbus` and `zvariant` (systemd over D-Bus) |
| 1.1 MiB | `tokio` |
| 0.5–0.7 MiB each | `rustls` with `ring`; `regex` with its Unicode tables; `serde_yaml_ng` |

Beside the code: 2.4 MB of `.eh_frame`, 1.2 MB of `.gcc_except_table`, 1.6 MB of `.rodata`.

### What the profile alone does

Same tree, same toolchain (1.94), `codegen-units = 1`, stripped. Every variant answered
`--version` and ran `-c 'echo hello'`:

| release profile | size |
|---|---|
| `opt-level = 3`, `lto = "thin"` (the profile today) | 35.4 MB |
| `lto = "fat"` | 32.5 MB |
| `panic = "abort"` | 30.8 MB |
| `opt-level = "s"` | 26.7 MB |
| `opt-level = "s"` + `lto = "fat"` | 22.0 MB |
| `opt-level = "s"` + `lto = "fat"` + `panic = "abort"` | 19.2 MB |
| `opt-level = "z"` + `lto = "fat"` + `panic = "abort"` | 16.5 MB |

Fat LTO doubles the link time of a full release build here (398 s against 207 s).

### What the compiler costs

Built with wasmtime's `runtime`, `component-model`, `async` and `std` features only, loading
through `Component::deserialize_file` instead of `Component::from_file`:

| release profile | with cranelift | without |
|---|---|---|
| today's | 35.4 MB | 24.7 MB |
| `opt-level = "s"` + `lto = "fat"` | 22.0 MB | 15.4 MB |
| `opt-level = "z"` + `lto = "fat"` + `panic = "abort"` | 16.5 MB | 11.9 MB |

### What the object shell alone costs

The repository's history holds the shell without its enhancements, so the floor is measured
rather than estimated. Both trees built with the toolchain of today and the profiles above, and
both ran pipelines through external programs; the second answered `get process`, `get interface`
and `get service`:

| tree | today's profile | `s` + fat | `z` + fat + abort |
|---|---|---|---|
| 47d3240 — language, evaluator, pipeline, editor, job control, no providers | 3.0 MB | 2.3 MB | 2.2 MB |
| 9bb2ee7 — the object pipeline with the Linux, netlink and systemd providers | 8.2 MB | 5.5 MB | 4.6 MB |
| 75beb43 — everything | 35.4 MB | 22.0 MB | 16.5 MB |

The 9bb2ee7 binary links `libc`, `libm` and `libgcc_s` and nothing else, and runs `get process`
in 7.7 MB of resident memory. For scale: `bash` on the measuring machine is 1.5 MB, `busybox`
2.2 MB, and nushell, the nearest object shell, ships at roughly 40 MB.

Two things the measurements rule out as cheap wins. Dropping only the systemd and container
registrations from `providers.rs` saves 0.3 MB, because the storage provider in
`ono-provider-linux` uses the systemd bus for mount units and keeps `zbus` alive. And trimming
`regex`'s Unicode features or wasmtime's optional features is worth well under half a megabyte
each.

## Decision

Four increments, each its own issue and its own commit (AGENTS.md §4: an optimisation, a
harness change and two features are four changes). This record decides the first three and
records the fourth as the user's direction.

### 1. The release profile takes `opt-level = "s"` and `lto = "fat"` (#124)

22.0 MB, no change in behaviour, and the only cost is link time on the release path. Two of the
measured knobs are refused:

- `panic = "abort"` is refused for a login shell. A panic in one tokio task would end the
  session instead of the task. Nothing in the workspace calls `catch_unwind` today, so the refusal
  costs nothing now; it keeps the choice open for the day something does.
- `opt-level = "z"` is refused. The last twenty percent are paid in throughput that §34 budgets,
  and 22 MB against 16.5 MB changes nothing for any target this shell is packaged for.

The profile change is accepted only if `cargo run -p xtask -- perf --profile S` holds every §34
target it held before, since ADR-0010's startup budget was won at `opt-level = 3`.

### 2. The stripped size is a metric with a budget (#125)

`metrics` records the stripped size of the release binary per target triple, `build-manifest`
carries it into the release input manifest of Appendix H, and a budget beside the other limits in
`docs/spec/hardening/limits.yaml` fails the gate when the release binary exceeds it. The first
budget is what #124 lands at plus ten percent, so the next 15 MB arrive as a failing gate on the
commit that adds them, and the ADR that justifies them raises the budget.

### 3. The KUANG/11 compiler runs once, outside the shell (#126)

§31.36 stays untouched: a package ships `runtime/component.wasm`, and a compiler runs on the
installing machine. It runs once, in a tool beside `kuang-sign`, when the package is installed or
trusted, and writes the engine's own `.cwasm` beside the component. The shell links the runtime
only and refuses a package whose compiled artifact is missing or was written by a different
engine version, naming the tool to run. The Docker image compiles the example plugin where the
Dockerfile already builds it.

`Component::deserialize_file` is an `unsafe` API in wasmtime, so the closing commit carries the
workspace's first `#[allow(unsafe_code)]` with its reason in the ADR of that commit: the artifact
is trusted because the shell's own tool wrote it into a directory only the installing user writes,
and the engine checks its version and target before it maps anything.

### 4. A core build is the user's direction, and its floor is known (#127)

The user wants a build of the object shell alone for container images and embedded Linux
targets: language, evaluator, typed pipelines, native commands, the Linux and network providers;
statically linked; without the systemd, container, remote, spatial, graph, adapter and KUANG/11
tiers. The measured floor is 4.6 to 8.2 MB on the August tree, and the target for today's crates
is 5 to 6 MB at the profile of §1, without `opt-level = "z"` or `panic = "abort"`. It is the
largest of the four increments and the least urgent, and it is recorded in `docs/STATE.md` under
*Product direction from the user*.

## Consequences

Easy: the packaged binary drops from 35 MB to 22 MB on the first increment and to about 15 MB on
the third, both measured before a line of the work is written, and the fourth has a floor a
reader can check against the table above.

Hard: the size becomes a gate failure, which is the point. A dependency that adds five megabytes
now needs the ADR that a new cryptographic dependency already needs (§45.4), and a contributor
who reaches for a crate will meet the budget before the reviewer does.

Also hard: after §3 a KUANG/11 package is two files on disk where it was one, and a shell upgrade
that changes the engine version invalidates every compiled component on the machine. The refusal
names the tool, and the tool is idempotent, so the cost is one command per package per engine
version; the ADR of #126 has to say where the artifacts live and who may write there.

The measurements themselves are in this record and in the issues, and nowhere else: a number in
a README rots, and §2 turns the number into a check so it cannot.

## Alternatives considered

**Compress the binary (UPX).** Roughly halves any of the variants above and costs an unpack on
every start of a login shell, on every `ono -c` a script runs, and every process image is then
private memory instead of a shared file mapping. ADR-0010's startup budget is the wrong thing to
spend on this, and distributions strip it out anyway.

**Build the shell without KUANG/11 at all.** The 20 MB tree of 2026-09-01 was that shell. §31 is
normative, and a shell that cannot load a component is a different product; the core build of §4
is the place for that product, with its own acceptance profile, and it is the user's call rather
than an optimisation.

**Keep the compiler in the shell and cache compiled components.** wasmtime's own cache would
spare the compile time on the second load and none of the 10.7 MB, because the compiler is still
linked in for the first. The size is the problem this record is about.

**Winch or Pulley instead of Cranelift.** Both still link `cranelift_codegen` in wasmtime 47 —
Winch for its backend, Pulley as a compilation target — so neither removes the cost §3 removes.

**Trim features instead of tiers.** `regex` without Unicode, wasmtime without
`parallel-compilation`, `tokio` without `tracing`: each measured or estimated under half a
megabyte, together under two. Worth doing on the way, and none of them is a decision.
