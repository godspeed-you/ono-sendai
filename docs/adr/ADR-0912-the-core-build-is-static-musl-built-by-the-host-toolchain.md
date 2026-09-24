# ADR-0912: The core build is a static musl binary, built by the pinned host toolchain and held to 8 MB

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §44.3 (pinned toolchain); ADR-0863 §1, §4; ADR-0910; ADR-0123
- Issues: #127
- Decided by: agent (autonomous)

## Context

The exit test of #127 is a command:

```text
cargo build --release -p ono-cli --no-default-features --features core \
  --target x86_64-unknown-linux-musl
```

It must produce a static binary under 8 MB at the release profile of ADR-0863 §1 (`opt-level =
"s"`, `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`). The profile may not use
`opt-level = "z"` or `panic = "abort"`.

A static musl build usually needs a musl C toolchain (`musl-gcc`) for crates that compile C:
`ring` and the bundled SQLite. The development machine has none, and nobody there can install
one. The packages of ADR-0123 are built in a container, never with the host toolchain, because a
glibc binary depends on the glibc of the machine that linked it.

## Decision

1. **The host toolchain builds the core binary.** The version is the one `rust-toolchain.toml`
   pins, and the target is added with `rustup target add x86_64-unknown-linux-musl`. No
   container is needed:
   - A static musl binary depends on no libc of the machine that built it. The reason ADR-0123
     builds packages in a container does not apply here.
   - The core graph of ADR-0910 contains no C code: no `ring`, no SQLite amalgamation, no `cc`
     build dependency. rustup's self-contained musl target carries the C runtime objects it
     links, and the system `cc` only drives the link. No musl C compiler is involved.

2. **`scripts/build-core.sh` is the recipe.** It runs the exit-test command with the ADR-0863
   profile stated in `CARGO_PROFILE_RELEASE_*`, so the measurement holds whatever the workspace
   profile says on a given branch. It then checks what the exit test claims:
   - The binary is static: `readelf` finds no `INTERP` program header and no `NEEDED` library.
   - It runs, and names itself the core build.
   - It is under the budget of **8,000,000 bytes**. "8 MB" is read as decimal megabytes, the
     stricter reading.

   `--stage <dir>` lays out the build context of `docker/core/Dockerfile` (ADR-0913).

3. **The budget is enforced in CI** on every push, by the job that builds the core binary
   (ADR-0913).

## Consequences

Measured on 2026-09-24 with toolchain 1.94 at the ADR-0863 profile, all stripped, on top of #126
(ADR-0870: wasmtime without its compiler in `ono`):

| build | size | links |
|---|---|---|
| core, `x86_64-unknown-linux-musl` | 7,063,744 bytes | nothing (static-pie) |
| core, `x86_64-unknown-linux-gnu` | 6,913,976 bytes | libc, libm, libgcc_s |
| full, `x86_64-unknown-linux-gnu` | 22,155,744 bytes | libc, libm, libgcc_s |

The full build measured 28,736,736 bytes before #126.

- The musl binary is 0.94 MB under budget. ADR-0863 estimated 5 to 6 MB for today's crates; the
  measured core is larger because the object shell's own code has grown since August:
  `ono-cli`, the command registry and the value model.
- If a C dependency enters the core graph, the build fails for want of a musl C compiler. That
  is deliberate: the dependency needs an ADR that says whether the core should carry it (§45.4).
- The core build compiles and links in about 2.5 minutes on the development machine from a cold
  target directory for its target, against about 19 minutes for the full build at the same profile
  (measured before #126, on a machine shared with other builds).

## Alternatives considered

**Building inside `rust:1.94-alpine`.** That also works, but it needs a pinned image, a
registry pull and a second cargo cache on every CI run, and it proves nothing more than the host
build does, because the result depends on no libc. It would become the right choice if the core
graph ever regained a C dependency.

**`cross` with its musl image.** This was rejected for the same reason, and `cross` is pinned for
a different job (ADR-0450).

**Reading "8 MB" as MiB (8,388,608 bytes).** This was rejected: the binary passes under either
reading, and the budget keeps the stricter one.
